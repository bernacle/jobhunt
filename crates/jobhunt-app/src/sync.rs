//! Local ↔ cloud sync: the client side.
//!
//! What syncs is what belongs to the person and cannot be rebuilt: the
//! profile (every record as an entity: basics, documents, experiences,
//! projects, education, skills, claims with their confirm/reject
//! decisions, preferences, statements) and feedback (save, reject,
//! applied, …, with reasons), from which the pipeline and learned taste
//! are folded. Jobs are not copied wholesale: a pull brings the source
//! records the person's feedback and cloud recommendations refer to (with
//! their latest verification), so they can be looked at offline.
//!
//! # Merging
//!
//! The local store keeps, per entity, the last cloud version it agreed
//! with (the *base*). A sync pulls what changed in the cloud, then decides
//! each entity by comparing three states: base, local, cloud.
//!
//! * Same on both sides: nothing to do.
//! * Changed on one side only: that side wins (a local edit is pushed; a
//!   cloud edit is applied here).
//! * Changed on both: the change is merged field by field. A field changed
//!   on one side takes that side; timestamps take the later one; lists of
//!   field names and parser notes are united. A field changed differently
//!   on both sides (for example a claim confirmed here and rejected in the
//!   cloud), or a record edited on one side and deleted on the other, is
//!   a **conflict**: nothing is overwritten, the local version is kept and
//!   not pushed, and the conflict is listed until the person decides
//!   (`jobhunt sync --keep local|cloud`, or per record).
//!
//! Feedback events are never changed after they are made and have stable
//! ids, so they merge by union: nothing can conflict and nothing is lost.
//!
//! Pushes are compare-and-set in the cloud ("based on version 3"), all or
//! nothing: if the cloud changed in between, the push is refused and the
//! sync pulls and merges again. Retrying a sync after a lost answer is
//! safe (a push that already landed is recognized by its content).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_jobs::JobRecord;
use jobhunt_profile::entities::{Entity, EntityKey, compose, decompose, digest};
use jobhunt_profile::{ProfileEvent, ProfileEventKind};
use jobhunt_ranking::{FeedbackAction, FeedbackEvent, FeedbackId};
use jobhunt_storage::StateImport;
use jobhunt_storage::sync::{
    EntityBase, EntityMutation, EntityState, LedgerUpdate, PullRequest, PullResponse, PushRequest,
    PushResponse, SYNC_PROTOCOL, SyncAccount, SyncConflict, SyncLedger,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::LocalApp;
use crate::error::AppError;

/// How many pull-merge-push rounds a sync tries before giving up (each
/// retry means the cloud changed while this sync ran).
const ROUNDS: usize = 4;

/// Talks to JobHunt Cloud (HTTP in the CLI; in-process in tests).
#[async_trait]
pub trait SyncTransport: Send + Sync {
    async fn pull(&self, request: &PullRequest) -> Result<PullResponse, AppError>;
    async fn push(&self, request: &PushRequest) -> Result<PushResponse, AppError>;
}

/// Which side a conflict resolution keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Local,
    Cloud,
}

/// Where this database syncs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub server: String,
    /// `usr_…`.
    pub user_id: String,
}

/// One unresolved conflict, for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConflictView {
    pub kind: String,
    pub id: String,
    pub reason: String,
    /// Short description of the local record ("claim: Led the payments
    /// migration", "deleted here").
    pub local: String,
    pub cloud: String,
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Profile records changed here from the cloud.
    pub applied_locally: usize,
    /// Profile records sent to the cloud.
    pub pushed: usize,
    /// Profile records merged field by field (changed on both sides).
    pub merged: usize,
    pub feedback_pulled: usize,
    pub feedback_pushed: usize,
    pub jobs_pulled: usize,
    pub verifications_pulled: usize,
    /// Unresolved conflicts after this sync (new and older ones).
    pub conflicts: Vec<ConflictView>,
    /// Pull-merge-push rounds (more than one: the cloud changed meanwhile).
    pub rounds: usize,
    pub cursor: u64,
}

/// Sync state, for `jobhunt account` / `jobhunt sync --status`.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncStatus {
    pub account: Option<SyncAccount>,
    pub conflicts: Vec<ConflictView>,
    pub synced_records: usize,
    pub unsynced_feedback: usize,
}

