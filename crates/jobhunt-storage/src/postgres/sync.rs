//! The cloud side of sync (see [`crate::sync`] for the protocol).

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use jobhunt_jobs::{JobRecord, StorageError};
use jobhunt_profile::entities::{Entity, EntityKey, EntityKind, compose};
use jobhunt_profile::{ProfileEvent, ProfileEventKind, ProfileId};
use jobhunt_ranking::{FeedbackAction, FeedbackEvent};
use sqlx::Row;

use super::jobs::{decode_record, import_job};
use super::profile::StoredEntity;
use super::user::{PgUserStore, next_seq};
use super::{corrupt, query_error};
use crate::sync::{
    AppliedEntity, EntityState, PullRequest, PullResponse, PushConflict, PushError, PushRequest,
    PushResponse, SYNC_PROTOCOL,
};

/// The most a single push may carry.
pub const MAX_PUSH_ENTITIES: usize = 5_000;
pub const MAX_PUSH_FEEDBACK: usize = 10_000;
pub const MAX_PUSH_JOBS: usize = 5_000;

fn profile_error(e: jobhunt_profile::StorageError) -> StorageError {
    match e {
        jobhunt_profile::StorageError::Corrupt { record, detail } => {
            StorageError::Corrupt { id: record, detail }
        }
        other => StorageError::Query {
            operation: "syncing the profile",
            source: Box::new(other),
        },
    }
}

/// The person's sync bookkeeping in the cloud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSyncState {
    pub change_seq: u64,
    pub profile_revision: u64,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_shortlist_at: Option<DateTime<Utc>>,
    pub notification_cursor: u64,
    pub feedback: u64,
}

impl PgUserStore {
    /// Where the person's data stands (for `narrow account`).
    pub async fn sync_state(&self) -> Result<CloudSyncState, StorageError> {
        let row = sqlx::query(
            "SELECT u.change_seq, s.last_sync_at, s.last_shortlist_at, \
             COALESCE(s.notification_cursor, 0) AS notification_cursor, \
             (SELECT revision FROM profiles p WHERE p.user_id = u.id AND p.profile_id = $2) \
               AS revision, \
             (SELECT COUNT(*) FROM feedback f WHERE f.user_id = u.id AND f.action <> 'seen') \
               AS feedback \
             FROM users u LEFT JOIN user_state s ON s.user_id = u.id WHERE u.id = $1",
        )
        .bind(self.user.as_str())
        .bind(ProfileId::local().to_string())
        .fetch_optional(self.shared.pool())
        .await
        .map_err(query_error("loading sync state"))?
        .ok_or_else(|| corrupt(self.user.as_str(), "the account does not exist"))?;
        let bad = |e: sqlx::Error| corrupt(self.user.as_str(), e);
        let n = |v: i64| u64::try_from(v).unwrap_or(0);
        Ok(CloudSyncState {
            change_seq: n(row.try_get("change_seq").map_err(bad)?),
            profile_revision: n(row
                .try_get::<Option<i64>, _>("revision")
                .map_err(bad)?
                .unwrap_or(0)),
            last_sync_at: row.try_get("last_sync_at").map_err(bad)?,
            last_shortlist_at: row.try_get("last_shortlist_at").map_err(bad)?,
            notification_cursor: n(row.try_get("notification_cursor").map_err(bad)?),
            feedback: n(row.try_get("feedback").map_err(bad)?),
        })
    }

