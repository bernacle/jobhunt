//! Ashby parsing and conversion against saved real responses.
//!
//! Fixtures under `tests/fixtures/ashby/` are trimmed copies of real
//! responses from `api.ashbyhq.com/posting-api/job-board/<board>` (captured
//! 2026-09-24). `modal_with_invalid_records.json` is the Modal fixture with
//! deliberately broken copies of a real record appended.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, RecordErrorReason, SourceBatch, SourceKey};
use jobhunt_jobs::{
    CompensationKind, EmploymentType, JobPosting, PayInterval, SourceLocation, WorkplaceType,
};
use jobhunt_sources::ashby::{BoardContext, parse_board};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/ashby/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn context(board: &str, company: &str) -> BoardContext {
    BoardContext {
        source: SourceKey::new("ashby", board).unwrap(),
        company: company.to_owned(),
        fetched_from: Some(
            CanonicalUrl::parse(&format!(
                "https://api.ashbyhq.com/posting-api/job-board/{board}?includeCompensation=true"
            ))
            .unwrap(),
        ),
    }
}

fn parse(name: &str, board: &str, company: &str) -> SourceBatch<JobPosting> {
    parse_board(&fixture(name), &context(board, company)).unwrap()
}

fn find<'a>(batch: &'a SourceBatch<JobPosting>, id: &str) -> &'a JobPosting {
    batch
        .records
        .iter()
        .find(|p| p.provenance.source_record_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("posting {id} not found"))
}

#[test]
fn converts_a_remote_linear_posting() {
    let batch = parse("linear.json", "linear", "Linear");
    assert_eq!(batch.records.len(), 3);
    assert!(batch.rejected.is_empty());
    assert_eq!(batch.skipped, 0);

    let job = find(&batch, "d3bc1ced-3ce4-4086-a050-555055dbb1ff");
    assert_eq!(job.title, "Senior / Staff Fullstack Engineer");
    assert_eq!(job.company, "Linear");
    assert_eq!(job.department.as_deref(), Some("Product"));
    assert_eq!(job.team.as_deref(), Some("Engineering"));
    assert_eq!(
        job.url.as_str(),
        "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff"
    );
    assert_eq!(
        job.apply_url.as_ref().map(CanonicalUrl::as_str),
        Some("https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application")
    );
    assert_eq!(job.location.as_deref(), Some("Europe"));
    assert_eq!(
        job.locations,
        vec![SourceLocation {
            name: Some("Europe".into()),
            country: Some("European Union".into()),
            ..Default::default()
        }]
    );
    assert_eq!(job.employment_type, Some(EmploymentType::FullTime));
    assert_eq!(job.workplace_type, Some(WorkplaceType::Remote));
    assert_eq!(job.is_remote, Some(true));
    assert_eq!(
        job.posted_at,
        Some(
            Utc.with_ymd_and_hms(2021, 4, 27, 20, 13, 45).unwrap()
                + chrono::Duration::milliseconds(158)
        )
    );
    assert_eq!(job.source_updated_at, None);
    // Linear does not publish compensation: it must stay unknown.
    assert_eq!(job.compensation, None);
    let text = job.description_text.as_deref().unwrap();
    assert!(text.starts_with("At Linear, we're building the product development system"));
    assert!(text.ends_with("security@linear.app."));
    assert!(job.description_html.as_deref().unwrap().starts_with("<p"));
    assert_eq!(
        job.provenance
            .fetched_from
            .as_ref()
            .map(CanonicalUrl::as_str),
        Some("https://api.ashbyhq.com/posting-api/job-board/linear?includeCompensation=true")
    );
}

#[test]
fn job_ids_are_stable() {
    let batch = parse("linear.json", "linear", "Linear");
    let job = find(&batch, "d3bc1ced-3ce4-4086-a050-555055dbb1ff");
    // Computed independently: sha256 over length-prefixed
    // ["jobhunt.job.v1", "ashby", "linear", "id", <ashby id>], first 16 bytes.
    assert_eq!(job.id().to_string(), "job_02e51190085f8a9a0772e845ddd9f329");

    let again = parse("linear.json", "linear", "Linear");
    let ids =
        |b: &SourceBatch<JobPosting>| b.records.iter().map(JobPosting::id).collect::<Vec<_>>();
    let fingerprints = |b: &SourceBatch<JobPosting>| {
        b.records
            .iter()
            .map(JobPosting::fingerprint)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&batch), ids(&again));
    assert_eq!(fingerprints(&batch), fingerprints(&again));
}

#[test]
fn keeps_secondary_and_structured_locations() {
    let batch = parse("linear.json", "linear", "Linear");
    let design = find(&batch, "f04f398b-6320-499d-8a60-290239d62da8");
    let names: Vec<_> = design.locations.iter().map(|l| l.name.as_deref()).collect();
    assert_eq!(names, vec![Some("North America"), Some("Europe")]);

    let london = find(&batch, "d37b3d76-3080-47f9-8a19-60505573112c");
    assert_eq!(london.workplace_type, Some(WorkplaceType::Hybrid));
    assert_eq!(
        london.locations[0],
        SourceLocation {
            name: Some("London".into()),
            locality: Some("London".into()),
            region: None,
            country: Some("United Kingdom".into()),
        }
    );
}

