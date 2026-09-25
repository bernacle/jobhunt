//! The client side of JobHunt Cloud, for the CLI (and the tests): API
//! calls, the sync transport, and signing in with the OAuth 2.0 device
//! authorization grant (RFC 8628) against the service's identity provider.
//!
//! Errors are the application's: an unreachable service is
//! [`AppError::CloudUnavailable`] (the CLI keeps working offline), a
//! refused token is [`AppError::Unauthenticated`], and an error answer
//! keeps its stable code's meaning.

use std::time::Duration;

use async_trait::async_trait;
use jobhunt_app::{AppError, SyncTransport};
use jobhunt_storage::sync::{PullRequest, PullResponse, PushRequest, PushResponse};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::api::types::{
    AccountView, AuthConfigView, CreateTokenRequest, CreatedToken, DevTokenRequest,
    DevTokenResponse, ErrorBody, Health, TokenList,
};

/// A JobHunt Cloud server.
#[derive(Clone)]
pub struct CloudClient {
    base: Url,
    http: reqwest::Client,
    token: Option<String>,
}

impl std::fmt::Debug for CloudClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudClient")
            .field("base", &self.base.as_str())
            .field("token", &self.token.as_ref().map(|_| "<set>"))
            .finish()
    }
}

fn unavailable(base: &Url, error: impl std::fmt::Display) -> AppError {
    AppError::CloudUnavailable(format!("could not reach JobHunt Cloud at {base}: {error}"))
}

/// Normalizes a server address typed by a person (`api.example.com`,
/// `https://api.example.com/`).
pub fn server_url(input: &str) -> Result<Url, AppError> {
    let input = input.trim();
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&with_scheme)
        .map_err(|e| AppError::InvalidArguments(format!("{input:?} is not a server URL: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::InvalidArguments(format!(
            "{input:?} is not an http(s) URL"
        )));
    }
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
    );
    if url.scheme() == "http" && !local {
        return Err(AppError::InvalidArguments(
            "JobHunt Cloud must be reached over https (http is only allowed for localhost)".into(),
        ));
    }
    Ok(url)
}

impl CloudClient {
    pub fn new(base: Url) -> Result<Self, AppError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("jobhunt/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AppError::Config(format!("HTTP client: {e}")))?;
        Ok(Self {
            base,
            http,
            token: None,
        })
    }

    /// The same client, sending `token` as its bearer token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn base(&self) -> &Url {
        &self.base
    }

    fn url(&self, path: &str) -> Result<Url, AppError> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(|e| AppError::InvalidArguments(e.to_string()))
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Option<T>, AppError> {
        let request = match &self.token {
            Some(t) => request.bearer_auth(t),
            None => request,
        };
        let response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| unavailable(&self.base, e))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| unavailable(&self.base, e))?;
        if status.is_success() {
            if bytes.is_empty() || status == reqwest::StatusCode::NO_CONTENT {
                return Ok(None);
            }
            return serde_json::from_slice(&bytes).map(Some).map_err(|e| {
                AppError::CloudUnavailable(format!("JobHunt Cloud sent an unexpected answer: {e}"))
            });
        }
        let error: Option<ErrorBody> = serde_json::from_slice(&bytes).ok();
        let (code, message) = error.map_or_else(
            || (String::new(), format!("JobHunt Cloud answered {status}")),
            |b| (b.error.code, b.error.message),
        );
        Err(match (status.as_u16(), code.as_str()) {
            (401, _) => AppError::Unauthenticated(message),
            (_, "conflict") => AppError::Conflict(message),
            (_, "unknown_opportunity") => AppError::UnknownOpportunity { input: message },
            (_, "no_profile") => AppError::NoProfile,
            (400 | 404 | 422, _) => AppError::InvalidArguments(message),
            _ => AppError::CloudUnavailable(message),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, AppError> {
        self.send(self.http.get(self.url(path)?))
            .await?
            .ok_or_else(|| AppError::CloudUnavailable("JobHunt Cloud sent no answer".into()))
    }

    async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, AppError> {
        let json =
            serde_json::to_vec(body).map_err(|e| AppError::InvalidArguments(e.to_string()))?;
        self.send(
            self.http
                .post(self.url(path)?)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(json),
        )
        .await?
        .ok_or_else(|| AppError::CloudUnavailable("JobHunt Cloud sent no answer".into()))
    }

    async fn call_empty(&self, request: reqwest::RequestBuilder) -> Result<(), AppError> {
        self.send::<serde_json::Value>(request).await.map(|_| ())
    }

    pub async fn health(&self) -> Result<Health, AppError> {
        self.get("/health").await
    }

    pub async fn auth_config(&self) -> Result<AuthConfigView, AppError> {
        self.get("/api/v1/auth/config").await
    }

    pub async fn dev_token(&self, subject: &str) -> Result<DevTokenResponse, AppError> {
        self.post(
            "/api/v1/auth/dev-token",
            &DevTokenRequest {
                subject: subject.to_owned(),
            },
        )
        .await
    }

    pub async fn account(&self) -> Result<AccountView, AppError> {
        self.get("/api/v1/account").await
    }

    /// Revokes every session and token of the account.
    pub async fn logout_everywhere(&self) -> Result<(), AppError> {
        self.call_empty(self.http.post(self.url("/api/v1/account/logout")?))
            .await
    }

    pub async fn tokens(&self) -> Result<TokenList, AppError> {
        self.get("/api/v1/tokens").await
    }

    pub async fn create_token(
        &self,
        name: &str,
        expires_in_days: Option<u32>,
    ) -> Result<CreatedToken, AppError> {
        self.post(
            "/api/v1/tokens",
            &CreateTokenRequest {
                name: name.to_owned(),
                expires_in_days,
            },
        )
        .await
    }

    pub async fn revoke_token(&self, id: &str) -> Result<(), AppError> {
        self.call_empty(self.http.delete(self.url(&format!("/api/v1/tokens/{id}"))?))
            .await
    }
}

#[async_trait]
impl SyncTransport for CloudClient {
    async fn pull(&self, request: &PullRequest) -> Result<PullResponse, AppError> {
        self.post("/api/v1/sync/pull", request).await
    }

    async fn push(&self, request: &PushRequest) -> Result<PushResponse, AppError> {
        self.post("/api/v1/sync/push", request).await
    }
}

/// What the person must do to finish signing in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    #[serde(default)]
    pub interval: Option<u64>,
}

