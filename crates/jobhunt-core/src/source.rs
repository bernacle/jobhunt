//! Source identity, provenance, and the adapter contract.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::BoxError;
use crate::urls::{CanonicalUrl, UrlError};

/// Identifies one configured source instance, written `kind:instance`
/// (for example `ashby:linear`: the Ashby adapter, reading the `linear` board).
///
/// `kind` names the adapter and is lowercase ASCII (`a-z`, `0-9`, `-`, `_`).
/// `instance` names what the adapter reads (a board, a feed, ...); it is
/// lowercased so that `Linear` and `linear` identify the same instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SourceKey {
    kind: String,
    instance: String,
}

impl SourceKey {
    pub fn new(kind: &str, instance: &str) -> Result<Self, SourceKeyError> {
        let kind = kind.trim();
        let instance = instance.trim();
        let kind_ok = !kind.is_empty()
            && kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
        if !kind_ok {
            return Err(SourceKeyError::InvalidKind(kind.to_owned()));
        }
        let instance_ok = !instance.is_empty()
            && !instance
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '/');
        if !instance_ok {
            return Err(SourceKeyError::InvalidInstance(instance.to_owned()));
        }
        Ok(Self {
            kind: kind.to_owned(),
            instance: instance.to_lowercase(),
        })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn instance(&self) -> &str {
        &self.instance
    }
}

impl fmt::Display for SourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.instance)
    }
}

impl FromStr for SourceKey {
    type Err = SourceKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, instance) = s
            .split_once(':')
            .ok_or_else(|| SourceKeyError::MissingSeparator(s.to_owned()))?;
        Self::new(kind, instance)
    }
}

impl TryFrom<String> for SourceKey {
    type Error = SourceKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<SourceKey> for String {
    fn from(value: SourceKey) -> Self {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceKeyError {
    #[error("source {0:?} must be written as kind:name (for example ashby:linear)")]
    MissingSeparator(String),
    #[error("source kind {0:?} must be non-empty lowercase letters, digits, '-' or '_'")]
    InvalidKind(String),
    #[error("source name {0:?} must be non-empty and contain no whitespace or '/'")]
    InvalidInstance(String),
}

/// Where a record came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The source instance that produced the record.
    pub source: SourceKey,
    /// The record's identifier inside that source, when the source has one.
    pub source_record_id: Option<String>,
    /// The endpoint the record was fetched from.
    pub fetched_from: Option<CanonicalUrl>,
}

/// A batch of records produced by one [`Source::fetch`] call.
#[derive(Debug)]
pub struct SourceBatch<T> {
    /// Records successfully converted into the domain model.
    pub records: Vec<T>,
    /// Records the source returned but that could not be converted.
    pub rejected: Vec<RecordError>,
    /// Records the source returned that were intentionally ignored.
    pub skipped: usize,
    /// True only when the batch is the source's *entire* current listing:
    /// every record the source publishes was received (converted, rejected
    /// or skipped). Consumers may then treat records missing from the batch
    /// as gone. Adapters must leave this `false` whenever they cannot vouch
    /// for completeness (pagination cut short, a sub-request failed, a
    /// count mismatch). Defaults to `false`.
    pub complete: bool,
    /// Opaque validator (an HTTP `ETag`) identifying this exact listing.
    /// Passed back in [`FetchRequest::validator`] on the next fetch so the
    /// adapter can ask the source whether anything changed.
    pub validator: Option<String>,
}

impl<T> SourceBatch<T> {
    pub fn received(&self) -> usize {
        self.records.len() + self.rejected.len() + self.skipped
    }
}

impl<T> Default for SourceBatch<T> {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            rejected: Vec::new(),
            skipped: 0,
            complete: false,
            validator: None,
        }
    }
}

/// What the caller already knows about a source before fetching it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchRequest {
    /// The validator of the last complete listing that was fully processed.
    /// Adapters that support conditional requests send it (for HTTP, as
    /// `If-None-Match`) and answer [`Fetched::NotModified`] when the source
    /// confirms nothing changed. Adapters without support ignore it.
    pub validator: Option<String>,
}

