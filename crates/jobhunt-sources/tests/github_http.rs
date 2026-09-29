//! The GitHub reader against a local server speaking GitHub's REST API
//! (payloads in GitHub's documented shape; the account is fictional).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use chrono::{TimeZone, Utc};
use jobhunt_sources::github::parse_login;
use jobhunt_sources::{GithubError, GithubReader, HttpClient, HttpSettings};
use serde_json::{Value, json};
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn repo(id: u64, name: &str, owner: &str, language: &str, pushed: &str) -> Value {
    json!({
        "id": id,
        "node_id": "R_x",
        "name": name,
        "full_name": format!("{owner}/{name}"),
        "private": false,
        "owner": { "login": owner, "id": 1, "type": "User" },
        "html_url": format!("https://github.com/{owner}/{name}"),
        "description": format!("{name}, a fictional project"),
        "fork": false,
        "url": format!("https://api.github.com/repos/{owner}/{name}"),
        "created_at": "2022-01-10T10:00:00Z",
        "updated_at": pushed,
        "pushed_at": pushed,
        "size": 120,
        "stargazers_count": 3,
        "watchers_count": 3,
        "language": language,
        "forks_count": 0,
        "archived": false,
        "disabled": false,
        "is_template": false,
        "topics": ["cli"],
        "visibility": "public",
        "default_branch": "main"
    })
}

async fn mount_user(server: &MockServer, kind: &str) {
    Mock::given(method("GET"))
        .and(path("/users/rileyx"))
        .and(header("accept", "application/vnd.github+json"))
        .and(header("x-github-api-version", "2022-11-28"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "login": "rileyx",
            "id": 1,
            "html_url": "https://github.com/rileyx",
            "type": kind,
            "name": "Riley Example",
            "email": "SENTINEL-EMAIL@example.com",
            "public_repos": 4,
            "created_at": "2015-03-01T00:00:00Z"
        })))
        .mount(server)
        .await;
}

async fn mount_account(server: &MockServer) {
    mount_user(server, "User").await;
    let mut fork = repo(3, "tokio", "rileyx", "Rust", "2026-08-01T00:00:00Z");
    fork["fork"] = json!(true);
    // Page 1 links to page 2.
    Mock::given(method("GET"))
        .and(path("/users/rileyx/repos"))
        .and(query_param("type", "owner"))
        .and(query_param("per_page", "100"))
        .and(|req: &wiremock::Request| !req.url.query().unwrap_or("").contains("page=2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "link",
                    format!(
                        "<{}/users/rileyx/repos?type=owner&per_page=100&page=2>; rel=\"next\"",
                        server.uri()
                    )
                    .as_str(),
                )
                .set_body_json(json!([
                    repo(1, "lox", "rileyx", "Rust", "2026-08-01T00:00:00Z"),
                    fork,
                ])),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/rileyx/repos"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([repo(
            2,
            "ledger",
            "rileyx",
            "Go",
            "2025-01-01T00:00:00Z"
        )])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/rileyx/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "login": "northwind-labs", "id": 9 }
        ])))
        .mount(server)
        .await;
}

async fn mount_languages(server: &MockServer, name: &str, body: Value) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/rileyx/{name}/languages")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn reads_a_public_account_with_pagination_and_only_needed_statistics() {
    let server = MockServer::start().await;
    mount_account(&server).await;
    mount_languages(&server, "lox", json!({"Rust": 9000, "Shell": 1000})).await;
    mount_languages(&server, "ledger", json!({"Go": 5000})).await;
    // A fork is not evidence: its statistics are never asked for.
    Mock::given(path("/repos/rileyx/tokio/languages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&server)
        .await;
    // No token: no Authorization header.
    Mock::given(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let now = Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let snapshot = reader
        .snapshot("https://github.com/rileyx", now, true)
        .await
        .unwrap();
    assert_eq!(snapshot.account.login, "rileyx");
    assert_eq!(snapshot.account.public_repos, 4);
    assert_eq!(snapshot.fetched_at, now);
    let names: Vec<&str> = snapshot
        .repos
        .iter()
        .map(|r| r.full_name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["rileyx/lox", "rileyx/tokio", "rileyx/ledger"],
        "both pages"
    );
    let lox = &snapshot.repos[0];
    assert_eq!(
        lox.languages,
        Some(vec![("Rust".into(), 9000), ("Shell".into(), 1000)])
    );
    assert!(snapshot.repos[1].fork && snapshot.repos[1].languages.is_none());
    assert_eq!(snapshot.orgs, vec!["northwind-labs"]);
    assert!(snapshot.problems.is_empty(), "{:?}", snapshot.problems);
    // Only what is used is kept: no e-mail address, no name.
    let debug = format!("{snapshot:?}");
    assert!(!debug.contains("SENTINEL") && !debug.contains("Riley Example"));
}

#[tokio::test]
async fn without_statistics_an_import_is_three_requests() {
    let server = MockServer::start().await;
    mount_account(&server).await;
    Mock::given(path("/repos/rileyx/lox/languages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Rust": 1})))
        .expect(0)
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let snapshot = reader.snapshot("rileyx", Utc::now(), false).await.unwrap();
    assert!(snapshot.repos.iter().all(|r| r.languages.is_none()));
    assert!(snapshot.problems.is_empty());
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        4,
        "user, two repository pages, organizations"
    );
}

