//! The persistence boundary for profiles.
//!
//! The profile domain talks to storage only through [`ProfileRepository`].
//! Backends (SQLite in `jobhunt-storage` today, Postgres later) store the
//! aggregate in relational tables and translate driver errors into
//! [`StorageError`]; all rules (re-import, evidence policy, preference
//! supersession) live in this crate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::BoxError;

use crate::aggregate::ProfileData;
use crate::evidence::{Claim, ClaimQuery};
use crate::ids::ProfileId;

/// Kinds of change recorded in a profile's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileEventKind {
    ResumeImported,
    ProfileImported,
    BasicsEdited,
    RecordAdded,
    RecordEdited,
    RecordRemoved,
    RecordRejected,
    ClaimAdded,
    ClaimEdited,
    ClaimConfirmed,
    ClaimRejected,
    ClaimReset,
    StatementAdded,
    StatementRemoved,
    PreferenceSet,
    PreferenceRemoved,
}

impl ProfileEventKind {
    const ALL: [ProfileEventKind; 16] = [
        Self::ResumeImported,
        Self::ProfileImported,
        Self::BasicsEdited,
        Self::RecordAdded,
        Self::RecordEdited,
        Self::RecordRemoved,
        Self::RecordRejected,
        Self::ClaimAdded,
        Self::ClaimEdited,
        Self::ClaimConfirmed,
        Self::ClaimRejected,
        Self::ClaimReset,
        Self::StatementAdded,
        Self::StatementRemoved,
        Self::PreferenceSet,
        Self::PreferenceRemoved,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResumeImported => "resume_imported",
            Self::ProfileImported => "profile_imported",
            Self::BasicsEdited => "basics_edited",
            Self::RecordAdded => "record_added",
            Self::RecordEdited => "record_edited",
            Self::RecordRemoved => "record_removed",
            Self::RecordRejected => "record_rejected",
            Self::ClaimAdded => "claim_added",
            Self::ClaimEdited => "claim_edited",
            Self::ClaimConfirmed => "claim_confirmed",
            Self::ClaimRejected => "claim_rejected",
            Self::ClaimReset => "claim_reset",
            Self::StatementAdded => "statement_added",
            Self::StatementRemoved => "statement_removed",
            Self::PreferenceSet => "preference_set",
            Self::PreferenceRemoved => "preference_removed",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

/// One entry of a profile's history: who changed what, when.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileEvent {
    pub at: DateTime<Utc>,
    pub kind: ProfileEventKind,
    /// The record or claim concerned, when there is one.
    pub record: Option<String>,
    /// A short human-readable description.
    pub detail: String,
}

impl ProfileEvent {
    pub fn new(
        at: DateTime<Utc>,
        kind: ProfileEventKind,
        record: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            at,
            kind,
            record,
            detail: detail.into(),
        }
    }
}

/// Storage for profiles.
#[async_trait]
pub trait ProfileRepository: Send + Sync {
    /// The whole profile, or `None` if it was never saved.
    async fn load_profile(&self, id: ProfileId) -> Result<Option<ProfileData>, StorageError>;

    /// Stores `data` as the complete state of its profile, atomically:
    /// records are upserted by id, and records of the profile that are not
    /// in `data` are deleted. `events` are appended to the history.
    ///
    /// `expected_revision` is the revision `data` was loaded at (0 for a
    /// profile that did not exist). If the stored revision differs, someone
    /// else changed the profile meanwhile and [`StorageError::Conflict`] is
    /// returned without writing anything.
    async fn save_profile(
        &self,
        data: &ProfileData,
        expected_revision: u64,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError>;

    /// Claims of a profile matching `query`, including the evidence-policy
    /// filters.
    async fn find_claims(
        &self,
        profile: ProfileId,
        query: &ClaimQuery,
    ) -> Result<Vec<Claim>, StorageError>;

    /// The most recent history entries, newest first.
    async fn profile_events(
        &self,
        profile: ProfileId,
        limit: usize,
    ) -> Result<Vec<ProfileEvent>, StorageError>;
}

/// A storage backend failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database operation failed while {operation}")]
    Query {
        operation: &'static str,
        #[source]
        source: BoxError,
    },
    #[error("stored profile record {record} is corrupt: {detail}")]
    Corrupt { record: String, detail: String },
    #[error(
        "the profile changed while this command ran (expected revision {expected}, found {found}); run it again"
    )]
    Conflict { expected: u64, found: u64 },
}
