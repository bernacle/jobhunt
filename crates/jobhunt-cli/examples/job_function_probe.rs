//! Function-only job-classifier probe (see
//! `docs/job-function-only-fresh-snapshot-experiment.md`). Not part of the
//! product: it samples a frozen corpus, classifies postings with
//! [`jobhunt_eval::job_function`] and scores the answers. No candidate data
//! is read or sent.
//!
//! ```text
//! # a seeded, stratified sample of the corpus's open postings, leaving out
//! # the ids in [exclude.txt]
//! job_function_probe sample <corpus.db> <seed> <ids.txt> <selection.json> [exclude.txt]
//! # the exact input each posting would send (no network)
//! job_function_probe dump   <corpus.db> <ids.txt> <out dir>
//! # input size of every open posting (for cost estimates; no network)
//! job_function_probe size   <corpus.db>
//! # one fresh classification per posting (OPENAI_API_KEY from the
//! # environment; never written anywhere)
//! job_function_probe run    <corpus.db> <ids.txt> <run.json>
//! # agreement with the annotations, stability across runs, the gates
//! job_function_probe score  <annotations.json> <run1.json> <run2.json> <run3.json>
//! ```
//!
//! `ids.txt` holds one job id per line (`job_<hex>`). `CLASSIFIER_MODEL`
//! overrides the model; `CLASSIFIER_BASE_URL` points at another
//! OpenAI-compatible server (a local mock for a dry run).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use jobhunt_ai::{ModelConfig, ModelInterpreter};
use jobhunt_core::StableId;
use jobhunt_eval::job_function::{
    self, Annotation, CLASSIFIER_VERSION, Classification, JobInput, PostingFields, Validated,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

const PROVIDER: &str = "openai";
const DEFAULT_MODEL: &str = "gpt-6-luna";
const CONCURRENCY: usize = 4;
/// Longest quote kept in a committed run file.
const MAX_QUOTE_CHARS: usize = 160;

async fn pool(db: &str) -> sqlx::SqlitePool {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{db}?mode=ro"))
        .await
        .unwrap()
}

struct Posting {
    id: String,
    company: String,
    title: String,
    content_fingerprint: Option<String>,
    input: JobInput,
}

fn posting(row: &sqlx::sqlite::SqliteRow) -> Posting {
    let get = |c: &str| row.get::<Option<String>, _>(c);
    let title = get("title").unwrap();
    let (department, team, description) = (get("department"), get("team"), get("description_text"));
    let input = JobInput::build(&PostingFields {
        title: &title,
        department: department.as_deref(),
        team: team.as_deref(),
        // Not sent (see `job_function::render`).
        location: None,
        workplace: None,
        description: description.as_deref(),
    });
    Posting {
        id: get("id").unwrap(),
        company: get("company").unwrap(),
        title,
        content_fingerprint: get("content_fingerprint"),
        input,
    }
}

const COLUMNS: &str = "id, company, title, department, team, description_text, content_fingerprint";

async fn load(db: &str, ids: &[String]) -> Vec<Posting> {
    let pool = pool(db).await;
    let mut out = Vec::new();
    for id in ids {
        let row = sqlx::query(&format!("select {COLUMNS} from jobs where id = ?"))
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        out.push(posting(&row));
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

// --- sampling ---------------------------------------------------------------

/// A sampling stratum: a title rule and how many postings to draw.
struct Stratum {
    name: &'static str,
    quota: usize,
    matches: fn(&str) -> bool,
}

fn any(title: &str, words: &[&str]) -> bool {
    words.iter().any(|w| title.contains(w))
}

/// "Engineer", "developer" and the like: a title that reads as engineering.
fn eng(t: &str) -> bool {
    any(
        t,
        &[
            "engineer",
            "developer",
            " sre",
            "sre ",
            "programmer",
            "devops",
        ],
    )
}

/// Titles are lowercased and padded with a space on each side. The first
/// stratum whose rule matches owns the posting; the boundary strata come
/// first, so "Solutions Engineer" is drawn as solutions, not engineering.
const STRATA: &[Stratum] = &[
    Stratum {
        name: "curriculum_or_training",
        quota: 4,
        matches: |t| {
            any(
                t,
                &[
                    "curriculum",
                    "instructor",
                    "trainer",
                    "training",
                    "educat",
                    "enablement",
                    "course",
                ],
            ) && !any(t, &["pretrain", "post-train", "inference"])
        },
    },
    Stratum {
        name: "gtm_or_revenue",
        quota: 5,
        matches: |t| {
            any(
                t,
                &[
                    " gtm",
                    "gtm ",
                    "go-to-market",
                    "go to market",
                    "revenue",
                    "revops",
                    "rev ops",
                    "marketing engineer",
                    "marketing operations",
                    "marketing ops",
                    "sales operations",
                    "sales ops",
                    "business systems",
                    "salesforce",
                    " crm",
                    "hubspot",
                    "growth engineer",
                ],
            )
        },
    },
    Stratum {
        name: "developer_relations",
        quota: 4,
        matches: |t| {
            any(
                t,
                &[
                    "developer relations",
                    "devrel",
                    "advocate",
                    "evangelist",
                    "community",
                ],
            )
        },
    },
    Stratum {
        name: "research",
        quota: 5,
        matches: |t| any(t, &["research", "scientist"]),
    },
    Stratum {
        name: "product_or_program_management",
        quota: 5,
        matches: |t| {
            any(
                t,
                &[
                    "product manager",
                    "program manager",
                    "project manager",
                    "technical program",
                    "product owner",
                    "product lead",
                    " tpm",
                    "product management",
                    "program management",
                ],
            )
        },
    },
    Stratum {
        name: "engineering_management",
        quota: 6,
        matches: |t| {
            (any(
                t,
                &[
                    "manager",
                    "director",
                    "head of",
                    " vp ",
                    " vp,",
                    "vice president",
                    "chief",
                    " cto",
                ],
            ) && any(
                t,
                &[
                    "engineer",
                    "software",
                    "infrastructure",
                    "platform",
                    "reliability",
                    "security",
                    "data",
                    "development",
                    "technical",
                    "technology",
                ],
            )) || any(
                t,
                &[
                    "tech lead",
                    "team lead",
                    "engineering lead",
                    "lead engineer",
                    "lead software",
                ],
            )
        },
    },
    Stratum {
        name: "solutions_or_architecture",
        quota: 7,
        matches: |t| {
            any(
                t,
                &[
                    "solution",
                    "sales engineer",
                    "pre-sales",
                    "presales",
                    "pre sales",
                    "field engineer",
                    "forward deployed",
                    "forward-deployed",
                    "technical account",
                    "architect",
                    "customer engineer",
                ],
            )
        },
    },
    Stratum {
        name: "support_or_success",
        quota: 6,
        matches: |t| {
            any(
                t,
                &[
                    "support",
                    "success",
                    "technical services",
                    "escalation",
                    "customer reliability",
                    "customer operations",
                ],
            )
        },
    },
    Stratum {
        name: "implementation_integration_consulting",
        quota: 5,
        matches: |t| {
            any(
                t,
                &[
                    "implementation",
                    "integration",
                    "onboarding",
                    "consult",
                    "professional services",
                    "deployment",
                    "partner engineer",
                ],
            )
        },
    },
    Stratum {
        name: "platform_in_name_not_engineering_title",
        quota: 4,
        matches: |t| t.contains("platform") && !eng(t),
    },
    Stratum {
        name: "sre_or_devops",
        quota: 4,
        matches: |t| {
            eng(t)
                && any(
                    t,
                    &[
                        "site reliability",
                        " sre",
                        "sre ",
                        "reliability",
                        "devops",
                        "production engineer",
                    ],
                )
        },
    },
    Stratum {
        name: "security_engineering",
        quota: 4,
        matches: |t| eng(t) && t.contains("security"),
    },
    Stratum {
        name: "data_or_ml_engineering",
        quota: 5,
        matches: |t| {
            eng(t)
                && any(
                    t,
                    &[
                        "data",
                        "machine learning",
                        " ml",
                        " ai",
                        "ai ",
                        "llm",
                        "inference",
                        "model",
                    ],
                )
        },
    },
    Stratum {
        name: "mobile",
        quota: 3,
        matches: |t| eng(t) && any(t, &["ios", "android", "mobile"]),
    },
    Stratum {
        name: "frontend",
        quota: 3,
        matches: |t| eng(t) && any(t, &["frontend", "front-end", "front end", "ui ", "web "]),
    },
    Stratum {
        name: "full_stack",
        quota: 4,
        matches: |t| eng(t) && any(t, &["full stack", "fullstack", "full-stack"]),
    },
    Stratum {
        name: "product_engineering",
        quota: 4,
        matches: |t| eng(t) && t.contains("product"),
    },
    Stratum {
        name: "platform_engineering",
        quota: 4,
        matches: |t| eng(t) && t.contains("platform"),
    },
    Stratum {
        name: "infrastructure",
        quota: 4,
        matches: |t| {
            eng(t)
                && any(
                    t,
                    &[
                        "infrastructure",
                        "infra",
                        "cloud",
                        "systems",
                        "kubernetes",
                        "network",
                        "storage",
                        "compute",
                        "kernel",
                    ],
                )
        },
    },
    Stratum {
        name: "backend",
        quota: 4,
        matches: |t| {
            eng(t)
                && any(
                    t,
                    &[
                        "backend",
                        "back-end",
                        "back end",
                        "api",
                        "distributed",
                        "server",
                    ],
                )
        },
    },
    Stratum {
        name: "other_engineering_titles",
        quota: 3,
        matches: eng,
    },
    Stratum {
        name: "other_titles",
        quota: 3,
        matches: |_| true,
    },
];

/// At most this many sampled postings per company.
const COMPANY_CAP: usize = 5;

fn stratum(title: &str) -> &'static Stratum {
    let t = format!(" {} ", title.to_lowercase());
    STRATA.iter().find(|s| (s.matches)(&t)).unwrap()
}

/// Draws the sample: every open posting goes to the first stratum its
/// title matches; within a stratum, postings are taken in the order of a
/// seeded hash of their id. A company appears once per stratum (twice only
/// if the stratum can't fill otherwise) and at most [`COMPANY_CAP`] times
/// overall, unless a stratum can't fill any other way, and a company's
/// repeated title is drawn once. Postings in
/// `exclude` (an earlier experiment's) are never drawn.
async fn sample(db: &str, seed: &str, ids_out: &str, selection_out: &str, exclude: Option<&str>) {
    let excluded: BTreeSet<String> = exclude.map(ids).unwrap_or_default().into_iter().collect();
    let pool = pool(db).await;
    let rows = sqlx::query("select id, company, title from jobs where status = 'open'")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    let mut pools: BTreeMap<&str, Vec<(String, String, String, String)>> = BTreeMap::new();
    for row in &rows {
        let (id, company, title): (String, String, String) =
            (row.get("id"), row.get("company"), row.get("title"));
        if excluded.contains(&id) {
            continue;
        }
        let key = StableId::derive("narrow.eval.job_function.sample", &[seed, &id]).to_string();
        pools
            .entry(stratum(&title).name)
            .or_default()
            .push((key, id, company, title));
    }
    let mut per_company: BTreeMap<String, usize> = BTreeMap::new();
    let mut titles: BTreeSet<(String, String)> = BTreeSet::new();
    let mut picked = Vec::new();
    let mut summary = Vec::new();
    for s in STRATA {
        let mut candidates = pools.remove(s.name).unwrap_or_default();
        candidates.sort();
        let mut here: Vec<&(String, String, String, String)> = Vec::new();
        // (per company in this stratum, whether the overall cap holds)
        for (per_stratum, capped) in [(1, true), (2, true), (1, false), (2, false)] {
            for c in &candidates {
                if here.len() >= s.quota {
                    break;
                }
                let (_, _, company, title) = c;
                let title_key = (company.clone(), title.trim().to_lowercase());
                let in_stratum = here.iter().filter(|h| &h.2 == company).count();
                if in_stratum >= per_stratum
                    || (capped && per_company.get(company).copied().unwrap_or(0) >= COMPANY_CAP)
                    || titles.contains(&title_key)
                {
                    continue;
                }
                titles.insert(title_key);
                *per_company.entry(company.clone()).or_default() += 1;
                here.push(c);
            }
        }
        summary.push(json!({
            "stratum": s.name, "quota": s.quota, "pool": candidates.len(), "drawn": here.len(),
        }));
        for (_, id, company, title) in here {
            picked.push(json!({"job": id, "company": company, "title": title, "stratum": s.name}));
        }
    }
    let ids: Vec<&str> = picked.iter().map(|p| p["job"].as_str().unwrap()).collect();
    std::fs::write(ids_out, ids.join("\n") + "\n").unwrap();
    let out = json!({
        "seed": seed,
        "excluded": excluded.len(),
        "open_postings": rows.len(),
        "company_cap": COMPANY_CAP,
        "strata": summary,
        "sampled": picked.len(),
        "jobs": picked,
    });
    std::fs::write(selection_out, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    println!("sampled {} of {} open postings", picked.len(), rows.len());
}

// --- inputs -----------------------------------------------------------------

async fn dump(db: &str, ids_path: &str, dir: &str) {
    std::fs::create_dir_all(dir).unwrap();
    for p in load(db, &ids(ids_path)).await {
        let text = format!(
            "# {} · {} · omitted {}\n{}",
            p.company,
            p.id,
            p.input.omitted,
            job_function::render(&p.input)
        );
        std::fs::write(format!("{dir}/{}.txt", p.id), text).unwrap();
    }
}

fn fixed_chars() -> usize {
    job_function::SYSTEM_PROMPT.chars().count() + job_function::schema().to_string().chars().count()
}

/// The prompt and schema, digested: two runs with the same digest sent the
/// same instructions.
fn prompt_digest() -> String {
    StableId::derive(
        "narrow.eval.job_function.prompt",
        &[
            CLASSIFIER_VERSION,
            job_function::SYSTEM_PROMPT,
            &job_function::schema().to_string(),
        ],
    )
    .to_string()
}

async fn size(db: &str) {
    let pool = pool(db).await;
    let rows = sqlx::query(&format!("select {COLUMNS} from jobs where status = 'open'"))
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    let mut chars: Vec<usize> = rows
        .iter()
        .map(|r| job_function::render(&posting(r).input).chars().count())
        .collect();
    chars.sort_unstable();
    let total: usize = chars.iter().sum();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "open_postings": chars.len(),
            "job_chars": {"mean": total / chars.len().max(1), "median": chars.get(chars.len() / 2),
                          "max": chars.last(), "total": total},
            "fixed_chars_per_call": fixed_chars(),
            "prompt_digest": prompt_digest(),
        }))
        .unwrap()
    );
}

