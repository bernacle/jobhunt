//! Loads the saved real responses of every adapter
//! (`crates/jobhunt-sources/tests/fixtures/`) as stored job records.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, SourceKey};
use jobhunt_jobs::{JobPosting, JobRecord, JobStatus, OpportunityId};
use jobhunt_sources::yc::{CompanyContext, page_data, to_posting};
use jobhunt_sources::{ashby, greenhouse, lever};

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
}

fn fixture(family: &str, name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../jobhunt-sources/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

pub fn record(posting: JobPosting, seen: DateTime<Utc>) -> JobRecord {
    JobRecord {
        id: posting.id(),
        opportunity_id: OpportunityId::founded_by(posting.id()),
        posting,
        first_seen_at: seen,
        last_seen_at: seen,
        content_updated_at: seen,
        status: JobStatus::Open,
        closed_at: None,
    }
}

fn ashby(board: &str, company: &str) -> Vec<JobPosting> {
    let context = ashby::BoardContext {
        source: SourceKey::new("ashby", board).unwrap(),
        company: company.to_owned(),
        fetched_from: None,
    };
    ashby::parse_board(&fixture("ashby", &format!("{board}.json")), &context)
        .unwrap()
        .records
}

fn greenhouse(board: &str) -> Vec<JobPosting> {
    let context = greenhouse::BoardContext {
        source: SourceKey::new("greenhouse", board).unwrap(),
        board: board.to_owned(),
        company: None,
        fetched_from: None,
    };
    greenhouse::parse_board(&fixture("greenhouse", &format!("{board}.json")), &context)
        .unwrap()
        .records
}

fn lever(site: &str, company: &str) -> Vec<JobPosting> {
    let context = lever::SiteContext {
        source: SourceKey::new("lever", site).unwrap(),
        company: company.to_owned(),
        fetched_from: None,
    };
    lever::parse_site(&fixture("lever", &format!("{site}.json")), &context)
        .unwrap()
        .records
}

fn yc(slug: &str, company: &str, ids: &[&str]) -> Vec<JobPosting> {
    let context = CompanyContext {
        source: SourceKey::new("yc", slug).unwrap(),
        company: company.to_owned(),
        fetched_from: CanonicalUrl::parse(&format!(
            "https://www.ycombinator.com/companies/{slug}/jobs"
        ))
        .ok(),
    };
    ids.iter()
        .map(|id| {
            let page = page_data(&fixture("yc", &format!("{slug}_job_{id}.html"))).unwrap();
            to_posting(page.props["job"].clone(), &context).unwrap()
        })
        .collect()
}

/// Every fixture posting, as records last seen at [`now`].
pub fn all() -> Vec<JobRecord> {
    let mut postings = Vec::new();
    postings.extend(ashby("linear", "Linear"));
    postings.extend(ashby("modal", "Modal"));
    postings.extend(ashby("ramp", "Ramp"));
    for board in ["stripe", "airbnb", "anthropic", "figma"] {
        postings.extend(greenhouse(board));
    }
    postings.extend(lever("palantir", "Palantir"));
    postings.extend(lever("spotify", "Spotify"));
    postings.extend(lever("outreach", "Outreach"));
    postings.extend(yc(
        "posthog",
        "PostHog",
        &["104329", "105450", "106408", "108951", "109574", "110003"],
    ));
    postings.extend(yc(
        "pine-park-health",
        "Pine Park Health",
        &["93346", "93347"],
    ));
    postings.into_iter().map(|p| record(p, now())).collect()
}

/// The fixture record with this title (and company, when given).
pub fn find(title: &str) -> JobRecord {
    all()
        .into_iter()
        .find(|r| r.posting.title == title)
        .unwrap_or_else(|| panic!("no fixture job titled {title:?}"))
}