    /// Everything that changed after `request.cursor`, from one consistent
    /// snapshot.
    pub async fn sync_pull(
        &self,
        request: &PullRequest,
        now: DateTime<Utc>,
    ) -> Result<PullResponse, StorageError> {
        let pid = ProfileId::local().to_string();
        let cursor = i64::try_from(request.cursor).unwrap_or(i64::MAX);
        let mut tx = self
            .shared
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .map_err(query_error("starting a transaction"))?;
        let seq_now: i64 = sqlx::query_scalar("SELECT change_seq FROM users WHERE id = $1")
            .bind(self.user.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(query_error("reading the change counter"))?
            .ok_or_else(|| corrupt(self.user.as_str(), "the account does not exist"))?;
        let revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM profiles WHERE user_id = $1 AND profile_id = $2",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(query_error("reading the profile revision"))?;

        let rows = sqlx::query(
            "SELECT kind, entity_id, version, deleted, digest, body FROM profile_entities \
             WHERE user_id = $1 AND profile_id = $2 AND seq > $3 ORDER BY seq, kind, entity_id",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .bind(cursor)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading changed profile records"))?;
        let mut entities = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row
                .try_get("entity_id")
                .map_err(|e| corrupt("<profile_entities>", e))?;
            let bad = |e: sqlx::Error| corrupt(&id, e);
            let kind: EntityKind = row
                .try_get::<String, _>("kind")
                .map_err(bad)?
                .parse()
                .map_err(|e: String| corrupt(&id, e))?;
            let deleted: bool = row.try_get("deleted").map_err(bad)?;
            let version: i64 = row.try_get("version").map_err(bad)?;
            let body: Option<Vec<u8>> = row.try_get("body").map_err(bad)?;
            let entity = body
                .map(|sealed| {
                    self.open_entity(&pid, EntityKey::new(kind, id.clone()), &sealed)
                        .map_err(profile_error)
                })
                .transpose()?;
            entities.push(EntityState {
                kind,
                id: id.clone(),
                version: u64::try_from(version).unwrap_or(0),
                deleted,
                digest: entity.as_ref().map(|e| e.digest.clone()),
                body: entity.map(|e| e.body),
            });
        }

        let feedback_rows = sqlx::query(
            "SELECT id, profile_id, opportunity_id, job_id, action, reason, title, company, \
             recorded_at FROM feedback WHERE user_id = $1 AND seq > $2 AND action <> 'seen' \
             ORDER BY recorded_at, id",
        )
        .bind(self.user.as_str())
        .bind(cursor)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading changed feedback"))?;
        let feedback: Vec<FeedbackEvent> = feedback_rows
            .iter()
            .map(|r| self.decode_feedback(r))
            .collect::<Result<_, _>>()?;

        let shown: Vec<String> = sqlx::query_scalar(
            "SELECT opportunity_id FROM user_opportunities WHERE user_id = $1 AND seq > $2",
        )
        .bind(self.user.as_str())
        .bind(cursor)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading recommendations"))?;

        // Every record of every opportunity the changes refer to, plus the
        // record each feedback event names.
        let mut opportunities: BTreeSet<String> = shown.into_iter().collect();
        opportunities.extend(feedback.iter().map(|e| e.opportunity.to_string()));
        let named: Vec<String> = feedback.iter().map(|e| e.job.to_string()).collect();
        let job_rows = sqlx::query(
            "SELECT * FROM jobs WHERE opportunity_id = ANY($1) OR id = ANY($2) \
             OR opportunity_id IN (SELECT opportunity_id FROM jobs WHERE id = ANY($2)) \
             ORDER BY first_seen_at, id",
        )
        .bind(opportunities.into_iter().collect::<Vec<_>>())
        .bind(&named)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading jobs to sync"))?;
        let jobs: Vec<JobRecord> = job_rows
            .iter()
            .map(decode_record)
            .collect::<Result<_, _>>()?;
        let job_ids: Vec<String> = jobs.iter().map(|j| j.id.to_string()).collect();
        let verification_rows = sqlx::query(
            "SELECT DISTINCT ON (job_id) id, record FROM job_verifications \
             WHERE job_id = ANY($1) AND succeeded ORDER BY job_id, attempted_at DESC, id DESC",
        )
        .bind(&job_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading verifications to sync"))?;
        let verifications = verification_rows
            .iter()
            .map(|r| {
                let sqlx::types::Json(v): sqlx::types::Json<
                    jobhunt_jobs::verification::VerificationRecord,
                > = r
                    .try_get("record")
                    .map_err(|e| corrupt("<verification>", e))?;
                Ok(v)
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        tx.commit().await.map_err(query_error("finishing a pull"))?;

        sqlx::query("UPDATE user_state SET last_sync_at = $2 WHERE user_id = $1")
            .bind(self.user.as_str())
            .bind(now)
            .execute(self.shared.pool())
            .await
            .map_err(query_error("recording a sync"))?;
        Ok(PullResponse {
            protocol: SYNC_PROTOCOL,
            cursor: u64::try_from(seq_now).unwrap_or(0),
            profile_revision: revision.map_or(0, |r| u64::try_from(r).unwrap_or(0)),
            entities,
            feedback,
            jobs,
            verifications,
        })
    }

    /// Applies a push all or nothing (see [`crate::sync`]).
    pub async fn sync_push(
        &self,
        request: &PushRequest,
        now: DateTime<Utc>,
    ) -> Result<PushResponse, PushError> {
        if request.protocol != SYNC_PROTOCOL {
            return Err(PushError::Invalid(format!(
                "sync protocol {} is not supported (this server speaks {SYNC_PROTOCOL}); \
                 update JobHunt",
                request.protocol
            )));
        }
        if request.entities.len() > MAX_PUSH_ENTITIES
            || request.feedback.len() > MAX_PUSH_FEEDBACK
            || request.jobs.len() > MAX_PUSH_JOBS
        {
            return Err(PushError::Invalid(format!(
                "a push carries at most {MAX_PUSH_ENTITIES} records, {MAX_PUSH_FEEDBACK} \
                 feedback events and {MAX_PUSH_JOBS} jobs"
            )));
        }
        let mut keys = HashSet::new();
        for m in &request.entities {
            if !keys.insert(m.key()) {
                return Err(PushError::Invalid(format!("{} appears twice", m.key())));
            }
        }
        for e in &request.feedback {
            if e.action == FeedbackAction::Seen {
                return Err(PushError::Invalid(format!(
                    "feedback {} is a \"seen\" mark, which is not synced",
                    e.id
                )));
            }
        }
        for j in &request.jobs {
            if j.posting.id() != j.id {
                return Err(PushError::Invalid(format!(
                    "job {} does not match its source and source id",
                    j.id
                )));
            }
        }

        let pid = ProfileId::local();
        let pid_text = pid.to_string();
        let mut tx = self
            .shared
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        let seq = next_seq(&mut tx, &self.user).await?;

        let stored: Vec<StoredEntity> = self
            .stored_entities(&mut tx, &pid_text)
            .await
            .map_err(profile_error)?;
        let stored_by_key: HashMap<&EntityKey, &StoredEntity> =
            stored.iter().map(|s| (&s.key, s)).collect();

        // Compare-and-set, all or nothing.
        let mut to_apply: Vec<(EntityKey, Option<Entity>, i64)> = Vec::new();
        let mut applied = Vec::new();
        let mut conflicts = Vec::new();
        for m in &request.entities {
            let key = m.key();
            let incoming = m
                .body
                .clone()
                .map(|body| Entity::from_body(key.clone(), body));
            let current = stored_by_key.get(&key).copied();
            let current_version = current.map_or(0, |s| u64::try_from(s.version).unwrap_or(0));
            let current_digest =
                current.and_then(|s| (!s.deleted).then(|| s.digest.clone()).flatten());
            let incoming_digest = incoming.as_ref().map(|e| e.digest.clone());
            if current_version == m.base_version {
                if current_digest == incoming_digest {
                    // Nothing to change (already in this state).
                    applied.push(AppliedEntity {
                        kind: key.kind,
                        id: key.id.clone(),
                        version: current_version,
                        deleted: current.is_none_or(|s| s.deleted),
                    });
                    continue;
                }
                let version = i64::try_from(current_version + 1).unwrap_or(i64::MAX);
                applied.push(AppliedEntity {
                    kind: key.kind,
                    id: key.id.clone(),
                    version: current_version + 1,
                    deleted: incoming.is_none(),
                });
                to_apply.push((key, incoming, version));
            } else if current_digest == incoming_digest {
                // A retry of a push that already landed, or the same change
                // made elsewhere: idempotent.
                applied.push(AppliedEntity {
                    kind: key.kind,
                    id: key.id.clone(),
                    version: current_version,
                    deleted: current.is_none_or(|s| s.deleted),
                });
            } else {
                let current_state = self
                    .entity_state(&mut tx, &pid_text, &key)
                    .await
                    .map_err(profile_error)?;
                conflicts.push(PushConflict {
                    mutation: m.clone(),
                    current: current_state,
                });
            }
        }
        if !conflicts.is_empty() {
            tx.rollback()
                .await
                .map_err(query_error("rolling back a push"))?;
            let revision = self.revision_now().await?;
            return Ok(PushResponse {
                applied: Vec::new(),
                conflicts,
                feedback_added: 0,
                feedback_present: 0,
                jobs_added: 0,
                profile_revision: revision,
            });
        }

        let mut revision = self.revision_in(&mut tx, &pid_text).await?;
        if !to_apply.is_empty() {
            // The profile after the push must be consistent.
            let mut live: HashMap<EntityKey, Entity> = self
                .live_entities(&mut tx, &pid_text)
                .await
                .map_err(profile_error)?
                .into_iter()
                .map(|e| (e.key.clone(), e))
                .collect();
            for (key, entity, _) in &to_apply {
                match entity {
                    Some(e) => live.insert(key.clone(), e.clone()),
                    None => live.remove(key),
                };
            }
            let mut entities: Vec<Entity> = live.into_values().collect();
            entities.sort_by(|a, b| a.key.cmp(&b.key));
            let data = compose(pid, revision + 1, &entities, true)
                .map_err(|e| PushError::Invalid(format!("the pushed profile is invalid: {e}")))?;
            let mut basics = data.profile.clone();
            basics.revision = revision + 1;
            basics.updated_at = now;
            self.claim_revision(&mut tx, &basics, revision)
                .await
                .map_err(profile_error)?;
            revision += 1;
            for (key, entity, version) in &to_apply {
                self.write_entity(&mut tx, &pid_text, key, entity.as_ref(), *version, seq)
                    .await
                    .map_err(profile_error)?;
            }
            self.append_events(
                &mut tx,
                &pid_text,
                &[ProfileEvent::new(
                    now,
                    ProfileEventKind::Synced,
                    None,
                    format!("{} records synced from another device", to_apply.len()),
                )],
            )
            .await
            .map_err(profile_error)?;
        }

        let mut jobs_added = 0;
        for job in &request.jobs {
            if import_job(&mut tx, job).await? {
                jobs_added += 1;
            }
        }
        let mut feedback_added = 0;
        let mut feedback_present = 0;
        if !request.feedback.is_empty() {
            let named: Vec<String> = request.feedback.iter().map(|e| e.job.to_string()).collect();
            let known: HashSet<String> =
                sqlx::query_scalar("SELECT id FROM jobs WHERE id = ANY($1)")
                    .bind(&named)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(query_error("checking feedback jobs"))?
                    .into_iter()
                    .collect();
            for e in &request.feedback {
                if !known.contains(&e.job.to_string()) {
                    return Err(PushError::Invalid(format!(
                        "feedback {} is about job {} which neither the cloud nor the push has",
                        e.id, e.job
                    )));
                }
                let mut event = e.clone();
                event.profile_id = pid_text.clone();
                if self.insert_feedback(&mut tx, &event, seq, true).await? {
                    feedback_added += 1;
                } else {
                    feedback_present += 1;
                }
            }
        }
        sqlx::query(
            "INSERT INTO user_state (user_id, last_sync_at) VALUES ($1, $2) \
             ON CONFLICT (user_id) DO UPDATE SET last_sync_at = excluded.last_sync_at",
        )
        .bind(self.user.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(query_error("recording a sync"))?;
        tx.commit()
            .await
            .map_err(query_error("committing a push"))?;
        self.cache.clear();
        Ok(PushResponse {
            applied,
            conflicts: Vec::new(),
            feedback_added,
            feedback_present,
            jobs_added,
            profile_revision: revision,
        })
    }

    async fn entity_state(
        &self,
        tx: &mut sqlx::PgConnection,
        profile: &str,
        key: &EntityKey,
    ) -> Result<EntityState, jobhunt_profile::StorageError> {
        let row = sqlx::query(
            "SELECT version, deleted, body FROM profile_entities WHERE user_id = $1 \
             AND profile_id = $2 AND kind = $3 AND entity_id = $4",
        )
        .bind(self.user.as_str())
        .bind(profile)
        .bind(key.kind.as_str())
        .bind(&key.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| jobhunt_profile::StorageError::Query {
            operation: "loading a profile record",
            source: Box::new(e),
        })?;
        let Some(row) = row else {
            return Ok(EntityState {
                kind: key.kind,
                id: key.id.clone(),
                version: 0,
                deleted: true,
                body: None,
                digest: None,
            });
        };
        let bad = |e: sqlx::Error| jobhunt_profile::StorageError::Corrupt {
            record: key.id.clone(),
            detail: e.to_string(),
        };
        let version: i64 = row.try_get("version").map_err(bad)?;
        let body: Option<Vec<u8>> = row.try_get("body").map_err(bad)?;
        let entity = body
            .map(|sealed| self.open_entity(profile, key.clone(), &sealed))
            .transpose()?;
        Ok(EntityState {
            kind: key.kind,
            id: key.id.clone(),
            version: u64::try_from(version).unwrap_or(0),
            deleted: row.try_get("deleted").map_err(bad)?,
            digest: entity.as_ref().map(|e| e.digest.clone()),
            body: entity.map(|e| e.body),
        })
    }

    async fn revision_in(
        &self,
        tx: &mut sqlx::PgConnection,
        profile: &str,
    ) -> Result<u64, StorageError> {
        let r: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM profiles WHERE user_id = $1 AND profile_id = $2",
        )
        .bind(self.user.as_str())
        .bind(profile)
        .fetch_optional(&mut *tx)
        .await
        .map_err(query_error("reading the profile revision"))?;
        Ok(r.map_or(0, |r| u64::try_from(r).unwrap_or(0)))
    }

    async fn revision_now(&self) -> Result<u64, StorageError> {
        let mut conn = self
            .shared
            .pool()
            .acquire()
            .await
            .map_err(query_error("reading the profile revision"))?;
        self.revision_in(&mut conn, &ProfileId::local().to_string())
            .await
    }
}
