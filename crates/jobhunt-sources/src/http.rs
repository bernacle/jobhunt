//! Shared HTTP client for source adapters: one connection pool, a clear
//! User-Agent, timeouts, and bounded retries for transient failures.

use std::time::Duration;

use jobhunt_core::SourceError;
use tracing::{debug, warn};
use url::Url;

const USER_AGENT: &str = concat!("jobhunt/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpSettings {
    /// Total time allowed for one request, including reading the body.
    pub timeout: Duration,
    pub connect_timeout: Duration,
    /// Retries after the first attempt for transient failures
    /// (timeouts, connection errors, HTTP 429 and 5xx).
    pub max_retries: u32,
    /// Delay before the first retry; doubles on each subsequent retry.
    pub retry_base_delay: Duration,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            max_retries: 2,
            retry_base_delay: Duration::from_millis(500),
        }
    }
}

/// Cheap to clone; clones share the connection pool.
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    settings: HttpSettings,
}

#[derive(Debug, thiserror::Error)]
#[error("could not initialize the HTTP client")]
pub struct HttpClientError(#[source] reqwest::Error);

impl HttpClient {
    pub fn new(settings: HttpSettings) -> Result<Self, HttpClientError> {
        let inner = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(settings.timeout)
            .connect_timeout(settings.connect_timeout)
            .gzip(true)
            .build()
            .map_err(HttpClientError)?;
        Ok(Self { inner, settings })
    }

    /// GETs `url` and returns the body of a successful (2xx) response.
    ///
    /// Non-success statuses become [`SourceError::Status`]; transport
    /// failures become [`SourceError::Request`]. Transient failures are
    /// retried with exponential backoff first.
    pub async fn get_bytes(&self, url: &Url) -> Result<Vec<u8>, SourceError> {
        let mut attempt = 0;
        loop {
            match self.attempt(url).await {
                Ok(body) => return Ok(body),
                Err(failure) if failure.is_transient() && attempt < self.settings.max_retries => {
                    let delay = self.settings.retry_base_delay * 2u32.saturating_pow(attempt);
                    attempt += 1;
                    warn!(
                        url = %url,
                        attempt,
                        delay_ms = delay.as_millis() as u64,
                        reason = %failure,
                        "transient HTTP failure, retrying"
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(failure) => return Err(failure.into_source_error(url)),
            }
        }
    }

    async fn attempt(&self, url: &Url) -> Result<Vec<u8>, Failure> {
        debug!(url = %url, "GET");
        let response = self
            .inner
            .get(url.clone())
            .send()
            .await
            .map_err(Failure::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(Failure::Status(status.as_u16()));
        }
        let body = response.bytes().await.map_err(Failure::Transport)?;
        debug!(url = %url, bytes = body.len(), "response received");
        Ok(body.to_vec())
    }
}

enum Failure {
    Status(u16),
    Transport(reqwest::Error),
}

impl Failure {
    fn is_transient(&self) -> bool {
        match self {
            Self::Status(status) => *status == 429 || (500..=599).contains(status),
            Self::Transport(error) => error.is_timeout() || error.is_connect() || error.is_body(),
        }
    }

    fn into_source_error(self, url: &Url) -> SourceError {
        match self {
            Self::Status(status) => SourceError::Status {
                url: url.to_string(),
                status,
            },
            Self::Transport(error) => SourceError::Request {
                url: url.to_string(),
                source: Box::new(error.without_url()),
            },
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(status) => write!(f, "HTTP {status}"),
            Self::Transport(error) => write!(f, "{}", jobhunt_core::ErrorChain(error)),
        }
    }
}
