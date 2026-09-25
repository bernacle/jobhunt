//! Cloud sync: the wire protocol both sides speak, and the bookkeeping the
//! local store keeps.
//!
//! What syncs is what belongs to the person: their profile (as
//! [`jobhunt_profile::entities`], one versioned entity per record) and
//! their feedback (append-only events with stable ids). Jobs do not sync as
//! such: the cloud keeps one shared corpus, and a pull only carries the
//! source records the person's feedback and recommendations refer to, with
//! their latest verification.
//!
//! The cloud gives every entity a version and every change a per-person
//! sequence number. A pull returns what changed after a cursor; a push is a
//! compare-and-set per entity ("I based this on version 3"), applied all
//! or nothing. Merging happens on the client, which alone knows both its
//! own changes and the last version it saw (the *base*); see
//! `jobhunt_app::sync`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_jobs::verification::VerificationRecord;
use jobhunt_jobs::{JobRecord, StorageError};
use jobhunt_profile::entities::{EntityKey, EntityKind};
use jobhunt_ranking::{FeedbackEvent, FeedbackId};
use serde::{Deserialize, Serialize};

/// Version of the sync protocol. A server answers requests of other
/// versions with an error asking to update.
pub const SYNC_PROTOCOL: u32 = 1;

/// The cloud's state of one profile entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityState {
    pub kind: EntityKind,
    pub id: String,
    /// Incremented by every change, deletion included. Never reused.
    pub version: u64,
    /// The record was deleted (its last version is a tombstone).
    #[serde(default)]
    pub deleted: bool,
    /// The record, absent when deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Digest of `body`, absent when deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

impl EntityState {
    pub fn key(&self) -> EntityKey {
        EntityKey::new(self.kind, self.id.clone())
    }
}

/// `POST /api/v1/sync/pull`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequest {
    pub protocol: u32,
    /// The cursor of the last pull (0 for everything).
    pub cursor: u64,
}

/// What changed in the cloud after a cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullResponse {
    pub protocol: u32,
    /// Pass it to the next pull.
    pub cursor: u64,
    /// The cloud profile's revision now (0 without a profile).
    pub profile_revision: u64,
    /// Entities changed after the cursor, tombstones included.
    pub entities: Vec<EntityState>,
    /// Feedback added after the cursor, oldest first (never "seen" marks).
    pub feedback: Vec<FeedbackEvent>,
    /// Source records the feedback and the person's recent cloud
    /// recommendations refer to.
    pub jobs: Vec<JobRecord>,
    /// The latest successful verification of those records.
    pub verifications: Vec<VerificationRecord>,
}

/// One entity change a client wants applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityMutation {
    pub kind: EntityKind,
    pub id: String,
    /// The cloud version this change was made on top of (0: the client
    /// never saw the entity in the cloud).
    pub base_version: u64,
    /// The new record; `None` deletes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl EntityMutation {
    pub fn key(&self) -> EntityKey {
        EntityKey::new(self.kind, self.id.clone())
    }
}

/// `POST /api/v1/sync/push`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushRequest {
    pub protocol: u32,
    /// Chosen by the client, for logs (retrying a push is safe without it).
    pub push_id: String,
    #[serde(default)]
    pub entities: Vec<EntityMutation>,
    /// Feedback events the cloud may not have; ones it has are ignored.
    #[serde(default)]
    pub feedback: Vec<FeedbackEvent>,
    /// Source records the pushed feedback refers to, for the ones the cloud
    /// has never seen.
    #[serde(default)]
    pub jobs: Vec<JobRecord>,
}

/// An entity as stored after a push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedEntity {
    pub kind: EntityKind,
    pub id: String,
    pub version: u64,
    pub deleted: bool,
}

/// A mutation that was based on an outdated version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushConflict {
    pub mutation: EntityMutation,
    /// What the cloud has now.
    pub current: EntityState,
}

/// The answer to a push. With conflicts nothing was applied (not even
/// feedback): pull, merge and push again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushResponse {
    pub applied: Vec<AppliedEntity>,
    pub conflicts: Vec<PushConflict>,
    pub feedback_added: usize,
    pub feedback_present: usize,
    pub jobs_added: usize,
    pub profile_revision: u64,
}

/// Why a push was refused as a whole.
#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

// ---------------------------------------------------------------------
// Local bookkeeping
// ---------------------------------------------------------------------

/// Which account and server this database syncs with, and how far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAccount {
    pub server: String,
    /// `usr_…` of the cloud account.
    pub user_id: String,
    pub cursor: u64,
    pub last_sync_at: Option<DateTime<Utc>>,
}

/// The last cloud version of an entity this database saw and agreed with:
/// the common ancestor for the next merge.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityBase {
    pub key: EntityKey,
    pub version: u64,
    pub deleted: bool,
    pub body: Option<serde_json::Value>,
    pub digest: Option<String>,
}

/// A change that could not be merged, kept until the person decides.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncConflict {
    pub key: EntityKey,
    /// Why ("both changed verification", "deleted in the cloud, edited
    /// here").
    pub reason: String,
    /// The local record at the time (`None`: deleted here).
    pub local: Option<serde_json::Value>,
    /// The cloud record (`None`: deleted there).
    pub cloud: Option<serde_json::Value>,
    pub cloud_version: u64,
    pub detected_at: DateTime<Utc>,
}

/// Everything one sync round records locally, written atomically.
#[derive(Debug, Clone, Default)]
pub struct LedgerUpdate {
    pub account: Option<SyncAccount>,
    /// Bases to store (replacing ones with the same key).
    pub bases: Vec<EntityBase>,
    /// Conflicts to store (replacing ones with the same key).
    pub conflicts: Vec<SyncConflict>,
    /// Conflicts that no longer apply.
    pub resolved: Vec<EntityKey>,
    /// Feedback known to be in the cloud.
    pub feedback_synced: Vec<FeedbackId>,
}

/// The local store's sync bookkeeping. Only the local store keeps one.
#[async_trait]
pub trait SyncLedger: Send + Sync {
    async fn sync_account(&self) -> Result<Option<SyncAccount>, StorageError>;

    async fn sync_bases(&self) -> Result<Vec<EntityBase>, StorageError>;

    async fn sync_conflicts(&self) -> Result<Vec<SyncConflict>, StorageError>;

    /// Ids of feedback events known to be in the cloud.
    async fn synced_feedback(&self) -> Result<Vec<FeedbackId>, StorageError>;

    async fn update_ledger(&self, update: &LedgerUpdate) -> Result<(), StorageError>;

    /// Forgets all sync state (another account or server).
    async fn reset_ledger(&self) -> Result<(), StorageError>;
}