fn describe(kind: &str, body: Option<&Value>) -> String {
    let Some(body) = body else {
        return "deleted".into();
    };
    let text = [
        "text",
        "title",
        "name",
        "institution",
        "headline",
        "file_name",
        "place",
    ]
    .iter()
    .find_map(|k| body.get(*k).and_then(Value::as_str))
    .or_else(|| body.pointer("/value/place").and_then(Value::as_str))
    .unwrap_or("");
    let mut short: String = text.chars().take(60).collect();
    if text.chars().count() > 60 {
        short.push('…');
    }
    if let Some(v) = body.get("verification").and_then(Value::as_str) {
        return format!("{kind} ({v}): {short}");
    }
    format!("{kind}: {short}")
}

impl ConflictView {
    fn of(c: &SyncConflict) -> Self {
        let kind = c.key.kind.as_str();
        Self {
            kind: kind.to_owned(),
            id: c.key.id.clone(),
            reason: c.reason.clone(),
            local: describe(kind, c.local.as_ref()),
            cloud: describe(kind, c.cloud.as_ref()),
        }
    }
}

/// Keys whose values are timestamps: when both sides changed one, the
/// later wins.
fn is_timestamp(key: &str) -> bool {
    key.ends_with("_at") || key.ends_with("_since")
}

/// Lists that only accumulate (field names the person edited, parser
/// notes): both sides' additions are kept.
fn is_set(key: &str) -> bool {
    matches!(key, "edited_fields" | "notes")
}

/// Field-by-field three-way merge of two changed versions of one record.
/// `Err` names the field that changed differently on both sides.
pub fn merge3(base: &Value, local: &Value, cloud: &Value) -> Result<Value, String> {
    match (base, local, cloud) {
        (_, l, c) if l == c => Ok(l.clone()),
        (b, l, c) if l == b => Ok(c.clone()),
        (b, l, c) if c == b => Ok(l.clone()),
        (b, Value::Object(l), Value::Object(c)) => {
            let empty = serde_json::Map::new();
            let b = b.as_object().unwrap_or(&empty);
            let keys: BTreeSet<&String> = b.keys().chain(l.keys()).chain(c.keys()).collect();
            let mut out = serde_json::Map::new();
            for key in keys {
                let (bv, lv, cv) = (
                    b.get(key).unwrap_or(&Value::Null),
                    l.get(key).unwrap_or(&Value::Null),
                    c.get(key).unwrap_or(&Value::Null),
                );
                let merged = if lv == cv || cv == bv {
                    lv.clone()
                } else if lv == bv {
                    cv.clone()
                } else if is_timestamp(key)
                    && let (Some(ls), Some(cs)) = (lv.as_str(), cv.as_str())
                {
                    // Canonical timestamps are fixed width: text order is
                    // time order.
                    Value::from(ls.max(cs))
                } else if is_set(key)
                    && let (Some(la), Some(ca)) = (lv.as_array(), cv.as_array())
                {
                    let mut all: Vec<Value> = la.iter().chain(ca).cloned().collect();
                    all.sort_by_key(Value::to_string);
                    all.dedup();
                    Value::Array(all)
                } else if lv.is_object() && cv.is_object() {
                    merge3(bv, lv, cv).map_err(|field| format!("{key}.{field}"))?
                } else {
                    return Err(key.clone());
                };
                if !merged.is_null() || l.contains_key(key) || c.contains_key(key) {
                    out.insert(key.clone(), merged);
                }
            }
            Ok(Value::Object(out))
        }
        _ => Err("value".into()),
    }
}

fn conflict_reason(kind: &str, field: &str) -> String {
    match field {
        "verification" => format!(
            "the {kind} was decided differently here and in the cloud (confirmed vs rejected)"
        ),
        "active" => format!("the {kind} was turned on in one place and off in the other"),
        other => format!("the {kind}'s {other} changed differently here and in the cloud"),
    }
}

