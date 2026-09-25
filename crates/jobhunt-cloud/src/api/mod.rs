//! The HTTP API: `/api/v1/…`, health checks, OAuth protected-resource
//! metadata, and the hosted MCP endpoint (`/mcp`).
//!
//! Every endpoint is one application use case for the authenticated
//! account: the handler parses the request, builds the account's
//! application (a user-scoped view of the store, see
//! [`ApiState::app_for`]), calls the use case and returns the view the
//! MCP tool of the same name returns. Nothing here decides anything about
//! jobs, profiles, eligibility or ranking.
//!
//! | Method | Path | Answer |
//! | --- | --- | --- |
//! | GET | `/health` | the process is up |
//! | GET | `/ready` | Postgres answers and the schema is current |
//! | GET | `/.well-known/oauth-protected-resource[/mcp]` | RFC 9728 metadata |
//! | GET | `/api/v1/auth/config` | how to sign in |
//! | POST | `/api/v1/auth/dev-token` | development servers only |
//! | GET, DELETE | `/api/v1/account` | the account; delete it and its data |
//! | POST | `/api/v1/account/logout` | revoke every session and token |
//! | GET, POST | `/api/v1/tokens` | personal access tokens |
//! | DELETE | `/api/v1/tokens/{id}` | revoke one |
//! | POST | `/api/v1/sync/pull`, `/api/v1/sync/push` | sync (see `jobhunt_storage::sync`) |
//! | POST | `/api/v1/search` | the shortlist (`search_jobs`) |
//! | GET | `/api/v1/opportunities/{id}` | details (`get_job`) |
//! | POST | `/api/v1/opportunities/{id}/verify` | `verify_job` |
//! | POST | `/api/v1/opportunities/{id}/feedback` | save, reject, applied, … |
//! | GET | `/api/v1/opportunities/{id}/application-context` | `prepare_application_context` |
//! | GET | `/api/v1/profile` | `get_profile` |
//! | POST | `/api/v1/preferences` | `update_preferences` |
//! | GET | `/api/v1/pipeline` | `get_pipeline` |
//! | GET | `/api/v1/export` | the portable state file |
//! | * | `/mcp` | hosted MCP (Streamable HTTP) |

pub mod error;
pub mod types;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::Utc;
use http::StatusCode;
use jobhunt_app::context::ApplicationContext;
use jobhunt_app::feedback::{FeedbackResult, PipelineView};
use jobhunt_app::inspect::{JobDetail, VerificationReport};
use jobhunt_app::preferences::{PreferenceUpdate, PreferenceUpdateResult};
use jobhunt_app::profile_view::ProfileView;
use jobhunt_app::shortlist::MAX_LIMIT;
use jobhunt_app::state::StateExport;
use jobhunt_app::{App, AppError, DiscoveryMode, FindRequest, Quiet, RefreshMode, SearchResults};
use jobhunt_jobs::verification::VerifyMode;
use jobhunt_mcp::{SearchJobsParams, UpdatePreferencesParams};
use jobhunt_ranking::FeedbackAction;
use jobhunt_storage::postgres::{PgStore, PgUserStore};
use jobhunt_storage::sync::{PullRequest, PullResponse, PushError, PushRequest, PushResponse};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use self::error::ApiError;
use self::types::*;
use crate::auth::{Authenticator, DevVerifier, Method, OidcVerifier, Principal};
use crate::config::{AuthConfig, CloudConfig};
use crate::observability::{REQUEST_ID, RandomRequestId, make_span, on_response};
use crate::usage::UsageLog;

/// Everything the handlers share.
#[derive(Clone)]
pub struct ApiState {
    inner: Arc<Inner>,
}

struct Inner {
    store: PgStore,
    config: Arc<CloudConfig>,
    auth: Authenticator,
    oidc: Option<Arc<OidcVerifier>>,
    dev: Option<DevVerifier>,
    usage: UsageLog,
}

impl std::fmt::Debug for ApiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiState").finish_non_exhaustive()
    }
}