// --- runs -------------------------------------------------------------------

/// The global cache key a classification would be stored under: the
/// classifier revision, the model, and exactly what was sent. No candidate.
fn cache_key(input: &JobInput, model: &str) -> String {
    let sent = job_function::render(input);
    format!(
        "jfn_{}",
        StableId::derive(
            "narrow.job_function",
            &[CLASSIFIER_VERSION, &format!("{PROVIDER}:{model}"), &sent]
        )
    )
}

fn clip(q: &str) -> String {
    q.chars().take(MAX_QUOTE_CHARS).collect()
}

async fn classify(model: &ModelInterpreter, posting: &Posting, model_name: &str) -> Value {
    let user = job_function::render(&posting.input);
    let mut attempts = Vec::new();
    let max_retries = model.config().max_retries;
    let mut result = None;
    for attempt in 0..=max_retries {
        let started = Instant::now();
        let answer = model
            .structured(
                job_function::SYSTEM_PROMPT,
                &user,
                job_function::schema(),
                "job_function",
            )
            .await;
        let ms = started.elapsed().as_millis();
        match answer {
            Ok(a) => {
                let parsed = job_function::parse(&a.text, &posting.input);
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
        "sent_chars": fixed_chars() + user.chars().count(),
        "attempts": attempts,
    });
    if let Some(mut v) = result {
        let c = &mut v.classification;
        for q in c
            .function_evidence
            .iter_mut()
            .chain(c.customer_facing_evidence.iter_mut())
        {
            *q = clip(q);
        }
        for (_, q) in &mut v.invalid_quotes {
            *q = clip(q);
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
        "prompt_digest": prompt_digest(),
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

// --- scoring ----------------------------------------------------------------

#[derive(Deserialize)]
struct Annotated {
    job: String,
    company: String,
    title: String,
    annotation: Annotation,
}

#[derive(Deserialize)]
struct Annotations {
    jobs: Vec<Annotated>,
}

fn answers(run: &Value) -> BTreeMap<String, Validated> {
    run["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|j| {
            let v: Validated = serde_json::from_value(j.get("result")?.clone()).ok()?;
            Some((j["job"].as_str().unwrap().to_owned(), v))
        })
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
    let (mut input, mut output, mut reasoning) = (0, 0, 0);
    let mut ms = Vec::new();
    let (mut retries, mut malformed, mut errors, mut failed) = (0, 0, 0, 0);
    let (mut valid, mut invalid, mut unsupported) = (0, 0, 0);
    for job in run["jobs"].as_array().unwrap() {
        let attempts = job["attempts"].as_array().unwrap();
        retries += attempts.len().saturating_sub(1);
        for a in attempts {
            input += a["input_tokens"].as_u64().unwrap_or(0);
            output += a["output_tokens"].as_u64().unwrap_or(0);
            reasoning += a["reasoning_tokens"].as_u64().unwrap_or(0);
            ms.push(a["ms"].as_u64().unwrap());
            malformed += usize::from(!a["malformed"].is_null());
            errors += usize::from(a.get("error").is_some());
        }
        failed += usize::from(job.get("result").is_none());
        if let Ok(v) = serde_json::from_value::<Validated>(job["result"].clone()) {
            valid += v.valid_quotes;
            invalid += v.invalid_quotes.len();
            unsupported += v.unsupported.len();
        }
    }
    ms.sort_unstable();
    json!({
        "classifications": run["jobs"].as_array().unwrap().len(),
        "calls": ms.len(),
        "input_tokens": input, "output_tokens": output, "reasoning_tokens": reasoning,
        "retries": retries, "malformed": malformed, "errors": errors, "failed": failed,
        "quotes": {"valid": valid, "invalid": invalid, "unsupported_labels": unsupported},
        "latency_ms": {"median": percentile(&ms, 0.5), "p90": percentile(&ms, 0.9), "max": ms.last()},
        "wall_ms": run["wall_ms"],
        "prompt_digest": run["prompt_digest"],
    })
}

fn score(annotations: &str, run_paths: &[String]) {
    let annotations: Annotations =
        serde_json::from_str(&std::fs::read_to_string(annotations).unwrap()).unwrap();
    let runs: Vec<Value> = run_paths
        .iter()
        .map(|r| serde_json::from_str(&std::fs::read_to_string(r).unwrap()).unwrap())
        .collect();
    let validated: Vec<BTreeMap<String, Validated>> = runs.iter().map(answers).collect();
    let maps: Vec<BTreeMap<String, Classification>> = validated
        .iter()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.classification.clone()))
                .collect()
        })
        .collect();
    let labels: Vec<(String, Annotation)> = annotations
        .jobs
        .iter()
        .map(|j| (j.job.clone(), j.annotation.clone()))
        .collect();
    let evaluation = job_function::evaluate(&labels, &maps);
    // Every job where any run disagrees with the annotation or with
    // another run.
    let mut rows = Vec::new();
    for j in &annotations.jobs {
        let got: Vec<Option<&Validated>> = validated.iter().map(|m| m.get(&j.job)).collect();
        let wrong = got.iter().any(|g| {
            g.is_none_or(|g| {
                !j.annotation.function_ok(g.classification.function)
                    || !j
                        .annotation
                        .customer_facing_ok(g.classification.primary_customer_facing)
            })
        });
        let unstable = got.windows(2).any(|w| {
            w[0].map(|v| &v.classification)
                .map(|c| (c.function, c.primary_customer_facing))
                != w[1]
                    .map(|v| &v.classification)
                    .map(|c| (c.function, c.primary_customer_facing))
        });
        if wrong || unstable {
            rows.push(json!({
                "job": j.job, "company": j.company, "title": j.title,
                "annotation": j.annotation,
                "answers": got.iter().map(|g| g.map(|v| json!({
                    "function": v.classification.function,
                    "customer_facing": v.classification.primary_customer_facing,
                    "function_evidence": v.classification.function_evidence,
                    "customer_facing_evidence": v.classification.customer_facing_evidence,
                }))).collect::<Vec<_>>(),
            }));
        }
    }
    let out = json!({
        "usage": runs.iter().map(usage).collect::<Vec<_>>(),
        "evaluation": evaluation,
        "disagreements": rows,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("sample") => {
            sample(
                &args[2],
                &args[3],
                &args[4],
                &args[5],
                args.get(6).map(String::as_str),
            )
            .await;
        }
        Some("dump") => dump(&args[2], &args[3], &args[4]).await,
        Some("size") => size(&args[2]).await,
        Some("run") => run(&args[2], &args[3], &args[4]).await,
        Some("score") => score(&args[2], &args[3..]),
        _ => eprintln!("usage: job_function_probe sample|dump|size|run|score …"),
    }
}