/// The outcome of deciding one entity.
enum Decision {
    /// Already equal; remember the cloud state as the new base.
    InSync(EntityState),
    /// Write this locally (`None`: delete it here); it matches the cloud.
    TakeCloud(EntityState),
    /// Send this to the cloud (`None`: delete it there).
    Push {
        body: Option<Value>,
        base_version: u64,
        /// Also write it locally (a merge).
        local_write: bool,
    },
    Conflict(SyncConflict),
}

fn state_of(base: &EntityBase) -> EntityState {
    EntityState {
        kind: base.key.kind,
        id: base.key.id.clone(),
        version: base.version,
        deleted: base.deleted,
        body: base.body.clone(),
        digest: base.digest.clone(),
    }
}

fn decide(
    key: &EntityKey,
    base: Option<&EntityBase>,
    local: Option<&Entity>,
    cloud: &EntityState,
    prefer: Option<Side>,
    now: DateTime<Utc>,
) -> Decision {
    let b_digest = base.and_then(|b| b.digest.clone());
    let l_digest = local.map(|e| e.digest.clone());
    let c_digest = if cloud.deleted {
        None
    } else {
        cloud.digest.clone()
    };
    if l_digest == c_digest {
        return Decision::InSync(cloud.clone());
    }
    if l_digest == b_digest {
        return Decision::TakeCloud(cloud.clone());
    }
    let push = |body: Option<Value>, local_write: bool| Decision::Push {
        body,
        base_version: cloud.version,
        local_write,
    };
    if c_digest == b_digest {
        return push(local.map(|e| e.body.clone()), false);
    }
    // Both changed.
    let kind = key.kind.as_str();
    let reason = match (
        base.and_then(|b| b.body.as_ref()),
        local,
        cloud.body.as_ref(),
    ) {
        (base_body, Some(l), Some(c)) => {
            match merge3(base_body.unwrap_or(&Value::Null), &l.body, c) {
                Ok(merged) => {
                    let merged = Entity::from_body(key.clone(), merged);
                    if Some(&merged.digest) == c_digest.as_ref() {
                        return Decision::TakeCloud(cloud.clone());
                    }
                    return push(Some(merged.body), true);
                }
                Err(field) => conflict_reason(kind, &field),
            }
        }
        (_, None, Some(_)) => format!("the {kind} was deleted here and changed in the cloud"),
        (_, Some(_), None) => format!("the {kind} was changed here and deleted in the cloud"),
        (_, None, None) => return Decision::InSync(cloud.clone()),
    };
    match prefer {
        Some(Side::Local) => push(local.map(|e| e.body.clone()), false),
        Some(Side::Cloud) => Decision::TakeCloud(cloud.clone()),
        None => Decision::Conflict(SyncConflict {
            key: key.clone(),
            reason,
            local: local.map(|e| e.body.clone()),
            cloud: cloud.body.clone(),
            cloud_version: cloud.version,
            detected_at: now,
        }),
    }
}

fn base_of(state: &EntityState) -> EntityBase {
    let body = if state.deleted {
        None
    } else {
        state.body.clone()
    };
    EntityBase {
        key: state.key(),
        version: state.version,
        deleted: state.deleted || body.is_none(),
        digest: body.as_ref().map(digest),
        body,
    }
}

impl LocalApp {
    fn ledger(&self) -> Result<&dyn SyncLedger, AppError> {
        self.store()
            .sync_ledger()
            .ok_or_else(|| AppError::InvalidArguments("this store does not sync".into()))
    }

    /// Where sync stands: the account, unresolved conflicts, what is
    /// waiting to be pushed.
    pub async fn sync_status(&self) -> Result<SyncStatus, AppError> {
        let ledger = self.ledger()?;
        let synced: HashSet<FeedbackId> = ledger.synced_feedback().await?.into_iter().collect();
        let unsynced = self
            .ranking()
            .feedback()
            .await?
            .iter()
            .filter(|e| e.action != FeedbackAction::Seen && !synced.contains(&e.id))
            .count();
        Ok(SyncStatus {
            account: ledger.sync_account().await?,
            conflicts: ledger
                .sync_conflicts()
                .await?
                .iter()
                .map(ConflictView::of)
                .collect(),
            synced_records: ledger.sync_bases().await?.len(),
            unsynced_feedback: unsynced,
        })
    }

