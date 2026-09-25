//! [`ProfileRepository`] for [`PgUserStore`].
//!
//! A profile is stored as its entities ([`jobhunt_profile::entities`]): one
//! encrypted row per record with a version and the person's change
//! sequence, so sync can ask "what changed since" and a database reader
//! sees ciphertext. Saving compares each record's digest with the stored
//! one and writes only what changed; records missing from the aggregate
//! become tombstones. The revision check is the same optimistic
//! concurrency rule as the local store, taken under a row lock.

use async_trait::async_trait;
use chrono::Utc;
use jobhunt_profile::entities::{Entity, EntityKey, EntityKind, compose, decompose};
use jobhunt_profile::{
    Claim, ClaimQuery, ProfileData, ProfileEvent, ProfileEventKind, ProfileId, ProfileRepository,
    StorageError,
};
use sqlx::{PgConnection, Row};

use super::jobs::import_job;
use super::user::{PgUserStore, next_seq};
use crate::state::{StateImport, StateImported};

fn query_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |source| StorageError::Query {
        operation,
        source: Box::new(source),
    }
}

fn corrupt(record: &str, detail: impl ToString) -> StorageError {
    StorageError::Corrupt {
        record: record.to_owned(),
        detail: detail.to_string(),
    }
}

/// A job-side storage error in the profile domain's terms.
pub(crate) fn from_jobs(error: jobhunt_jobs::StorageError) -> StorageError {
    match error {
        jobhunt_jobs::StorageError::Corrupt { id, detail } => {
            StorageError::Corrupt { record: id, detail }
        }
        other => StorageError::Query {
            operation: "writing private data",
            source: Box::new(other),
        },
    }
}

/// A stored entity's bookkeeping (not its body).
#[derive(Debug, Clone)]
pub(crate) struct StoredEntity {
    pub key: EntityKey,
    pub version: i64,
    pub deleted: bool,
    pub digest: Option<String>,
}

impl PgUserStore {
    pub(crate) fn entity_context(&self, profile: &str, key: &EntityKey) -> String {
        format!(
            "profile_entities|{}|{profile}|{}|{}",
            self.user, key.kind, key.id
        )
    }

    fn event_context(&self, profile: &str) -> String {
        format!("profile_events|{}|{profile}", self.user)
    }

    /// Seals an entity's body.
    pub(crate) fn seal_entity(
        &self,
        profile: &str,
        entity: &Entity,
    ) -> Result<Vec<u8>, StorageError> {
        self.seal(
            &self.entity_context(profile, &entity.key),
            entity.body.to_string().as_bytes(),
        )
        .map_err(from_jobs)
    }

    /// Opens an entity's body.
    pub(crate) fn open_entity(
        &self,
        profile: &str,
        key: EntityKey,
        sealed: &[u8],
    ) -> Result<Entity, StorageError> {
        let json = self
            .open(&self.entity_context(profile, &key), sealed)
            .map_err(from_jobs)?;
        let body = serde_json::from_slice(&json).map_err(|e| corrupt(&key.id, e))?;
        Ok(Entity::from_body(key, body))
    }

    /// Every entity's bookkeeping, tombstones included.
    pub(crate) async fn stored_entities(
        &self,
        tx: &mut PgConnection,
        profile: &str,
    ) -> Result<Vec<StoredEntity>, StorageError> {
        sqlx::query(
            "SELECT kind, entity_id, version, deleted, digest FROM profile_entities \
             WHERE user_id = $1 AND profile_id = $2",
        )
        .bind(self.user.as_str())
        .bind(profile)
        .fetch_all(&mut *tx)
        .await
        .map_err(query_error("loading profile records"))?
        .iter()
        .map(|row| {
            let id: String = row
                .try_get("entity_id")
                .map_err(|e| corrupt("<profile_entities>", e))?;
            let kind: String = row.try_get("kind").map_err(|e| corrupt(&id, e))?;
            Ok(StoredEntity {
                key: EntityKey::new(
                    kind.parse().map_err(|e: String| corrupt(&id, e))?,
                    id.clone(),
                ),
                version: row.try_get("version").map_err(|e| corrupt(&id, e))?,
                deleted: row.try_get("deleted").map_err(|e| corrupt(&id, e))?,
                digest: row.try_get("digest").map_err(|e| corrupt(&id, e))?,
            })
        })
        .collect()
    }

    /// The live entities (bodies decrypted).
    pub(crate) async fn live_entities(
        &self,
        conn: &mut PgConnection,
        profile: &str,
    ) -> Result<Vec<Entity>, StorageError> {
        let rows = sqlx::query(
            "SELECT kind, entity_id, body FROM profile_entities \
             WHERE user_id = $1 AND profile_id = $2 AND NOT deleted ORDER BY kind, entity_id",
        )
        .bind(self.user.as_str())
        .bind(profile)
        .fetch_all(&mut *conn)
        .await
        .map_err(query_error("loading profile records"))?;
        rows.iter()
            .map(|row| {
                let id: String = row
                    .try_get("entity_id")
                    .map_err(|e| corrupt("<profile_entities>", e))?;
                let kind: EntityKind = row
                    .try_get::<String, _>("kind")
                    .map_err(|e| corrupt(&id, e))?
                    .parse()
                    .map_err(|e: String| corrupt(&id, e))?;
                let body: Vec<u8> = row.try_get("body").map_err(|e| corrupt(&id, e))?;
                self.open_entity(profile, EntityKey::new(kind, id), &body)
            })
            .collect()
    }

