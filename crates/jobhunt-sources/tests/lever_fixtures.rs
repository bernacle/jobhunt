//! Lever parsing and conversion against saved real responses.
//!
//! Fixtures under `tests/fixtures/lever/` are trimmed copies of real
//! responses from `api.lever.co/v0/postings/<site>?mode=json` (captured
//! 2026-09-25): spotify (remote/hybrid, free-text commitments, multiple
//! locations), outreach (salary ranges), palantir (internships, on-site,
//! salary text in `additional`). `spotify_with_invalid_records.json`
//! appends deliberately broken copies of a real record.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, RecordErrorReason, SourceBatch, SourceKey};
use jobhunt_jobs::{
    CompensationKind, EmploymentType, JobPosting, PayInterval, SourceLocation, WorkplaceType,
    ats_job_ref,
};
use jobhunt_sources::lever::{SiteContext, parse_site};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/lever/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn context(site: &str, company: &str) -> SiteContext {
    SiteContext {
        source: SourceKey::new("lever", site).unwrap(),
        company: company.to_owned(),
        fetched_from: CanonicalUrl::parse(&format!(
            "https://api.lever.co/v0/postings/{site}?mode=json"
        ))
        .ok(),
    }
}

fn parse(site: &str, company: &str) -> SourceBatch<JobPosting> {
    parse_site(&fixture(&format!("{site}.json")), &context(site, company)).unwrap()
}

fn find<'a>(batch: &'a SourceBatch<JobPosting>, id: &str) -> &'a JobPosting {
    batch
        .records
        .iter()
        .find(|p| p.provenance.source_record_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("posting {id} not found"))
}

#[test]
fn converts_a_posting_with_multiple_locations() {
    let batch = parse("spotify", "Spotify");
    assert_eq!(batch.records.len(), 3);
    assert!(batch.rejected.is_empty());

    let job = find(&batch, "2193db3f-77c5-43b8-b030-8f92c9882bf1");
    assert_eq!(job.title, "Android Engineer - Experience");
    assert_eq!(job.company, "Spotify");
    assert_eq!(
        job.url.as_str(),
        "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1"
    );
    assert_eq!(
        job.apply_url.as_ref().map(CanonicalUrl::as_str),
        Some("https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1/apply")
    );
    assert_eq!(job.department.as_deref(), Some("Engineering"));
    assert_eq!(job.team.as_deref(), Some("Experience"));
    assert_eq!(job.location.as_deref(), Some("London"));
    assert_eq!(
        job.locations,
        vec![
            SourceLocation {
                name: Some("London".into()),
                country: Some("GB".into()),
                ..Default::default()
            },
            SourceLocation {
                name: Some("Stockholm".into()),
                ..Default::default()
            },
        ],
        "the country code belongs to the primary location only"
    );
    assert_eq!(job.workplace_type, Some(WorkplaceType::Hybrid));
    assert_eq!(job.is_remote, None);
    assert_eq!(
        job.employment_type,
        Some(EmploymentType::Other("Permanent".into())),
        "\"Permanent\" does not say full- or part-time"
    );
    assert_eq!(
        job.posted_at,
        Some(Utc.timestamp_millis_opt(1_782_214_185_805).unwrap())
    );
    assert_eq!(job.source_updated_at, None);
    assert_eq!(job.compensation, None);

    let no_commitment = find(&batch, "03437e2a-2d5e-4593-9e97-11271014932e");
    assert_eq!(no_commitment.employment_type, None);
    assert_eq!(no_commitment.workplace_type, Some(WorkplaceType::Remote));
}