    /// Forgets which cloud account this database synced with (after
    /// `jobhunt logout`); local data is untouched.
    pub async fn forget_sync(&self) -> Result<(), AppError> {
        Ok(self.ledger()?.reset_ledger().await?)
    }

    /// Resolves conflicts: `keys` (or all, when empty) keep `side`. Keeping
    /// the local version sends it on the next sync; keeping the cloud's
    /// replaces the local one now.
    pub async fn resolve_sync_conflicts(
        &self,
        keys: &[EntityKey],
        side: Side,
        now: DateTime<Utc>,
    ) -> Result<usize, AppError> {
        self.exclusive(async {
            let ledger = self.ledger()?;
            let conflicts: Vec<SyncConflict> = ledger
                .sync_conflicts()
                .await?
                .into_iter()
                .filter(|c| keys.is_empty() || keys.contains(&c.key))
                .collect();
            if conflicts.is_empty() {
                return Ok(0);
            }
            if side == Side::Cloud {
                let takes: Vec<(EntityKey, Option<Value>)> = conflicts
                    .iter()
                    .map(|c| (c.key.clone(), c.cloud.clone()))
                    .collect();
                self.write_entities(&takes, "conflicts resolved with the cloud's version", now)
                    .await?;
            }
            let update = LedgerUpdate {
                bases: conflicts
                    .iter()
                    .map(|c| {
                        base_of(&EntityState {
                            kind: c.key.kind,
                            id: c.key.id.clone(),
                            version: c.cloud_version,
                            deleted: c.cloud.is_none(),
                            body: c.cloud.clone(),
                            digest: None,
                        })
                    })
                    .collect(),
                resolved: conflicts.iter().map(|c| c.key.clone()).collect(),
                ..LedgerUpdate::default()
            };
            ledger.update_ledger(&update).await?;
            Ok(conflicts.len())
        })
        .await
    }

    /// Replaces or deletes local profile entities, as one profile save.
    async fn write_entities(
        &self,
        writes: &[(EntityKey, Option<Value>)],
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        if writes.is_empty() {
            return Ok(());
        }
        let profiles = self.profiles();
        let current = profiles.load().await?;
        let revision = current.as_ref().map_or(0, |d| d.profile.revision);
        let mut entities: BTreeMap<EntityKey, Entity> = match &current {
            Some(data) => decompose(data)
                .map_err(|e| AppError::storage("reading the profile", e))?
                .into_iter()
                .map(|e| (e.key.clone(), e))
                .collect(),
            None => BTreeMap::new(),
        };
        for (key, body) in writes {
            match body {
                Some(body) => {
                    entities.insert(key.clone(), Entity::from_body(key.clone(), body.clone()));
                }
                None => {
                    entities.remove(key);
                }
            }
        }
        let list: Vec<Entity> = entities.into_values().collect();
        if list.is_empty() {
            return Ok(());
        }
        let mut data = compose(profiles.profile_id(), revision + 1, &list, true).map_err(|e| {
            AppError::Conflict(format!(
                "the cloud's changes cannot be applied to this profile ({e}); \
                 run `jobhunt sync --keep local` or `--keep cloud` to choose"
            ))
        })?;
        data.profile.revision = revision + 1;
        let events = [ProfileEvent::new(
            now,
            ProfileEventKind::Synced,
            None,
            format!("{} records: {detail}", writes.len()),
        )];
        self.store()
            .save_profile(&data, revision, &events)
            .await
            .map_err(AppError::from)
    }