    /// Writes one entity (a new version, or a tombstone with `None`).
    pub(crate) async fn write_entity(
        &self,
        tx: &mut PgConnection,
        profile: &str,
        key: &EntityKey,
        entity: Option<&Entity>,
        version: i64,
        seq: i64,
    ) -> Result<(), StorageError> {
        let body = entity.map(|e| self.seal_entity(profile, e)).transpose()?;
        sqlx::query(
            "INSERT INTO profile_entities (user_id, profile_id, kind, entity_id, version, seq, \
             deleted, digest, body, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (user_id, profile_id, kind, entity_id) DO UPDATE SET \
             version = excluded.version, seq = excluded.seq, deleted = excluded.deleted, \
             digest = excluded.digest, body = excluded.body, updated_at = excluded.updated_at",
        )
        .bind(self.user.as_str())
        .bind(profile)
        .bind(key.kind.as_str())
        .bind(&key.id)
        .bind(version)
        .bind(seq)
        .bind(entity.is_none())
        .bind(entity.map(|e| e.digest.clone()))
        .bind(body)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await
        .map_err(query_error("saving a profile record"))?;
        Ok(())
    }

    pub(crate) async fn append_events(
        &self,
        tx: &mut PgConnection,
        profile: &str,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError> {
        for event in events {
            let detail = self
                .seal(&self.event_context(profile), event.detail.as_bytes())
                .map_err(from_jobs)?;
            sqlx::query(
                "INSERT INTO profile_events (user_id, profile_id, at, kind, record, detail) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(self.user.as_str())
            .bind(profile)
            .bind(event.at)
            .bind(event.kind.as_str())
            .bind(&event.record)
            .bind(detail)
            .execute(&mut *tx)
            .await
            .map_err(query_error("recording profile history"))?;
        }
        Ok(())
    }

    /// Checks and bumps the profile row: the stored revision must be
    /// `expected` (0: no profile yet). Locks the row until the transaction
    /// ends.
    pub(crate) async fn claim_revision(
        &self,
        tx: &mut PgConnection,
        data_profile: &jobhunt_profile::Profile,
        expected: u64,
    ) -> Result<(), StorageError> {
        let pid = data_profile.id.to_string();
        let stored: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM profiles WHERE user_id = $1 AND profile_id = $2 FOR UPDATE",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .fetch_optional(&mut *tx)
        .await
        .map_err(query_error("checking the profile revision"))?;
        let found = stored.map_or(0, |r| u64::try_from(r).unwrap_or(0));
        if found != expected {
            return Err(StorageError::Conflict { expected, found });
        }
        sqlx::query(
            "INSERT INTO profiles (user_id, profile_id, revision, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT (user_id, profile_id) DO UPDATE SET \
             revision = excluded.revision, updated_at = excluded.updated_at",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .bind(i64::try_from(data_profile.revision).unwrap_or(i64::MAX))
        .bind(data_profile.created_at)
        .bind(data_profile.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(query_error("saving the profile"))?;
        Ok(())
    }

    /// Saves a whole aggregate inside a transaction (see the module docs).
    pub(crate) async fn save_profile_tx(
        &self,
        tx: &mut PgConnection,
        data: &ProfileData,
        expected_revision: u64,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError> {
        let seq = next_seq(&mut *tx, &self.user).await.map_err(from_jobs)?;
        self.claim_revision(&mut *tx, &data.profile, expected_revision)
            .await?;
        let pid = data.id().to_string();
        let stored = self.stored_entities(&mut *tx, &pid).await?;
        let entities = decompose(data).map_err(|e| corrupt(&pid, e))?;
        let mut live: std::collections::HashSet<&EntityKey> = std::collections::HashSet::new();
        for entity in &entities {
            live.insert(&entity.key);
            let current = stored.iter().find(|s| s.key == entity.key);
            let unchanged = current
                .is_some_and(|s| !s.deleted && s.digest.as_deref() == Some(entity.digest.as_str()));
            if unchanged {
                continue;
            }
            let version = current.map_or(1, |s| s.version + 1);
            self.write_entity(&mut *tx, &pid, &entity.key, Some(entity), version, seq)
                .await?;
        }
        for s in stored
            .iter()
            .filter(|s| !s.deleted && !live.contains(&s.key))
        {
            self.write_entity(&mut *tx, &pid, &s.key, None, s.version + 1, seq)
                .await?;
        }
        self.append_events(&mut *tx, &pid, events).await
    }

    /// Loads the aggregate from a connection.
    pub(crate) async fn load_in(
        &self,
        conn: &mut PgConnection,
        id: ProfileId,
    ) -> Result<Option<ProfileData>, StorageError> {
        let pid = id.to_string();
        let Some(revision): Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM profiles WHERE user_id = $1 AND profile_id = $2",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .fetch_optional(&mut *conn)
        .await
        .map_err(query_error("loading a profile"))?
        else {
            return Ok(None);
        };
        let entities = self.live_entities(&mut *conn, &pid).await?;
        if entities.is_empty() {
            return Ok(None);
        }
        let revision = u64::try_from(revision).unwrap_or(0);
        compose(id, revision, &entities, false)
            .map(Some)
            .map_err(|e| corrupt(&pid, e))
    }

    /// Stores an imported state in one transaction.
    pub(crate) async fn import_state_tx(
        &self,
        state: StateImport<'_>,
    ) -> Result<StateImported, StorageError> {
        let mut tx = self
            .shared
            .pool()
            .begin()
            .await
            .map_err(query_error("starting an import"))?;
        let mut out = StateImported::default();
        if let Some(p) = state.profile {
            self.save_profile_tx(&mut tx, p.data, p.expected_revision, p.events)
                .await?;
            out.profile = true;
        }
        for record in state.jobs {
            if import_job(&mut tx, record).await.map_err(from_jobs)? {
                out.jobs_added += 1;
            } else {
                out.jobs_present += 1;
            }
        }
        if !state.feedback.is_empty() {
            let seq = next_seq(&mut tx, &self.user).await.map_err(from_jobs)?;
            for event in state.feedback {
                if self
                    .insert_feedback(&mut tx, event, seq, true)
                    .await
                    .map_err(from_jobs)?
                {
                    out.feedback_added += 1;
                } else {
                    out.feedback_present += 1;
                }
            }
        }
        tx.commit()
            .await
            .map_err(query_error("committing an import"))?;
        self.cache.clear();
        Ok(out)
    }
}

#[async_trait]
impl ProfileRepository for PgUserStore {
    async fn load_profile(&self, id: ProfileId) -> Result<Option<ProfileData>, StorageError> {
        // One transaction, so the aggregate is a consistent snapshot.
        let mut tx = self
            .shared
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(query_error("starting a transaction"))?;
        let data = self.load_in(&mut tx, id).await?;
        tx.commit().await.map_err(query_error("finishing a read"))?;
        Ok(data)
    }

    async fn save_profile(
        &self,
        data: &ProfileData,
        expected_revision: u64,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError> {
        let mut tx = self
            .shared
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        self.save_profile_tx(&mut tx, data, expected_revision, events)
            .await?;
        tx.commit()
            .await
            .map_err(query_error("committing a profile"))?;
        Ok(())
    }

    async fn find_claims(
        &self,
        profile: ProfileId,
        query: &ClaimQuery,
    ) -> Result<Vec<Claim>, StorageError> {
        let Some(data) = self.load_profile(profile).await? else {
            return Ok(Vec::new());
        };
        let mut claims: Vec<Claim> = data.claims(query).into_iter().cloned().collect();
        // The local store's order.
        claims.sort_by(|a, b| {
            a.subject
                .kind_str()
                .cmp(b.subject.kind_str())
                .then_with(|| a.subject.id_string().cmp(&b.subject.id_string()))
                .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
                .then_with(|| a.position.cmp(&b.position))
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        Ok(claims)
    }

    async fn profile_events(
        &self,
        profile: ProfileId,
        limit: usize,
    ) -> Result<Vec<ProfileEvent>, StorageError> {
        let pid = profile.to_string();
        let rows = sqlx::query(
            "SELECT id, at, kind, record, detail FROM profile_events \
             WHERE user_id = $1 AND profile_id = $2 ORDER BY id DESC LIMIT $3",
        )
        .bind(self.user.as_str())
        .bind(&pid)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(self.shared.pool())
        .await
        .map_err(query_error("loading profile history"))?;
        rows.iter()
            .map(|row| {
                let id: i64 = row.try_get("id").unwrap_or_default();
                let label = format!("profile event {id}");
                let kind: String = row.try_get("kind").map_err(|e| corrupt(&label, e))?;
                let detail: Vec<u8> = row.try_get("detail").map_err(|e| corrupt(&label, e))?;
                let detail = self
                    .open(&self.event_context(&pid), &detail)
                    .map_err(from_jobs)?;
                Ok(ProfileEvent {
                    at: row.try_get("at").map_err(|e| corrupt(&label, e))?,
                    kind: ProfileEventKind::from_canonical(&kind)
                        .ok_or_else(|| corrupt(&label, format!("unknown kind {kind:?}")))?,
                    record: row.try_get("record").map_err(|e| corrupt(&label, e))?,
                    detail: String::from_utf8(detail).map_err(|e| corrupt(&label, e))?,
                })
            })
            .collect()
    }
}