#[tokio::test]
async fn a_token_is_sent_only_as_authorization() {
    let server = MockServer::start().await;
    Mock::given(header("authorization", "Bearer test-token-123"))
        .and(path("/users/rileyx"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), Some("test-token-123")).unwrap();
    let err = reader
        .snapshot("rileyx", Utc::now(), true)
        .await
        .unwrap_err();
    assert!(matches!(err, GithubError::BadToken), "{err}");
    assert!(!err.to_string().contains("test-token-123"));
    assert!(
        !format!("{reader:?}").contains("test-token-123"),
        "never in debug output"
    );
}

#[tokio::test]
async fn unknown_accounts_organizations_and_bad_input_are_named() {
    let server = MockServer::start().await;
    Mock::given(path("/users/nobody-here"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let err = reader
        .snapshot("nobody-here", Utc::now(), true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, GithubError::NotFound(ref l) if l == "nobody-here"),
        "{err}"
    );

    let server = MockServer::start().await;
    mount_user(&server, "Organization").await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let err = reader
        .snapshot("rileyx", Utc::now(), true)
        .await
        .unwrap_err();
    assert!(matches!(err, GithubError::Organization(_)), "{err}");

    assert!(matches!(
        reader.snapshot("../../admin", Utc::now(), true).await,
        Err(GithubError::InvalidLogin(_))
    ));
    assert_eq!(parse_login("@rileyx").unwrap(), "rileyx");
}

#[tokio::test]
async fn the_rate_limit_fails_clearly_with_its_reset_time() {
    let server = MockServer::start().await;
    Mock::given(path("/users/rileyx"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", "1790000000")
                .set_body_json(json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let err = reader
        .snapshot("rileyx", Utc::now(), true)
        .await
        .unwrap_err();
    match &err {
        GithubError::RateLimited { reset } => assert!(reset.is_some()),
        other => panic!("expected a rate limit, got {other}"),
    }
    assert!(err.to_string().contains("GITHUB_TOKEN"), "{err}");
}

#[tokio::test]
async fn a_repository_list_that_cannot_be_read_whole_fails_the_import() {
    let server = MockServer::start().await;
    mount_user(&server, "User").await;
    Mock::given(path("/users/rileyx/repos"))
        .and(|req: &wiremock::Request| !req.url.query().unwrap_or("").contains("page=2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "link",
                    format!("<{}/users/rileyx/repos?page=2>; rel=\"next\"", server.uri()).as_str(),
                )
                .set_body_json(json!([repo(
                    1,
                    "lox",
                    "rileyx",
                    "Rust",
                    "2026-08-01T00:00:00Z"
                )])),
        )
        .mount(&server)
        .await;
    Mock::given(path("/users/rileyx/repos"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let err = reader
        .snapshot("rileyx", Utc::now(), true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, GithubError::Unavailable { ref what, .. } if what == "the repository list"),
        "{err}"
    );
}

#[tokio::test]
async fn one_failing_repository_does_not_stop_the_import() {
    let server = MockServer::start().await;
    mount_account(&server).await;
    mount_languages(&server, "lox", json!({"Rust": 9000})).await;
    Mock::given(path("/repos/rileyx/ledger/languages"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&server)
        .await;
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let snapshot = reader.snapshot("rileyx", Utc::now(), true).await.unwrap();
    let ledger = snapshot.repos.iter().find(|r| r.name == "ledger").unwrap();
    assert_eq!(ledger.languages, None);
    assert_eq!(
        ledger.language.as_deref(),
        Some("Go"),
        "the primary language stays"
    );
    assert_eq!(
        snapshot.problems,
        vec!["languages of rileyx/ledger unavailable (HTTP 502); used its primary language"]
    );

    // Rate-limited statistics are reported once, and the import goes on.
    let server = MockServer::start().await;
    mount_account(&server).await;
    Mock::given(path("/repos/rileyx/lox/languages"))
        .respond_with(ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "0"))
        .mount(&server)
        .await;
    Mock::given(path("/repos/rileyx/ledger/languages"))
        .respond_with(ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "0"))
        .mount(&server)
        .await;
    // And the organizations failing is only a problem, too.
    let reader = GithubReader::new(http(), &server.uri(), None).unwrap();
    let snapshot = reader.snapshot("rileyx", Utc::now(), true).await.unwrap();
    assert_eq!(snapshot.repos.len(), 3);
    assert!(snapshot.problems.iter().any(|p| p
        == "languages of 2 repositories unavailable (rate limit); used their primary language"));
}
