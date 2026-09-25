//! Privacy-conscious usage events, kept apart from product data.
//!
//! An event is a name (`sync`, `find`, `mcp_tool`, …), a time, the internal
//! account id, and small metadata the caller chose to be safe: counts,
//! durations, tool names, outcome codes. Never text the person wrote, never
//! profile content, never tokens. Usage rows are deleted with the account.

use chrono::{DateTime, Utc};
use jobhunt_jobs::StorageError;
use sqlx::types::Json;

use super::accounts::UserId;
use super::{PgStore, corrupt, query_error};

/// One usage event.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageEvent {
    pub user: Option<UserId>,
    pub at: DateTime<Utc>,
    pub event: String,
    pub metadata: serde_json::Value,
}

impl PgStore {
    /// Stores events (a batch from the server's usage writer).
    pub async fn record_usage(&self, events: &[UsageEvent]) -> Result<(), StorageError> {
        if events.is_empty() {
            return Ok(());
        }
        let users: Vec<Option<String>> = events
            .iter()
            .map(|e| e.user.as_ref().map(|u| u.as_str().to_owned()))
            .collect();
        let ats: Vec<DateTime<Utc>> = events.iter().map(|e| e.at).collect();
        let names: Vec<String> = events.iter().map(|e| e.event.clone()).collect();
        let metadata: Vec<Json<&serde_json::Value>> =
            events.iter().map(|e| Json(&e.metadata)).collect();
        // Events of accounts deleted meanwhile are dropped by the join.
        sqlx::query(
            "INSERT INTO usage_events (user_id, at, event, metadata) \
             SELECT e.u, e.a, e.n, e.m FROM unnest($1::text[], $2::timestamptz[], $3::text[], \
             $4::jsonb[]) AS e (u, a, n, m) \
             WHERE e.u IS NULL OR EXISTS (SELECT 1 FROM users WHERE id = e.u)",
        )
        .bind(&users)
        .bind(&ats)
        .bind(&names)
        .bind(&metadata)
        .execute(self.pool())
        .await
        .map_err(query_error("recording usage"))?;
        Ok(())
    }

    /// Event counts since `since`, for operations.
    pub async fn usage_summary(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<(String, u64, u64)>, StorageError> {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT event, COUNT(*), COUNT(DISTINCT user_id) FROM usage_events \
             WHERE at >= $1 GROUP BY event ORDER BY event",
        )
        .bind(since)
        .fetch_all(self.pool())
        .await
        .map_err(query_error("summarizing usage"))?;
        rows.into_iter()
            .map(|(event, n, users)| {
                Ok((
                    event,
                    u64::try_from(n).map_err(|e| corrupt("<usage>", e))?,
                    u64::try_from(users).map_err(|e| corrupt("<usage>", e))?,
                ))
            })
            .collect()
    }
}