/// Tokens from the identity provider.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Signs in with the OAuth 2.0 device authorization grant.
#[derive(Debug, Clone)]
pub struct DeviceFlow {
    http: reqwest::Client,
    device_endpoint: String,
    token_endpoint: String,
    client_id: String,
    scopes: String,
    audience: Option<String>,
}

impl DeviceFlow {
    /// The flow the server advertises, if it supports one.
    pub fn from_config(config: &AuthConfigView) -> Result<Self, AppError> {
        let missing = |what: &str| {
            AppError::Config(format!(
                "this JobHunt Cloud server does not support CLI sign-in ({what} is not \
                 configured); use a personal access token: jobhunt login --token"
            ))
        };
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| AppError::Config(format!("HTTP client: {e}")))?,
            device_endpoint: config
                .device_authorization_endpoint
                .clone()
                .ok_or_else(|| missing("device authorization"))?,
            token_endpoint: config
                .token_endpoint
                .clone()
                .ok_or_else(|| missing("the token endpoint"))?,
            client_id: config
                .cli_client_id
                .clone()
                .ok_or_else(|| missing("the CLI client id"))?,
            scopes: config
                .scopes
                .clone()
                .unwrap_or_else(|| "openid offline_access".into()),
            audience: config.audience.clone(),
        })
    }

    async fn form<T: DeserializeOwned>(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> Result<Result<T, OAuthError>, AppError> {
        let body = form
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| AppError::CloudUnavailable(format!("identity provider: {e}")))?;
        let ok = response.status().is_success();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::CloudUnavailable(format!("identity provider: {e}")))?;
        if ok {
            serde_json::from_slice(&bytes).map(Ok).map_err(|e| {
                AppError::CloudUnavailable(format!("unexpected identity provider answer: {e}"))
            })
        } else {
            serde_json::from_slice::<OAuthError>(&bytes)
                .map(Err)
                .map_err(|_| AppError::CloudUnavailable("the identity provider refused".into()))
        }
    }

    /// Starts the flow: the code the person enters at the provider.
    pub async fn start(&self) -> Result<DeviceCode, AppError> {
        let mut form = vec![
            ("client_id", self.client_id.as_str()),
            ("scope", self.scopes.as_str()),
        ];
        if let Some(a) = &self.audience {
            form.push(("audience", a.as_str()));
        }
        self.form(&self.device_endpoint, &form).await?.map_err(|e| {
            AppError::Unauthenticated(format!(
                "the identity provider refused to start sign-in: {}",
                e.error_description.unwrap_or(e.error)
            ))
        })
    }

    /// Polls until the person approves (or the code expires).
    pub async fn wait(&self, code: &DeviceCode) -> Result<TokenSet, AppError> {
        let mut interval = Duration::from_secs(code.interval.unwrap_or(5).max(1));
        let deadline = std::time::Instant::now() + Duration::from_secs(code.expires_in);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(AppError::Unauthenticated(
                    "the sign-in code expired; run `jobhunt login` again".into(),
                ));
            }
            tokio::time::sleep(interval).await;
            let answer = self
                .form::<TokenSet>(
                    &self.token_endpoint,
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("device_code", &code.device_code),
                        ("client_id", &self.client_id),
                    ],
                )
                .await?;
            match answer {
                Ok(tokens) => return Ok(tokens),
                Err(e) if e.error == "authorization_pending" => {}
                Err(e) if e.error == "slow_down" => interval += Duration::from_secs(5),
                Err(e) => {
                    return Err(AppError::Unauthenticated(format!(
                        "sign-in failed: {}",
                        e.error_description.unwrap_or(e.error)
                    )));
                }
            }
        }
    }

    /// A new access token from a refresh token.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet, AppError> {
        self.form::<TokenSet>(
            &self.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.client_id),
            ],
        )
        .await?
        .map_err(|e| {
            AppError::Unauthenticated(format!(
                "the session expired ({}); run `jobhunt login` again",
                e.error
            ))
        })
    }

    /// Revokes a refresh token at the provider (RFC 7009), when it has a
    /// revocation endpoint. Best effort.
    pub async fn revoke(&self, endpoint: &str, refresh_token: &str) -> Result<(), AppError> {
        let _ = self
            .form::<serde_json::Value>(
                endpoint,
                &[
                    ("token", refresh_token),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", &self.client_id),
                ],
            )
            .await?;
        Ok(())
    }
}

/// `application/x-www-form-urlencoded` encoding of one value.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_urls_are_normalized_and_https_only() {
        assert_eq!(
            server_url("api.example.com").unwrap().as_str(),
            "https://api.example.com/"
        );
        assert!(server_url("http://api.example.com").is_err());
        assert!(server_url("http://127.0.0.1:8080").is_ok());
        assert!(server_url("ftp://x").is_err());
    }

    #[test]
    fn form_values_are_encoded() {
        assert_eq!(encode("openid offline_access"), "openid+offline_access");
        assert_eq!(encode("a&b=c/d"), "a%26b%3Dc%2Fd");
    }
}
