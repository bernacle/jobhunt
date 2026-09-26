//! JobHunt Cloud over real HTTP: a server on a local port, a real
//! Postgres, a mock OpenID provider serving a test signing key, and the
//! official MCP client for the hosted MCP endpoint.
//!
//! Needs `JOBHUNT_TEST_DATABASE_URL` (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_app::context::ApplicationContext;
use jobhunt_app::feedback::{FeedbackResult, PipelineView};
use jobhunt_app::inspect::{JobDetail, VerificationReport};
use jobhunt_app::preferences::PreferenceUpdateResult;
use jobhunt_app::profile_view::ProfileView;
use jobhunt_app::state::StateExport;
use jobhunt_app::{App, AppConfig, LoadedConfig, Remote, SearchResults};
use jobhunt_cloud::api::types::{AccountView, CreatedToken, ErrorBody, Ready, TokenList};
use jobhunt_cloud::api::{ApiState, router};
use jobhunt_cloud::client::CloudClient;
use jobhunt_cloud::config::CloudConfig;
use jobhunt_cloud::usage::UsageLog;
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::{JobPosting, JobRepository, ScanBody, ScanWrite, WorkplaceType};
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};
use jobhunt_storage::SqliteJobStore;
use jobhunt_storage::postgres::testing::TestDatabase;
use jobhunt_storage::postgres::{Keyring, PgSettings, PgStore};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::StatusCode;
use serde_json::{Value, json};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const DEV_SECRET: &str = "0123456789abcdef0123456789abcdef";
const AUDIENCE: &str = "https://api.jobhunt.test";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Auth {
    Dev,
    /// OpenID Connect discovery; the issuer ends with a slash (Auth0).
    Oidc,
    /// RFC 8414 metadata only; the issuer has no trailing slash, and MCP
    /// tokens carry the `/mcp` resource as audience (WorkOS AuthKit).
    Rfc8414,
    /// No metadata at all: the JWKS and endpoints are set by hand.
    Manual,
}

