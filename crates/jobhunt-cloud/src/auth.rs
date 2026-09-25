//! Authentication: who is making a request.
//!
//! JobHunt keeps no passwords. People sign in with an OpenID Connect
//! identity provider; requests carry the provider's access token (a JWT)
//! or a JobHunt personal access token (`jh_pat_…`, for MCP clients and
//! scripts without OAuth). Either way the request is resolved to a
//! [`Principal`]: the internal account id, never the provider's data.
//!
//! * [`OidcVerifier`] verifies JWTs against the issuer's published keys
//!   (OpenID Connect discovery, else OAuth authorization server metadata
//!   (RFC 8414), else a JWKS URL set by hand; the keys are cached and an
//!   unknown key id triggers one refresh, rate limited). Only asymmetric
//!   algorithms are accepted; the issuer, the audience (the API's
//!   identifier) and the expiry are checked.
//! * [`DevVerifier`] signs and verifies HS256 tokens with a shared secret,
//!   for local development and tests; the configuration refuses it in
//!   production.
//! * [`Authenticator`] turns a bearer token into a principal: personal
//!   access tokens are looked up by digest, JWT identities are mapped to
//!   accounts (created on first sign-in), and tokens issued before the
//!   account's "logged out everywhere" instant are refused.
//!
//! Domain code never sees any of this: use cases receive a user-scoped
//! store.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use jobhunt_storage::postgres::{PgStore, TOKEN_PREFIX, UserId};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use url::Url;

use crate::config::OidcSettings;

/// How a request authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// An identity provider's access token.
    Oidc,
    /// A personal access token.
    Token,
    /// A development token.
    Dev,
}

/// The authenticated account of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user: UserId,
    pub method: Method,
}

/// A verified identity-provider token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
    pub issued_at: Option<DateTime<Utc>>,
}

/// Why a request is not authenticated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("sign in to use JobHunt Cloud (no bearer token)")]
    Missing,
    #[error("the access token is not valid: {0}")]
    Invalid(String),
    #[error("the access token has expired; sign in again")]
    Expired,
    #[error("this session was signed out; sign in again")]
    Revoked,
    #[error("the identity provider could not be reached to check the token")]
    Unavailable(String),
}

/// Verifies identity-provider tokens.
#[async_trait]
pub trait TokenVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<Identity, AuthError>;
}

#[derive(Debug, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    #[serde(default)]
    iat: Option<i64>,
}

fn identity_of(claims: Claims) -> Identity {
    Identity {
        issuer: claims.iss,
        subject: claims.sub,
        issued_at: claims.iat.and_then(|t| Utc.timestamp_opt(t, 0).single()),
    }
}

fn classify(error: &jsonwebtoken::errors::Error) -> AuthError {
    use jsonwebtoken::errors::ErrorKind as K;
    match error.kind() {
        K::ExpiredSignature => AuthError::Expired,
        K::InvalidAudience => AuthError::Invalid("issued for another audience".into()),
        K::InvalidIssuer => AuthError::Invalid("issued by another issuer".into()),
        K::InvalidSignature => AuthError::Invalid("bad signature".into()),
        K::ImmatureSignature => AuthError::Invalid("not valid yet".into()),
        _ => AuthError::Invalid("malformed token".into()),
    }
}

/// The parts of the provider's metadata JobHunt uses (OpenID Connect
/// discovery and RFC 8414 share these names).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub issuer: String,
    #[serde(default)]
    pub jwks_uri: Option<String>,
    #[serde(default)]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub device_authorization_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub authorization_endpoint: Option<String>,
}

/// JWKS kept this long before it is fetched again.
const KEYS_TTL: Duration = Duration::from_secs(600);
/// An unknown key id fetches the keys again at most this often.
const REFRESH_MIN: Duration = Duration::from_secs(60);

struct Keys {
    set: JwkSet,
    fetched: Instant,
}

/// Verifies access tokens from an OpenID Connect provider.
pub struct OidcVerifier {
    settings: OidcSettings,
    http: reqwest::Client,
    metadata: RwLock<Option<ProviderMetadata>>,
    keys: RwLock<Option<Keys>>,
    refreshing: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for OidcVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcVerifier")
            .field("issuer", &self.settings.issuer)
            .field("audiences", &self.settings.audiences)
            .finish_non_exhaustive()
    }
}

