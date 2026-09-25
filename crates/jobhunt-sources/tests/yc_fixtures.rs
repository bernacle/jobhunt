//! YC / Work at a Startup parsing and conversion against saved real pages.
//!
//! Fixtures under `tests/fixtures/yc/` are real pages from
//! `www.ycombinator.com/companies/<slug>/jobs` and the job pages they link
//! to (captured 2026-09-25), reduced to the element carrying the embedded
//! `data-page` JSON. Personal data the adapter never reads (founders,
//! hiring managers) and presigned asset URLs were blanked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use jobhunt_core::{CanonicalUrl, RecordErrorReason, SourceKey};
use jobhunt_jobs::{
    CompensationKind, EmploymentType, JobPosting, SourceLocation, WorkplaceType, ats_job_ref,
};
use jobhunt_sources::YcCompany;
use jobhunt_sources::yc::{CompanyContext, PageError, page_data, parse_listing, to_posting};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/yc/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn company(slug: &str) -> YcCompany {
    YcCompany {
        slug: slug.to_owned(),
        company: None,
    }
}

fn context(slug: &str, name: &str) -> CompanyContext {
    CompanyContext {
        source: SourceKey::new("yc", slug).unwrap(),
        company: name.to_owned(),
        fetched_from: CanonicalUrl::parse(&format!(
            "https://www.ycombinator.com/companies/{slug}/jobs"
        ))
        .ok(),
    }
}

fn job(slug: &str, name: &str, id: &str) -> JobPosting {
    let page = page_data(&fixture(&format!("{slug}_job_{id}.html"))).unwrap();
    assert_eq!(page.component, "WaasShowJobPage");
    to_posting(page.props["job"].clone(), &context(slug, name)).unwrap()
}

#[test]
fn lists_every_job_of_a_company() {
    let listing = parse_listing(&fixture("posthog_jobs.html"), &company("posthog")).unwrap();
    assert_eq!(listing.company, "PostHog");
    let ids: Vec<&str> = listing.jobs.iter().map(|j| j.id.as_str()).collect();
    assert_eq!(
        ids,
        ["110003", "109574", "108951", "106408", "105450", "104329"]
    );
    assert!(listing.rejected.is_empty());
    assert!(
        listing
            .jobs
            .iter()
            .all(|j| j.path.starts_with("/companies/posthog/jobs/"))
    );

    let named = YcCompany {
        slug: "posthog".into(),
        company: Some("PostHog Inc.".into()),
    };
    assert_eq!(
        parse_listing(&fixture("posthog_jobs.html"), &named)
            .unwrap()
            .company,
        "PostHog Inc."
    );
}

#[test]
fn converts_a_job_with_mixed_location_options() {
    let job = job("pine-park-health", "Pine Park Health", "93346");
    assert_eq!(job.title, "Senior Software Engineer");
    assert_eq!(job.company, "Pine Park Health");
    assert_eq!(
        job.url.as_str(),
        "https://www.ycombinator.com/companies/pine-park-health/jobs/wx8rhlj-senior-software-engineer"
    );
    assert_eq!(
        job.apply_url.as_ref().map(CanonicalUrl::as_str),
        Some("https://www.workatastartup.com/application?signup_job_id=93346")
    );
    let ats = ats_job_ref(job.apply_url.as_ref().unwrap()).unwrap();
    assert_eq!((ats.system, ats.id.as_str()), ("workatastartup", "93346"));

    assert_eq!(job.department.as_deref(), Some("Engineering"));
    assert_eq!(job.employment_type, Some(EmploymentType::FullTime));
    // An office option plus a remote option: remote is allowed, but the
    // job is neither purely remote nor purely on-site.
    assert_eq!(job.workplace_type, None);
    assert_eq!(job.is_remote, Some(true));
    assert_eq!(
        job.locations[0],
        SourceLocation {
            name: Some("Berkeley, CA, US".into()),
            locality: Some("Berkeley".into()),
            region: Some("CA".into()),
            country: Some("US".into()),
        }
    );
    assert_eq!(job.locations.len(), 2);

    let comp = job.compensation.as_ref().unwrap();
    assert_eq!(comp.summary.as_deref(), Some("$190K - $215K"));
    assert_eq!(comp.components[0].kind, CompensationKind::Salary);
    assert_eq!(comp.components[0].currency.as_deref(), Some("USD"));
    assert_eq!(
        (comp.components[0].min, comp.components[0].max),
        (Some(190_000.0), Some(215_000.0))
    );
    assert_eq!(comp.components[0].interval, None, "YC does not state it");
    assert_eq!(job.posted_at, None, "only relative dates are published");

    let text = job.description_text.as_deref().unwrap();
    assert!(text.starts_with("About the Role\n\nPine Park Health brings doctors and nurses"));
    assert!(!text.contains("**"));
    assert!(!text.contains("###"));
    assert_eq!(job.description_html, None);
}

