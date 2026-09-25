//! Shared HTTP client for source adapters: one connection pool, a clear
//! User-Agent, timeouts, bounded retries for transient failures (honoring
//! `Retry-After`), a per-host limit on concurrent requests, and conditional
//! requests (`If-None-Match` / `304 Not Modified`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jobhunt_core::SourceError;
use reqwest::StatusCode;
use reqwest::header::{ETAG, HeaderMap, IF_NONE_MATCH, RETRY_AFTER};
use tokio::sync::Semaphore;
use tracing::{debug, warn};
use url::Url;

const USER_AGENT: &str = concat!("jobhunt/", env!("CARGO_PKG_VERSION"), " (job discovery)");

/// Longest `Retry-After` the client is willing to wait before a retry.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

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
    /// Requests in flight to one host at the same time, across all sources.
    pub max_per_host: usize,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            max_retries: 2,
            retry_base_delay: Duration::from_millis(500),
            max_per_host: 4,
        }
    }
}

/// Response of a conditional GET.
#[derive(Debug)]
pub enum Conditional {
    Modified {
        body: Vec<u8>,
        /// The response's `ETag`, to send next time.
        etag: Option<String>,
    },
    /// `304 Not Modified`: the resource still matches the given `ETag`.
    NotModified,
}

/// Response of [`HttpClient::probe`]: whatever status the URL answered
/// with, after redirects.
#[derive(Debug)]
pub struct Probe {
    pub status: u16,
    /// Where the redirects (if any) ended.
    pub final_url: Url,
    pub body: Vec<u8>,
}

