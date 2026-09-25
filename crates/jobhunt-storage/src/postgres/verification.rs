//! [`VerificationRepository`] for Postgres. Verification attempts are
//! facts about a listing, not about a person, so they are shared: one
//! verification of a job serves everyone who looks at it.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::UpsertOutcome;
use jobhunt_jobs::verification::{VerificationRecord, VerificationRepository};
use jobhunt_jobs::{JobId, JobPosting, ScanResult, StorageError};
use sqlx::Row;
use sqlx::types::Json;

use super::jobs::{Observation, apply_listing, lock_source};
use super::{PgStore, corrupt, query_error};

fn decode(row: &sqlx::postgres::PgRow) -> Result<VerificationRecord, StorageError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| corrupt("<verification>", e))?;
    let Json(record): Json<VerificationRecord> = row
        .try_get("record")
        .map_err(|e| corrupt(&id, format!("verification record: {e}")))?;
    Ok(record)
}

impl PgStore {
    /// Inserts one attempt. With `if_absent`, an attempt whose id is
    /// already stored is left alone and `false` is returned.
    pub(crate) async fn insert_verification(
        &self,
        record: &VerificationRecord,
        if_absent: bool,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(&format!(
            "INSERT INTO job_verifications (id, job_id, opportunity_id, source_kind, \
             source_instance, attempted_at, succeeded, method, listing_status, \
             application_status, authority, checked_url, content_fingerprint, failure_kind, \
             revision, record) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
             $14, $15, $16){}",
            if if_absent {
                " ON CONFLICT (id) DO NOTHING"
            } else {
                ""
            }
        ))
        .bind(record.id.to_string())
        .bind(record.job_id.to_string())
        .bind(record.opportunity_id.to_string())
        .bind(record.source.kind())
        .bind(record.source.instance())
        .bind(record.attempted_at)
        .bind(record.succeeded())
        .bind(record.method.as_str())
        .bind(record.listing.as_str())
        .bind(record.application.status.as_str())
        .bind(record.authority.as_str())
        .bind(record.checked_url.as_deref())
        .bind(record.content_fingerprint.as_deref())
        .bind(record.failure.as_ref().map(|f| f.kind.as_str()))
        .bind(&record.revision)
        .bind(Json(record))
        .execute(self.pool())
        .await
        .map_err(query_error("saving a verification"))?;
        Ok(result.rows_affected() > 0)
    }

    async fn verifications_where(
        &self,
        job: JobId,
        successes_only: bool,
        limit: Option<i64>,
    ) -> Result<Vec<VerificationRecord>, StorageError> {
        let sql = format!(
            "SELECT id, record FROM job_verifications WHERE job_id = $1{} \
             ORDER BY attempted_at DESC, id DESC{}",
            if successes_only { " AND succeeded" } else { "" },
            if limit.is_some() { " LIMIT $2" } else { "" }
        );
        let mut query = sqlx::query(&sql).bind(job.to_string());
        if let Some(limit) = limit {
            query = query.bind(limit);
        }
        let rows = query
            .fetch_all(self.pool())
            .await
            .map_err(query_error("loading verifications"))?;
        rows.iter().map(decode).collect()
    }

    /// The latest attempt and the latest success of many jobs at once.
    pub(crate) async fn latest_verifications(
        &self,
        jobs: &[String],
    ) -> Result<Vec<(bool, VerificationRecord)>, StorageError> {
        let rows = sqlx::query(
            "(SELECT DISTINCT ON (job_id) FALSE AS success_only, id, record \
              FROM job_verifications WHERE job_id = ANY($1) \
              ORDER BY job_id, attempted_at DESC, id DESC) \
             UNION ALL \
             (SELECT DISTINCT ON (job_id) TRUE AS success_only, id, record \
              FROM job_verifications WHERE job_id = ANY($1) AND succeeded \
              ORDER BY job_id, attempted_at DESC, id DESC)",
        )
        .bind(jobs)
        .fetch_all(self.pool())
        .await
        .map_err(query_error("loading verifications"))?;
        rows.iter()
            .map(|row| {
                let success_only: bool = row
                    .try_get("success_only")
                    .map_err(|e| corrupt("<verification>", e))?;
                Ok((success_only, decode(row)?))
            })
            .collect()
    }
}

#[async_trait]
impl VerificationRepository for PgStore {
    async fn save_verification(&self, record: &VerificationRecord) -> Result<(), StorageError> {
        self.insert_verification(record, false).await.map(|_| ())
    }

    async fn verification_history(
        &self,
        job: JobId,
    ) -> Result<Vec<VerificationRecord>, StorageError> {
        self.verifications_where(job, false, None).await
    }

    async fn latest_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError> {
        Ok(self
            .verifications_where(job, false, Some(1))
            .await?
            .into_iter()
            .next())
    }

    async fn latest_successful_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError> {
        Ok(self
            .verifications_where(job, true, Some(1))
            .await?
            .into_iter()
            .next())
    }

    async fn record_observation(
        &self,
        posting: &JobPosting,
        observed_at: DateTime<Utc>,
    ) -> Result<UpsertOutcome, StorageError> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        lock_source(&mut tx, &posting.provenance.source).await?;
        let observation = Observation {
            source: &posting.provenance.source,
            observed_at,
            run: None,
        };
        let mut result = ScanResult::default();
        apply_listing(
            &mut tx,
            &observation,
            std::slice::from_ref(posting),
            false,
            &[],
            &mut result,
        )
        .await?;
        tx.commit()
            .await
            .map_err(query_error("committing an observation"))?;
        Ok(result
            .outcomes
            .first()
            .copied()
            .unwrap_or(UpsertOutcome::Unchanged))
    }
}
