//! Greenhouse parsing and conversion against saved real responses.
//!
//! Fixtures under `tests/fixtures/greenhouse/` are trimmed copies of real
//! responses from
//! `boards-api.greenhouse.io/v1/boards/<board>/jobs?content=true&pay_transparency=true`
//! (captured 2026-09-25), chosen to cover the variations seen across boards:
//! Greenhouse-hosted URLs (anthropic), first-party URLs (stripe, airbnb),
//! `gh_jid` duplicated in the URL (figma), workplace metadata under
//! different names, multi-currency pay ranges and non-English postings.
//! `anthropic_with_invalid_records.json` appends deliberately broken copies
//! of a real record.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, RecordErrorReason, SourceBatch, SourceKey};
use jobhunt_jobs::{
    CompensationKind, JobPosting, PayInterval, SourceLocation, WorkplaceType, ats_job_ref,
};
use jobhunt_sources::greenhouse::{BoardContext, parse_board};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/greenhouse/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn context(board: &str, company: Option<&str>) -> BoardContext {
    BoardContext {
        source: SourceKey::new("greenhouse", board).unwrap(),
        board: board.to_owned(),
        company: company.map(str::to_owned),
        fetched_from: Some(
            CanonicalUrl::parse(&format!(
                "https://boards-api.greenhouse.io/v1/boards/{board}/jobs?content=true&pay_transparency=true"
            ))
            .unwrap(),
        ),
    }
}

fn parse(board: &str) -> SourceBatch<JobPosting> {
    parse_board(&fixture(&format!("{board}.json")), &context(board, None)).unwrap()
}

fn find<'a>(batch: &'a SourceBatch<JobPosting>, id: &str) -> &'a JobPosting {
    batch
        .records
        .iter()
        .find(|p| p.provenance.source_record_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("posting {id} not found"))
}

#[test]
fn converts_a_hosted_board_posting() {
    let batch = parse("anthropic");
    assert_eq!(batch.records.len(), 3);
    assert!(batch.rejected.is_empty());
    assert!(batch.complete, "job count matches meta.total");

    let job = find(&batch, "5394958008");
    assert_eq!(job.title, "Business Systems Analyst");
    assert_eq!(job.company, "Anthropic", "company comes from Greenhouse");
    assert_eq!(
        job.url.as_str(),
        "https://job-boards.greenhouse.io/anthropic/jobs/5394958008"
    );
    assert_eq!(job.apply_url, None);
    assert_eq!(job.department.as_deref(), Some("Security"));
    assert_eq!(job.team, None);
    assert_eq!(
        job.location.as_deref(),
        Some(
            "Remote-Friendly (Travel-Required) | San Francisco, CA | Seattle, WA | New York City, NY"
        )
    );
    assert_eq!(
        job.locations,
        vec![SourceLocation {
            name: Some("Remote-Friendly US (Travel Required)".into()),
            ..Default::default()
        }]
    );
    // "Location Type" metadata.
    assert_eq!(job.workplace_type, Some(WorkplaceType::Remote));
    assert_eq!(job.is_remote, None, "Greenhouse has no remote flag");
    assert_eq!(job.employment_type, None, "not published: stays unknown");
    assert_eq!(
        job.posted_at,
        Some(Utc.with_ymd_and_hms(2026, 8, 20, 4, 46, 8).unwrap())
    );
    assert_eq!(
        job.source_updated_at,
        Some(Utc.with_ymd_and_hms(2026, 8, 21, 16, 51, 9).unwrap())
    );

    let comp = job.compensation.as_ref().unwrap();
    assert_eq!(comp.summary, None);
    let salary = &comp.components[0];
    assert_eq!(salary.kind, CompensationKind::Salary);
    assert_eq!(salary.label.as_deref(), Some("Annual Salary"));
    assert_eq!(salary.currency.as_deref(), Some("USD"));
    assert_eq!((salary.min, salary.max), (Some(205_000.0), Some(270_000.0)));
    assert_eq!(salary.interval, Some(PayInterval::Year));
}

#[test]
fn escaped_html_content_becomes_clean_text() {
    let batch = parse("anthropic");
    let job = find(&batch, "5394958008");
    let html = job.description_html.as_deref().unwrap();
    assert!(html.starts_with("<div class=\"content-intro\"><h2><strong>About Anthropic"));
    assert!(
        !html.contains("&lt;"),
        "content is unescaped once into HTML"
    );

    let text = job.description_text.as_deref().unwrap();
    assert!(text.starts_with("About Anthropic\n\nAnthropic’s mission is to create reliable"));
    for markup in ["<", "&nbsp;", "&amp;", "&lt;", "&mdash;"] {
        assert!(
            !text.contains(markup),
            "{markup:?} left in description text"
        );
    }
    assert!(text.contains("\n• "), "lists become bullets");
    assert!(!text.contains("\n\n\n"), "no runs of blank lines");
}