impl ApiState {
    /// The shared state: the authenticator for the configured mode.
    pub fn new(
        store: PgStore,
        config: Arc<CloudConfig>,
        usage: UsageLog,
    ) -> Result<Self, crate::config::ConfigProblems> {
        let (auth, oidc, dev) = match &config.auth {
            Some(AuthConfig::Oidc {
                issuer, audience, ..
            }) => {
                let verifier = Arc::new(
                    OidcVerifier::new(issuer.clone(), audience.clone())
                        .map_err(|e| crate::config::ConfigProblems(vec![e.to_string()]))?,
                );
                (
                    Authenticator::new(verifier.clone(), Method::Oidc, store.clone()),
                    Some(verifier),
                    None,
                )
            }
            Some(AuthConfig::Dev { secret }) => {
                let dev = DevVerifier::new(secret.clone());
                (
                    Authenticator::new(Arc::new(dev.clone()), Method::Dev, store.clone()),
                    None,
                    Some(dev),
                )
            }
            None => {
                return Err(crate::config::ConfigProblems(vec![
                    "authentication is not configured".into(),
                ]));
            }
        };
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                config,
                auth,
                oidc,
                dev,
                usage,
            }),
        })
    }

    pub fn store(&self) -> &PgStore {
        &self.inner.store
    }

    pub fn usage(&self) -> &UsageLog {
        &self.inner.usage
    }

    pub fn config(&self) -> &CloudConfig {
        &self.inner.config
    }

    /// The account's application: the shared configuration over a view of
    /// the store bound to this account. Built per request; cheap.
    pub fn app_for(&self, principal: &Principal) -> Arc<App> {
        Arc::new(App::from_parts(
            Arc::clone(&self.inner.config.app),
            Arc::new(self.user_store(principal)),
            DiscoveryMode::Background,
        ))
    }

    fn user_store(&self, principal: &Principal) -> PgUserStore {
        self.inner.store.for_user(principal.user.clone())
    }

    /// Where MCP clients read how to authenticate (RFC 9728).
    pub fn resource_metadata_url(&self) -> Option<String> {
        self.inner.config.public_url.as_ref().map(|u| {
            format!(
                "{}/.well-known/oauth-protected-resource/mcp",
                u.as_str().trim_end_matches('/')
            )
        })
    }
}

/// A JSON body whose rejection is a structured error.
pub struct Body<T>(pub T);

impl<S, T> FromRequest<S> for Body<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| Self(v))
            .map_err(|e| ApiError::invalid(e.body_text()))
    }
}

/// Query parameters whose rejection is a structured error.
pub struct Params<T>(pub T);

impl<S, T> FromRequestParts<S> for Params<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(v)| Self(v))
            .map_err(|e: QueryRejection| ApiError::invalid(e.body_text()))
    }
}

/// Runs a use case on the account's application.
///
/// Like the MCP server, the use case runs on a blocking thread driven by
/// the runtime: discovery and verification drive stream combinators over
/// borrowed data whose futures cannot be proven `Send` in a generic
/// context, which axum handlers require.
async fn run<T, F, Fut>(app: Arc<App>, work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(Arc<App>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || handle.block_on(work(app)))
        .await
        .map_err(|error| {
            tracing::error!(%error, "a use case panicked or was cancelled");
            ApiError::internal()
        })?
        .map_err(ApiError::from)
}

