//! Shared checks for the opt-in live tests.

use std::time::Instant;

use jobhunt_core::{FetchRequest, Fetched, SourceBatch};
use jobhunt_jobs::{JobPosting, JobSource};
use jobhunt_sources::{HttpClient, HttpSettings, SourceSpec};

/// Names from `var` (comma-separated) or the given defaults.
pub fn names(var: &str, defaults: &[&str]) -> Vec<String> {
    match std::env::var(var) {
        Ok(value) if !value.trim().is_empty() => value
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => defaults.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// Fetches every source, prints what happened, and checks the conversion
/// held up: most records convert, and every posting is valid.
pub async fn check(kind: &str, instances: &[String], url_prefix: &str) {
    let http = HttpClient::new(HttpSettings::default()).unwrap();
    let mut totals = (0, 0, 0);
    let started = Instant::now();
    for instance in instances {
        let key = format!("{kind}:{instance}").parse().unwrap();
        let source: Box<JobSource> = SourceSpec::from_key(&key).unwrap().build(&http).unwrap();
        let clock = Instant::now();
        let Fetched::Batch(batch) = source.fetch(&FetchRequest::default()).await.unwrap() else {
            panic!("unconditional fetch answered not modified");
        };
        report(&key.to_string(), &batch, clock.elapsed().as_millis());
        totals.0 += batch.received();
        totals.1 += batch.records.len();
        totals.2 += batch.rejected.len();

        assert!(batch.complete, "{key}: listing not complete");
        assert!(
            batch.rejected.len() * 20 <= batch.received(),
            "{key}: more than 5% of postings were rejected; the payload format may have changed"
        );
        for posting in &batch.records {
            posting.validate().unwrap();
            assert!(
                posting.url.as_str().starts_with(url_prefix) || kind == "greenhouse",
                "{key}: unexpected URL {}",
                posting.url
            );
            assert!(posting.provenance.source_record_id.is_some());
            let text = posting.description_text.as_deref().unwrap_or_default();
            assert!(
                !text.contains("&lt;") && !text.contains("<p>"),
                "{key}: markup in text"
            );
        }
    }
    println!(
        "{kind} total: {} received, {} converted, {} rejected in {} ms",
        totals.0,
        totals.1,
        totals.2,
        started.elapsed().as_millis()
    );
    assert!(totals.1 > 0, "{kind}: no postings at all");
}

fn report(key: &str, batch: &SourceBatch<JobPosting>, ms: u128) {
    let described = batch
        .records
        .iter()
        .filter(|p| p.description_text.is_some())
        .count();
    let paid = batch
        .records
        .iter()
        .filter(|p| p.compensation.is_some())
        .count();
    let workplace = batch
        .records
        .iter()
        .filter(|p| p.workplace_type.is_some())
        .count();
    println!(
        "{key}: {} received, {} converted, {} rejected, {} skipped in {ms} ms \
         (description {described}, compensation {paid}, workplace {workplace})",
        batch.received(),
        batch.records.len(),
        batch.rejected.len(),
        batch.skipped
    );
    for error in batch.rejected.iter().take(5) {
        println!("  rejected: {error}");
    }
}
