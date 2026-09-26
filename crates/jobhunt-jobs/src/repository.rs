//! The persistence boundary for jobs.
//!
//! The domain and the discovery pipeline talk to storage only through
//! [`JobRepository`]. Backends (the SQLite store in `jobhunt-storage` today, a
//! Postgres store later) implement it and translate their native errors into
//! [`StorageError`], so no SQL dialect or driver leaks into this crate.
//!
//! Lifecycle decisions (NEW / UNCHANGED / UPDATED / CLOSED / REOPENED) are
//! made by [`crate::lifecycle::plan_scan`]; backends apply the plan
//! atomically and keep history.

use std::collections::HashMap;
use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use jobhunt_core::{BoxError, IngestCounts, SourceKey, UpsertOutcome};

use crate::identity::IdentityEntry;
use crate::model::{JobId, JobPosting, JobRecord, JobSnapshot, JobStatus, OpportunityId};

/// Filters for listing stored jobs. All filters combine with AND.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobQuery {
    /// Every term must match the start of a word (or run of words) in the
    /// posting's [`JobPosting::search_document`]. Terms are normalized with
    /// [`search_key`]; build them with [`JobQuery::with_text`].
    pub terms: Vec<String>,
    /// Only jobs from these sources. Empty means all sources.
    pub sources: Vec<SourceKey>,
    /// Only jobs with this status.
    pub status: Option<JobStatus>,
    /// Only jobs seen at their source at or after this instant.
    pub seen_since: Option<DateTime<Utc>>,
    /// Return one record per opportunity (the earliest-seen open record that
    /// matches), so a job listed by several sources appears once.
    pub distinct_opportunities: bool,
    /// Maximum number of results. `None` means no limit.
    pub limit: Option<usize>,
}

impl JobQuery {
    /// Splits free text into normalized search terms. Punctuation inside a
    /// word keeps its parts together as a phrase (`node.js` → `node js`).
    pub fn with_text(mut self, text: &str) -> Self {
        self.terms = text
            .split_whitespace()
            .map(search_key)
            .filter(|term| !term.is_empty())
            .collect();
        self
    }
}

/// Identifies one discovery run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(pub i64);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "run {}", self.0)
    }
}

/// One source's scan within a run, ready to be persisted atomically.
#[derive(Debug)]
pub struct ScanWrite<'a> {
    pub run: RunId,
    pub source: &'a SourceKey,
    pub started_at: DateTime<Utc>,
    /// The observation time stamped on every job seen by this scan.
    pub observed_at: DateTime<Utc>,
    /// Counts known before persisting (received, normalized, rejected,
    /// skipped, duplicates). The backend adds the lifecycle counts.
    pub counts: IngestCounts,
    pub body: ScanBody<'a>,
}

#[derive(Debug)]
pub enum ScanBody<'a> {
    /// The source returned a listing.
    Listing {
        /// Valid postings of this source, free of duplicate ids.
        postings: &'a [JobPosting],
        /// The adapter vouched that the listing is complete.
        complete: bool,
        /// Close open jobs missing from `postings` (see
        /// [`crate::lifecycle::Closing`]). Only ever true for complete
        /// listings; the pipeline may still withhold it.
        close_missing: bool,
        /// Why closing was withheld from a complete listing, if it was.
        closing_withheld: Option<&'static str>,
        /// Jobs the source listed but that could not be converted: never
        /// closed by this scan.
        retain: &'a [JobId],
        /// Validator (ETag) of the listing, stored for the next fetch.
        validator: Option<&'a str>,
    },
    /// The source confirmed its listing is unchanged since the last
    /// complete listing: every open job of the source is marked seen.
    NotModified,
    /// The fetch failed; nothing about the source's jobs changes.
    Failed { error: &'a str },
}

/// What persisting a scan did.
#[derive(Debug, Default)]
pub struct ScanResult {
    /// One outcome per posting of a [`ScanBody::Listing`], in order.
    pub outcomes: Vec<UpsertOutcome>,
    /// Jobs closed by the scan.
    pub closed: Vec<JobId>,
    /// Jobs marked seen by a [`ScanBody::NotModified`] scan.
    pub touched: usize,
}

/// The most recent listing scan of a source, as needed to plan the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastListing {
    pub finished_at: DateTime<Utc>,
    pub complete: bool,
    /// Records the source returned.
    pub received: usize,
    /// Validator of that listing, if the source provided one.
    pub validator: Option<String>,
    /// [`crate::model::CANONICAL_REVISION`] at the time.
    pub revision: String,
}

/// Totals written when a run finishes.
#[derive(Debug, Clone, Default)]
pub struct RunSummary {
    pub finished_at: DateTime<Utc>,
    pub sources: usize,
    pub failed: usize,
    pub counts: IngestCounts,
    /// Opportunities backed by more than one source record after the run.
    pub multi_source_opportunities: usize,
}

/// A change in a job's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEventKind {
    New,
    Updated,
    Closed,
    Reopened,
}