/// Cheap to clone; clones share the connection pool and host limits.
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    settings: HttpSettings,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
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
        Ok(Self {
            inner,
            settings,
            hosts: Arc::default(),
        })
    }

    /// GETs `url` and returns the body of a successful (2xx) response.
    ///
    /// Non-success statuses become [`SourceError::Status`]; timeouts
    /// [`SourceError::Timeout`]; other transport failures
    /// [`SourceError::Request`]. Transient failures are
    /// retried with exponential backoff first.
    pub async fn get_bytes(&self, url: &Url) -> Result<Vec<u8>, SourceError> {
        match self.get_conditional(url, None).await? {
            Conditional::Modified { body, .. } => Ok(body),
            // Without If-None-Match a server has no reason to answer 304.
            Conditional::NotModified => Err(SourceError::Status {
                url: url.to_string(),
                status: 304,
            }),
        }
    }

    /// Like [`HttpClient::get_bytes`], but sends `If-None-Match: <etag>`
    /// when an ETag is given and reports `304 Not Modified` as such.
    pub async fn get_conditional(
        &self,
        url: &Url,
        etag: Option<&str>,
    ) -> Result<Conditional, SourceError> {
        let mut attempt = 0;
        loop {
            let result = {
                let _permit = self.host_permit(url).await;
                self.attempt(url, etag).await
            };
            match result {
                Ok(response) => return Ok(response),
                Err(failure) if failure.is_transient() && attempt < self.settings.max_retries => {
                    let backoff = self.settings.retry_base_delay * 2u32.saturating_pow(attempt);
                    let delay = failure
                        .retry_after()
                        .map_or(backoff, |wait| wait.min(MAX_RETRY_AFTER).max(backoff));
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

    /// GETs `url` and reports the answer whatever its status (a 404 is an
    /// answer, not an error), following redirects. Transient failures (429,
    /// 5xx, timeouts, connection errors) are retried first like every
    /// request; a status that stays transient is returned as the answer.
    pub async fn probe(&self, url: &Url) -> Result<Probe, SourceError> {
        let mut attempt = 0;
        loop {
            let result = {
                let _permit = self.host_permit(url).await;
                self.probe_once(url).await
            };
            let transient = match &result {
                Ok(probe) => probe.status == 429 || (500..=599).contains(&probe.status),
                Err(failure) => failure.is_transient(),
            };
            if transient && attempt < self.settings.max_retries {
                let delay = self.settings.retry_base_delay * 2u32.saturating_pow(attempt);
                attempt += 1;
                warn!(url = %url, attempt, "transient answer to a probe, retrying");
                tokio::time::sleep(delay).await;
                continue;
            }
            return result.map_err(|failure| failure.into_source_error(url));
        }
    }

    async fn probe_once(&self, url: &Url) -> Result<Probe, Failure> {
        debug!(url = %url, "GET (probe)");
        let response = self
            .inner
            .get(url.clone())
            .send()
            .await
            .map_err(Failure::Transport)?;
        let status = response.status().as_u16();
        let final_url = response.url().clone();
        let body = response.bytes().await.map_err(Failure::Transport)?;
        debug!(url = %url, status, final_url = %final_url, "probe answered");
        Ok(Probe {
            status,
            final_url,
            body: body.to_vec(),
        })
    }

    async fn host_permit(&self, url: &Url) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let host = url.host_str()?.to_owned();
        let semaphore = {
            let mut hosts = match self.hosts.lock() {
                Ok(hosts) => hosts,
                Err(poisoned) => poisoned.into_inner(),
            };
            Arc::clone(
                hosts
                    .entry(host)
                    .or_insert_with(|| Arc::new(Semaphore::new(self.settings.max_per_host.max(1)))),
            )
        };
        // The semaphore is never closed, so acquiring cannot fail.
        semaphore.acquire_owned().await.ok()
    }

    async fn attempt(&self, url: &Url, etag: Option<&str>) -> Result<Conditional, Failure> {
        debug!(url = %url, conditional = etag.is_some(), "GET");
        let mut request = self.inner.get(url.clone());
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(Failure::Transport)?;
        let status = response.status();
        if status == StatusCode::NOT_MODIFIED && etag.is_some() {
            debug!(url = %url, "not modified");
            return Ok(Conditional::NotModified);
        }
        if !status.is_success() {
            return Err(Failure::Status {
                status: status.as_u16(),
                retry_after: retry_after(response.headers()),
            });
        }
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await.map_err(Failure::Transport)?;
        debug!(url = %url, bytes = body.len(), "response received");
        Ok(Conditional::Modified {
            body: body.to_vec(),
            etag,
        })
    }
}

/// `Retry-After` in its delay-seconds form. (The HTTP-date form is rare for
/// APIs and is treated as absent.)
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

enum Failure {
    Status {
        status: u16,
        retry_after: Option<Duration>,
    },
    Transport(reqwest::Error),
}

impl Failure {
    fn is_transient(&self) -> bool {
        match self {
            Self::Status { status, .. } => *status == 429 || (500..=599).contains(status),
            Self::Transport(error) => error.is_timeout() || error.is_connect() || error.is_body(),
        }
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            Self::Transport(_) => None,
        }
    }

    fn into_source_error(self, url: &Url) -> SourceError {
        match self {
            Self::Status { status, .. } => SourceError::Status {
                url: url.to_string(),
                status,
            },
            Self::Transport(error) if error.is_timeout() => SourceError::Timeout {
                url: url.to_string(),
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
            Self::Status { status, .. } => write!(f, "HTTP {status}"),
            Self::Transport(error) => write!(f, "{}", jobhunt_core::ErrorChain(error)),
        }
    }
}

/// Maps a 404 to [`SourceError::NotFound`] naming what was missing.
pub(crate) fn not_found_as(
    what: impl FnOnce() -> String,
) -> impl FnOnce(SourceError) -> SourceError {
    move |error| match error {
        SourceError::Status { url, status: 404 } => SourceError::NotFound { url, what: what() },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn client(max_per_host: usize) -> HttpClient {
        HttpClient::new(HttpSettings {
            timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            max_retries: 1,
            retry_base_delay: Duration::from_millis(1),
            max_per_host,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn conditional_requests_report_not_modified() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/board"))
            .and(header("if-none-match", "W/\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/board"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "W/\"v1\"")
                    .set_body_string("[]"),
            )
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/board", server.uri())).unwrap();
        let http = client(4);

        match http.get_conditional(&url, None).await.unwrap() {
            Conditional::Modified { body, etag } => {
                assert_eq!(body, b"[]");
                assert_eq!(etag.as_deref(), Some("W/\"v1\""));
            }
            Conditional::NotModified => panic!("expected a body"),
        }
        assert!(matches!(
            http.get_conditional(&url, Some("W/\"v1\"")).await.unwrap(),
            Conditional::NotModified
        ));
        assert!(matches!(
            http.get_conditional(&url, Some("W/\"old\"")).await.unwrap(),
            Conditional::Modified { .. }
        ));
    }

    #[tokio::test]
    async fn honors_retry_after_on_429() {
        let server = MockServer::start().await;
        Mock::given(path("/limited"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(path("/limited"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/limited", server.uri())).unwrap();
        let started = Instant::now();
        let body = client(4).get_bytes(&url).await.unwrap();
        assert_eq!(body, b"ok");
        assert!(started.elapsed() >= Duration::from_millis(900));
    }

    #[tokio::test]
    async fn limits_concurrent_requests_per_host() {
        let server = MockServer::start().await;
        Mock::given(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("ok")
                    .set_delay(Duration::from_millis(200)),
            )
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/slow", server.uri())).unwrap();
        let http = client(1);
        let started = Instant::now();
        let (a, b) = tokio::join!(http.get_bytes(&url), http.get_bytes(&url));
        a.unwrap();
        b.unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(400),
            "requests to one host were serialized"
        );
    }
}
