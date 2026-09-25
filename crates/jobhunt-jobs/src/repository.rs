//! The persistence boundary for jobs.
//!
//! The domain and the discovery pipeline talk to storage only through
//! [`JobRepository`]. Backends (the SQLite store in `jobhunt-storage` today, a
//! Postgres store later) implement it and translate their native errors into
//! [`StorageError`], so no SQL dialect or driver leaks into this crate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use jobhunt_core::{BoxError, SourceKey, UpsertOutcome};

use crate::model::{JobId, JobPosting, JobRecord};

/// Filters for listing stored jobs. All filters combine with AND.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobQuery {
    /// Every term must match the start of a word (or run of words) in the
    /// posting's [`JobPosting::search_document`]. Terms are normalized with
    /// [`search_key`]; build them with [`JobQuery::with_text`].
    pub terms: Vec<String>,
    /// Only jobs from these sources. Empty means all sources.
    pub sources: Vec<SourceKey>,
    /// Only jobs seen at their source at or after this instant.
    pub seen_since: Option<DateTime<Utc>>,
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

/// Storage for jobs.
///
/// Results of [`JobRepository::search`] are ordered newest first: by
/// `posted_at` descending (jobs without a publish date last), then by
/// `first_seen_at` descending, then by id.
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Inserts new postings and refreshes existing ones, keyed by
    /// [`JobPosting::id`]. Returns one outcome per posting, in order.
    ///
    /// Every posting's `last_seen_at` becomes `observed_at`. A posting whose
    /// [`JobPosting::fingerprint`] is unchanged is reported as
    /// [`UpsertOutcome::Unchanged`] and keeps its content timestamps. The call
    /// is atomic: on error nothing is written.
    async fn upsert_postings(
        &self,
        postings: &[JobPosting],
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<UpsertOutcome>, StorageError>;

    async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError>;

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
}