impl OidcVerifier {
    pub fn new(settings: OidcSettings) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("jobhunt-cloud/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AuthError::Unavailable(e.to_string()))?;
        Ok(Self {
            settings,
            http,
            metadata: RwLock::new(None),
            keys: RwLock::new(None),
            refreshing: Mutex::new(None),
        })
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, AuthError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AuthError::Unavailable(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AuthError::Unavailable(format!(
                "{url} answered {}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AuthError::Unavailable(e.to_string()))?;
        serde_json::from_slice(&bytes).map_err(|e| AuthError::Unavailable(format!("{url}: {e}")))
    }

    /// The provider's metadata (fetched once), with the endpoints set by
    /// hand taking precedence. With a JWKS URL set by hand there is no
    /// discovery at all.
    pub async fn metadata(&self) -> Result<ProviderMetadata, AuthError> {
        if let Some(m) = self.metadata.read().await.as_ref() {
            return Ok(m.clone());
        }
        let manual = &self.settings.endpoints;
        let mut metadata = if manual.jwks_uri.is_some() {
            ProviderMetadata {
                issuer: self.settings.issuer.clone(),
                jwks_uri: None,
                token_endpoint: None,
                device_authorization_endpoint: None,
                revocation_endpoint: None,
                authorization_endpoint: None,
            }
        } else {
            self.discover().await?
        };
        let set = |slot: &mut Option<String>, value: &Option<String>| {
            if value.is_some() {
                slot.clone_from(value);
            }
        };
        set(&mut metadata.jwks_uri, &manual.jwks_uri);
        set(&mut metadata.token_endpoint, &manual.token);
        set(
            &mut metadata.device_authorization_endpoint,
            &manual.device_authorization,
        );
        set(&mut metadata.revocation_endpoint, &manual.revocation);
        if metadata.jwks_uri.is_none() {
            return Err(AuthError::Unavailable(
                "the provider publishes no jwks_uri; set JOBHUNT_OIDC_JWKS_URL".into(),
            ));
        }
        *self.metadata.write().await = Some(metadata.clone());
        Ok(metadata)
    }

    /// OpenID Connect discovery, then RFC 8414 authorization server
    /// metadata (WorkOS AuthKit publishes the latter).
    async fn discover(&self) -> Result<ProviderMetadata, AuthError> {
        let issuer = &self.settings.issuer;
        let mut failures = Vec::new();
        for url in metadata_urls(issuer) {
            match self.get_json::<ProviderMetadata>(&url).await {
                Ok(metadata) if same_issuer(&metadata.issuer, issuer) => return Ok(metadata),
                Ok(metadata) => {
                    return Err(AuthError::Unavailable(format!(
                        "the provider says its issuer is {:?}, not {issuer:?}; fix \
                         JOBHUNT_OIDC_ISSUER",
                        metadata.issuer
                    )));
                }
                Err(e) => failures.push(e.to_string()),
            }
        }
        Err(AuthError::Unavailable(format!(
            "no provider metadata for {issuer} ({}); set JOBHUNT_OIDC_JWKS_URL if it \
             publishes none",
            failures.join("; ")
        )))
    }

    async fn fetch_keys(&self) -> Result<(), AuthError> {
        let metadata = self.metadata().await?;
        let jwks_uri = metadata.jwks_uri.as_deref().unwrap_or_default();
        let set: JwkSet = self.get_json(jwks_uri).await?;
        *self.keys.write().await = Some(Keys {
            set,
            fetched: Instant::now(),
        });
        Ok(())
    }

    async fn key(&self, kid: Option<&str>) -> Result<DecodingKey, AuthError> {
        let stale = self
            .keys
            .read()
            .await
            .as_ref()
            .is_none_or(|k| k.fetched.elapsed() > KEYS_TTL);
        if stale {
            self.fetch_keys().await?;
        }
        let find = |set: &JwkSet| match kid {
            Some(kid) => set.find(kid).cloned(),
            None if set.keys.len() == 1 => set.keys.first().cloned(),
            None => None,
        };
        let mut jwk = self.keys.read().await.as_ref().and_then(|k| find(&k.set));
        if jwk.is_none() {
            // Keys rotate: fetch again, but not on every bad token.
            let mut last = self.refreshing.lock().await;
            if last.is_none_or(|t| t.elapsed() > REFRESH_MIN) {
                *last = Some(Instant::now());
                self.fetch_keys().await?;
                jwk = self.keys.read().await.as_ref().and_then(|k| find(&k.set));
            }
        }
        let jwk = jwk.ok_or_else(|| AuthError::Invalid("signed with an unknown key".into()))?;
        DecodingKey::from_jwk(&jwk).map_err(|_| AuthError::Invalid("unusable signing key".into()))
    }
}

#[async_trait]
impl TokenVerifier for OidcVerifier {
    async fn verify(&self, token: &str) -> Result<Identity, AuthError> {
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| AuthError::Invalid("not a JWT".into()))?;
        let asymmetric = matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        );
        if !asymmetric {
            return Err(AuthError::Invalid(format!(
                "algorithm {:?} is not accepted",
                header.alg
            )));
        }
        let key = self.key(header.kid.as_deref()).await?;
        // The configured issuer, and the provider's own spelling of it
        // (they may differ by a trailing slash).
        let metadata = self.metadata().await?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.settings.issuer.as_str(), metadata.issuer.as_str()]);
        validation.set_audience(&self.settings.audiences);
        validation.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
        validation.leeway = 60;
        let data =
            jsonwebtoken::decode::<Claims>(token, &key, &validation).map_err(|e| classify(&e))?;
        Ok(identity_of(data.claims))
    }
}