#[test]
fn remote_and_hybrid_options() {
    let remote = job("posthog", "PostHog", "109574");
    assert_eq!(remote.title, "Security Engineer", "trailing space cleaned");
    assert_eq!(remote.location.as_deref(), Some("Remote (US)"));
    assert_eq!(remote.workplace_type, Some(WorkplaceType::Remote));
    assert_eq!(remote.is_remote, Some(true));
    assert_eq!(remote.compensation, None, "no salary published");

    let mixed = job("posthog", "PostHog", "105450");
    assert_eq!(mixed.location.as_deref(), Some("Hybrid (UK) / Remote (US)"));
    assert_eq!(mixed.workplace_type, None);
    assert_eq!(mixed.is_remote, Some(true));

    let onsite = job("pine-park-health", "Pine Park Health", "93347");
    assert_eq!(
        onsite.workplace_type, None,
        "no option says remote or hybrid"
    );
    assert_eq!(onsite.is_remote, None);
    assert_eq!(onsite.locations.len(), 6);
}

#[test]
fn every_fixture_job_is_valid_and_stable() {
    for (slug, name, ids) in [
        (
            "posthog",
            "PostHog",
            &["110003", "109574", "108951", "106408", "105450", "104329"][..],
        ),
        (
            "pine-park-health",
            "Pine Park Health",
            &["93347", "93346"][..],
        ),
    ] {
        for id in ids {
            let a = job(slug, name, id);
            a.validate().unwrap();
            assert_eq!(a.provenance.source_record_id.as_deref(), Some(*id));
            assert!(a.description_text.is_some());
            let b = job(slug, name, id);
            assert_eq!((a.id(), a.fingerprint()), (b.id(), b.fingerprint()));
        }
    }
}

#[test]
fn malformed_jobs_and_pages() {
    let ctx = context("posthog", "PostHog");
    let no_title = serde_json::json!({"id": 1, "url": "/companies/posthog/jobs/x-y"});
    assert!(matches!(
        to_posting(no_title, &ctx).unwrap_err().reason,
        RecordErrorReason::MissingField("title")
    ));
    let no_id = serde_json::json!({"title": "Engineer"});
    let err = to_posting(no_id, &ctx).unwrap_err();
    assert_eq!(err.record_id, None);
    let wrong_types = serde_json::json!({"id": 2, "title": ["x"]});
    assert!(matches!(
        to_posting(wrong_types, &ctx).unwrap_err().reason,
        RecordErrorReason::Malformed(_)
    ));

    // A listing entry pointing outside the company is rejected, not fetched.
    let html = std::str::from_utf8(&fixture("pine-park-health_jobs.html"))
        .unwrap()
        .replacen(
            "/companies/pine-park-health/jobs/EnkhUNH",
            "/companies/elsewhere/jobs/EnkhUNH",
            1,
        );
    let listing = parse_listing(html.as_bytes(), &company("pine-park-health")).unwrap();
    assert_eq!(listing.jobs.len(), 1);
    assert_eq!(listing.rejected.len(), 1);
    assert_eq!(listing.rejected[0].record_id.as_deref(), Some("93347"));

    // A job page is not a company jobs page.
    assert!(matches!(
        parse_listing(&fixture("posthog_job_109574.html"), &company("posthog")),
        Err(PageError::UnexpectedPage(_))
    ));
    assert!(matches!(
        parse_listing(b"<html>maintenance</html>", &company("posthog")),
        Err(PageError::MissingData)
    ));
}