#[test]
fn metadata_variants_and_missing_values() {
    let batch = parse("anthropic");
    // "Location Type": null → unknown.
    let sydney = find(&batch, "5117589008");
    assert_eq!(sydney.workplace_type, None);
    assert_eq!(sydney.compensation, None);
    assert_eq!(
        sydney.locations,
        vec![SourceLocation {
            name: Some("Sydney, Australia".into()),
            locality: Some("Sydney".into()),
            region: Some("New South Wales".into()),
            country: Some("Australia".into()),
        }]
    );
    // "Hybrid (Travel-Required)" → hybrid, three structured offices.
    let nyc = find(&batch, "4461444008");
    assert_eq!(nyc.workplace_type, Some(WorkplaceType::Hybrid));
    let localities: Vec<_> = nyc
        .locations
        .iter()
        .map(|l| l.locality.as_deref())
        .collect();
    assert_eq!(
        localities,
        vec![Some("New York"), Some("San Francisco"), Some("Seattle")]
    );

    // Airbnb names the same field "Workplace Type" and adds unrelated
    // metadata; pay ranges are labeled per country.
    let airbnb = parse("airbnb");
    let london = find(&airbnb, "8184174");
    assert_eq!(london.title, "Account Manager", "trailing space cleaned");
    assert_eq!(london.workplace_type, Some(WorkplaceType::Hybrid));
    let pay = &london.compensation.as_ref().unwrap().components[0];
    assert_eq!(
        pay.label.as_deref(),
        Some("United Kingdom Annual Pay Range")
    );
    assert_eq!(pay.currency.as_deref(), Some("GBP"));
    assert_eq!(pay.interval, Some(PayInterval::Year));

    let us = find(&airbnb, "8231416");
    let pay = &us.compensation.as_ref().unwrap().components[0];
    assert_eq!(pay.label.as_deref(), Some("Pay Range"));
    assert_eq!(pay.interval, None, "\"Pay Range\" names no interval");

    let french = find(&airbnb, "8152132");
    assert_eq!(french.title, "Gestionnaire des réclamations complexes");
    assert_eq!(french.workplace_type, Some(WorkplaceType::Remote));
    assert_eq!(french.locations.len(), 2);
}

#[test]
fn first_party_urls_are_kept_and_identify_the_greenhouse_job() {
    let stripe = parse("stripe");
    let job = find(&stripe, "8172510");
    assert_eq!(job.company, "Stripe");
    assert_eq!(
        job.url.as_str(),
        "https://stripe.com/jobs/search?gh_jid=8172510"
    );
    let ats = ats_job_ref(&job.url).unwrap();
    assert_eq!((ats.system, ats.id.as_str()), ("greenhouse", "8172510"));

    // Raw department names are kept as Stripe writes them.
    assert_eq!(job.department.as_deref(), Some("8611 Security Analytics"));

    // Figma repeats the id in gh_jid; the tracking-free URL is kept.
    let figma = parse("figma");
    let job = find(&figma, "5426468004");
    assert_eq!(
        job.url.as_str(),
        "https://boards.greenhouse.io/figma/jobs/5426468004?gh_jid=5426468004"
    );
    assert_eq!(ats_job_ref(&job.url).unwrap().id, "5426468004");
    assert_eq!(
        job.compensation.as_ref().unwrap().components[0]
            .label
            .as_deref(),
        Some("Annual Base Salary Range")
    );
}

#[test]
fn configured_company_name_wins() {
    let batch = parse_board(
        &fixture("stripe.json"),
        &context("stripe", Some("Stripe, Inc.")),
    )
    .unwrap();
    assert!(batch.records.iter().all(|p| p.company == "Stripe, Inc."));
}

#[test]
fn ids_and_fingerprints_are_stable() {
    let a = parse("figma");
    let b = parse("figma");
    let ids = |batch: &SourceBatch<JobPosting>| {
        batch
            .records
            .iter()
            .map(|p| (p.id(), p.fingerprint(), p.content_fingerprint()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), ids(&b));
    // sha256 over length-prefixed ["jobhunt.job.v1", "greenhouse", "figma",
    // "id", "5426468004"], first 16 bytes.
    assert_eq!(
        find(&a, "5426468004").id().to_string(),
        jobhunt_jobs::JobId::derive_from_record_id(
            &SourceKey::new("greenhouse", "figma").unwrap(),
            "5426468004"
        )
        .to_string()
    );
}

#[test]
fn bad_records_are_rejected_individually() {
    let batch = parse_board(
        &fixture("anthropic_with_invalid_records.json"),
        &context("anthropic", None),
    )
    .unwrap();
    assert_eq!(batch.received(), 7);
    assert_eq!(batch.records.len(), 3, "valid records survive");
    assert_eq!(batch.rejected.len(), 4);
    assert!(batch.complete);

    let reason = |id: &str| {
        &batch
            .rejected
            .iter()
            .find(|e| e.record_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("no rejection for {id}"))
            .reason
    };
    assert!(matches!(
        reason("1"),
        RecordErrorReason::MissingField("title")
    ));
    assert!(matches!(
        reason("2"),
        RecordErrorReason::InvalidUrl {
            field: "absolute_url",
            ..
        }
    ));
    assert!(matches!(reason("abc"), RecordErrorReason::Malformed(_)));
    assert!(matches!(reason("4"), RecordErrorReason::Malformed(_)));
}

#[test]
fn every_fixture_posting_is_valid_canonically() {
    for board in ["anthropic", "stripe", "figma", "airbnb"] {
        let batch = parse(board);
        assert!(batch.complete);
        for posting in &batch.records {
            posting.validate().unwrap();
            assert_eq!(
                posting.provenance.source.to_string(),
                format!("greenhouse:{board}")
            );
            assert!(posting.description_text.is_some());
            assert!(ats_job_ref(&posting.url).is_some(), "{}", posting.url);
        }
    }
}

#[test]
fn count_mismatch_or_missing_meta_is_partial() {
    let ctx = context("x", None);
    let short = br#"{"jobs": [], "meta": {"total": 3}}"#;
    assert!(!parse_board(short, &ctx).unwrap().complete);
    let no_meta = br#"{"jobs": []}"#;
    assert!(!parse_board(no_meta, &ctx).unwrap().complete);
    let empty = br#"{"jobs": [], "meta": {"total": 0}}"#;
    assert!(parse_board(empty, &ctx).unwrap().complete);
    assert!(parse_board(b"{\"status\":404,\"error\":\"Job not found\"}", &ctx).is_err());
    assert!(parse_board(b"<html>", &ctx).is_err());
}
