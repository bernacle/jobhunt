//! The YC adapter over HTTP: listing page plus detail pages, against a local
//! mock server serving the saved pages.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::{
    FetchRequest, Fetched, RecordErrorReason, Source, SourceBatch, SourceError, SourceKey,
};
use jobhunt_jobs::JobPosting;
use jobhunt_sources::yc::{ListingStub, parse_listing};
use jobhunt_sources::{HttpClient, HttpSettings, YcCompany, YcSource};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/yc/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn company(slug: &str) -> YcCompany {
    YcCompany {
        slug: slug.to_owned(),
        company: None,
    }
}

fn source(server: &MockServer, slug: &str) -> YcSource {
    let http = HttpClient::new(HttpSettings {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base_delay: Duration::from_millis(1),
        max_per_host: 4,
    })
    .unwrap();
    YcSource::with_base(
        SourceKey::new("yc", slug).unwrap(),
        company(slug),
        http,
        &server.uri(),
    )
    .unwrap()
}

fn stubs(slug: &str) -> Vec<ListingStub> {
    parse_listing(&fixture(&format!("{slug}_jobs.html")), &company(slug))
        .unwrap()
        .jobs
}

/// Serves the listing and the detail page of every listed job except
/// `missing`.
async fn serve(server: &MockServer, slug: &str, missing: &[&str]) {
    Mock::given(method("GET"))
        .and(path(format!("/companies/{slug}/jobs")))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture(&format!("{slug}_jobs.html"))),
        )
        .expect(1)
        .mount(server)
        .await;
    for stub in stubs(slug) {
        let response = if missing.contains(&stub.id.as_str()) {
            ResponseTemplate::new(500)
        } else {
            ResponseTemplate::new(200)
                .set_body_bytes(fixture(&format!("{slug}_job_{}.html", stub.id)))
        };
        Mock::given(path(stub.path.clone()))
            .respond_with(response)
            .expect(1)
            .mount(server)
            .await;
    }
}

async fn fetch(source: &YcSource) -> Result<SourceBatch<JobPosting>, SourceError> {
    match source.fetch(&FetchRequest::default()).await? {
        Fetched::Batch(batch) => Ok(batch),
        Fetched::NotModified => panic!("YC never answers not modified"),
    }
}

#[tokio::test]
async fn fetches_the_listing_and_every_detail_page() {
    let server = MockServer::start().await;
    serve(&server, "posthog", &[]).await;
    let batch = fetch(&source(&server, "posthog")).await.unwrap();
    assert!(batch.complete);
    assert!(batch.rejected.is_empty(), "{:?}", batch.rejected);
    let ids: Vec<_> = batch
        .records
        .iter()
        .map(|p| p.provenance.source_record_id.clone().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["110003", "109574", "108951", "106408", "105450", "104329"]
    );
    assert!(batch.records.iter().all(|p| p.company == "PostHog"));
    assert!(batch.records.iter().all(|p| p.description_text.is_some()));
    // Canonical URLs point at the real site, whatever host served the page.
    assert!(
        batch.records[0]
            .url
            .as_str()
            .starts_with("https://www.ycombinator.com/companies/posthog/jobs/")
    );
}

#[tokio::test]
async fn a_failed_detail_page_makes_that_job_unavailable_not_missing() {
    let server = MockServer::start().await;
    serve(&server, "pine-park-health", &["93347"]).await;
    let batch = fetch(&source(&server, "pine-park-health")).await.unwrap();
    assert!(batch.complete, "the listing itself was complete");
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.rejected.len(), 1);
    let rejected = &batch.rejected[0];
    assert_eq!(
        rejected.record_id.as_deref(),
        Some("93347"),
        "identifiable, so never closed"
    );
    assert!(matches!(rejected.reason, RecordErrorReason::Unavailable(_)));
}

#[tokio::test]
async fn unknown_company_is_reported_as_not_found() {
    let server = MockServer::start().await;
    Mock::given(path("/companies/nope/jobs"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "nope")).await.unwrap_err();
    assert!(
        matches!(&error, SourceError::NotFound { what, .. } if what == "YC company \"nope\""),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_page_without_data_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(path("/companies/posthog/jobs"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>Just a moment...</html>"))
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "posthog")).await.unwrap_err();
    assert!(matches!(error, SourceError::Decode { .. }), "{error:?}");
}