#[test]
fn description_sections_are_combined_in_page_order() {
    let batch = parse("spotify", "Spotify");
    let job = find(&batch, "2193db3f-77c5-43b8-b030-8f92c9882bf1");
    let text = job.description_text.as_deref().unwrap();
    let positions: Vec<usize> = ["What You'll Do", "Who You Are", "Where You'll Be"]
        .iter()
        .map(|heading| {
            text.find(&format!("\n{heading}\n"))
                .unwrap_or_else(|| panic!("list heading {heading:?} missing"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "lists keep their order"
    );
    assert!(text.contains("\n• Develop and maintain mobile client components"));
    // The closing "additional" section is part of the description.
    assert!(text.contains("Spotify is an equal opportunity employer"));
    assert!(!text.contains('<'));

    let html = job.description_html.as_deref().unwrap();
    assert!(html.contains("<h3>What You'll Do</h3><ul>"));
}

#[test]
fn salary_ranges_and_commitments() {
    let batch = parse("outreach", "Outreach");
    let job = find(&batch, "5becd4e1-3474-4f36-b5dd-4b2cd0eb1179");
    assert_eq!(job.employment_type, Some(EmploymentType::FullTime));
    assert_eq!(job.workplace_type, Some(WorkplaceType::Remote));
    let comp = job.compensation.as_ref().unwrap();
    let salary = &comp.components[0];
    assert_eq!(salary.kind, CompensationKind::Salary);
    assert_eq!(salary.currency.as_deref(), Some("USD"));
    assert_eq!((salary.min, salary.max), (Some(70_000.0), Some(110_000.0)));
    assert_eq!(salary.interval, Some(PayInterval::Year));
    // The salary description text is kept in the description.
    assert!(
        job.description_text
            .as_deref()
            .unwrap()
            .contains("The annual base salary range for this role is $70,000-$110,000 USD")
    );

    let palantir = parse("palantir", "Palantir");
    let intern = find(&palantir, "774cf5c9-bf6a-4d77-bf60-d50ef1beb1a0");
    assert_eq!(intern.employment_type, Some(EmploymentType::Internship));
    assert_eq!(intern.workplace_type, Some(WorkplaceType::OnSite));
    assert_eq!(intern.department, None, "no department category: unknown");
    assert_eq!(intern.team.as_deref(), Some("Echo"));
}

#[test]
fn urls_identify_the_lever_posting() {
    for (site, company) in [
        ("spotify", "Spotify"),
        ("outreach", "Outreach"),
        ("palantir", "Palantir"),
    ] {
        let batch = parse(site, company);
        for posting in &batch.records {
            posting.validate().unwrap();
            let id = posting.provenance.source_record_id.as_deref().unwrap();
            let listing = ats_job_ref(&posting.url).unwrap();
            let apply = ats_job_ref(posting.apply_url.as_ref().unwrap()).unwrap();
            assert_eq!((listing.system, listing.id.as_str()), ("lever", id));
            assert_eq!(
                listing, apply,
                "listing and apply URLs are the same posting"
            );
            assert_ne!(Some(&posting.url), posting.apply_url.as_ref());
        }
    }
}

#[test]
fn bad_records_are_rejected_individually() {
    let batch = parse_site(
        &fixture("spotify_with_invalid_records.json"),
        &context("spotify", "Spotify"),
    )
    .unwrap();
    assert_eq!(batch.received(), 6);
    assert_eq!(batch.records.len(), 3);
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
        RecordErrorReason::MissingField("text")
    ));
    assert!(matches!(
        reason("00000000-0000-4000-8000-000000000002"),
        RecordErrorReason::InvalidUrl {
            field: "hostedUrl",
            ..
        }
    ));
    assert!(matches!(
        reason("00000000-0000-4000-8000-000000000003"),
        RecordErrorReason::Malformed(_)
    ));
}

#[test]
fn non_listing_payloads_are_decode_errors() {
    let ctx = context("x", "X");
    assert!(parse_site(br#"{"ok":false,"error":"Document not found"}"#, &ctx).is_err());
    assert!(parse_site(b"<html>", &ctx).is_err());
    assert_eq!(parse_site(b"[]", &ctx).unwrap().received(), 0);
}
