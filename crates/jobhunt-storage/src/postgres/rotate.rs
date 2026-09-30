//! Key rotation: re-seals every private value that is not under the
//! active key (`narrow admin reencrypt`). Safe to run while the service
//! is up and to run again: each row is rewritten in its own statement,
//! only when it still holds the ciphertext that was read.

use jobhunt_jobs::StorageError;
use sqlx::Row;

use super::crypto::Keyring;
use super::{PgStore, corrupt, query_error};

/// What a rotation pass changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reencrypted {
    pub examined: u64,
    pub rewritten: u64,
}

/// A table holding sealed values: how to read each row's context and how
/// to write it back.
struct Sealed {
    table: &'static str,
    column: &'static str,
    /// SQL building the context from the row (must match what writers use).
    context: &'static str,
    /// Columns identifying the row.
    key: &'static [&'static str],
}

const TABLES: [Sealed; 8] = [
    Sealed {
        table: "profile_entities",
        column: "body",
        context: "'profile_entities|' || user_id || '|' || profile_id || '|' || kind || '|' || entity_id",
        key: &["user_id", "profile_id", "kind", "entity_id"],
    },
    Sealed {
        table: "profile_events",
        column: "detail",
        context: "'profile_events|' || user_id || '|' || profile_id",
        key: &["id"],
    },
    Sealed {
        table: "feedback",
        column: "reason",
        context: "'feedback|' || user_id || '|' || id",
        key: &["user_id", "id"],
    },
    Sealed {
        table: "eligibility_decisions",
        column: "decision",
        context: "'eligibility|' || user_id || '|' || opportunity_id || '|' || cache_key",
        key: &["user_id", "opportunity_id", "profile_id", "cache_key"],
    },
    Sealed {
        table: "rankings",
        column: "ranking",
        context: "'rankings|' || user_id || '|' || opportunity_id || '|' || rank_key",
        key: &["user_id", "opportunity_id", "profile_id", "rank_key"],
    },
    Sealed {
        table: "fit_reviews",
        column: "review",
        context: "'fit_reviews|' || user_id || '|' || profile_id || '|' || review_key",
        key: &["user_id", "profile_id", "review_key"],
    },
    Sealed {
        table: "notification_settings",
        column: "email",
        context: "'notification_settings|' || user_id",
        key: &["user_id"],
    },
    Sealed {
        table: "notification_deliveries",
        column: "message",
        context: "'notification_deliveries|' || user_id || '|' || id",
        key: &["id"],
    },
];

impl PgStore {
    /// Re-seals values under old keys with the active key.
    pub async fn reencrypt(&self) -> Result<Reencrypted, StorageError> {
        let keys: &Keyring = self.keys();
        let mut out = Reencrypted::default();
        for t in &TABLES {
            let key_cols = t.key.join(", ");
            let rows = sqlx::query(&format!(
                "SELECT {key_cols}, {ctx} AS ctx, {col} AS sealed FROM {table} \
                 WHERE {col} IS NOT NULL",
                ctx = t.context,
                col = t.column,
                table = t.table
            ))
            .fetch_all(self.pool())
            .await
            .map_err(query_error("reading sealed values"))?;
            for row in rows {
                out.examined += 1;
                let sealed: Vec<u8> = row.try_get("sealed").map_err(|e| corrupt(t.table, e))?;
                if keys.is_current(&sealed) {
                    continue;
                }
                let context: String = row.try_get("ctx").map_err(|e| corrupt(t.table, e))?;
                let plain = keys
                    .open(&context, &sealed)
                    .map_err(|e| corrupt(&context, e))?;
                let resealed = keys
                    .seal(&context, &plain)
                    .map_err(|e| corrupt(&context, e))?;
                let conditions: Vec<String> = t
                    .key
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{c}::text = ${}", i + 3))
                    .collect();
                let sql = format!(
                    "UPDATE {table} SET {col} = $1 WHERE {col} = $2 AND {cond}",
                    table = t.table,
                    col = t.column,
                    cond = conditions.join(" AND ")
                );
                let mut q = sqlx::query(&sql).bind(resealed).bind(&sealed);
                for c in t.key {
                    let v: String = if *c == "id" && t.table == "profile_events" {
                        row.try_get::<i64, _>(*c)
                            .map_err(|e| corrupt(t.table, e))?
                            .to_string()
                    } else {
                        row.try_get(*c).map_err(|e| corrupt(t.table, e))?
                    };
                    q = q.bind(v);
                }
                let changed = q
                    .execute(self.pool())
                    .await
                    .map_err(query_error("re-sealing a value"))?
                    .rows_affected();
                out.rewritten += changed;
            }
        }
        Ok(out)
    }
}
