//! Request and response bodies of the HTTP API that are not already the
//! application's views. The CLI uses these same types, so client and
//! server cannot drift apart. Product answers (search results, job details,
//! verification reports, feedback results, the profile, preferences, the
//! pipeline, application context) are the exact views the MCP tools
//! return (`jobhunt_app::views` and friends).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `GET /api/v1/auth/config`: how to sign in (public).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthConfigView {
    /// `oidc` or `dev`.
    pub mode: String,
    /// The identity provider (OIDC issuer URL), when `oidc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// The audience to request tokens for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    /// The request parameter that carries it (`resource`, RFC 8707, or
    /// `audience`); absent means none is sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_parameter: Option<String>,
    /// The public client the CLI signs in with (device authorization).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_authorization_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
}

/// `POST /api/v1/auth/dev-token` (development servers only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DevTokenRequest {
    /// Who to sign in as (any name; it becomes the identity's subject).
    pub subject: String,
}

/// A token issued by a development server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevTokenResponse {
    pub access_token: String,
    pub expires_in: u64,
}

/// One identity-provider identity linked to the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IdentityView {
    pub issuer: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: DateTime<Utc>,
}

/// Where the account's data stands in the cloud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CloudStateView {
    /// Changes recorded so far (the sync cursor's upper bound).
    pub change_seq: u64,
    pub profile_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sync_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_shortlist_at: Option<DateTime<Utc>>,
    /// Feedback events (not counting "seen" marks).
    pub feedback: u64,
    /// For notifications (BRU-295): the last recommendation change covered.
    pub notification_cursor: u64,
}

/// `GET /api/v1/account`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountView {
    /// `usr_…`: the internal account id.
    pub id: String,
    pub created_at: DateTime<Utc>,
    /// How this request authenticated: `oidc`, `token` or `dev`.
    pub authenticated_with: String,
    pub identities: Vec<IdentityView>,
    pub cloud: CloudStateView,
}

/// A personal access token (never its secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TokenView {
    /// `tok_…`.
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
}

/// `POST /api/v1/tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTokenRequest {
    /// What the token is for ("Claude Desktop").
    pub name: String,
    /// Days until it expires (default 90, at most 365).
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

/// A token just created: the only time its secret is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CreatedToken {
    pub token: TokenView,
    /// `jh_pat_…`: send it as `Authorization: Bearer <secret>`.
    pub secret: String,
}

/// `GET /api/v1/tokens`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TokenList {
    pub tokens: Vec<TokenView>,
}

/// Feedback actions accepted by the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackInput {
    Save,
    Unsave,
    Reject,
    Applied,
    Interview,
    Offer,
    Like,
    Dislike,
}

/// `POST /api/v1/opportunities/{id}/feedback`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackRequest {
    pub action: FeedbackInput,
    /// Why, in the person's own words. Stored verbatim (encrypted).
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/v1/opportunities/{id}/verify`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyRequest {
    /// Ask the sources even if they were asked in the last few minutes.
    #[serde(default)]
    pub force: bool,
}

/// `GET /health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
    pub status: String,
    pub version: String,
}

/// `GET /ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Ready {
    /// `ready` or `not_ready`.
    pub status: String,
    pub database: String,
    pub schema: String,
}

/// Every error: `{"error": {"code", "message", "hint", "request_id"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

/// An error, with a stable `code` (the same codes as the CLI and the MCP
/// tools: `unknown_opportunity`, `no_profile`, `conflict`, …, plus
/// `unauthenticated`, `forbidden`, `not_found`, `invalid_request`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}
