//! Transactional email behind one interface ([`EmailSender`]), so the
//! notification pipeline never depends on a provider.
//!
//! * [`ResendSender`]: Resend's HTTP API. Each message is sent with an
//!   `Idempotency-Key` (the delivery id): Resend remembers keys for 24
//!   hours and answers a repeat with the original result instead of
//!   sending again, which is what makes a retry after a crash safe.
//! * [`FileSender`]: appends each message as a JSON line to a file, for
//!   local development and the end-to-end tests (refused in production).
//! * [`MemorySender`]: in memory, for tests; it can be told to fail, and it
//!   honors idempotency keys like Resend does. No required test sends real
//!   email.
//!
//! A failure is [`SendError::Retryable`] (network, rate limit, the
//! provider's own errors) or [`SendError::Permanent`] (the provider refused
//! the message: an invalid address, a sender not verified). Neither marks
//! a notification delivered.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::EmailConfig;

/// A rendered message. Stored (sealed) in the outbox before it is sent, so
/// every retry sends exactly the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessage {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    /// Extra headers (`List-Unsubscribe`, …).
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

/// What the provider said when it accepted a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SendReceipt {
    /// The provider's id for the message.
    pub message_id: Option<String>,
}

/// Why a message was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// Worth trying again later.
    #[error("{0}")]
    Retryable(String),
    /// Trying again would fail the same way.
    #[error("{0}")]
    Permanent(String),
}

/// Sends email.
#[async_trait]
pub trait EmailSender: Send + Sync {
    /// The provider's name (`resend`, `file`, `memory`).
    fn name(&self) -> &'static str;

    /// Sends `message`. `idempotency_key` is the same on every attempt to
    /// send the same message.
    async fn send(
        &self,
        message: &EmailMessage,
        idempotency_key: &str,
    ) -> Result<SendReceipt, SendError>;
}

/// The configured sender, if email is configured.
pub fn sender(config: Option<&EmailConfig>) -> Result<Option<Arc<dyn EmailSender>>, String> {
    Ok(match config {
        None => None,
        Some(EmailConfig::Resend {
            api_key, api_url, ..
        }) => Some(Arc::new(ResendSender::new(
            api_key.clone(),
            api_url.clone(),
        )?)),
        Some(EmailConfig::File { path, .. }) => Some(Arc::new(FileSender::new(path.clone()))),
    })
}

/// The sender address of the configuration.
pub fn from_address(config: &EmailConfig) -> &str {
    match config {
        EmailConfig::Resend { from, .. } | EmailConfig::File { from, .. } => from,
    }
}

/// Resend (<https://resend.com/docs/api-reference/emails/send-email>).
pub struct ResendSender {
    http: reqwest::Client,
    api_key: String,
    endpoint: String,
}

impl std::fmt::Debug for ResendSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResendSender")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct ResendRequest<'a> {
    from: &'a str,
    to: [&'a str; 1],
    subject: &'a str,
    text: &'a str,
    html: &'a str,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    headers: std::collections::BTreeMap<&'a str, &'a str>,
}

#[derive(Deserialize)]
struct ResendAccepted {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize, Default)]
struct ResendError {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl ResendSender {
    pub fn new(api_key: String, api_url: String) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("jobhunt-cloud/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            http,
            api_key,
            endpoint: format!("{}/emails", api_url.trim_end_matches('/')),
        })
    }
}

#[async_trait]
impl EmailSender for ResendSender {
    fn name(&self) -> &'static str {
        "resend"
    }

    async fn send(
        &self,
        message: &EmailMessage,
        idempotency_key: &str,
    ) -> Result<SendReceipt, SendError> {
        let body = ResendRequest {
            from: &message.from,
            to: [message.to.as_str()],
            subject: &message.subject,
            text: &message.text,
            html: &message.html,
            headers: message
                .headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect(),
        };
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .header("Idempotency-Key", idempotency_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&body).map_err(|e| SendError::Permanent(e.to_string()))?)
            .send()
            .await
            .map_err(|e| SendError::Retryable(format!("could not reach Resend: {e}")))?;
        let status = response.status();
        let bytes = response.bytes().await.unwrap_or_default();
        if status.is_success() {
            let accepted: ResendAccepted =
                serde_json::from_slice(&bytes).unwrap_or(ResendAccepted { id: None });
            return Ok(SendReceipt {
                message_id: accepted.id,
            });
        }
        let error: ResendError = serde_json::from_slice(&bytes).unwrap_or_default();
        let name = error.name.unwrap_or_default();
        // The provider's message, not the recipient's address or content.
        let detail = format!(
            "Resend answered {status}{}{}",
            if name.is_empty() { "" } else { ": " },
            name
        );
        tracing::warn!(%status, name, message = error.message.as_deref().unwrap_or(""), "email not accepted");
        match status.as_u16() {
            // Rate limited, a concurrent request with the same key still in
            // progress, or the provider's own trouble: later.
            429 | 500..=599 => Err(SendError::Retryable(detail)),
            409 if name == "concurrent_idempotent_requests" => Err(SendError::Retryable(detail)),
            _ => Err(SendError::Permanent(detail)),
        }
    }
}

