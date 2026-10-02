//! Semantic job-classifier probe (BRU-330; see
//! `docs/job-function-semantic-classifier-experiment.md`). Not part of the
//! product: it classifies postings of a frozen corpus with
//! [`jobhunt_eval::job_class`] and scores the answers. No candidate data is
//! read or sent.
//!
//! ```text
//! # the exact input each posting would send (no network)
//! job_classifier_probe dump  <corpus.db> <ids.txt> <out dir>
//! # one fresh classification per posting (OPENAI_API_KEY from the
//! # environment; never written anywhere)
//! job_classifier_probe run   <corpus.db> <ids.txt> <run.json>
//! # agreement with the annotations, and stability between two runs
//! job_classifier_probe score <annotations.json> <run1.json> <run2.json>
//! ```
//!
//! `ids.txt` holds one job id per line (`job_<hex>`). `CLASSIFIER_MODEL`
//! overrides the model; `CLASSIFIER_BASE_URL` points at another
//! OpenAI-compatible server (a local mock for a dry run).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use jobhunt_ai::{ModelConfig, ModelInterpreter};
use jobhunt_core::StableId;
use jobhunt_eval::job_class::{
    self, CLASSIFIER_VERSION, Classification, Expected, Field, JobInput, PostingFields, Stability,
    Validated,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

const PROVIDER: &str = "openai";
const DEFAULT_MODEL: &str = "gpt-6-luna";
const CONCURRENCY: usize = 4;
/// Longest quote kept in a committed run file.
const MAX_QUOTE_CHARS: usize = 160;

struct Posting {
    id: String,
    company: String,
    title: String,
    content_fingerprint: Option<String>,
    input: JobInput,
}

async fn load(db: &str, ids: &[String]) -> Vec<Posting> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{db}?mode=ro"))
        .await
        .unwrap();
    let mut out = Vec::new();
    for id in ids {
        let row = sqlx::query(
            "select company, title, department, team, location, workplace_type, \
             description_text, content_fingerprint from jobs where id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("{id}: {e}"));
        let get = |c: &str| row.get::<Option<String>, _>(c);
        let (title, department, team) = (get("title").unwrap(), get("department"), get("team"));
        let (location, workplace, description) = (
            get("location"),
            get("workplace_type"),
            get("description_text"),
        );
        let input = JobInput::build(&PostingFields {
            title: &title,
            department: department.as_deref(),
            team: team.as_deref(),
            location: location.as_deref(),
            workplace: workplace.as_deref(),
            description: description.as_deref(),
        });
        out.push(Posting {
            id: id.clone(),
            company: get("company").unwrap(),
            title,
            content_fingerprint: get("content_fingerprint"),
            input,
        });
    }
    pool.close().await;
    out
}

fn ids(path: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// The global cache key a classification would be stored under: the
/// classifier revision, the model, and exactly what was sent. No candidate.
fn cache_key(input: &JobInput, model: &str) -> String {
    let sent = serde_json::to_string(input).unwrap();
    format!(
        "jcl_{}",
        StableId::derive(
            "jobhunt.job_class",
            &[CLASSIFIER_VERSION, &format!("{PROVIDER}:{model}"), &sent]
        )
    )
}

fn clip(quotes: &mut [String]) {
    for q in quotes.iter_mut() {
        *q = q.chars().take(MAX_QUOTE_CHARS).collect();
    }
}

async fn classify(model: &ModelInterpreter, posting: &Posting, model_name: &str) -> Value {
    let user = job_class::render(&posting.input);
    let mut attempts = Vec::new();
    let max_retries = model.config().max_retries;
    let mut result = None;
    for attempt in 0..=max_retries {
        let started = Instant::now();
        let answer = model
            .structured(
                job_class::SYSTEM_PROMPT,
                &user,
                job_class::schema(),
                "job_classification",
            )
            .await;
        let ms = started.elapsed().as_millis();
        match answer {
            Ok(a) => {
                let parsed = job_class::parse(&a.text, &posting.input);
                attempts.push(json!({
                    "attempt": attempt, "ms": ms, "input_tokens": a.input_tokens,
                    "output_tokens": a.output_tokens, "reasoning_tokens": a.reasoning_tokens,
                    "malformed": parsed.as_ref().err(),
                }));
                if let Ok(v) = parsed {
                    result = Some(v);
                    break;
                }
            }
            Err(e) => {
                let retryable = e.is_retryable();
                attempts.push(json!({"attempt": attempt, "ms": ms, "error": e.to_string()}));
                if !retryable {
                    break;
                }
            }
        }
        if attempt < max_retries {
            tokio::time::sleep(model.config().backoff * 2u32.pow(attempt)).await;
        }
    }
    let mut out = json!({
        "job": posting.id,
        "company": posting.company,
        "title": posting.title,
        "content_fingerprint": posting.content_fingerprint,
        "cache_key": cache_key(&posting.input, model_name),
        "sent_chars": job_class::SYSTEM_PROMPT.chars().count()
            + user.chars().count()
            + job_class::schema().to_string().chars().count(),
        "attempts": attempts,
    });
    if let Some(mut v) = result {
        let e = &mut v.classification.evidence;
        for q in [
            &mut e.function,
            &mut e.shapes,
            &mut e.specialization,
            &mut e.seniority,
            &mut e.customer_facing,
        ] {
            clip(q);
        }
        for (_, q) in &mut v.invalid_quotes {
            *q = q.chars().take(MAX_QUOTE_CHARS).collect();
        }
        out["result"] = serde_json::to_value(&v).unwrap();
    }
    out
}

async fn run(db: &str, ids_path: &str, out_path: &str) {
    let model_name = std::env::var("CLASSIFIER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());
    // A local OpenAI-compatible server, for a dry run of the pipeline.
    let base_url = std::env::var("CLASSIFIER_BASE_URL").ok();
    let config = ModelConfig::new(
        PROVIDER,
        Some(&model_name),
        base_url.as_deref(),
        std::env::var("OPENAI_API_KEY").ok(),
        "OPENAI_API_KEY",
    )
    .unwrap();
    let model = Arc::new(ModelInterpreter::new(config).unwrap());
    let postings = load(db, &ids(ids_path)).await;
    let started = Instant::now();
    let started_at = chrono::Utc::now();
    let mut calls: Vec<Value> = futures::stream::iter(postings.iter())
        .map(|p| {
            let model = model.clone();
            let name = model_name.clone();
            async move { classify(&model, p, &name).await }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    let order: BTreeMap<&str, usize> = postings
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    calls.sort_by_key(|c| order[c["job"].as_str().unwrap()]);
    let out = json!({
        "classifier": CLASSIFIER_VERSION,
        "provider": PROVIDER,
        "model": model_name,
        "started_at": started_at.to_rfc3339(),
        "wall_ms": started.elapsed().as_millis(),
        "concurrency": CONCURRENCY,
        "cache": "none: every posting called fresh",
        "jobs": calls,
    });
    std::fs::write(out_path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
}

async fn dump(db: &str, ids_path: &str, dir: &str) {
    std::fs::create_dir_all(dir).unwrap();
    for p in load(db, &ids(ids_path)).await {
        let text = format!(
            "# {} · {} · omitted {}\n{}",
            p.company,
            p.id,
            p.input.omitted,
            job_class::render(&p.input)
        );
        std::fs::write(format!("{dir}/{}.txt", p.id), text).unwrap();
    }
}

#[derive(Deserialize)]
struct Annotated {
    job: String,
    company: String,
    title: String,
    #[serde(default)]
    critical: Option<String>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Annotations {
    jobs: Vec<Annotated>,
}

fn classification(job: &Value) -> Option<(Classification, Validated)> {
    let v: Validated = serde_json::from_value(job.get("result")?.clone()).ok()?;
    Some((v.classification.clone(), v))
}

fn by_job(run: &Value) -> BTreeMap<String, Value> {
    run["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| (j["job"].as_str().unwrap().to_owned(), j.clone()))
        .collect()
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn usage(run: &Value) -> Value {
    let mut input = 0;
    let mut output = 0;
    let mut reasoning = 0;
    let mut final_ms = Vec::new();
    let mut all_ms = Vec::new();
    let (mut retries, mut malformed, mut errors, mut failed) = (0, 0, 0, 0);
    for job in run["jobs"].as_array().unwrap() {
        let attempts = job["attempts"].as_array().unwrap();
        retries += attempts.len().saturating_sub(1);
        for a in attempts {
            input += a["input_tokens"].as_u64().unwrap_or(0);
            output += a["output_tokens"].as_u64().unwrap_or(0);
            reasoning += a["reasoning_tokens"].as_u64().unwrap_or(0);
            all_ms.push(a["ms"].as_u64().unwrap());
            malformed += usize::from(!a["malformed"].is_null());
            errors += usize::from(a.get("error").is_some());
        }
        if let Some(last) = attempts.last() {
            final_ms.push(last["ms"].as_u64().unwrap());
        }
        failed += usize::from(job.get("result").is_none());
    }
    final_ms.sort_unstable();
    all_ms.sort_unstable();
    json!({
        "classifications": run["jobs"].as_array().unwrap().len(),
        "calls": all_ms.len(),
        "input_tokens": input, "output_tokens": output, "reasoning_tokens": reasoning,
        "retries": retries, "malformed": malformed, "errors": errors, "failed": failed,
        "latency_ms": {"median": percentile(&all_ms, 0.5), "p90": percentile(&all_ms, 0.9),
                        "max": all_ms.last()},
        "wall_ms": run["wall_ms"],
    })
}

fn score(annotations: &str, runs: &[String]) {
    let annotations: Annotations =
        serde_json::from_str(&std::fs::read_to_string(annotations).unwrap()).unwrap();
    let runs: Vec<Value> = runs
        .iter()
        .map(|r| serde_json::from_str(&std::fs::read_to_string(r).unwrap()).unwrap())
        .collect();
    let maps: Vec<_> = runs.iter().map(by_job).collect();
    let mut per_run = Vec::new();
    for (i, map) in maps.iter().enumerate() {
        let mut agree: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        let mut all_fields = 0;
        let mut misses = Vec::new();
        let (mut valid, mut invalid, mut unsupported) = (0, 0, 0);
        for a in &annotations.jobs {
            let Some((c, v)) = map.get(&a.job).and_then(classification) else {
                misses.push(json!({"job": a.job, "title": a.title, "missing": true}));
                continue;
            };
            valid += v.valid_quotes;
            invalid += v.invalid_quotes.len();
            unsupported += v.unsupported.len();
            let g = job_class::agreement(&a.expected, &c);
            let mut wrong = Vec::new();
            let mut ok_all = true;
            for f in Field::ALL {
                if let Some(ok) = g.get(f) {
                    let e = agree.entry(f.as_str()).or_default();
                    e.1 += 1;
                    if ok {
                        e.0 += 1;
                    } else {
                        ok_all = false;
                        wrong.push(f.as_str());
                    }
                }
            }
            all_fields += usize::from(ok_all);
            if !wrong.is_empty() {
                misses.push(json!({
                    "job": a.job, "company": a.company, "title": a.title,
                    "critical": a.critical, "wrong": wrong,
                    "got": short(&c), "expected": a.expected,
                }));
            }
        }
        per_run.push(json!({
            "run": runs[i]["started_at"],
            "usage": usage(&runs[i]),
            "agreement": agree.iter().map(|(k, (ok, n))| (k.to_string(), json!(format!("{ok}/{n}"))))
                .collect::<serde_json::Map<_, _>>(),
            "all_fields_agree": format!("{all_fields}/{}", annotations.jobs.len()),
            "quotes": {"valid": valid, "invalid": invalid, "unsupported_fields": unsupported},
            "disagreements": misses,
        }));
    }
    let mut stability = Value::Null;
    if maps.len() >= 2 {
        let mut counts: BTreeMap<&str, BTreeMap<Stability, usize>> = BTreeMap::new();
        let mut jobs_stable = 0;
        let mut jobs_exact = 0;
        let mut material = Vec::new();
        let mut differences = Vec::new();
        for a in &annotations.jobs {
            let (Some((x, _)), Some((y, _))) = (
                maps[0].get(&a.job).and_then(classification),
                maps[1].get(&a.job).and_then(classification),
            ) else {
                continue;
            };
            let cmp = job_class::compare(&x, &y);
            jobs_stable += usize::from(cmp.iter().all(|(_, s)| *s != Stability::Material));
            jobs_exact += usize::from(cmp.iter().all(|(_, s)| *s == Stability::Exact));
            for (f, s) in &cmp {
                *counts.entry(f.as_str()).or_default().entry(*s).or_default() += 1;
            }
            let diff: Vec<_> = cmp
                .iter()
                .filter(|(_, s)| *s != Stability::Exact)
                .map(|(f, s)| json!({"field": f.as_str(), "stability": s}))
                .collect();
            if !diff.is_empty() {
                let row = json!({"job": a.job, "company": a.company, "title": a.title,
                    "fields": diff, "run1": short(&x), "run2": short(&y)});
                if cmp.iter().any(|(_, s)| *s == Stability::Material) {
                    material.push(row);
                } else {
                    differences.push(row);
                }
            }
        }
        stability = json!({
            "per_field": counts,
            "jobs_without_material_disagreement": format!("{jobs_stable}/{}", annotations.jobs.len()),
            "jobs_identical": format!("{jobs_exact}/{}", annotations.jobs.len()),
            "material": material,
            "equivalent_only": differences,
        });
    }
    let out = json!({"runs": per_run, "stability": stability});
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

fn short(c: &Classification) -> Value {
    json!({
        "function": c.function, "shapes": c.engineering_shapes,
        "depth": c.specialization_depth, "specialties": c.specialties,
        "seniority": c.seniority, "customer_facing": c.customer_facing,
    })
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("dump") => dump(&args[2], &args[3], &args[4]).await,
        Some("run") => run(&args[2], &args[3], &args[4]).await,
        Some("score") => score(&args[2], &args[3..]),
        _ => eprintln!("usage: job_classifier_probe dump|run|score …"),
    }
}
