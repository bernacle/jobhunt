//! The Lever adapter over HTTP, against a local mock server serving the
//! saved fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::{FetchRequest, Fetched, Source, SourceBatch, SourceError, SourceKey};
use jobhunt_jobs::JobPosting;
use jobhunt_sources::{HttpClient, HttpSettings, LeverRegion, LeverSite, LeverSource};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/lever/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn source(server: &MockServer, site: &str) -> LeverSource {
    let http = HttpClient::new(HttpSettings {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        max_retries: 0,
        retry_base_delay: Duration::from_millis(1),
        max_per_host: 4,
    })
    .unwrap();
    LeverSource::with_api_base(
        SourceKey::new("lever", site).unwrap(),
        LeverSite {
            site: site.to_owned(),
            company: Some("Spotify".into()),
            region: LeverRegion::Global,
        },
        http,
        &server.uri(),
    )
    .unwrap()
}

async fn fetch(source: &LeverSource) -> Result<SourceBatch<JobPosting>, SourceError> {
    match source.fetch(&FetchRequest::default()).await? {
        Fetched::Batch(batch) => Ok(batch),
        Fetched::NotModified => panic!("unconditional fetch answered not modified"),
    }
}

#[tokio::test]
async fn fetches_and_converts_a_site() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v0/postings/spotify"))
        .and(query_param("mode", "json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"be2a1\"")
                .set_body_bytes(fixture("spotify.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let batch = fetch(&source(&server, "spotify")).await.unwrap();
    assert_eq!(batch.records.len(), 3);
    assert!(batch.complete);
    assert_eq!(batch.validator.as_deref(), Some("W/\"be2a1\""));
    assert!(batch.records.iter().all(|p| p.company == "Spotify"));
}

#[tokio::test]
async fn unchanged_sites_answer_not_modified() {
    let server = MockServer::start().await;
    Mock::given(path("/v0/postings/spotify"))
        .and(header("if-none-match", "W/\"be2a1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    let fetched = source(&server, "spotify")
        .fetch(&FetchRequest {
            validator: Some("W/\"be2a1\"".into()),
        })
        .await
        .unwrap();
    assert!(matches!(fetched, Fetched::NotModified));
}

#[tokio::test]
async fn an_empty_site_is_an_empty_complete_listing() {
    let server = MockServer::start().await;
    Mock::given(path("/v0/postings/spotify"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let batch = fetch(&source(&server, "spotify")).await.unwrap();
    assert_eq!(batch.received(), 0);
    assert!(
        batch.complete,
        "the pipeline, not the adapter, guards against sudden empties"
    );
}

#[tokio::test]
async fn unknown_site_is_reported_as_not_found() {
    let server = MockServer::start().await;
    Mock::given(path("/v0/postings/spotify"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_string(r#"{"ok":false,"error":"Document not found"}"#),
        )
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "spotify")).await.unwrap_err();
    assert!(
        matches!(&error, SourceError::NotFound { what, .. } if what == "Lever site \"spotify\""),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_non_array_body_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(path("/v0/postings/spotify"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "spotify")).await.unwrap_err();
    assert!(matches!(error, SourceError::Decode { .. }), "{error:?}");
}