/// Where an issuer's metadata may be: OpenID Connect discovery, then RFC
/// 8414 (the well-known segment goes between the host and the issuer's
/// path), then the common variant that appends it to the issuer.
fn metadata_urls(issuer: &str) -> Vec<String> {
    let trimmed = issuer.trim_end_matches('/');
    let mut urls = vec![format!("{trimmed}/.well-known/openid-configuration")];
    if let Ok(url) = Url::parse(issuer) {
        let origin = url.origin().ascii_serialization();
        let path = url.path().trim_end_matches('/');
        urls.push(format!(
            "{origin}/.well-known/oauth-authorization-server{path}"
        ));
        if !path.is_empty() {
            urls.push(format!("{trimmed}/.well-known/oauth-authorization-server"));
        }
    }
    urls
}

/// The same issuer, ignoring a trailing slash.
fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The issuer of development tokens.
pub const DEV_ISSUER: &str = "jobhunt-dev";
const DEV_AUDIENCE: &str = "jobhunt";

/// Development-only tokens signed with a shared secret.
#[derive(Clone)]
pub struct DevVerifier {
    secret: String,
}

impl std::fmt::Debug for DevVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DevVerifier")
    }
}

#[derive(Serialize)]
struct DevClaims<'a> {
    iss: &'a str,
    aud: &'a str,
    sub: &'a str,
    iat: i64,
    exp: i64,
}

impl DevVerifier {
    pub fn new(secret: String) -> Self {
        Self { secret }
    }

    /// A token for `subject`, valid for `ttl`.
    pub fn mint(&self, subject: &str, ttl: chrono::Duration) -> Result<String, AuthError> {
        let now = Utc::now();
        jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &DevClaims {
                iss: DEV_ISSUER,
                aud: DEV_AUDIENCE,
                sub: subject,
                iat: now.timestamp(),
                exp: (now + ttl).timestamp(),
            },
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| AuthError::Invalid(e.to_string()))
    }
}

#[async_trait]
impl TokenVerifier for DevVerifier {
    async fn verify(&self, token: &str) -> Result<Identity, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[DEV_ISSUER]);
        validation.set_audience(&[DEV_AUDIENCE]);
        validation.leeway = 5;
        let data = jsonwebtoken::decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| classify(&e))?;
        Ok(identity_of(data.claims))
    }
}