impl JobEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Updated => "updated",
            Self::Closed => "closed",
            Self::Reopened => "reopened",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "new" => Some(Self::New),
            "updated" => Some(Self::Updated),
            "closed" => Some(Self::Closed),
            "reopened" => Some(Self::Reopened),
            _ => None,
        }
    }
}

/// One entry of a job's history. History records changes only: a scan that
/// finds a job unchanged adds nothing (it only moves `last_seen_at`).
#[derive(Debug, Clone, PartialEq)]
pub struct JobEvent {
    pub kind: JobEventKind,
    pub at: DateTime<Utc>,
    pub run: Option<RunId>,
    /// Material fields that changed (see [`JobSnapshot::changed_fields`]).
    pub changed_fields: Vec<String>,
    /// The material content this change replaced, when content changed.
    pub previous: Option<JobSnapshot>,
}

/// Storage for jobs, their history, and discovery bookkeeping.
///
/// Results of [`JobRepository::search`] are ordered newest first: by
/// `posted_at` descending (jobs without a publish date last), then by
/// `first_seen_at` descending, then by id.
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Records the start of a discovery run.
    async fn begin_run(&self, started_at: DateTime<Utc>) -> Result<RunId, StorageError>;

    /// Records the end of a discovery run.
    async fn finish_run(&self, run: RunId, summary: &RunSummary) -> Result<(), StorageError>;

    /// The most recent listing scan of `source` (complete or not), if any.
    async fn last_listing(&self, source: &SourceKey) -> Result<Option<LastListing>, StorageError>;

    /// Persists one source scan atomically: applies the lifecycle plan for
    /// its postings (inserting, refreshing, reopening and closing jobs,
    /// appending history), stores identity evidence, and records the scan
    /// with its final counts. On error nothing is written.
    async fn apply_scan(&self, scan: &ScanWrite<'_>) -> Result<ScanResult, StorageError>;

    /// Every stored record with its identity evidence, for grouping.
    async fn identity_index(&self) -> Result<Vec<IdentityEntry>, StorageError>;

    /// Moves records to new opportunities (see [`crate::identity::group`]).
    async fn assign_opportunities(
        &self,
        assignments: &[(JobId, OpportunityId)],
    ) -> Result<(), StorageError>;

    async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError>;

    /// Every source record of an opportunity, earliest seen first.
    async fn opportunity_records(&self, id: OpportunityId) -> Result<Vec<JobRecord>, StorageError>;

    /// The job's history, oldest first.
    async fn history(&self, id: JobId) -> Result<Vec<JobEvent>, StorageError>;

    /// [`JobRepository::opportunity_records`] of many opportunities in one
    /// call (opportunities without records are absent). Networked backends
    /// answer it with one query; the default asks one by one.
    async fn opportunity_records_many(
        &self,
        ids: &[OpportunityId],
    ) -> Result<HashMap<OpportunityId, Vec<JobRecord>>, StorageError> {
        let mut out = HashMap::new();
        for id in ids {
            if out.contains_key(id) {
                continue;
            }
            let records = self.opportunity_records(*id).await?;
            if !records.is_empty() {
                out.insert(*id, records);
            }
        }
        Ok(out)
    }

    /// [`JobRepository::history`] of many jobs in one call (jobs without
    /// history are absent).
    async fn histories(
        &self,
        ids: &[JobId],
    ) -> Result<HashMap<JobId, Vec<JobEvent>>, StorageError> {
        let mut out = HashMap::new();
        for id in ids {
            if out.contains_key(id) {
                continue;
            }
            let events = self.history(*id).await?;
            if !events.is_empty() {
                out.insert(*id, events);
            }
        }
        Ok(out)
    }

    async fn search(&self, query: &JobQuery) -> Result<Vec<JobRecord>, StorageError>;

    /// Number of jobs matching `query`, ignoring its `limit`.
    async fn count(&self, query: &JobQuery) -> Result<u64, StorageError>;
}

/// A storage backend failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not open the job database at {location}")]
    Open {
        location: String,
        #[source]
        source: BoxError,
    },
    #[error("could not apply database migrations")]
    Migration(#[source] BoxError),
    #[error("database operation failed while {operation}")]
    Query {
        operation: &'static str,
        #[source]
        source: BoxError,
    },
    #[error("stored job {id} is corrupt: {detail}")]
    Corrupt { id: String, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_text_splits_on_whitespace() {
        let q = JobQuery::default().with_text("  Rust   backend\tNode.js -- ");
        assert_eq!(q.terms, vec!["rust", "backend", "node js"]);
        assert!(JobQuery::default().with_text("   ").terms.is_empty());
    }

    #[test]
    fn event_kinds_round_trip() {
        for kind in [
            JobEventKind::New,
            JobEventKind::Updated,
            JobEventKind::Closed,
            JobEventKind::Reopened,
        ] {
            assert_eq!(JobEventKind::from_canonical(kind.as_str()), Some(kind));
        }
    }
}
