//! [`FeedbackRepository`] and [`RankingRepository`] for the SQLite store
//! (tables from the `feedback_ranking` migration).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_jobs::{JobId, StorageError};
use jobhunt_ranking::{
    FeedbackAction, FeedbackEvent, FeedbackRepository, FitReview, RankKey, Ranking,
    RankingRepository,
};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};

use crate::sqlite::{SqliteJobStore, decode_timestamp, encode_timestamp};

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

const FEEDBACK_COLUMNS: &str =
    "id, profile_id, opportunity_id, job_id, action, reason, title, company, recorded_at";

fn decode(row: &sqlx::sqlite::SqliteRow) -> Result<FeedbackEvent, StorageError> {
    let id: String = row.try_get("id").map_err(|e| corrupt("<feedback>", e))?;
    let text = |column: &str| -> Result<String, StorageError> {
        row.try_get(column).map_err(|e| corrupt(&id, e))
    };
    let action = text("action")?;
    Ok(FeedbackEvent {
        id: id.parse().map_err(|e| corrupt(&id, format!("id: {e}")))?,
        profile_id: text("profile_id")?,
        opportunity: text("opportunity_id")?
            .parse()
            .map_err(|e| corrupt(&id, format!("opportunity_id: {e}")))?,
        job: text("job_id")?
            .parse()
            .map_err(|e| corrupt(&id, format!("job_id: {e}")))?,
        action: FeedbackAction::from_canonical(&action)
            .ok_or_else(|| corrupt(&id, format!("unknown action {action:?}")))?,
        reason: row.try_get("reason").map_err(|e| corrupt(&id, e))?,
        title: text("title")?,
        company: text("company")?,
        at: decode_timestamp(&text("recorded_at")?)
            .map_err(|e| corrupt(&id, format!("recorded_at: {e}")))?,
    })
}

/// Inserts one feedback event. With `if_absent`, an event whose id is
/// already stored is left alone and `false` is returned.
pub(crate) async fn insert_feedback(
    conn: &mut SqliteConnection,
    event: &FeedbackEvent,
    if_absent: bool,
) -> Result<bool, StorageError> {
    let verb = if if_absent {
        "INSERT OR IGNORE"
    } else {
        "INSERT"
    };
    let result = sqlx::query(&format!(
        "{verb} INTO opportunity_feedback ({FEEDBACK_COLUMNS}) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    ))
    .bind(event.id.to_string())
    .bind(&event.profile_id)
    .bind(event.opportunity.to_string())
    .bind(event.job.to_string())
    .bind(event.action.as_str())
    .bind(event.reason.as_deref())
    .bind(&event.title)
    .bind(&event.company)
    .bind(encode_timestamp(event.at))
    .execute(&mut *conn)
    .await
    .map_err(query_error("saving feedback"))?;
    Ok(result.rows_affected() > 0)
}

#[async_trait]
impl FeedbackRepository for SqliteJobStore {
    async fn record_feedback(&self, event: &FeedbackEvent) -> Result<(), StorageError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(query_error("saving feedback"))?;
        insert_feedback(&mut conn, event, false).await?;
        Ok(())
    }

    async fn feedback(&self, profile_id: &str) -> Result<Vec<FeedbackEvent>, StorageError> {
        let rows = sqlx::query(&format!(
            "SELECT {FEEDBACK_COLUMNS} FROM opportunity_feedback WHERE profile_id = ? \
             ORDER BY recorded_at, id"
        ))
        .bind(profile_id)
        .fetch_all(&self.pool)
        .await
        .map_err(query_error("loading feedback"))?;
        rows.iter().map(decode).collect()
    }

    async fn feedback_for_jobs(
        &self,
        profile_id: &str,
        jobs: &[JobId],
    ) -> Result<Vec<FeedbackEvent>, StorageError> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let mut query: QueryBuilder<'_, Sqlite> = QueryBuilder::new(format!(
            "SELECT {FEEDBACK_COLUMNS} FROM opportunity_feedback WHERE profile_id = "
        ));
        query.push_bind(profile_id).push(" AND job_id IN (");
        let mut ids = query.separated(", ");
        for job in jobs {
            ids.push_bind(job.to_string());
        }
        query.push(") ORDER BY recorded_at, id");
        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(query_error("loading an opportunity's feedback"))?;
        rows.iter().map(decode).collect()
    }
}

#[async_trait]
impl RankingRepository for SqliteJobStore {
    async fn cached_ranking(&self, key: &RankKey) -> Result<Option<Ranking>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT ranking FROM opportunity_rankings \
             WHERE opportunity_id = ? AND profile_id = ? AND rank_key = ?",
        )
        .bind(key.opportunity.to_string())
        .bind(&key.profile_id)
        .bind(&key.key)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_error("loading a ranking"))?;
        row.map(|json| {
            serde_json::from_str(&json).map_err(|e| corrupt(&key.key, format!("ranking: {e}")))
        })
        .transpose()
    }

    async fn store_ranking(
        &self,
        key: &RankKey,
        ranking: &Ranking,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(ranking).map_err(|e| StorageError::Query {
            operation: "encoding a ranking",
            source: Box::new(e),
        })?;
        sqlx::query(
            "INSERT INTO opportunity_rankings (opportunity_id, profile_id, rank_key, \
             ranking_version, tier, ranked_at, ranking) VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (opportunity_id, profile_id, rank_key) DO UPDATE SET \
             tier = excluded.tier, ranked_at = excluded.ranked_at, ranking = excluded.ranking",
        )
        .bind(key.opportunity.to_string())
        .bind(&key.profile_id)
        .bind(&key.key)
        .bind(&ranking.ranking_version)
        .bind(ranking.tier.as_str())
        .bind(encode_timestamp(at))
        .bind(json)
        .execute(&self.pool)
        .await
        .map_err(query_error("storing a ranking"))?;
        Ok(())
    }

    async fn cached_fit_review(
        &self,
        profile_id: &str,
        key: &str,
    ) -> Result<Option<FitReview>, StorageError> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT review FROM fit_reviews WHERE profile_id = ? AND review_key = ?",
        )
        .bind(profile_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(query_error("loading a fit review"))?;
        row.map(|json| {
            serde_json::from_str(&json).map_err(|e| corrupt(key, format!("fit review: {e}")))
        })
        .transpose()
    }

    async fn store_fit_review(
        &self,
        profile_id: &str,
        key: &str,
        review: &FitReview,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(review).map_err(|e| StorageError::Query {
            operation: "encoding a fit review",
            source: Box::new(e),
        })?;
        sqlx::query(
            "INSERT INTO fit_reviews (profile_id, review_key, reviewer, fit, reviewed_at, review) \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (profile_id, review_key) DO UPDATE SET \
             reviewer = excluded.reviewer, fit = excluded.fit, \
             reviewed_at = excluded.reviewed_at, review = excluded.review",
        )
        .bind(profile_id)
        .bind(key)
        .bind(&review.reviewer)
        .bind(review.fit.as_str())
        .bind(encode_timestamp(at))
        .bind(json)
        .execute(&self.pool)
        .await
        .map_err(query_error("storing a fit review"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