/// Result of a successful [`Source::fetch`].
#[derive(Debug)]
pub enum Fetched<T> {
    /// The source's current records.
    Batch(SourceBatch<T>),
    /// The source confirmed that its listing is identical to the one
    /// identified by [`FetchRequest::validator`].
    NotModified,
}

/// A source adapter: fetches external data and converts it into the domain
/// record type.
///
/// Adapters own everything source-specific (HTTP endpoints, payload formats,
/// field mappings). A failure of the whole fetch is a [`SourceError`]; a
/// failure of an individual record is a [`RecordError`] inside the batch, so
/// one malformed record never discards the rest.
#[async_trait]
pub trait Source: Send + Sync {
    type Record: Send;

    fn key(&self) -> &SourceKey;

    async fn fetch(&self, request: &FetchRequest) -> Result<Fetched<Self::Record>, SourceError>;
}

/// The whole fetch from a source failed.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("request to {url} failed")]
    Request {
        url: String,
        #[source]
        source: BoxError,
    },
    #[error("{url} responded with HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{what} was not found ({url} responded with HTTP 404)")]
    NotFound { url: String, what: String },
    #[error("could not decode the response from {url}")]
    Decode {
        url: String,
        #[source]
        source: BoxError,
    },
    #[error("invalid source configuration: {0}")]
    Config(String),
}

/// A single record could not be converted into the domain model.
#[derive(Debug, thiserror::Error)]
#[error("record {}: {reason}", record_id.as_deref().unwrap_or("<no id>"))]
pub struct RecordError {
    pub record_id: Option<String>,
    pub reason: RecordErrorReason,
}

impl RecordError {
    pub fn new(record_id: Option<String>, reason: RecordErrorReason) -> Self {
        Self { record_id, reason }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecordErrorReason {
    #[error("malformed record: {0}")]
    Malformed(String),
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
    // Not marked `#[source]`: the cause is already part of the message.
    #[error("invalid URL in `{field}`: {error}")]
    InvalidUrl {
        field: &'static str,
        error: UrlError,
    },
    #[error("invalid value in `{field}`: {detail}")]
    InvalidValue { field: &'static str, detail: String },
    /// The record is listed by the source but part of it (for example a
    /// detail page) could not be fetched. The record still exists, so it must
    /// not be treated as removed.
    #[error("record details unavailable: {0}")]
    Unavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays_source_keys() {
        let key: SourceKey = "ashby:Linear".parse().unwrap();
        assert_eq!(key.kind(), "ashby");
        assert_eq!(key.instance(), "linear");
        assert_eq!(key.to_string(), "ashby:linear");
    }

    #[test]
    fn rejects_malformed_source_keys() {
        assert!(matches!(
            "ashby".parse::<SourceKey>(),
            Err(SourceKeyError::MissingSeparator(_))
        ));
        assert!(matches!(
            "Ashby:linear".parse::<SourceKey>(),
            Err(SourceKeyError::InvalidKind(_))
        ));
        assert!(matches!(
            "ashby:".parse::<SourceKey>(),
            Err(SourceKeyError::InvalidInstance(_))
        ));
        assert!(matches!(
            "ashby:a b".parse::<SourceKey>(),
            Err(SourceKeyError::InvalidInstance(_))
        ));
    }

    #[test]
    fn batch_counts_everything_received() {
        let batch = SourceBatch {
            records: vec![1, 2],
            rejected: vec![RecordError::new(
                None,
                RecordErrorReason::MissingField("title"),
            )],
            skipped: 1,
            ..Default::default()
        };
        assert_eq!(batch.received(), 4);
        assert!(!batch.complete, "completeness must be claimed explicitly");
    }

    #[test]
    fn record_errors_read_well() {
        let err = RecordError::new(
            Some("abc".into()),
            RecordErrorReason::InvalidUrl {
                field: "jobUrl",
                error: UrlError::Empty,
            },
        );
        assert_eq!(
            err.to_string(),
            "record abc: invalid URL in `jobUrl`: URL is empty"
        );
    }
}
