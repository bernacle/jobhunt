//! Live verification against real first-party listings (opt-in: `cargo
//! test -p jobhunt-eligibility --test verification_live -- --ignored
//! --nocapture`; also the "verification" job of the Live sources
//! workflow). Never part of the required offline suite.
//!
//! For each family it reads one listing (as discovery would), verifies a
//! few of its jobs through their narrowest endpoints, and prints what was
//! found: listing and application status, the location scope and
//! restrictions read from the job, and compensation with its currency
//! evidence. It also asks for a job that does not exist, which must come
//! back "not found" rather than an error. Load is small: one listing plus
//! at most `JOBHUNT_LIVE_VERIFY_JOBS` (default 3) jobs per family.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use chrono::Utc;
use jobhunt_core::{FetchRequest, Fetched, SourceKey};
use jobhunt_eligibility::describe;
use jobhunt_eligibility::requirements;
use jobhunt_jobs::verification::{
    CompensationCheck, ListingVerifier, ObservedListing, SourceObservation,
};
use jobhunt_jobs::{JobPosting, JobRecord, JobStatus, OpportunityId};
use jobhunt_sources::{HttpClient, HttpSettings, HttpVerifier, SourceSpec};

fn names(var: &str, default: &[&str]) -> Vec<String> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_owned()).collect(),
        _ => default.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn per_family() -> usize {
    std::env::var("JOBHUNT_LIVE_VERIFY_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
}

fn record(posting: JobPosting) -> JobRecord {
    let now = Utc::now();
    JobRecord {
        id: posting.id(),
        opportunity_id: OpportunityId::founded_by(posting.id()),
        posting,
        first_seen_at: now,
        last_seen_at: now,
        content_updated_at: now,
        status: JobStatus::Open,
        closed_at: None,
    }
}

fn report(record: &JobRecord, o: &SourceObservation) {
    println!("  {} — {}", record.posting.title, record.posting.url);
    println!("    checked {} ({})", o.checked_url, o.method.as_str());
    println!(
        "    application: {} via {}{}",
        o.application.status.as_str(),
        o.application.basis.as_str(),
        o.application
            .detail
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default()
    );
    if let ObservedListing::Found(posting) = &o.listing {
        let job = requirements(&self::record((**posting).clone()));
        for f in describe::location(&job)
            .iter()
            .chain(&describe::employment(&job))
            .chain(&describe::timezone(&job))
        {
            println!("    {}: {}", f.label, f.value);
        }
        for c in &job.conflicts {
            println!("    conflict: {}", c.summary);
        }
        let comp = CompensationCheck::observe(posting.compensation.as_ref(), None);
        println!("    compensation: {:?}", comp.status);
        for r in &comp.ranges {
            println!("      {}", r.describe());
        }
        if comp.ambiguous_currency {
            println!("      (ambiguous currency symbol kept unknown)");
        }
    }
}

async fn family(kind: &str, instances: &[String], missing_id: &str) {
    let http = HttpClient::new(HttpSettings::default()).unwrap();
    let verifier = HttpVerifier::new(http.clone());
    let mut checked = 0;
    let mut active = 0;
    for instance in instances {
        let key: SourceKey = format!("{kind}:{instance}").parse().unwrap();
        let source = SourceSpec::from_key(&key).unwrap().build(&http).unwrap();
        let Fetched::Batch(batch) = source.fetch(&FetchRequest::default()).await.unwrap() else {
            panic!("{key}: unconditional fetch answered not modified");
        };
        println!("{key}: {} postings listed", batch.records.len());
        let mut sample: Vec<JobPosting> = batch.records.into_iter().take(per_family()).collect();
        for posting in sample.drain(..) {
            let record = record(posting);
            let started = Instant::now();
            let o = verifier
                .observe(&record)
                .await
                .unwrap_or_else(|e| panic!("{key}: verifying {}: {e}", record.id));
            checked += 1;
            match &o.listing {
                ObservedListing::Found(p) => {
                    active += 1;
                    assert_eq!(p.id(), record.id, "{key}: the same job");
                    let changed = record.posting.snapshot().changed_fields(&p.snapshot());
                    println!(
                        "  active in {} ms; fields differing from the listing: {:?}",
                        started.elapsed().as_millis(),
                        changed
                    );
                }
                other => println!("  NOT active: {other:?}"),
            }
            report(&record, &o);
        }
        // A job that does not exist: "not found", not an error.
        let mut ghost = record(source_posting(kind, instance, missing_id));
        ghost.posting.provenance.source = key.clone();
        let o = verifier.observe(&ghost).await.unwrap_or_else(|e| {
            panic!("{key}: a missing job must be an answer, not an error: {e}")
        });
        assert!(
            matches!(
                o.listing,
                ObservedListing::NotFound { .. }
                    | ObservedListing::MissingFromCompleteListing { .. }
            ),
            "{key}: a missing job was {:?}",
            o.listing
        );
        println!("  missing job {missing_id}: {:?}", o.listing);
    }
    println!("{kind}: {checked} jobs checked, {active} active");
    assert!(checked > 0, "{kind}: nothing to verify");
    assert_eq!(
        active, checked,
        "{kind}: jobs just listed must verify as active"
    );
}

/// A posting for a job id that does not exist.
fn source_posting(kind: &str, instance: &str, id: &str) -> JobPosting {
    let url = match kind {
        "yc" => format!("https://www.ycombinator.com/companies/{instance}/jobs/{id}-no-such-job"),
        "lever" => format!("https://jobs.lever.co/{instance}/{id}"),
        "ashby" => format!("https://jobs.ashbyhq.com/{instance}/{id}"),
        _ => format!("https://job-boards.greenhouse.io/{instance}/jobs/{id}"),
    };
    JobPosting {
        provenance: jobhunt_core::Provenance {
            source: format!("{kind}:{instance}").parse().unwrap(),
            source_record_id: Some(id.to_owned()),
            fetched_from: None,
        },
        url: jobhunt_core::CanonicalUrl::parse(&url).unwrap(),
        apply_url: None,
        company: instance.to_owned(),
        title: "No such job".into(),
        department: None,
        team: None,
        location: None,
        locations: Vec::new(),
        employment_type: None,
        workplace_type: None,
        is_remote: None,
        compensation: None,
        work_authorization: None,
        description_text: None,
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    }
}

#[tokio::test]
#[ignore = "live: talks to real job boards"]
async fn ashby() {
    family(
        "ashby",
        &names("JOBHUNT_LIVE_VERIFY_ASHBY", &["linear", "posthog"]),
        "00000000-0000-4000-8000-000000000000",
    )
    .await;
}

#[tokio::test]
#[ignore = "live: talks to real job boards"]
async fn greenhouse() {
    family(
        "greenhouse",
        &names("JOBHUNT_LIVE_VERIFY_GREENHOUSE", &["anthropic", "figma"]),
        "1",
    )
    .await;
}

#[tokio::test]
#[ignore = "live: talks to real job boards"]
async fn lever() {
    family(
        "lever",
        &names("JOBHUNT_LIVE_VERIFY_LEVER", &["spotify", "palantir"]),
        "00000000-0000-4000-8000-000000000000",
    )
    .await;
}

#[tokio::test]
#[ignore = "live: talks to real job boards"]
async fn yc() {
    family("yc", &names("JOBHUNT_LIVE_VERIFY_YC", &["posthog"]), "1").await;
}
