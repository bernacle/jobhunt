//! The Greenhouse adapter over HTTP, against a local mock server serving
//! the saved fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::{FetchRequest, Fetched, Source, SourceBatch, SourceError, SourceKey};
use jobhunt_jobs::JobPosting;
use jobhunt_sources::{GreenhouseBoard, GreenhouseSource, HttpClient, HttpSettings};
use wiremock::matchers::{header, header_regex, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/greenhouse/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(HttpSettings {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        max_retries: 1,
        retry_base_delay: Duration::from_millis(1),
        max_per_host: 4,
    })
    .unwrap()
}

fn source(server: &MockServer, board: &str) -> GreenhouseSource {
    GreenhouseSource::with_api_base(
        SourceKey::new("greenhouse", board).unwrap(),
        GreenhouseBoard {
            board: board.to_owned(),
            company: None,
        },
        http(),
        &server.uri(),
    )
    .unwrap()
}

fn board_path(board: &str) -> String {
    format!("/v1/boards/{board}/jobs")
}

async fn fetch(source: &GreenhouseSource) -> Result<SourceBatch<JobPosting>, SourceError> {
    match source.fetch(&FetchRequest::default()).await? {
        Fetched::Batch(batch) => Ok(batch),
        Fetched::NotModified => panic!("unconditional fetch answered not modified"),
    }
}

#[tokio::test]
async fn fetches_and_converts_a_board_with_content_and_pay() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(board_path("anthropic")))
        .and(query_param("content", "true"))
        .and(query_param("pay_transparency", "true"))
        .and(header_regex("user-agent", "^jobhunt/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"abc\"")
                .set_body_bytes(fixture("anthropic.json")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let batch = fetch(&source(&server, "anthropic")).await.unwrap();
    assert_eq!(batch.records.len(), 3);
    assert!(batch.complete);
    assert_eq!(batch.validator.as_deref(), Some("W/\"abc\""));
    let posting = &batch.records[0];
    assert_eq!(posting.company, "Anthropic");
    assert!(
        posting
            .provenance
            .fetched_from
            .as_ref()
            .unwrap()
            .as_str()
            .ends_with("/v1/boards/anthropic/jobs?content=true&pay_transparency=true")
    );
}

#[tokio::test]
async fn unchanged_boards_answer_not_modified() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("figma")))
        .and(header("if-none-match", "W/\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    let fetched = source(&server, "figma")
        .fetch(&FetchRequest {
            validator: Some("W/\"v1\"".into()),
        })
        .await
        .unwrap();
    assert!(matches!(fetched, Fetched::NotModified));
}

#[tokio::test]
async fn a_truncated_board_is_partial() {
    let server = MockServer::start().await;
    let body = String::from_utf8(fixture("stripe.json"))
        .unwrap()
        .replace("\"total\": 3", "\"total\": 690");
    Mock::given(path(board_path("stripe")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    let batch = fetch(&source(&server, "stripe")).await.unwrap();
    assert_eq!(batch.records.len(), 3);
    assert!(!batch.complete, "3 of 690 jobs is not a complete listing");
}

#[tokio::test]
async fn unknown_board_is_reported_as_not_found() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("nope")))
        .respond_with(
            ResponseTemplate::new(404).set_body_string(r#"{"status":404,"error":"Job not found"}"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "nope")).await.unwrap_err();
    assert!(
        matches!(&error, SourceError::NotFound { what, .. } if what == "Greenhouse job board \"nope\""),
        "{error:?}"
    );
}

#[tokio::test]
async fn server_errors_are_retried_then_reported() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("airbnb")))
        .respond_with(ResponseTemplate::new(502))
        .expect(2)
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "airbnb")).await.unwrap_err();
    assert!(
        matches!(error, SourceError::Status { status: 502, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn garbage_body_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("airbnb")))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;
    let error = fetch(&source(&server, "airbnb")).await.unwrap_err();
    assert!(matches!(error, SourceError::Decode { .. }), "{error:?}");
}