#[test]
fn converts_multi_tier_compensation() {
    let batch = parse("ramp.json", "ramp", "Ramp");
    assert_eq!(batch.records.len(), 3);

    let job = find(&batch, "b9568fb8-a47e-4738-87fb-a6d88c3a505f");
    let comp = job.compensation.as_ref().unwrap();
    assert_eq!(
        comp.summary.as_deref(),
        Some("$131K – $191K • Offers Equity • Offers Commission • Multiple Ranges")
    );
    let kinds: Vec<_> = comp.components.iter().map(|c| c.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            CompensationKind::Commission,
            CompensationKind::EquityCashValue,
            CompensationKind::Salary
        ]
    );
    let salary = &comp.components[2];
    assert_eq!(salary.currency.as_deref(), Some("USD"));
    assert_eq!((salary.min, salary.max), (Some(131_000.0), Some(191_000.0)));
    assert_eq!(salary.interval, Some(PayInterval::Year));
    // Commission has no amount in the source: stays unknown.
    assert_eq!(
        (
            comp.components[0].min,
            comp.components[0].currency.as_deref()
        ),
        (None, None)
    );

    // Primary plus three secondary locations, with source whitespace cleaned
    // ("California " in the payload) but values otherwise untouched.
    assert_eq!(job.locations.len(), 4);
    assert_eq!(job.locations[0].locality.as_deref(), Some("New York City"));
    assert_eq!(job.locations[1].region.as_deref(), Some("California"));
    assert_eq!(job.locations[1].locality.as_deref(), Some("San Fransisco"));
}

#[test]
fn cleans_titles_and_maps_equity_percentage() {
    let batch = parse("ramp.json", "ramp", "Ramp");
    let job = find(&batch, "34413f8d-26bf-4bbc-8ade-eb309a0e2245");
    // The source title has a leading space.
    assert_eq!(job.title, "Security Engineer, Cloud");
    let comp = job.compensation.as_ref().unwrap();
    assert_eq!(comp.components[0].kind, CompensationKind::EquityPercentage);
    assert_eq!(
        comp.components[0].interval, None,
        "\"NONE\" interval stays unknown"
    );
    assert_eq!(
        (comp.components[1].min, comp.components[1].max),
        (Some(211_400.0), Some(290_600.0))
    );
}

#[test]
fn missing_workplace_data_stays_unknown() {
    let batch = parse("modal.json", "modal", "Modal");
    let job = find(&batch, "9fadb51f-ce11-41b1-84d5-470e66cc8ee9");
    assert_eq!(job.workplace_type, None);
    assert_eq!(job.is_remote, None);
    let onsite = find(&batch, "9b33ebe7-e829-4f03-97ba-5c94dbd7daf6");
    assert_eq!(onsite.workplace_type, Some(WorkplaceType::OnSite));
    assert_eq!(onsite.is_remote, Some(false));
}

#[test]
fn bad_records_are_rejected_individually() {
    let batch = parse("modal_with_invalid_records.json", "modal", "Modal");
    assert_eq!(batch.received(), 6);
    assert_eq!(batch.records.len(), 2, "valid records survive");
    assert_eq!(batch.skipped, 1, "unlisted posting is skipped");
    assert_eq!(batch.rejected.len(), 3);

    let reason = |id: &str| {
        &batch
            .rejected
            .iter()
            .find(|e| e.record_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("no rejection for {id}"))
            .reason
    };
    assert!(matches!(
        reason("00000000-0000-4000-8000-000000000001"),
        RecordErrorReason::MissingField("title")
    ));
    assert!(matches!(
        reason("00000000-0000-4000-8000-000000000002"),
        RecordErrorReason::InvalidUrl {
            field: "jobUrl",
            ..
        }
    ));
    assert!(matches!(
        reason("00000000-0000-4000-8000-000000000003"),
        RecordErrorReason::Malformed(_)
    ));
}

#[test]
fn every_fixture_posting_is_valid_canonically() {
    for (file, board) in [
        ("linear.json", "linear"),
        ("ramp.json", "ramp"),
        ("modal.json", "modal"),
    ] {
        let batch = parse(file, board, board);
        for posting in &batch.records {
            posting.validate().unwrap();
            assert_eq!(
                posting.provenance.source.to_string(),
                format!("ashby:{board}")
            );
            assert!(
                posting
                    .url
                    .as_str()
                    .starts_with("https://jobs.ashbyhq.com/")
            );
        }
    }
}

#[test]
fn a_non_board_payload_is_a_decode_error() {
    let ctx = context("linear", "Linear");
    assert!(parse_board(b"{\"error\":\"nope\"}", &ctx).is_err());
    assert!(parse_board(b"Not Found", &ctx).is_err());
    let empty = parse_board(b"{\"jobs\":[],\"apiVersion\":\"1\"}", &ctx).unwrap();
    assert_eq!(empty.received(), 0);
}
