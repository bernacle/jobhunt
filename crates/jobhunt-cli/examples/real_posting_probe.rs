//! Real-posting precision probe (BRU-325; see
//! `docs/real-posting-precision-experiment.md`). Not part of the product.
//!
//! Ranks every stored open job offline (`refresh: never`, `verify: false`)
//! at a fixed instant and prints the whole funnel, Today exactly as the
//! feed selects it, and every strong and plausible job, as JSON. With
//! `[ai] review_fit = true` the configured reviewer reviews the shortlist
//! within the production budget; the probe repeats the feed until nothing
//! is deferred (the steady state of a person opening Today a few times) and
//! records every model call it makes.
//!
//! ```text
//! cargo run --release -p jobhunt-cli --example real_posting_probe -- \
//!     <config.toml> <jobhunt.db> <now: RFC 3339> [max passes] [verify] > cell.json
//! ```
//!
//! `verify` verifies Today's candidates at their sources first, as the
//! cloud feed does (network; changes the database's verification state).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_app::feed::FeedRequest;
use jobhunt_app::{FitReview as AppFitReview, LocalApp, Quiet, RefreshMode};
use jobhunt_ranking::review::{self, CandidateBrief, FitReviewRequest, review_key};
use jobhunt_ranking::{FitLevel, FitReview, FitReviewer, Ranking, ReviewError, ReviewState, Tier};
use serde_json::{Value, json};

/// Every call the reviewer makes, in order.
struct Recording {
    inner: Arc<dyn FitReviewer>,
    calls: Mutex<Vec<Value>>,
}

#[async_trait]
impl FitReviewer for Recording {
    fn name(&self) -> String {
        self.inner.name()
    }

    async fn review(&self, request: &FitReviewRequest) -> Result<FitReview, ReviewError> {
        let started = Instant::now();
        let answer = self.inner.review(request).await;
        let ms = started.elapsed().as_millis();
        let mut call = json!({
            "key": review_key(request, &self.inner.name()),
            "company": request.job.company,
            "title": request.job.title,
            "ms": ms,
        });
        match &answer {
            Ok(r) => {
                call["fit"] = json!(r.fit.as_str());
                call["input_tokens"] = json!(r.input_tokens);
                call["output_tokens"] = json!(r.output_tokens);
            }
            Err(e) => call["error"] = json!(e.to_string()),
        }
        self.calls.lock().unwrap().push(call);
        answer
    }
}