/// Appends messages to a file, one JSON object per line (development and
/// end-to-end tests).
#[derive(Debug)]
pub struct FileSender {
    path: PathBuf,
    lock: Mutex<()>,
}

/// One line of a [`FileSender`]'s file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub idempotency_key: String,
    pub message: EmailMessage,
}

impl FileSender {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    /// Every message in the file.
    pub fn read(path: &std::path::Path) -> Vec<FileRecord> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

#[async_trait]
impl EmailSender for FileSender {
    fn name(&self) -> &'static str {
        "file"
    }

    async fn send(
        &self,
        message: &EmailMessage,
        idempotency_key: &str,
    ) -> Result<SendReceipt, SendError> {
        use std::io::Write;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| SendError::Retryable("the email file lock is poisoned".into()))?;
        // Like a provider, a key already used is not sent again.
        if Self::read(&self.path)
            .iter()
            .any(|r| r.idempotency_key == idempotency_key)
        {
            return Ok(SendReceipt {
                message_id: Some(idempotency_key.to_owned()),
            });
        }
        let line = serde_json::to_string(&FileRecord {
            idempotency_key: idempotency_key.to_owned(),
            message: message.clone(),
        })
        .map_err(|e| SendError::Permanent(e.to_string()))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| SendError::Retryable(format!("could not open the email file: {e}")))?;
        writeln!(file, "{line}")
            .map_err(|e| SendError::Retryable(format!("could not write the email file: {e}")))?;
        Ok(SendReceipt {
            message_id: Some(idempotency_key.to_owned()),
        })
    }
}

/// How a [`MemorySender`] answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryMode {
    #[default]
    Accept,
    /// Every send fails, retryably (the provider is down).
    FailRetryable,
    /// Every send fails permanently.
    FailPermanent,
    /// The provider accepts the message, then the answer is lost (the
    /// connection drops): the caller sees a retryable failure.
    AcceptThenLoseAnswer,
}

/// In-memory email, for tests.
#[derive(Debug, Clone, Default)]
pub struct MemorySender {
    inner: Arc<Mutex<MemoryState>>,
}

#[derive(Debug, Default)]
struct MemoryState {
    mode: MemoryMode,
    sent: Vec<(String, EmailMessage)>,
    attempts: usize,
}

impl MemorySender {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_mode(&self, mode: MemoryMode) {
        if let Ok(mut s) = self.inner.lock() {
            s.mode = mode;
        }
    }

    /// Messages actually delivered (at most one per idempotency key).
    pub fn sent(&self) -> Vec<EmailMessage> {
        self.inner
            .lock()
            .map(|s| s.sent.iter().map(|(_, m)| m.clone()).collect())
            .unwrap_or_default()
    }

    /// Calls to `send`, including failed and repeated ones.
    pub fn attempts(&self) -> usize {
        self.inner.lock().map(|s| s.attempts).unwrap_or(0)
    }
}

#[async_trait]
impl EmailSender for MemorySender {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn send(
        &self,
        message: &EmailMessage,
        idempotency_key: &str,
    ) -> Result<SendReceipt, SendError> {
        let mut s = self
            .inner
            .lock()
            .map_err(|_| SendError::Retryable("poisoned".into()))?;
        s.attempts += 1;
        let known = s.sent.iter().any(|(k, _)| k == idempotency_key);
        match s.mode {
            MemoryMode::FailRetryable => Err(SendError::Retryable("provider unavailable".into())),
            MemoryMode::FailPermanent => Err(SendError::Permanent("refused".into())),
            MemoryMode::Accept => {
                if !known {
                    s.sent.push((idempotency_key.to_owned(), message.clone()));
                }
                Ok(SendReceipt {
                    message_id: Some(format!("mem-{idempotency_key}")),
                })
            }
            MemoryMode::AcceptThenLoseAnswer => {
                if !known {
                    s.sent.push((idempotency_key.to_owned(), message.clone()));
                }
                Err(SendError::Retryable("the connection dropped".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> EmailMessage {
        EmailMessage {
            from: "JobHunt <n@jobhunt.test>".into(),
            to: "ana@example.com".into(),
            subject: "s".into(),
            text: "t".into(),
            html: "<p>t</p>".into(),
            headers: Vec::new(),
        }
    }

    #[tokio::test]
    async fn memory_sender_honors_idempotency_keys() {
        let s = MemorySender::new();
        s.send(&message(), "k1").await.unwrap();
        s.send(&message(), "k1").await.unwrap();
        assert_eq!(s.sent().len(), 1);
        s.set_mode(MemoryMode::FailRetryable);
        assert!(matches!(
            s.send(&message(), "k2").await,
            Err(SendError::Retryable(_))
        ));
        assert_eq!(s.sent().len(), 1);
        assert_eq!(s.attempts(), 3);
    }

    #[tokio::test]
    async fn file_sender_appends_json_lines_once_per_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.jsonl");
        let s = FileSender::new(path.clone());
        s.send(&message(), "a").await.unwrap();
        s.send(&message(), "a").await.unwrap();
        s.send(&message(), "b").await.unwrap();
        let records = FileSender::read(&path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].message, message());
    }
}