/// Resolves the bearer token; the principal goes into the request's
/// extensions (the hosted MCP server reads it from there).
async fn authenticate(State(state): State<ApiState>, mut request: Request, next: Next) -> Response {
    let bearer = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let result = match bearer {
        Some(token) => state.inner.auth.authenticate(&token).await,
        None => Err(crate::auth::AuthError::Missing),
    };
    match result {
        Ok(principal) => {
            tracing::Span::current().record("user", principal.user.as_str());
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(error) => {
            tracing::info!(reason = %error, "unauthenticated request");
            ApiError::unauthenticated(&error, state.resource_metadata_url().as_deref())
                .into_response()
        }
    }
}

type Auth = axum::Extension<Principal>;

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn ready(State(state): State<ApiState>) -> Response {
    let database = state.store().ping().await;
    let schema = state.store().schema().await;
    let (db, schema_text, ok) = match (&database, &schema) {
        (Ok(()), Ok(s)) if s.is_current() => ("ok".to_owned(), "current".to_owned(), true),
        (Ok(()), Ok(s)) => (
            "ok".to_owned(),
            format!("{} of {} migrations applied", s.applied, s.known),
            false,
        ),
        _ => ("unreachable".to_owned(), "unknown".to_owned(), false),
    };
    let body = Json(Ready {
        status: if ok { "ready" } else { "not_ready" }.into(),
        database: db,
        schema: schema_text,
    });
    if ok {
        body.into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
    }
}

/// OAuth 2.0 Protected Resource Metadata (RFC 9728), which MCP clients
/// read to find the authorization server.
async fn protected_resource(State(state): State<ApiState>) -> Result<Response, ApiError> {
    let Some(base) = state.config().public_url.as_ref() else {
        return Err(ApiError::not_found(
            "resource metadata (JOBHUNT_PUBLIC_URL is not set)",
        ));
    };
    let issuer = match &state.config().auth {
        Some(AuthConfig::Oidc { issuer, .. }) => vec![issuer.to_string()],
        _ => Vec::new(),
    };
    let scopes = match &state.config().auth {
        Some(AuthConfig::Oidc { scopes, .. }) => {
            scopes.split_whitespace().map(str::to_owned).collect()
        }
        _ => Vec::<String>::new(),
    };
    Ok(Json(serde_json::json!({
        "resource": format!("{}/mcp", base.as_str().trim_end_matches('/')),
        "authorization_servers": issuer,
        "bearer_methods_supported": ["header"],
        "scopes_supported": scopes,
        "resource_name": "JobHunt",
    }))
    .into_response())
}

async fn auth_config(State(state): State<ApiState>) -> Result<Json<AuthConfigView>, ApiError> {
    Ok(Json(match &state.config().auth {
        Some(AuthConfig::Oidc {
            issuer,
            audience,
            cli_client_id,
            scopes,
        }) => {
            let metadata = match &state.inner.oidc {
                Some(v) => v.metadata().await.ok(),
                None => None,
            };
            AuthConfigView {
                mode: "oidc".into(),
                issuer: Some(issuer.to_string()),
                audience: Some(audience.clone()),
                cli_client_id: cli_client_id.clone(),
                scopes: Some(scopes.clone()),
                device_authorization_endpoint: metadata
                    .as_ref()
                    .and_then(|m| m.device_authorization_endpoint.clone()),
                token_endpoint: metadata.as_ref().and_then(|m| m.token_endpoint.clone()),
                revocation_endpoint: metadata.and_then(|m| m.revocation_endpoint),
            }
        }
        _ => AuthConfigView {
            mode: "dev".into(),
            issuer: None,
            audience: None,
            cli_client_id: None,
            scopes: None,
            device_authorization_endpoint: None,
            token_endpoint: None,
            revocation_endpoint: None,
        },
    }))
}

async fn dev_token(
    State(state): State<ApiState>,
    Body(request): Body<DevTokenRequest>,
) -> Result<Json<DevTokenResponse>, ApiError> {
    let Some(dev) = &state.inner.dev else {
        return Err(ApiError::not_found("development sign-in"));
    };
    let subject = request.subject.trim();
    if subject.is_empty() || subject.len() > 64 {
        return Err(ApiError::invalid("subject must be 1 to 64 characters"));
    }
    let ttl = chrono::Duration::hours(12);
    let token = dev.mint(subject, ttl).map_err(|_| ApiError::internal())?;
    state
        .usage()
        .record(None, "login", serde_json::json!({"method": "dev"}));
    Ok(Json(DevTokenResponse {
        access_token: token,
        expires_in: 12 * 3600,
    }))
}

async fn account(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<Json<AccountView>, ApiError> {
    let account = state
        .store()
        .account(&principal.user)
        .await?
        .ok_or_else(|| ApiError::not_found("account"))?;
    let cloud = state.user_store(&principal).sync_state().await?;
    Ok(Json(AccountView {
        id: account.id.to_string(),
        created_at: account.created_at,
        authenticated_with: match principal.method {
            Method::Oidc => "oidc",
            Method::Token => "token",
            Method::Dev => "dev",
        }
        .into(),
        identities: account
            .identities
            .into_iter()
            .map(|i| IdentityView {
                issuer: i.issuer,
                created_at: i.created_at,
                last_login_at: i.last_login_at,
            })
            .collect(),
        cloud: CloudStateView {
            change_seq: cloud.change_seq,
            profile_revision: cloud.profile_revision,
            last_sync_at: cloud.last_sync_at,
            last_shortlist_at: cloud.last_shortlist_at,
            feedback: cloud.feedback,
            notification_cursor: cloud.notification_cursor,
        },
    }))
}

async fn logout(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<StatusCode, ApiError> {
    state
        .store()
        .revoke_sessions(&principal.user, Utc::now())
        .await?;
    state.inner.auth.forget(&principal.user).await;
    state
        .usage()
        .record(Some(&principal.user), "logout", serde_json::json!({}));
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_account(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<StatusCode, ApiError> {
    state.store().delete_account(&principal.user).await?;
    state.inner.auth.forget(&principal.user).await;
    tracing::info!(user = %principal.user, "account deleted");
    Ok(StatusCode::NO_CONTENT)
}

fn token_view(t: jobhunt_storage::postgres::ApiToken) -> TokenView {
    TokenView {
        id: t.id,
        name: t.name,
        created_at: t.created_at,
        last_used_at: t.last_used_at,
        expires_at: t.expires_at,
        revoked_at: t.revoked_at,
    }
}

async fn list_tokens(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<Json<TokenList>, ApiError> {
    let tokens = state.store().api_tokens(&principal.user).await?;
    Ok(Json(TokenList {
        tokens: tokens.into_iter().map(token_view).collect(),
    }))
}

async fn create_token(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Body(request): Body<CreateTokenRequest>,
) -> Result<(StatusCode, Json<CreatedToken>), ApiError> {
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::invalid("name must be 1 to 100 characters"));
    }
    let days = request.expires_in_days.unwrap_or(90);
    if !(1..=365).contains(&days) {
        return Err(ApiError::invalid(
            "expires_in_days must be between 1 and 365",
        ));
    }
    let now = Utc::now();
    let created = state
        .store()
        .create_api_token(
            &principal.user,
            name,
            Some(now + chrono::Duration::days(i64::from(days))),
            now,
        )
        .await?;
    state.usage().record(
        Some(&principal.user),
        "token_created",
        serde_json::json!({"days": days}),
    );
    Ok((
        StatusCode::CREATED,
        Json(CreatedToken {
            token: token_view(created.token),
            secret: created.secret,
        }),
    ))
}

async fn revoke_token(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if state
        .store()
        .revoke_api_token(&principal.user, &id, Utc::now())
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("token"))
    }
}

async fn sync_pull(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Body(request): Body<PullRequest>,
) -> Result<Json<PullResponse>, ApiError> {
    if request.protocol != jobhunt_storage::sync::SYNC_PROTOCOL {
        return Err(ApiError::invalid(format!(
            "sync protocol {} is not supported (this server speaks {}); update JobHunt",
            request.protocol,
            jobhunt_storage::sync::SYNC_PROTOCOL
        )));
    }
    let pulled = state
        .user_store(&principal)
        .sync_pull(&request, Utc::now())
        .await?;
    state.usage().record(
        Some(&principal.user),
        "sync_pull",
        serde_json::json!({
            "entities": pulled.entities.len(),
            "feedback": pulled.feedback.len(),
            "jobs": pulled.jobs.len(),
        }),
    );
    Ok(Json(pulled))
}

async fn sync_push(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Body(request): Body<PushRequest>,
) -> Result<Json<PushResponse>, ApiError> {
    let pushed = state
        .user_store(&principal)
        .sync_push(&request, Utc::now())
        .await
        .map_err(|e| match e {
            PushError::Invalid(message) => {
                ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_sync", message)
            }
            PushError::Storage(e) => e.into(),
        })?;
    state.usage().record(
        Some(&principal.user),
        "sync_push",
        serde_json::json!({
            "entities": request.entities.len(),
            "feedback": request.feedback.len(),
            "conflicts": pushed.conflicts.len(),
        }),
    );
    Ok(Json(pushed))
}

async fn search(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Body(p): Body<SearchJobsParams>,
) -> Result<Json<SearchResults>, ApiError> {
    let limit = usize::from(p.limit.unwrap_or(5));
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::invalid(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let request = FindRequest {
        text: p.query.unwrap_or_default(),
        limit,
        all_tiers: p.include_lower_tiers,
        // Discovery runs in the background; `refresh: never` still means
        // "don't verify either".
        refresh: RefreshMode::Never,
        verify: p.refresh != jobhunt_mcp::RefreshInput::Never && p.verify.unwrap_or(true),
    };
    let now = Utc::now();
    let results = run(state.app_for(&principal), move |app| async move {
        let found = app.find(&request, &Quiet, now).await?;
        Ok(SearchResults::of(&found, now))
    })
    .await?;
    state.usage().record(
        Some(&principal.user),
        "find",
        serde_json::json!({"limit": limit, "results": results.results.len()}),
    );
    Ok(Json(results))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailQuery {
    #[serde(default)]
    include_sources: bool,
    #[serde(default)]
    full_description: bool,
}

async fn opportunity(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Path(id): Path<String>,
    Params(q): Params<DetailQuery>,
) -> Result<Json<JobDetail>, ApiError> {
    let detail = run(state.app_for(&principal), move |app| async move {
        let opportunity = app.resolve(&id).await?;
        let inspection = app.inspect(&opportunity, false, Utc::now()).await?;
        Ok(JobDetail::of(
            &inspection,
            q.include_sources,
            q.full_description,
        ))
    })
    .await?;
    Ok(Json(detail))
}

async fn verify(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Path(id): Path<String>,
    Body(request): Body<VerifyRequest>,
) -> Result<Json<VerificationReport>, ApiError> {
    let mode = if request.force {
        VerifyMode::Force
    } else {
        VerifyMode::IfDue
    };
    let report = run(state.app_for(&principal), move |app| async move {
        let opportunity = app.resolve(&id).await?;
        let checked = app.verify(&opportunity, mode, &Quiet, Utc::now()).await?;
        Ok(VerificationReport::of(&opportunity, &checked))
    })
    .await?;
    state.usage().record(
        Some(&principal.user),
        "verify",
        serde_json::json!({"force": request.force}),
    );
    Ok(Json(report))
}

async fn feedback(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Path(id): Path<String>,
    Body(request): Body<FeedbackRequest>,
) -> Result<Json<FeedbackResult>, ApiError> {
    let action = match request.action {
        FeedbackInput::Save => FeedbackAction::Save,
        FeedbackInput::Unsave => FeedbackAction::Unsave,
        FeedbackInput::Reject => FeedbackAction::Reject,
        FeedbackInput::Applied => FeedbackAction::Applied,
        FeedbackInput::Interview => FeedbackAction::Interview,
        FeedbackInput::Offer => FeedbackAction::Offer,
        FeedbackInput::Like => FeedbackAction::Like,
        FeedbackInput::Dislike => FeedbackAction::Dislike,
    };
    let reason = request.reason;
    let with_reason = reason.is_some();
    let result = run(state.app_for(&principal), move |app| async move {
        let opportunity = app.resolve(&id).await?;
        let outcome = app
            .record_feedback(&opportunity, action, reason.as_deref(), Utc::now())
            .await?;
        Ok(FeedbackResult::of(&outcome))
    })
    .await?;
    state.usage().record(
        Some(&principal.user),
        "feedback",
        serde_json::json!({
            "action": action.as_str(),
            "with_reason": with_reason,
            "recorded": result.recorded,
        }),
    );
    Ok(Json(result))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextQuery {
    #[serde(default)]
    include_contact_details: bool,
}

async fn application_context(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Path(id): Path<String>,
    Params(q): Params<ContextQuery>,
) -> Result<Json<ApplicationContext>, ApiError> {
    let context = run(state.app_for(&principal), move |app| async move {
        let opportunity = app.resolve(&id).await?;
        app.application_context(&opportunity, q.include_contact_details, Utc::now())
            .await
    })
    .await?;
    Ok(Json(context))
}

async fn profile(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<Json<ProfileView>, ApiError> {
    let view = run(state.app_for(&principal), |app| async move {
        app.profile_view().await
    })
    .await?;
    Ok(Json(view))
}

async fn preferences(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Body(p): Body<UpdatePreferencesParams>,
) -> Result<Json<PreferenceUpdateResult>, ApiError> {
    let update = PreferenceUpdate {
        statement: p.statement,
        set: p.set,
        remove: p.remove,
    };
    let counts = serde_json::json!({
        "statement": update.statement.is_some(),
        "set": update.set.len(),
        "remove": update.remove.len(),
    });
    let result = run(state.app_for(&principal), move |app| async move {
        let changes = app.update_preferences(&update, Utc::now()).await?;
        Ok(PreferenceUpdateResult::of(&changes))
    })
    .await?;
    state
        .usage()
        .record(Some(&principal.user), "preferences", counts);
    Ok(Json(result))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineQuery {
    #[serde(default)]
    include_rejected: bool,
}

async fn pipeline(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
    Params(q): Params<PipelineQuery>,
) -> Result<Json<PipelineView>, ApiError> {
    let view = run(state.app_for(&principal), move |app| async move {
        Ok(PipelineView::of(&app.pipeline(q.include_rejected).await?))
    })
    .await?;
    Ok(Json(view))
}

async fn export(
    State(state): State<ApiState>,
    axum::Extension(principal): Auth,
) -> Result<Json<StateExport>, ApiError> {
    let export = run(state.app_for(&principal), |app| async move {
        app.export_state(
            Utc::now(),
            Some(format!("jobhunt-cloud {}", env!("CARGO_PKG_VERSION"))),
        )
        .await
    })
    .await?;
    Ok(Json(export))
}

async fn not_found() -> ApiError {
    ApiError::not_found("route")
}

/// The whole service.
pub fn router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/api/v1/account", get(account).delete(delete_account))
        .route("/api/v1/account/logout", post(logout))
        .route("/api/v1/tokens", get(list_tokens).post(create_token))
        .route("/api/v1/tokens/{id}", delete(revoke_token))
        .route("/api/v1/sync/pull", post(sync_pull))
        .route("/api/v1/sync/push", post(sync_push))
        .route("/api/v1/search", post(search))
        .route("/api/v1/opportunities/{id}", get(opportunity))
        .route("/api/v1/opportunities/{id}/verify", post(verify))
        .route("/api/v1/opportunities/{id}/feedback", post(feedback))
        .route(
            "/api/v1/opportunities/{id}/application-context",
            get(application_context),
        )
        .route("/api/v1/profile", get(profile))
        .route("/api/v1/preferences", post(preferences))
        .route("/api/v1/pipeline", get(pipeline))
        .route("/api/v1/export", get(export))
        .nest_service("/mcp", crate::mcp::service(state.clone()))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ));
    let public = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource),
        )
        .route("/api/v1/auth/config", get(auth_config))
        .route("/api/v1/auth/dev-token", post(dev_token));
    let cors = if state.config().allowed_origins.is_empty() {
        CorsLayer::new()
    } else {
        let origins: Vec<http::HeaderValue> = state
            .config()
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([http::Method::GET, http::Method::POST, http::Method::DELETE])
            .allow_headers([http::header::AUTHORIZATION, http::header::CONTENT_TYPE])
    };
    public
        .merge(protected)
        .fallback(not_found)
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(16 * 1024 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(120),
        ))
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(make_span)
                .on_request(())
                .on_response(on_response),
        )
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        .layer(SetRequestIdLayer::new(REQUEST_ID, RandomRequestId))
        .layer(SetSensitiveRequestHeadersLayer::new([
            http::header::AUTHORIZATION,
            http::header::COOKIE,
        ]))
}
