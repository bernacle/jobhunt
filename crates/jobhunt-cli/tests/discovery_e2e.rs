//! The whole pipeline wired the way `jobhunt find` wires it: Ashby adapter →
//! discovery → SQLite file, against a local mock server serving saved real
//! responses.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::SourceKey;
use jobhunt_jobs::{Discovery, JobQuery, JobRepository, JobSource};
use jobhunt_sources::{AshbyBoard, AshbySource, HttpClient, HttpSettings};
use jobhunt_storage::SqliteJobStore;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../jobhunt-sources/tests/fixtures/ashby/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

async fn serve(server: &MockServer, board: &str, body: String) {
    Mock::given(path(format!("/posting-api/job-board/{board}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(server)
        .await;
}

fn sources(server: &MockServer, boards: &[(&str, &str)]) -> Vec<Box<JobSource>> {
    let http = HttpClient::new(HttpSettings {
        max_retries: 0,
        timeout: Duration::from_secs(5),
        ..HttpSettings::default()
    })
    .unwrap();
    boards
        .iter()
        .map(|(board, company)| {
            Box::new(
                AshbySource::with_api_base(
                    SourceKey::new("ashby", board).unwrap(),
                    AshbyBoard {
                        board: (*board).to_owned(),
                        company: Some((*company).to_owned()),
                    },
                    http.clone(),
                    &server.uri(),
                )
                .unwrap(),
            ) as Box<JobSource>
        })
        .collect()
}

#[tokio::test]
async fn repeated_discovery_is_idempotent_and_tracks_changes() {
    let server = MockServer::start().await;
    serve(&server, "linear", fixture("linear.json")).await;
    let ramp_v1 = fixture("ramp.json");
    serve(&server, "ramp", ramp_v1.clone()).await;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("jobhunt.db");
    let store = SqliteJobStore::open(&db).await.unwrap();
    let sources = sources(&server, &[("linear", "Linear"), ("ramp", "Ramp")]);

    // First run inserts everything.
    let first = Discovery::new(&store).run(&sources).await.unwrap();
    let totals = first.totals();
    assert_eq!(totals.received, 6);
    assert_eq!(totals.inserted, 6);
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 6);

    // Second run over the same data: nothing new, nothing duplicated.
    let second = Discovery::new(&store).run(&sources).await.unwrap();
    let totals = second.totals();
    assert_eq!(
        (totals.inserted, totals.updated, totals.unchanged),
        (0, 0, 6)
    );
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 6);
    let seen_now = JobQuery {
        seen_since: Some(second.started_at),
        ..JobQuery::default()
    };
    assert_eq!(store.count(&seen_now).await.unwrap(), 6);

    // The source edits one posting: exactly one update, still six rows.
    server.reset().await;
    serve(&server, "linear", fixture("linear.json")).await;
    serve(
        &server,
        "ramp",
        ramp_v1.replace(
            " Security Engineer, Cloud",
            "Staff Security Engineer, Cloud",
        ),
    )
    .await;
    let third = Discovery::new(&store).run(&sources).await.unwrap();
    let totals = third.totals();
    assert_eq!(
        (totals.inserted, totals.updated, totals.unchanged),
        (0, 1, 5)
    );
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 6);

    let found = store
        .search(&JobQuery::default().with_text("staff security"))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    let record = &found[0];
    assert_eq!(record.posting.company, "Ramp");
    assert!(record.content_updated_at > record.first_seen_at);

    // Data survives reopening the database file.
    store.close().await;
    let reopened = SqliteJobStore::open(&db).await.unwrap();
    assert_eq!(reopened.count(&JobQuery::default()).await.unwrap(), 6);
}

#[tokio::test]
async fn bad_records_and_failing_sources_do_not_stop_discovery() {
    let server = MockServer::start().await;
    serve(&server, "modal", fixture("modal_with_invalid_records.json")).await;
    Mock::given(path("/posting-api/job-board/gone"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let sources = sources(&server, &[("gone", "Gone"), ("modal", "Modal")]);
    let report = Discovery::new(&store).run(&sources).await.unwrap();

    assert_eq!(report.succeeded(), 1);
    let failures: Vec<String> = report.failures().map(|(key, _)| key.to_string()).collect();
    assert_eq!(failures, ["ashby:gone"]);

    let totals = report.totals();
    assert_eq!(totals.received, 6);
    assert_eq!(totals.inserted, 2);
    assert_eq!(totals.rejected, 3);
    assert_eq!(totals.skipped, 1);
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 2);
}