    /// Syncs with JobHunt Cloud (see the module docs).
    pub async fn sync(
        &self,
        transport: &dyn SyncTransport,
        remote: &Remote,
        prefer: Option<Side>,
        now: DateTime<Utc>,
    ) -> Result<SyncReport, AppError> {
        let ledger = self.ledger()?;
        if let Some(account) = ledger.sync_account().await?
            && (account.server != remote.server || account.user_id != remote.user_id)
        {
            // Another account or server: everything local is new to it.
            tracing::info!("sync target changed; starting over");
            ledger.reset_ledger().await?;
        }
        let mut report = SyncReport::default();
        for round in 1..=ROUNDS {
            report.rounds = round;
            let cursor = ledger.sync_account().await?.map_or(0, |a| a.cursor);
            let pull = transport
                .pull(&PullRequest {
                    protocol: SYNC_PROTOCOL,
                    cursor,
                })
                .await?;
            if pull.protocol != SYNC_PROTOCOL {
                return Err(AppError::CloudUnavailable(format!(
                    "JobHunt Cloud speaks sync protocol {}, this JobHunt speaks {SYNC_PROTOCOL}; \
                     update JobHunt",
                    pull.protocol
                )));
            }
            if self
                .sync_round(transport, ledger, remote, &pull, prefer, &mut report, now)
                .await?
            {
                report.cursor = pull.cursor;
                report.conflicts = ledger
                    .sync_conflicts()
                    .await?
                    .iter()
                    .map(ConflictView::of)
                    .collect();
                return Ok(report);
            }
            tracing::info!(round, "the cloud changed during sync; merging again");
        }
        Err(AppError::Conflict(
            "the cloud kept changing while syncing; run `jobhunt sync` again".into(),
        ))
    }