fn row(r: &Ranking, record: Option<&jobhunt_jobs::JobRecord>, review: Option<&FitReview>) -> Value {
    let p = record.map(|r| &r.posting);
    json!({
        "opp": r.opportunity.to_string(),
        "job": r.job.to_string(),
        "company": r.company,
        "title": r.title,
        "tier": r.tier.as_str(),
        "fit": r.fit.level.as_str(),
        "score": r.score,
        "role_fit": r.fit.role_fit,
        "support": r.fit.support,
        "gate": format!("{:?}", r.gate),
        "practicality": r.practicality.status.as_str(),
        "blockers": r.practicality.blockers,
        "concerns": r.practicality.concerns,
        "checks": r.practicality.checks,
        "reasons": r.fit.reason_texts(),
        "contradictions": r.fit.contradictions.iter()
            .map(|c| format!("{} {}: {}", c.severity.as_str(), c.kind.as_str(), c.text))
            .collect::<Vec<_>>(),
        "uncertainties": r.fit.uncertainties,
        "review_state": serde_json::to_value(&r.fit.review).unwrap(),
        "source": p.map(|p| p.provenance.source.to_string()),
        "source_job_id": p.and_then(|p| p.provenance.source_record_id.clone()),
        "url": p.map(|p| p.url.to_string()),
        "location": p.and_then(|p| p.location.clone()),
        "locations": p.map(|p| p.locations.iter().map(|l| format!("{l:?}")).collect::<Vec<_>>()),
        "workplace": p.and_then(|p| p.workplace_type.as_ref().map(|w| w.as_str().to_owned())),
        "is_remote": p.and_then(|p| p.is_remote),
        "work_authorization": p.and_then(|p| p.work_authorization.clone()),
        "posted_at": p.and_then(|p| p.posted_at.map(|d| d.to_rfc3339())),
        "first_seen_at": record.map(|r| r.first_seen_at.to_rfc3339()),
        "review": review.map(|v| serde_json::to_value(v).unwrap()),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The reviewer's own log lines (one per attempt: provider, model,
    // attempt, duration, tokens; never the prompt, answer or key), as JSON
    // on stderr, so retries can be counted.
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "warn,jobhunt_ai=info,jobhunt_ranking=info",
        ))
        .init();
    let args: Vec<String> = std::env::args().collect();
    let now: DateTime<Utc> = args[3].parse()?;
    let max_passes: usize = args.get(4).map_or(Ok(12), |a| a.parse())?;
    let verify = args.get(5).is_some_and(|a| a == "verify");
    let loaded =
        jobhunt_app::config::load(Some(Path::new(&args[1])), Some(Path::new(&args[2])), None)?;
    let mut app = LocalApp::open(loaded).await?;
    let configured = app.fit_review().clone();
    if let Some(problem) = &configured.problem {
        return Err(format!("the reviewer is configured but unusable: {problem}").into());
    }
    let recording = configured.reviewer.clone().map(|inner| {
        Arc::new(Recording {
            inner,
            calls: Mutex::new(Vec::new()),
        })
    });
    if let Some(r) = &recording {
        app = app.with_fit_review(AppFitReview {
            reviewer: Some(r.clone() as Arc<dyn FitReviewer>),
            budget: configured.budget,
            problem: None,
        });
    }
    let request = FeedRequest {
        limit: jobhunt_app::feed::DEFAULT_FEED_LIMIT,
        verify,
        refresh: RefreshMode::Never,
    };
    let mut passes = Vec::new();
    let started = Instant::now();
    let feed = loop {
        let pass = Instant::now();
        let feed = app.feed(&request, &Quiet, now).await?;
        let s = feed.report.review;
        let t = feed.report.timings;
        passes.push(json!({
            "wall_ms": pass.elapsed().as_millis(),
            "ranking_ms": t.person_ms + t.load_ms + t.eligibility_ms + t.ranking_ms,
            "review_ms": t.review_ms,
            "shortlisted": s.shortlisted, "cache_hits": s.cache_hits, "calls": s.calls,
            "failures": s.failures, "deferred": s.deferred, "changed": s.changed,
            "input_tokens": s.input_tokens, "output_tokens": s.output_tokens,
            "strong": feed.report.rankings.iter().filter(|r| r.tier == Tier::StrongFit).count(),
            "today": feed.entries.iter().map(|e| json!({
                "company": e.entry.ranking.company, "title": e.entry.ranking.title,
                "also": e.also_at_company.iter().map(|s| s.title.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }));
        if recording.is_none() || s.deferred == 0 || passes.len() >= max_passes {
            break feed;
        }
    };
    let total_ms = started.elapsed().as_millis();
    let r = &feed.report;
    // The stored review behind each shortlisted job, by the key the
    // ranking used.
    let ranking = app.ranking();
    let (data, person) = ranking.person().await?;
    let brief = data.as_ref().map(|data| {
        let taste = jobhunt_profile::taste::compose(
            data,
            &jobhunt_ranking::taste::learned_signals(&r.taste),
        );
        CandidateBrief::build(data, &taste, &person)
    });
    let interesting: Vec<&Ranking> = r
        .rankings
        .iter()
        // Plausible or better, and anything the reviewer looked at (a job it
        // pushed below plausible keeps its row).
        .filter(|x| x.fit.level >= FitLevel::Plausible || x.fit.review != ReviewState::NotReviewed)
        .collect();
    let ids: Vec<_> = interesting.iter().map(|x| x.opportunity).collect();
    let mut records = app.store().opportunity_records_many(&ids).await?;
    let mut rows = Vec::new();
    for x in interesting {
        let record = records
            .remove(&x.opportunity)
            .unwrap_or_default()
            .into_iter()
            .find(|rec| rec.id == x.job);
        // What a review of this job would send, and the key it would be
        // stored under (the configured reviewer's, or the default model's).
        let mut review = None;
        let mut sent = json!(null);
        if let (Some(brief), Some(rec)) = (&brief, &record) {
            let request = FitReviewRequest::build(brief, rec, &x.facets);
            let name = recording.as_ref().map_or_else(
                || "model/anthropic:claude-opus-5-5".to_owned(),
                |r| r.name(),
            );
            let key = review_key(&request, &name);
            let chars = review::SYSTEM_PROMPT.chars().count()
                + review::render(&request).chars().count()
                + review::review_schema().to_string().chars().count();
            sent = json!({"review_key": key, "prompt_chars": chars});
            if recording.is_some() {
                review = app
                    .store()
                    .cached_fit_review(&person.profile_id, &key)
                    .await?;
            }
        }
        let mut out = row(x, record.as_ref(), review.as_ref());
        out["sent"] = sent;
        rows.push(out);
    }
    let count = |t: Tier| r.rankings.iter().filter(|x| x.tier == t).count();
    let out = json!({
        "now": now.to_rfc3339(),
        "verify": verify,
        "verified_now": feed.verified_now,
        "reviewer": recording.as_ref().map(|r| r.name()),
        "budget": format!("{:?}", configured.budget),
        "candidate_digest": brief.as_ref().map(CandidateBrief::digest),
        "candidate": brief,
        "considered": r.considered,
        "excluded": {"ineligible": r.excluded.ineligible, "unmet_requirement": r.excluded.unmet_requirement,
                      "closed": r.excluded.closed, "below_minimum": r.excluded.below_minimum},
        "actionable": r.rankings.len(),
        "strong": count(Tier::StrongFit), "plausible": count(Tier::WorthReviewing),
        "insufficient": count(Tier::Maybe), "poor": count(Tier::LowPriority),
        "today_companies": feed.entries.len(),
        "today_jobs": feed.entries.iter().map(|e| 1 + e.also_at_company.len()).sum::<usize>(),
        "today": feed.entries.iter().map(|e| json!({
            "company": e.entry.ranking.company, "title": e.entry.ranking.title,
            "opp": e.entry.ranking.opportunity.to_string(),
            "also": e.also_at_company.iter().map(|s| json!({"opp": s.id, "title": s.title})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "total_ms": total_ms,
        "passes": passes,
        "calls": recording.as_ref().map(|r| r.calls.lock().unwrap().clone()),
        "ranked": rows,
        // Every actionable job, compactly (for recall audits).
        "actionable_jobs": r.rankings.iter().map(|x| json!({
            "opp": x.opportunity.to_string(), "job": x.job.to_string(), "company": x.company,
            "title": x.title, "tier": x.tier.as_str(), "gate": format!("{:?}", x.gate),
            "contradictions": x.fit.contradictions.iter().take(2)
                .map(|c| format!("{} {}: {}", c.severity.as_str(), c.kind.as_str(), c.text))
                .collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    app.close().await;
    Ok(())
}
