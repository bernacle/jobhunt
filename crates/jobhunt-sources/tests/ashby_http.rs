//! The Ashby adapter end to end over HTTP, against a local mock server that
//! serves the saved fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::{FetchRequest, Fetched, Source, SourceBatch, SourceError, SourceKey};
use jobhunt_jobs::JobPosting;
use jobhunt_sources::{AshbyBoard, AshbySource, HttpClient, HttpSettings};
use wiremock::matchers::{header_regex, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/ashby/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(HttpSettings {
        timeout: Duration::from_secs(5),
        connect_timeout: Duration::from_secs(5),
        max_retries: 2,
        retry_base_delay: Duration::from_millis(1),
        max_per_host: 4,
    })
    .unwrap()
}

fn source(server: &MockServer, board: &str) -> AshbySource {
    AshbySource::with_api_base(
        SourceKey::new("ashby", board).unwrap(),
        AshbyBoard {
            board: board.to_owned(),
            company: Some("Linear".into()),
        },
        http(),
        &server.uri(),
    )
    .unwrap()
}

async fn fetch(source: &AshbySource) -> Result<SourceBatch<JobPosting>, SourceError> {
    match source.fetch(&FetchRequest::default()).await? {
        Fetched::Batch(batch) => Ok(batch),
        Fetched::NotModified => panic!("unconditional fetch answered not modified"),
    }
}

fn board_path(board: &str) -> String {
    format!("/posting-api/job-board/{board}")
}

#[tokio::test]
async fn fetches_and_converts_a_board() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(board_path("linear")))
        .and(query_param("includeCompensation", "true"))
        .and(header_regex("user-agent", "^jobhunt/"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("linear.json")))
        .expect(1)
        .mount(&server)
        .await;

    let batch = fetch(&source(&server, "linear")).await.unwrap();
    assert_eq!(batch.records.len(), 3);
    assert!(batch.complete, "one response is the whole board");
    assert!(batch.records.iter().all(|p| p.company == "Linear"));
    let fetched_from = batch.records[0].provenance.fetched_from.as_ref().unwrap();
    assert!(
        fetched_from
            .as_str()
            .ends_with("/posting-api/job-board/linear?includeCompensation=true")
    );
}

#[tokio::test]
async fn unknown_board_is_reported_as_not_found() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("nope")))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .expect(1)
        .mount(&server)
        .await;

    let error = fetch(&source(&server, "nope")).await.unwrap_err();
    assert!(
        matches!(&error, SourceError::NotFound { what, .. } if what == "Ashby job board \"nope\""),
        "{error:?}"
    );
}

#[tokio::test]
async fn retries_transient_failures() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("linear")))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path(board_path("linear")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("linear.json")))
        .expect(1)
        .mount(&server)
        .await;

    let batch = fetch(&source(&server, "linear")).await.unwrap();
    assert_eq!(batch.records.len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("linear")))
        .respond_with(ResponseTemplate::new(500))
        .expect(3)
        .mount(&server)
        .await;

    let error = fetch(&source(&server, "linear")).await.unwrap_err();
    assert!(
        matches!(error, SourceError::Status { status: 500, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("linear")))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;

    let error = fetch(&source(&server, "linear")).await.unwrap_err();
    assert!(
        matches!(error, SourceError::Status { status: 403, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn garbage_body_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("linear")))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;

    let error = fetch(&source(&server, "linear")).await.unwrap_err();
    assert!(matches!(error, SourceError::Decode { .. }), "{error:?}");
}

#[tokio::test]
async fn unchanged_boards_answer_not_modified() {
    let server = MockServer::start().await;
    Mock::given(path(board_path("linear")))
        .and(wiremock::matchers::header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(board_path("linear")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_bytes(fixture("linear.json")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let source = source(&server, "linear");
    let batch = fetch(&source).await.unwrap();
    assert_eq!(batch.validator.as_deref(), Some("\"v1\""));
    let again = source
        .fetch(&FetchRequest {
            validator: batch.validator.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(again, Fetched::NotModified));
}