    /// One pull-merge-push round. Returns false when the push was refused
    /// because the cloud changed meanwhile (the caller pulls again).
    #[allow(clippy::too_many_arguments)]
    async fn sync_round(
        &self,
        transport: &dyn SyncTransport,
        ledger: &dyn SyncLedger,
        remote: &Remote,
        pull: &PullResponse,
        prefer: Option<Side>,
        report: &mut SyncReport,
        now: DateTime<Utc>,
    ) -> Result<bool, AppError> {
        // 1. Jobs, verifications and feedback from the cloud: union.
        let pulled_feedback: Vec<FeedbackEvent> = pull
            .feedback
            .iter()
            .filter(|e| e.action != FeedbackAction::Seen)
            .cloned()
            .collect();
        let imported = self
            .store()
            .import_state(StateImport {
                profile: None,
                jobs: &pull.jobs,
                feedback: &pulled_feedback,
            })
            .await?;
        report.jobs_pulled += imported.jobs_added;
        report.feedback_pulled += imported.feedback_added;
        for v in &pull.verifications {
            if self.store().import_verification(v).await? {
                report.verifications_pulled += 1;
            }
        }

        // 2. The profile, entity by entity, under the write lock.
        let merge = self
            .exclusive(async {
                let bases: HashMap<EntityKey, EntityBase> = ledger
                    .sync_bases()
                    .await?
                    .into_iter()
                    .map(|b| (b.key.clone(), b))
                    .collect();
                let open: HashMap<EntityKey, SyncConflict> = ledger
                    .sync_conflicts()
                    .await?
                    .into_iter()
                    .map(|c| (c.key.clone(), c))
                    .collect();
                let local_data = self.profiles().load().await?;
                let local: HashMap<EntityKey, Entity> = match &local_data {
                    Some(data) => decompose(data)
                        .map_err(|e| AppError::storage("reading the profile", e))?
                        .into_iter()
                        .map(|e| (e.key.clone(), e))
                        .collect(),
                    None => HashMap::new(),
                };
                let changed: HashMap<EntityKey, &EntityState> =
                    pull.entities.iter().map(|e| (e.key(), e)).collect();
                let keys: BTreeSet<&EntityKey> = bases
                    .keys()
                    .chain(local.keys())
                    .chain(changed.keys())
                    .collect();

                let mut outcome = MergeOutcome::default();
                for key in keys {
                    let base = bases.get(key);
                    let cloud = match (changed.get(key), base) {
                        (Some(c), _) => (*c).clone(),
                        (None, Some(b)) => state_of(b),
                        (None, None) => EntityState {
                            kind: key.kind,
                            id: key.id.clone(),
                            version: 0,
                            deleted: true,
                            body: None,
                            digest: None,
                        },
                    };
                    if let Some(existing) = open.get(key) {
                        // Still waiting for the person: keep the local
                        // version, refresh what the cloud has if it changed
                        // again.
                        if changed.contains_key(key) && cloud.version != existing.cloud_version {
                            outcome.conflicts.push(SyncConflict {
                                cloud: cloud.body.clone(),
                                cloud_version: cloud.version,
                                ..existing.clone()
                            });
                        }
                        continue;
                    }
                    match decide(key, base, local.get(key), &cloud, prefer, now) {
                        Decision::InSync(state) => {
                            if base.is_none_or(|b| b.version != state.version) {
                                outcome.bases.push(base_of(&state));
                            }
                        }
                        Decision::TakeCloud(state) => {
                            outcome.writes.push((key.clone(), state.body.clone()));
                            outcome.bases.push(base_of(&state));
                        }
                        Decision::Push {
                            body,
                            base_version,
                            local_write,
                        } => {
                            if local_write {
                                outcome.writes.push((key.clone(), body.clone()));
                                outcome.merged += 1;
                            }
                            outcome.pushes.push(EntityMutation {
                                kind: key.kind,
                                id: key.id.clone(),
                                base_version,
                                body,
                            });
                        }
                        Decision::Conflict(conflict) => outcome.conflicts.push(conflict),
                    }
                }
                self.write_entities(&outcome.writes, "changes from JobHunt Cloud", now)
                    .await?;
                Ok(outcome)
            })
            .await?;
        report.applied_locally += merge.writes.len().saturating_sub(merge.merged);
        report.merged += merge.merged;

        // 3. Local feedback the cloud may not have, with its jobs.
        let synced: HashSet<FeedbackId> = ledger.synced_feedback().await?.into_iter().collect();
        let pulled_ids: HashSet<FeedbackId> = pulled_feedback.iter().map(|e| e.id).collect();
        let feedback: Vec<FeedbackEvent> = self
            .ranking()
            .feedback()
            .await?
            .into_iter()
            .filter(|e| e.action != FeedbackAction::Seen)
            .filter(|e| !synced.contains(&e.id) && !pulled_ids.contains(&e.id))
            .collect();
        let mut jobs: Vec<JobRecord> = Vec::new();
        let mut seen_jobs = HashSet::new();
        for e in &feedback {
            if seen_jobs.insert(e.job)
                && let Some(record) = self.store().get(e.job).await?
            {
                jobs.push(record);
            }
        }

        // 4. Push, all or nothing.
        let mut update = LedgerUpdate {
            account: Some(SyncAccount {
                server: remote.server.clone(),
                user_id: remote.user_id.clone(),
                cursor: pull.cursor,
                last_sync_at: Some(now),
            }),
            bases: merge.bases,
            conflicts: merge.conflicts,
            resolved: Vec::new(),
            feedback_synced: pulled_ids.into_iter().collect(),
        };
        if !merge.pushes.is_empty() || !feedback.is_empty() {
            let response = transport
                .push(&PushRequest {
                    protocol: SYNC_PROTOCOL,
                    push_id: format!("{}-{}", remote.user_id, now.timestamp_micros()),
                    entities: merge.pushes.clone(),
                    feedback: feedback.clone(),
                    jobs,
                })
                .await?;
            if !response.conflicts.is_empty() {
                // The cloud changed after the pull. What was merged
                // locally stays; keep the pulled feedback and bases too,
                // but not the cursor, and merge again.
                update.account = None;
                ledger.update_ledger(&update).await?;
                return Ok(false);
            }
            let pushed: HashMap<EntityKey, &EntityMutation> =
                merge.pushes.iter().map(|m| (m.key(), m)).collect();
            for applied in &response.applied {
                let key = EntityKey::new(applied.kind, applied.id.clone());
                let body = pushed.get(&key).and_then(|m| m.body.clone());
                update.bases.push(base_of(&EntityState {
                    kind: applied.kind,
                    id: applied.id.clone(),
                    version: applied.version,
                    deleted: applied.deleted,
                    body,
                    digest: None,
                }));
            }
            report.pushed += merge.pushes.len();
            report.feedback_pushed += response.feedback_added;
            update.feedback_synced.extend(feedback.iter().map(|e| e.id));
        }
        ledger.update_ledger(&update).await?;
        Ok(true)
    }
}

