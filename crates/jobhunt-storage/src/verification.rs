//! [`VerificationRepository`] and [`EligibilityRepository`] for the SQLite
//! store (tables from the `verification_eligibility` migration).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::UpsertOutcome;
use jobhunt_eligibility::{CacheKey, EligibilityDecision, EligibilityRepository};
use jobhunt_jobs::verification::{VerificationRecord, VerificationRepository};
use jobhunt_jobs::{JobId, JobPosting, ScanResult, StorageError};
use sqlx::Row;

use crate::sqlite::{Observation, SqliteJobStore, apply_listing, encode_timestamp};

fn query_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |source| StorageError::Query {
        operation,
        source: Box::new(source),
    }
}

fn corrupt(id: &str, detail: impl ToString) -> StorageError {
    StorageError::Corrupt {
        id: id.to_owned(),
        detail: detail.to_string(),
    }
}

fn decode(row: &sqlx::sqlite::SqliteRow) -> Result<VerificationRecord, StorageError> {
    let id: String = row
        .try_get("id")
        .map_err(|e| corrupt("<verification>", e))?;
    let json: String = row.try_get("record").map_err(|e| corrupt(&id, e))?;
    serde_json::from_str(&json).map_err(|e| corrupt(&id, format!("verification record: {e}")))
}

impl SqliteJobStore {
    /// Inserts one attempt. With `if_absent`, an attempt whose id is
    /// already stored is left alone and `false` is returned.
    pub(crate) async fn insert_verification(
        &self,
        record: &VerificationRecord,
        if_absent: bool,
    ) -> Result<bool, StorageError> {
        let json = serde_json::to_string(record).map_err(|e| StorageError::Query {
            operation: "encoding a verification",
            source: Box::new(e),
        })?;
        let verb = if if_absent {
            "INSERT OR IGNORE"
        } else {
            "INSERT"
        };
        let result = sqlx::query(&format!(
            "{verb} INTO job_verifications (id, job_id, opportunity_id, source_kind, \
             source_instance, attempted_at, succeeded, method, listing_status, \
             application_status, authority, checked_url, content_fingerprint, failure_kind, \
             revision, record) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(record.id.to_string())
        .bind(record.job_id.to_string())
        .bind(record.opportunity_id.to_string())
        .bind(record.source.kind())
        .bind(record.source.instance())
        .bind(encode_timestamp(record.attempted_at))
        .bind(record.succeeded())
        .bind(record.method.as_str())
        .bind(record.listing.as_str())
        .bind(record.application.status.as_str())
        .bind(record.authority.as_str())
        .bind(record.checked_url.as_deref())
        .bind(record.content_fingerprint.as_deref())
        .bind(record.failure.as_ref().map(|f| f.kind.as_str()))
        .bind(&record.revision)
        .bind(json)
        .execute(&self.pool)
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
            "SELECT id, record FROM job_verifications WHERE job_id = ?{} \
             ORDER BY attempted_at DESC, id DESC{}",
            if successes_only {
                " AND succeeded = 1"
            } else {
                ""
            },
            if limit.is_some() { " LIMIT ?" } else { "" }
        );
        let mut query = sqlx::query(&sql).bind(job.to_string());
        if let Some(limit) = limit {
            query = query.bind(limit);
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(query_error("loading verifications"))?;
        rows.iter().map(decode).collect()
    }
}

#[async_trait]
impl VerificationRepository for SqliteJobStore {
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
            .begin_write()
            .await
            .map_err(query_error("starting a transaction"))?;
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

#[async_trait]
impl EligibilityRepository for SqliteJobStore {
    async fn cached_decision(
        &self,
        key: &CacheKey,
    ) -> Result<Option<EligibilityDecision>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT decision FROM eligibility_decisions \
             WHERE opportunity_id = ? AND profile_id = ? AND cache_key = ?",
        )
        .bind(key.opportunity.to_string())
        .bind(&key.profile_id)
        .bind(&key.key)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_error("loading an eligibility decision"))?;
        row.map(|json| {
            serde_json::from_str(&json)
                .map_err(|e| corrupt(&key.key, format!("eligibility decision: {e}")))
        })
        .transpose()
    }

    async fn store_decision(
        &self,
        key: &CacheKey,
        decision: &EligibilityDecision,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(decision).map_err(|e| StorageError::Query {
            operation: "encoding an eligibility decision",
            source: Box::new(e),
        })?;
        sqlx::query(
            "INSERT INTO eligibility_decisions (opportunity_id, profile_id, cache_key, \
             profile_revision, rules_version, status, decided_at, decision) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (opportunity_id, profile_id, cache_key) DO UPDATE SET \
             status = excluded.status, decided_at = excluded.decided_at, \
             decision = excluded.decision",
        )
        .bind(key.opportunity.to_string())
        .bind(&key.profile_id)
        .bind(&key.key)
        .bind(i64::try_from(key.profile_revision).unwrap_or(i64::MAX))
        .bind(&decision.rules_version)
        .bind(decision.status.as_str())
        .bind(encode_timestamp(at))
        .bind(json)
        .execute(&self.pool)
        .await
        .map_err(query_error("storing an eligibility decision"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