/// How long a resolved identity is trusted before the account is read
/// again (so "log out everywhere" takes effect within this).
const IDENTITY_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Cached {
    user: UserId,
    tokens_valid_after: Option<DateTime<Utc>>,
    at: Instant,
}

/// Resolves bearer tokens to principals.
pub struct Authenticator {
    verifier: Arc<dyn TokenVerifier>,
    method: Method,
    store: PgStore,
    identities: Mutex<HashMap<(String, String), Cached>>,
}

impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl Authenticator {
    pub fn new(verifier: Arc<dyn TokenVerifier>, method: Method, store: PgStore) -> Self {
        Self {
            verifier,
            method,
            store,
            identities: Mutex::new(HashMap::new()),
        }
    }

    /// Forgets cached identities of an account (after it logs out).
    pub async fn forget(&self, user: &UserId) {
        self.identities.lock().await.retain(|_, c| &c.user != user);
    }

    /// The principal of a bearer token.
    pub async fn authenticate(&self, bearer: &str) -> Result<Principal, AuthError> {
        let now = Utc::now();
        if bearer.starts_with(TOKEN_PREFIX) {
            let check = self
                .store
                .check_api_token(bearer, now)
                .await
                .map_err(|e| AuthError::Unavailable(e.to_string()))?
                .ok_or_else(|| AuthError::Invalid("unknown, expired or revoked token".into()))?;
            return Ok(Principal {
                user: check.user,
                method: Method::Token,
            });
        }
        let identity = self.verifier.verify(bearer).await?;
        let key = (identity.issuer.clone(), identity.subject.clone());
        let cached = self
            .identities
            .lock()
            .await
            .get(&key)
            .filter(|c| c.at.elapsed() < IDENTITY_TTL)
            .cloned();
        let cached = match cached {
            Some(c) => c,
            None => {
                let account = self
                    .store
                    .sign_in(&identity.issuer, &identity.subject, now)
                    .await
                    .map_err(|e| AuthError::Unavailable(e.to_string()))?;
                if account.created_now {
                    tracing::info!(user = %account.id, "account created");
                }
                let c = Cached {
                    user: account.id,
                    tokens_valid_after: account.tokens_valid_after,
                    at: Instant::now(),
                };
                self.identities.lock().await.insert(key, c.clone());
                c
            }
        };
        if let (Some(after), Some(issued)) = (cached.tokens_valid_after, identity.issued_at)
            && issued < after
        {
            return Err(AuthError::Revoked);
        }
        Ok(Principal {
            user: cached.user,
            method: self.method,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dev_tokens_round_trip_and_are_checked() {
        let dev = DevVerifier::new("0123456789abcdef0123456789abcdef".into());
        let token = dev.mint("alice", chrono::Duration::minutes(5)).unwrap();
        let identity = dev.verify(&token).await.unwrap();
        assert_eq!(identity.subject, "alice");
        assert_eq!(identity.issuer, DEV_ISSUER);
        let other = DevVerifier::new("another secret, 32 characters...".into());
        assert!(matches!(
            other.verify(&token).await,
            Err(AuthError::Invalid(_))
        ));
        let expired = dev.mint("alice", chrono::Duration::minutes(-5)).unwrap();
        assert_eq!(dev.verify(&expired).await, Err(AuthError::Expired));
        assert!(dev.verify("garbage").await.is_err());
    }

    #[test]
    fn metadata_is_looked_up_where_providers_publish_it() {
        assert_eq!(
            metadata_urls("https://acme.authkit.app"),
            [
                "https://acme.authkit.app/.well-known/openid-configuration",
                "https://acme.authkit.app/.well-known/oauth-authorization-server",
            ]
        );
        assert_eq!(
            metadata_urls("https://id.example.com/tenants/7/"),
            [
                "https://id.example.com/tenants/7/.well-known/openid-configuration",
                "https://id.example.com/.well-known/oauth-authorization-server/tenants/7",
                "https://id.example.com/tenants/7/.well-known/oauth-authorization-server",
            ]
        );
        assert!(same_issuer("https://a.test/", "https://a.test"));
        assert!(!same_issuer("https://a.test", "https://b.test"));
    }
}