#[derive(Default)]
struct MergeOutcome {
    writes: Vec<(EntityKey, Option<Value>)>,
    bases: Vec<EntityBase>,
    pushes: Vec<EntityMutation>,
    conflicts: Vec<SyncConflict>,
    merged: usize,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn one_sided_changes_win_and_both_sided_ones_merge_by_field() {
        let base = json!({"text": "a", "verification": "unverified", "updated_at": "2026-01-01T00:00:00.000000Z"});
        let local = json!({"text": "a", "verification": "confirmed", "updated_at": "2026-01-02T00:00:00.000000Z"});
        let cloud = json!({"text": "b", "verification": "unverified", "updated_at": "2026-01-03T00:00:00.000000Z"});
        let merged = merge3(&base, &local, &cloud).unwrap();
        assert_eq!(
            merged,
            json!({"text": "b", "verification": "confirmed", "updated_at": "2026-01-03T00:00:00.000000Z"})
        );
    }

    #[test]
    fn different_decisions_conflict() {
        let base = json!({"verification": "unverified"});
        let local = json!({"verification": "confirmed"});
        let cloud = json!({"verification": "rejected"});
        assert_eq!(merge3(&base, &local, &cloud), Err("verification".into()));
        assert!(conflict_reason("claim", "verification").contains("decided differently"));
    }

    #[test]
    fn nested_fields_and_sets_merge() {
        let base = json!({"meta": {"edited_fields": ["title"], "verification": "unverified", "notes": []}});
        let local = json!({"meta": {"edited_fields": ["title", "company"], "verification": "unverified", "notes": []}});
        let cloud = json!({"meta": {"edited_fields": ["summary", "title"], "verification": "confirmed", "notes": []}});
        let merged = merge3(&base, &local, &cloud).unwrap();
        assert_eq!(
            merged["meta"]["edited_fields"],
            json!(["company", "summary", "title"])
        );
        assert_eq!(merged["meta"]["verification"], "confirmed");
        let clash = json!({"meta": {"company": "X"}});
        let other = json!({"meta": {"company": "Y"}});
        assert_eq!(
            merge3(&json!({"meta": {}}), &clash, &other),
            Err("meta.company".into())
        );
    }

    #[test]
    fn deciding_entities() {
        let key = EntityKey::new(jobhunt_profile::entities::EntityKind::Claim, "clm_1");
        let body = |v: &str| json!({"id": "clm_1", "verification": v});
        let entity = |v: &str| Entity::from_body(key.clone(), body(v));
        let state = |v: Option<&str>, version: u64| EntityState {
            kind: key.kind,
            id: key.id.clone(),
            version,
            deleted: v.is_none(),
            digest: v.map(|v| entity(v).digest),
            body: v.map(body),
        };
        let base = base_of(&state(Some("unverified"), 1));
        let now = Utc::now();
        // Local change only: push, based on the cloud's version.
        let local = entity("confirmed");
        assert!(matches!(
            decide(
                &key,
                Some(&base),
                Some(&local),
                &state(Some("unverified"), 1),
                None,
                now
            ),
            Decision::Push {
                base_version: 1,
                local_write: false,
                ..
            }
        ));
        // Cloud change only: take it.
        let unchanged = entity("unverified");
        assert!(matches!(
            decide(
                &key,
                Some(&base),
                Some(&unchanged),
                &state(Some("rejected"), 2),
                None,
                now
            ),
            Decision::TakeCloud(_)
        ));
        // Both decided differently: a conflict, unless a side is preferred.
        assert!(matches!(
            decide(
                &key,
                Some(&base),
                Some(&local),
                &state(Some("rejected"), 2),
                None,
                now
            ),
            Decision::Conflict(_)
        ));
        assert!(matches!(
            decide(
                &key,
                Some(&base),
                Some(&local),
                &state(Some("rejected"), 2),
                Some(Side::Local),
                now
            ),
            Decision::Push {
                base_version: 2,
                ..
            }
        ));
        // Edited here, deleted there: a conflict.
        assert!(matches!(
            decide(&key, Some(&base), Some(&local), &state(None, 2), None, now),
            Decision::Conflict(_)
        ));
        // New on both sides and equal: in sync.
        assert!(matches!(
            decide(
                &key,
                None,
                Some(&local),
                &state(Some("confirmed"), 1),
                None,
                now
            ),
            Decision::InSync(_)
        ));
    }
}