struct Server {
    base: String,
    db: TestDatabase,
    store: PgStore,
    provider: Option<MockServer>,
    /// The issuer of the provider's tokens.
    issuer: String,
    http: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn start(auth: Auth) -> Option<Self> {
        let db = TestDatabase::create().await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let mut vars: HashMap<String, String> = [
            ("DATABASE_URL", db.url()),
            ("JOBHUNT_ENCRYPTION_KEYS", KEY.to_owned()),
            ("JOBHUNT_PUBLIC_URL", base.clone()),
            ("JOBHUNT_ENV", "test".to_owned()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        let (provider, issuer) = match auth {
            Auth::Dev => {
                vars.insert("JOBHUNT_AUTH_MODE".into(), "dev".into());
                vars.insert("JOBHUNT_AUTH_DEV_SECRET".into(), DEV_SECRET.into());
                (None, String::new())
            }
            Auth::Oidc | Auth::Rfc8414 | Auth::Manual => {
                let provider = MockServer::start().await;
                let uri = provider.uri();
                let issuer = if auth == Auth::Oidc {
                    format!("{uri}/")
                } else {
                    uri.clone()
                };
                let metadata = json!({
                    "issuer": issuer,
                    "jwks_uri": format!("{uri}/jwks"),
                    "token_endpoint": format!("{uri}/oauth/token"),
                    "device_authorization_endpoint": format!("{uri}/oauth/device/code"),
                });
                match auth {
                    Auth::Oidc => {
                        Mock::given(path("/.well-known/openid-configuration"))
                            .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
                            .mount(&provider)
                            .await;
                        vars.insert("JOBHUNT_OIDC_AUDIENCE".into(), AUDIENCE.into());
                        vars.insert("JOBHUNT_OIDC_AUDIENCE_PARAMETER".into(), "audience".into());
                    }
                    Auth::Rfc8414 => {
                        Mock::given(path("/.well-known/oauth-authorization-server"))
                            .respond_with(ResponseTemplate::new(200).set_body_json(metadata))
                            .mount(&provider)
                            .await;
                        vars.insert(
                            "JOBHUNT_OIDC_AUDIENCE".into(),
                            format!("{AUDIENCE}, {base}/mcp"),
                        );
                    }
                    _ => {
                        vars.insert("JOBHUNT_OIDC_AUDIENCE".into(), AUDIENCE.into());
                        vars.insert("JOBHUNT_OIDC_AUDIENCE_PARAMETER".into(), "none".into());
                        vars.insert("JOBHUNT_OIDC_JWKS_URL".into(), format!("{uri}/jwks"));
                        vars.insert(
                            "JOBHUNT_OIDC_DEVICE_AUTHORIZATION_URL".into(),
                            format!("{uri}/authorize/device"),
                        );
                        vars.insert(
                            "JOBHUNT_OIDC_TOKEN_URL".into(),
                            format!("{uri}/authenticate"),
                        );
                    }
                }
                let jwk: Value = serde_json::from_str(
                    &std::fs::read_to_string(fixture("test_signing_key.jwk.json")).unwrap(),
                )
                .unwrap();
                Mock::given(path("/jwks"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys": [jwk]})))
                    .mount(&provider)
                    .await;
                vars.insert("JOBHUNT_OIDC_ISSUER".into(), issuer.clone());
                vars.insert("JOBHUNT_OIDC_CLI_CLIENT_ID".into(), "cli-client".into());
                (Some(provider), issuer)
            }
        };
        let config = CloudConfig::from_env(&move |name: &str| vars.get(name).cloned());
        assert!(config.problems(jobhunt_cloud::Role::Server).is_empty());
        let store = PgStore::connect(
            &db.url(),
            &PgSettings::default(),
            Keyring::parse(KEY).unwrap(),
        )
        .await
        .unwrap();
        store.migrate().await.unwrap();
        let (usage, _) = UsageLog::start(store.clone());
        let state = ApiState::new(store.clone(), Arc::new(config), usage).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });
        Some(Self {
            base,
            db,
            store,
            provider,
            issuer,
            http: reqwest::Client::new(),
            task,
        })
    }

    async fn dev_token(&self, subject: &str) -> String {
        let r: Value = self
            .http
            .post(format!("{}/api/v1/auth/dev-token", self.base))
            .json(&json!({ "subject": subject }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        r["access_token"].as_str().unwrap().to_owned()
    }

    fn oidc_token(&self, subject: &str, tweak: impl FnOnce(&mut Value, &mut Header)) -> String {
        let issuer = &self.issuer;
        let now = Utc::now().timestamp();
        let mut claims = json!({
            "iss": issuer, "aud": AUDIENCE, "sub": subject, "iat": now - 5, "exp": now + 600,
        });
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key-1".into());
        tweak(&mut claims, &mut header);
        let pem = std::fs::read(fixture("test_signing_key.pem")).unwrap();
        jsonwebtoken::encode(&header, &claims, &EncodingKey::from_rsa_pem(&pem).unwrap()).unwrap()
    }

    async fn call(
        &self,
        method: reqwest::Method,
        route: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value, reqwest::header::HeaderMap) {
        let mut request = self.http.request(method, format!("{}{route}", self.base));
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        if let Some(b) = body {
            request = request.json(&b);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.unwrap();
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        (status, value, headers)
    }

    async fn get(&self, route: &str, token: &str) -> (StatusCode, Value) {
        let (s, v, _) = self
            .call(reqwest::Method::GET, route, Some(token), None)
            .await;
        (s, v)
    }

    async fn post(&self, route: &str, token: &str, body: Value) -> (StatusCode, Value) {
        let (s, v, _) = self
            .call(reqwest::Method::POST, route, Some(token), Some(body))
            .await;
        (s, v)
    }

    /// Discovers jobs into the shared corpus (as the worker would).
    async fn seed_jobs(&self, postings: &[JobPosting]) {
        let source = SourceKey::new("greenhouse", "acme").unwrap();
        let when = Utc::now() - Duration::hours(1);
        let run = self.store.begin_run(when).await.unwrap();
        self.store
            .apply_scan(&ScanWrite {
                run,
                source: &source,
                started_at: when,
                observed_at: when,
                counts: IngestCounts::default(),
                body: ScanBody::Listing {
                    postings,
                    complete: true,
                    close_missing: false,
                    closing_withheld: None,
                    retain: &[],
                    validator: None,
                },
            })
            .await
            .unwrap();
    }

    async fn finish(self) {
        self.task.abort();
        self.store.close().await;
        self.db.drop_database().await;
    }
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

fn posting(id: &str, title: &str) -> JobPosting {
    JobPosting {
        provenance: Provenance {
            source: SourceKey::new("greenhouse", "acme").unwrap(),
            source_record_id: Some(id.into()),
            fetched_from: None,
        },
        url: CanonicalUrl::parse(&format!("https://job-boards.greenhouse.io/acme/jobs/{id}"))
            .unwrap(),
        apply_url: None,
        company: "Acme".into(),
        title: title.into(),
        department: None,
        team: None,
        location: Some("Remote - Worldwide".into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: Some(true),
        compensation: None,
        work_authorization: None,
        description_text: Some("Requirements\nStrong Rust and PostgreSQL.".into()),
        description_html: None,
        posted_at: Some(Utc.with_ymd_and_hms(2026, 9, 20, 0, 0, 0).unwrap()),
        source_updated_at: None,
    }
}

#[tokio::test]
async fn health_readiness_and_resource_metadata() {
    let Some(server) = Server::start(Auth::Oidc).await else {
        return;
    };
    let (status, body, _) = server
        .call(reqwest::Method::GET, "/health", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    let (status, body, headers) = server
        .call(reqwest::Method::GET, "/ready", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let ready: Ready = serde_json::from_value(body).unwrap();
    assert_eq!(ready.status, "ready");
    assert!(
        headers.get("x-request-id").is_some(),
        "every answer carries a request id"
    );

    let (status, meta, _) = server
        .call(
            reqwest::Method::GET,
            "/.well-known/oauth-protected-resource/mcp",
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["resource"], format!("{}/mcp", server.base));
    // The display name MCP clients show when asking to sign in.
    assert_eq!(meta["resource_name"], "Narrow");
    assert_eq!(
        meta["authorization_servers"][0],
        format!("{}/", server.provider.as_ref().unwrap().uri())
    );

    let (status, config, _) = server
        .call(reqwest::Method::GET, "/api/v1/auth/config", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["mode"], "oidc");
    assert_eq!(config["cli_client_id"], "cli-client");
    assert!(
        config["device_authorization_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/oauth/device/code")
    );

    // Readiness depends on the schema: a database without it is not ready.
    let Some(empty) = TestDatabase::create().await else {
        return;
    };
    let bare = PgStore::connect(&empty.url(), &PgSettings::default(), Keyring::ephemeral())
        .await
        .unwrap();
    assert!(!bare.schema().await.unwrap().is_current());
    bare.close().await;
    empty.drop_database().await;
    server.finish().await;
}

#[tokio::test]
async fn requests_without_valid_tokens_are_refused() {
    let Some(server) = Server::start(Auth::Oidc).await else {
        return;
    };
    let (status, body, headers) = server
        .call(reqwest::Method::GET, "/api/v1/account", None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "unauthenticated");
    let challenge = headers["www-authenticate"].to_str().unwrap();
    assert!(challenge.contains("resource_metadata="));
    assert!(challenge.contains("/.well-known/oauth-protected-resource/mcp"));

    let valid = server.oidc_token("auth0|alice", |_, _| {});
    let (status, account) = server.get("/api/v1/account", &valid).await;
    assert_eq!(status, StatusCode::OK, "{account}");
    let account: AccountView = serde_json::from_value(account).unwrap();
    assert!(account.id.starts_with("usr_"));
    assert_eq!(account.authenticated_with, "oidc");

    let cases: Vec<(&str, String)> = vec![
        (
            "another audience",
            server.oidc_token("auth0|alice", |c, _| c["aud"] = json!("https://evil.test")),
        ),
        (
            "another issuer",
            server.oidc_token("auth0|alice", |c, _| c["iss"] = json!("https://evil.test/")),
        ),
        (
            "expired",
            server.oidc_token("auth0|alice", |c, _| {
                c["exp"] = json!(Utc::now().timestamp() - 3600);
            }),
        ),
        (
            "unknown key",
            server.oidc_token("auth0|alice", |_, h| h.kid = Some("other".into())),
        ),
        (
            "symmetric algorithm",
            jsonwebtoken::encode(
                &Header::new(Algorithm::HS256),
                &json!({"iss": server.issuer,
                        "aud": AUDIENCE, "sub": "x", "exp": Utc::now().timestamp() + 60}),
                &EncodingKey::from_secret(b"guess"),
            )
            .unwrap(),
        ),
        ("garbage", "not-a-token".into()),
        ("unknown personal token", "jh_pat_nope".into()),
    ];
    for (why, token) in cases {
        let (status, body) = server.get("/api/v1/account", &token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{why}: {body}");
        assert_eq!(error_code(&body), "unauthenticated", "{why}");
    }
    server.finish().await;
}

/// A provider that publishes only RFC 8414 metadata, names itself without
/// a trailing slash, and gives MCP clients tokens for the `/mcp` resource
/// (WorkOS AuthKit does all three).
#[tokio::test]
async fn authkit_style_providers_are_accepted() {
    let Some(server) = Server::start(Auth::Rfc8414).await else {
        return;
    };
    assert!(!server.issuer.ends_with('/'));
    let (status, config, _) = server
        .call(reqwest::Method::GET, "/api/v1/auth/config", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config["issuer"], server.issuer);
    assert_eq!(config["audience"], AUDIENCE);
    assert_eq!(config["audience_parameter"], "resource");
    assert!(
        config["device_authorization_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/oauth/device/code")
    );
    let (_, meta, _) = server
        .call(
            reqwest::Method::GET,
            "/.well-known/oauth-protected-resource/mcp",
            None,
            None,
        )
        .await;
    assert_eq!(meta["authorization_servers"][0], server.issuer);

    // The CLI's token (the API) and an MCP client's (the /mcp resource)
    // are the same person.
    let cli = server.oidc_token("user_01ALICE", |_, _| {});
    let resource = format!("{}/mcp", server.base);
    let mcp = server.oidc_token("user_01ALICE", |c, _| c["aud"] = json!(resource));
    let (status, a) = server.get("/api/v1/account", &cli).await;
    assert_eq!(status, StatusCode::OK, "{a}");
    let (status, b) = server.get("/api/v1/account", &mcp).await;
    assert_eq!(status, StatusCode::OK, "{b}");
    assert_eq!(a["id"], b["id"]);
    // The web app signs in with its own OAuth client for the API: still
    // the same person, the same account.
    let web = server.oidc_token("user_01ALICE", |c, _| {
        c["client_id"] = json!("web-client");
        c["azp"] = json!("web-client");
    });
    let (status, w) = server.get("/api/v1/account", &web).await;
    assert_eq!(status, StatusCode::OK, "{w}");
    assert_eq!(a["id"], w["id"]);

    for (why, token) in [
        (
            "another audience",
            server.oidc_token("user_01ALICE", |c, _| c["aud"] = json!("https://evil.test")),
        ),
        (
            "another issuer",
            server.oidc_token("user_01ALICE", |c, _| c["iss"] = json!("https://evil.test")),
        ),
    ] {
        let (status, body) = server.get("/api/v1/account", &token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{why}: {body}");
    }
    server.finish().await;
}

/// A provider without metadata: the JWKS and the CLI's endpoints are set
/// by hand, and no audience parameter is sent (the provider sets `aud`).
#[tokio::test]
async fn providers_without_metadata_use_endpoints_set_by_hand() {
    let Some(server) = Server::start(Auth::Manual).await else {
        return;
    };
    let (status, config, _) = server
        .call(reqwest::Method::GET, "/api/v1/auth/config", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(config.get("audience_parameter").is_none(), "{config}");
    assert!(
        config["device_authorization_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/authorize/device")
    );
    assert!(
        config["token_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/authenticate")
    );
    let token = server.oidc_token("user_01BOB", |_, _| {});
    let (status, body) = server.get("/api/v1/account", &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    server.finish().await;
}

#[tokio::test]
async fn logout_and_personal_access_tokens() {
    let Some(server) = Server::start(Auth::Oidc).await else {
        return;
    };
    let alice = server.oidc_token("auth0|alice", |_, _| {});
    let bob = server.oidc_token("auth0|bob", |_, _| {});
    let (status, created) = server
        .post(
            "/api/v1/tokens",
            &alice,
            json!({"name": "Claude Desktop", "expires_in_days": 30}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let created: CreatedToken = serde_json::from_value(created).unwrap();
    assert!(created.secret.starts_with("jh_pat_"));
    let (status, me) = server.get("/api/v1/account", &created.secret).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["authenticated_with"], "token");
    let (_, list) = server.get("/api/v1/tokens", &alice).await;
    let list: TokenList = serde_json::from_value(list).unwrap();
    assert_eq!(list.tokens.len(), 1);
    assert!(!serde_json::to_string(&list).unwrap().contains("jh_pat_"));
    // Bob can neither see nor revoke it.
    let (_, bob_list) = server.get("/api/v1/tokens", &bob).await;
    assert_eq!(bob_list["tokens"], json!([]));
    let (status, _, _) = server
        .call(
            reqwest::Method::DELETE,
            &format!("/api/v1/tokens/{}", created.token.id),
            Some(&bob),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Invalid input is a structured 400.
    let (status, body) = server
        .post("/api/v1/tokens", &alice, json!({"name": "", "extra": 1}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "invalid_request");

    // Logging out everywhere revokes the session and every token.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (status, _, _) = server
        .call(
            reqwest::Method::POST,
            "/api/v1/account/logout",
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for token in [&alice, &created.secret] {
        let (status, _) = server.get("/api/v1/account", token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    // A token issued after logging out works.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let fresh = server.oidc_token("auth0|alice", |c, _| {
        c["iat"] = json!(Utc::now().timestamp());
    });
    let (status, _) = server.get("/api/v1/account", &fresh).await;
    assert_eq!(status, StatusCode::OK);
    server.finish().await;
}

async fn import_resume_via_sync(server: &Server, token: &str) {
    // A laptop imports a resume and syncs it up through the real client.
    let laptop = laptop().await;
    let file = ResumeFile::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../jobhunt-resume/tests/fixtures/ana_lima.md"),
    )
    .unwrap();
    let parser = DeterministicParser;
    let now = Utc::now();
    laptop
        .profiles()
        .import_resume(
            file.source_document(parser.name(), now),
            &parser.parse(&file.text),
            now,
        )
        .await
        .unwrap();
    let client = CloudClient::new(server.base.parse().unwrap())
        .unwrap()
        .with_token(token);
    let account = client.account().await.unwrap();
    let report = laptop
        .sync(
            &client,
            &Remote {
                server: server.base.clone(),
                user_id: account.id,
            },
            None,
            now,
        )
        .await
        .unwrap();
    assert!(report.pushed > 0);
}

async fn laptop() -> App {
    App::with_store(
        LoadedConfig {
            config: AppConfig::default(),
            file: None,
            default_file: None,
            database: PathBuf::from(":memory:"),
        },
        SqliteJobStore::open_in_memory().await.unwrap(),
    )
}

#[tokio::test]
async fn product_endpoints_return_the_shared_views() {
    let Some(server) = Server::start(Auth::Dev).await else {
        return;
    };
    let token = server.dev_token("ana").await;
    // No profile yet: the stable error code, as a 409.
    let (status, body) = server.get("/api/v1/profile", &token).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "no_profile");

    import_resume_via_sync(&server, &token).await;
    let (status, profile) = server.get("/api/v1/profile", &token).await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    let profile: ProfileView = serde_json::from_value(profile).unwrap();
    let text = serde_json::to_string(&profile).unwrap();
    assert!(
        !text.contains('@'),
        "the profile view never includes contact details"
    );

    let (status, prefs) = server
        .post(
            "/api/v1/preferences",
            &token,
            json!({"statement": "Remote only, backend work, at least USD 120k"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{prefs}");
    let prefs: PreferenceUpdateResult = serde_json::from_value(prefs).unwrap();
    assert!(!prefs.interpreted.is_empty());

    let jobs = [
        posting("1", "Senior Rust Engineer"),
        posting("2", "Frontend Engineer"),
    ];
    server.seed_jobs(&jobs).await;
    let (status, found) = server
        .post(
            "/api/v1/search",
            &token,
            json!({"query": "engineer", "limit": 5, "verify": false, "include_lower_tiers": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    let found: SearchResults = serde_json::from_value(found).unwrap();
    assert_eq!(found.funnel.checked, 2);
    assert!(
        !found.refresh.performed,
        "searches never read job boards in the cloud"
    );

    let id = jobhunt_jobs::OpportunityId::founded_by(jobs[0].id()).to_string();
    let (status, detail) = server
        .get(
            &format!("/api/v1/opportunities/{id}?include_sources=true"),
            &token,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let _: JobDetail = serde_json::from_value(detail).unwrap();

    let (status, fb) = server
        .post(
            &format!("/api/v1/opportunities/{id}/feedback"),
            &token,
            json!({"action": "save", "reason": "great team"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{fb}");
    let fb: FeedbackResult = serde_json::from_value(fb).unwrap();
    assert!(fb.recorded);
    let (_, again) = server
        .post(
            &format!("/api/v1/opportunities/{id}/feedback"),
            &token,
            json!({"action": "save", "reason": "great team"}),
        )
        .await;
    assert_eq!(again["recorded"], false, "retries are safe");

    let (status, pipeline) = server.get("/api/v1/pipeline", &token).await;
    assert_eq!(status, StatusCode::OK);
    let pipeline: PipelineView = serde_json::from_value(pipeline).unwrap();
    assert_eq!(pipeline.entries.len(), 1);

    let (status, context) = server
        .get(
            &format!("/api/v1/opportunities/{id}/application-context"),
            &token,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{context}");
    let context: ApplicationContext = serde_json::from_value(context).unwrap();
    assert!(context.contact.is_none());

    // Verification: no network in tests, so the sources are unreachable;
    // the answer is still a report (and an attempt is recorded).
    let (status, report) = server
        .post(
            &format!("/api/v1/opportunities/{id}/verify"),
            &token,
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let _: VerificationReport = serde_json::from_value(report).unwrap();

    let (status, export) = server.get("/api/v1/export", &token).await;
    assert_eq!(status, StatusCode::OK);
    let export: StateExport = serde_json::from_value(export).unwrap();
    assert_eq!(export.feedback.len(), 1);

    // Errors keep their stable codes.
    let (status, body) = server
        .get(
            "/api/v1/opportunities/opp_00000000000000000000000000000000",
            &token,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "unknown_opportunity");
    let (status, body) = server.get("/api/v1/opportunities/not-an-id", &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "invalid_arguments");
    let (status, body) = server
        .post("/api/v1/search", &token, json!({"limit": 99}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&body), "invalid_request");
    let (status, body) = server
        .post("/api/v1/search", &token, json!({"unexpected": true}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let _: ErrorBody = serde_json::from_value(body).unwrap();
    let (status, body) = server.get("/api/v1/nope", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");

    // Usage events were recorded, without any of the person's words.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let summary = server
        .store
        .usage_summary(Utc::now() - Duration::hours(1))
        .await
        .unwrap();
    let events: Vec<&str> = summary.iter().map(|(e, _, _)| e.as_str()).collect();
    for expected in ["find", "feedback", "preferences", "sync_push", "verify"] {
        assert!(events.contains(&expected), "{expected} in {events:?}");
    }
    let pool = sqlx_pool(&server).await;
    let metadata: Vec<String> = sqlx::query_scalar("SELECT metadata::text FROM usage_events")
        .fetch_all(&pool)
        .await
        .unwrap();
    for m in &metadata {
        assert!(!m.contains("great team") && !m.contains("USD 120k"), "{m}");
    }
    pool.close().await;
    server.finish().await;
}

async fn sqlx_pool(server: &Server) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(server.db.options())
        .await
        .unwrap()
}

#[tokio::test]
async fn accounts_are_isolated_across_the_api() {
    let Some(server) = Server::start(Auth::Dev).await else {
        return;
    };
    let alice = server.dev_token("alice").await;
    let bob = server.dev_token("bob").await;
    import_resume_via_sync(&server, &alice).await;
    let jobs = [posting("1", "Backend Engineer")];
    server.seed_jobs(&jobs).await;
    let id = jobhunt_jobs::OpportunityId::founded_by(jobs[0].id()).to_string();
    server
        .post(
            &format!("/api/v1/opportunities/{id}/feedback"),
            &alice,
            json!({"action": "reject", "reason": "my ex works there"}),
        )
        .await;

    // Bob sees the shared job, none of Alice's data.
    let (status, _) = server
        .get(&format!("/api/v1/opportunities/{id}"), &bob)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = server.get("/api/v1/profile", &bob).await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::CONFLICT, "no_profile")
    );
    let (_, pipeline) = server
        .get("/api/v1/pipeline?include_rejected=true", &bob)
        .await;
    assert_eq!(pipeline["entries"], json!([]));
    let (_, export) = server.get("/api/v1/export", &bob).await;
    assert_eq!(export["feedback"], json!([]));
    assert!(export.get("profile").is_none());
    let (_, pulled) = server
        .post(
            "/api/v1/sync/pull",
            &bob,
            json!({"protocol": 1, "cursor": 0}),
        )
        .await;
    assert_eq!(pulled["entities"], json!([]));
    assert_eq!(pulled["feedback"], json!([]));
    assert!(!pulled.to_string().contains("my ex works there"));
    // Alice's view is intact.
    let (_, alice_pipeline) = server
        .get("/api/v1/pipeline?include_rejected=true", &alice)
        .await;
    assert_eq!(alice_pipeline["entries"].as_array().unwrap().len(), 1);
    server.finish().await;
}

#[tokio::test]
async fn hosted_mcp_serves_the_same_tools_per_account() {
    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    let Some(server) = Server::start(Auth::Dev).await else {
        return;
    };
    let alice = server.dev_token("alice").await;
    let bob = server.dev_token("bob").await;
    import_resume_via_sync(&server, &alice).await;
    let jobs = [posting("1", "Backend Engineer")];
    server.seed_jobs(&jobs).await;

    let connect = |token: Option<String>| {
        let mut config =
            StreamableHttpClientTransportConfig::with_uri(format!("{}/mcp", server.base));
        if let Some(t) = token {
            config = config.auth_header(t);
        }
        config.allow_stateless = true;
        StreamableHttpClientTransport::with_client(reqwest::Client::new(), config)
    };
    let client = ().serve(connect(Some(alice.clone()))).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "get_feed",
            "get_job",
            "get_pipeline",
            "get_profile",
            "get_taste",
            "mark_applied",
            "prepare_application_context",
            "record_feedback",
            "reject_job",
            "save_job",
            "search_jobs",
            "update_preferences",
            "verify_job"
        ]
    );
    assert!(
        tools.iter().all(|t| t.output_schema.is_some()),
        "structured output"
    );

    let call = |name: &'static str, args: Value| {
        CallToolRequestParams::new(name)
            .with_arguments(args.as_object().cloned().unwrap_or_default())
    };
    let profile = client
        .call_tool(call("get_profile", json!({})))
        .await
        .unwrap();
    assert_ne!(profile.is_error, Some(true));
    let structured = profile.structured_content.clone().unwrap();
    let _: ProfileView = serde_json::from_value(structured).unwrap();

    let id = jobhunt_jobs::OpportunityId::founded_by(jobs[0].id()).to_string();
    let saved = client
        .call_tool(call("save_job", json!({"id": id})))
        .await
        .unwrap();
    let saved: FeedbackResult = serde_json::from_value(saved.structured_content.unwrap()).unwrap();
    assert!(saved.recorded);
    // The same account state the web shows: the feed knows it is saved
    // (so it is not "new"), and taste lists what was said.
    let feed = client
        .call_tool(call("get_feed", json!({"limit": 3})))
        .await
        .unwrap();
    assert_ne!(feed.is_error, Some(true), "{:?}", feed.content);
    let feed: jobhunt_app::feed::FeedView =
        serde_json::from_value(feed.structured_content.unwrap()).unwrap();
    assert!(feed.items.iter().all(|i| i.item.id != id));
    assert_eq!(feed.pipeline.saved, 1);
    let (_, web_feed) = server.get("/api/v1/feed?limit=3", &alice).await;
    assert_eq!(web_feed["pipeline"]["saved"], 1);
    let taste = client
        .call_tool(call("get_taste", json!({})))
        .await
        .unwrap();
    let taste: jobhunt_app::taste_view::TasteView =
        serde_json::from_value(taste.structured_content.unwrap()).unwrap();
    assert_eq!(taste.feedback_events, 1);
    let too_many = client
        .call_tool(call("get_feed", json!({"limit": 50})))
        .await;
    assert!(
        too_many.is_err() || too_many.unwrap().is_error == Some(true),
        "the feed stays short"
    );
    let unknown = client
        .call_tool(call(
            "get_job",
            json!({"id": "opp_00000000000000000000000000000000"}),
        ))
        .await
        .unwrap();
    assert_eq!(unknown.is_error, Some(true));
    assert!(format!("{:?}", unknown.content).contains("unknown_opportunity"));
    let _ = client.cancel().await;

    // Bob's MCP session sees his own (empty) data.
    let bob_client = ().serve(connect(Some(bob))).await.unwrap();
    let pipeline = bob_client
        .call_tool(call("get_pipeline", json!({})))
        .await
        .unwrap();
    let pipeline: PipelineView =
        serde_json::from_value(pipeline.structured_content.unwrap()).unwrap();
    assert!(pipeline.entries.is_empty());
    let no_profile = bob_client
        .call_tool(call("get_profile", json!({})))
        .await
        .unwrap();
    assert_eq!(no_profile.is_error, Some(true));
    let _ = bob_client.cancel().await;

    // Without a token the endpoint refuses with the OAuth challenge.
    let (status, _, headers) = server
        .call(
            reqwest::Method::POST,
            "/mcp",
            None,
            Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("resource_metadata")
    );
    assert!(().serve(connect(None)).await.is_err());

    // Tool calls are counted, by name only.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let summary = server
        .store
        .usage_summary(Utc::now() - Duration::hours(1))
        .await
        .unwrap();
    assert!(summary.iter().any(|(e, n, _)| e == "mcp_tool" && *n >= 4));
    server.finish().await;
}

#[tokio::test]
async fn sync_through_the_http_api_and_a_second_machine() {
    let Some(server) = Server::start(Auth::Dev).await else {
        return;
    };
    let token = server.dev_token("ana").await;
    import_resume_via_sync(&server, &token).await;
    let client = CloudClient::new(server.base.parse().unwrap())
        .unwrap()
        .with_token(&token);
    let account = client.account().await.unwrap();
    let desktop = laptop().await;
    let remote = Remote {
        server: server.base.clone(),
        user_id: account.id.clone(),
    };
    let report = desktop
        .sync(&client, &remote, None, Utc::now())
        .await
        .unwrap();
    assert!(report.applied_locally > 5);
    // A change made through the API reaches the desktop.
    server
        .post(
            "/api/v1/preferences",
            &token,
            json!({"statement": "No crypto companies"}),
        )
        .await;
    let report = desktop
        .sync(&client, &remote, None, Utc::now())
        .await
        .unwrap();
    assert!(report.applied_locally >= 1);
    assert!(
        desktop
            .profiles()
            .require()
            .await
            .unwrap()
            .statements
            .iter()
            .any(|s| s.text == "No crypto companies")
    );
    // An invalid push is a structured 422.
    let (status, body) = server
        .post(
            "/api/v1/sync/push",
            &token,
            json!({"protocol": 1, "push_id": "x", "entities": [
                {"kind": "claim", "id": "clm_x", "base_version": 0, "body": {"id": "nope"}}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "invalid_sync");
    let (status, _) = server
        .post(
            "/api/v1/sync/pull",
            &token,
            json!({"protocol": 99, "cursor": 0}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let _: DateTime<Utc> = Utc::now();
    server.finish().await;
}

#[tokio::test]
async fn device_flow_signs_in_refreshes_and_revokes() {
    use jobhunt_cloud::api::types::AuthConfigView;
    use jobhunt_cloud::client::DeviceFlow;
    use wiremock::matchers::{body_string_contains, method};

    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/device/code"))
        .and(body_string_contains("client_id=cli-client"))
        .and(body_string_contains(
            "resource=https%3A%2F%2Fapi.jobhunt.test",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dev-123", "user_code": "ABCD-EFGH",
            "verification_uri": "https://id.test/activate",
            "verification_uri_complete": "https://id.test/activate?user_code=ABCD-EFGH",
            "expires_in": 60, "interval": 1,
        })))
        .mount(&provider)
        .await;
    // Pending once, then approved.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("device_code=dev-123"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({"error": "authorization_pending"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&provider)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("device_code=dev-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 3600,
        })))
        .with_priority(2)
        .mount(&provider)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-2", "expires_in": 3600,
        })))
        .mount(&provider)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&provider)
        .await;
    let config = AuthConfigView {
        mode: "oidc".into(),
        issuer: Some(format!("{}/", provider.uri())),
        audience: Some(AUDIENCE.into()),
        audience_parameter: Some("resource".into()),
        cli_client_id: Some("cli-client".into()),
        scopes: Some("openid offline_access".into()),
        device_authorization_endpoint: Some(format!("{}/oauth/device/code", provider.uri())),
        token_endpoint: Some(format!("{}/oauth/token", provider.uri())),
        revocation_endpoint: Some(format!("{}/oauth/revoke", provider.uri())),
    };
    let flow = DeviceFlow::from_config(&config).unwrap();
    let code = flow.start().await.unwrap();
    assert_eq!(code.user_code, "ABCD-EFGH");
    let tokens = flow.wait(&code).await.unwrap();
    assert_eq!(tokens.access_token, "at-1");
    assert_eq!(tokens.refresh_token.as_deref(), Some("rt-1"));
    assert!(
        !format!("{tokens:?}").contains("at-1"),
        "tokens are not logged"
    );
    let session =
        DeviceFlow::for_session(&format!("{}/oauth/token", provider.uri()), "cli-client").unwrap();
    assert_eq!(session.refresh("rt-1").await.unwrap().access_token, "at-2");
    session
        .revoke(&format!("{}/oauth/revoke", provider.uri()), "rt-1")
        .await
        .unwrap();
    // A server without device sign-in says how to sign in instead.
    let without = AuthConfigView {
        device_authorization_endpoint: None,
        ..config
    };
    assert!(
        DeviceFlow::from_config(&without)
            .unwrap_err()
            .to_string()
            .contains("--token")
    );
}
