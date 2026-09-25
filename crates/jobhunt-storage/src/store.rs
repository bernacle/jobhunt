//! [`Store`]: everything the application needs from one storage backend.
//!
//! The domain crates define one repository trait per concern (jobs,
//! verification, eligibility, feedback, rankings, profiles). An application
//! works with all of them at once, plus a few lookups that are not domain
//! rules (short-id prefixes, when sources were last read, diagnostics) and
//! the atomic state import. `Store` bundles them so the application can hold
//! one `Arc<dyn Store>` and run unchanged over the local SQLite file
//! ([`crate::SqliteJobStore`]) or a user's view of the cloud database
//! ([`crate::postgres::PgUserStore`]).

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::SourceKey;
use jobhunt_eligibility::EligibilityRepository;
use jobhunt_jobs::verification::{VerificationRecord, VerificationRepository};
use jobhunt_jobs::{JobId, JobRepository, OpportunityId, StorageError};
use jobhunt_profile::ProfileRepository;
use jobhunt_ranking::{FeedbackRepository, RankingRepository, Tier};

use crate::sqlite::StoreStats;
use crate::state::{StateImport, StateImported};
use crate::sync::SyncLedger;

/// Held while a read-modify-write use case runs (recording feedback,
/// updating preferences, importing state), so two requests for the same
/// person cannot interleave their checks and writes. Dropping it releases
/// the lock.
pub struct WriteGuard {
    _inner: Box<dyn Send>,
}

impl WriteGuard {
    pub fn new(inner: impl Send + 'static) -> Self {
        Self {
            _inner: Box::new(inner),
        }
    }
}

impl std::fmt::Debug for WriteGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WriteGuard")
    }
}

/// One opportunity shown to the person in a shortlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub opportunity: OpportunityId,
    pub tier: Tier,
}

/// A storage backend for the whole application (see the module docs).
#[async_trait]
pub trait Store:
    JobRepository
    + VerificationRepository
    + EligibilityRepository
    + FeedbackRepository
    + RankingRepository
    + ProfileRepository
    + Send
    + Sync
{
    /// Where the data lives, for diagnostics (a path, `:memory:`, or the
    /// database host without credentials).
    fn location(&self) -> String;

    /// Opportunity ids that start with `prefix`, at most `limit`.
    async fn opportunities_with_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<OpportunityId>, StorageError>;

    /// Job ids that start with `prefix`, at most `limit`.
    async fn jobs_with_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<JobId>, StorageError>;

    /// When each source was last read successfully.
    async fn last_checked(&self) -> Result<HashMap<SourceKey, DateTime<Utc>>, StorageError>;

    /// Counts for diagnostics.
    async fn stats(&self) -> Result<StoreStats, StorageError>;

    /// Stores an imported state atomically.
    async fn import_state(
        &self,
        state: StateImport<'_>,
    ) -> Result<StateImported, jobhunt_profile::StorageError>;

    /// Stores a verification attempt made elsewhere (by the cloud), unless
    /// one with its id is already stored. Returns whether it was stored.
    async fn import_verification(&self, record: &VerificationRecord) -> Result<bool, StorageError>;

    /// Serializes read-modify-write use cases for this person (see
    /// [`WriteGuard`]).
    async fn write_lock(&self) -> Result<WriteGuard, StorageError>;

    /// Records the opportunities a shortlist showed, for "new since you
    /// last looked" and notifications. Backends without per-person search
    /// state ignore it.
    async fn record_shown(&self, _shown: &[Shown], _at: DateTime<Utc>) -> Result<(), StorageError> {
        Ok(())
    }

    /// The local bookkeeping of cloud sync, when this backend keeps one
    /// (the local SQLite store does; the cloud is the other side).
    fn sync_ledger(&self) -> Option<&dyn SyncLedger> {
        None
    }

    /// Closes connections (flushing SQLite's write-ahead log).
    async fn shutdown(&self);
}
