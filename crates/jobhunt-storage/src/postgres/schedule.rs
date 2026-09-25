//! Scheduled work, coordinated through Postgres alone.
//!
//! * Discovery: every configured source has a row in `source_schedule`
//!   with when it is next due. A worker *claims* due sources by writing a
//!   lease on them in one statement (`FOR UPDATE SKIP LOCKED`), so two
//!   workers started at the same time (a cron run overlapping a manual
//!   one, a second replica) get disjoint sources, and a worker that dies
//!   leaves leases that simply expire. Finishing a source sets its next
//!   due time: its tier's interval after a success; after failures, an
//!   exponential backoff (1 h, 2 h, 4 h, … capped at 48 h).
//! * Verification: candidates are open jobs that matter to someone (in a
//!   pipeline, recently recommended, pushed by a person's sync, or newly
//!   discovered) whose last attempt is older than the freshness window.
//!   A worker claims jobs in `verification_leases` with one upsert that
//!   only takes free or expired leases.
//! * `worker_runs` records every execution (start, end, outcome, counts).

use std::time::Duration;

use chrono::{DateTime, Utc};
use jobhunt_core::SourceKey;
use jobhunt_jobs::{JobId, StorageError};
use serde::Serialize;
use sqlx::Row;

use super::{PgStore, corrupt, query_error};

/// How often sources are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSettings {
    /// Sources whose jobs people acted on or were recommended recently.
    pub active_every: Duration,
    /// Every other source.
    pub normal_every: Duration,
    /// A source that failed is tried again after this, doubled for every
    /// further consecutive failure…
    pub retry_after: Duration,
    /// …up to this.
    pub max_backoff: Duration,
    /// "Recently", for the active tier.
    pub active_window: Duration,
    /// How long a claim on a source lasts before another worker may take
    /// it over (longer than one source can take to read).
    pub lease: Duration,
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self {
            active_every: Duration::from_secs(3 * 3600),
            normal_every: Duration::from_secs(12 * 3600),
            retry_after: Duration::from_secs(3600),
            max_backoff: Duration::from_secs(48 * 3600),
            active_window: Duration::from_secs(14 * 24 * 3600),
            lease: Duration::from_secs(15 * 60),
        }
    }
}

fn chrono(d: Duration) -> chrono::Duration {
    chrono::Duration::from_std(d).unwrap_or(chrono::Duration::MAX)
}

/// A source's scheduling tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTier {
    Active,
    Normal,
}

impl SourceTier {
    fn parse(value: &str) -> Self {
        if value == "active" {
            Self::Active
        } else {
            Self::Normal
        }
    }
}

/// A source the cloud should discover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledSource {
    pub key: SourceKey,
    pub company: Option<String>,
}

/// A source a worker holds the lease of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSource {
    pub key: SourceKey,
    pub company: Option<String>,
    pub tier: SourceTier,
    pub consecutive_failures: u32,
}

/// How reading a source went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOutcome {
    Succeeded,
    Failed { error: String },
}

/// Kinds of scheduled work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    Discovery,
    Verification,
}

impl WorkerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Verification => "verification",
        }
    }
}

/// One recorded worker execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerRun {
    pub id: i64,
}

/// Why a job is worth verifying now, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason {
    /// Someone saved it, applied, is interviewing or has an offer.
    InPipeline,
    /// It was on someone's shortlist recently.
    Recommended,
    /// A person's sync brought it; cloud discovery has not seen it.
    Synced,
    /// Discovered recently.
    New,
}

/// A job the verification worker should look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationCandidate {
    pub job: JobId,
    pub reason: VerifyReason,
    pub last_attempt: Option<DateTime<Utc>>,
}

/// What to pick for verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPick {
    /// Attempts younger than this are left alone (the freshness policy's
    /// "fresh" window).
    pub fresh_for: Duration,
    /// "Recently recommended".
    pub recommended_within: Duration,
    /// "Recently discovered".
    pub new_within: Duration,
    pub limit: usize,
}

impl PgStore {
    /// Makes `source_schedule` match the configured sources: new ones are
    /// due now, removed ones are disabled (their jobs stay).
    pub async fn register_sources(
        &self,
        sources: &[ScheduledSource],
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let kinds: Vec<String> = sources.iter().map(|s| s.key.kind().to_owned()).collect();
        let instances: Vec<String> = sources
            .iter()
            .map(|s| s.key.instance().to_owned())
            .collect();
        let companies: Vec<Option<String>> = sources.iter().map(|s| s.company.clone()).collect();
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        sqlx::query(
            "INSERT INTO source_schedule (source_kind, source_instance, company, next_due_at) \
             SELECT k, i, c, $4 FROM unnest($1::text[], $2::text[], $3::text[]) AS s (k, i, c) \
             ON CONFLICT (source_kind, source_instance) DO UPDATE SET \
             company = excluded.company, enabled = true",
        )
        .bind(&kinds)
        .bind(&instances)
        .bind(&companies)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(query_error("registering sources"))?;
        sqlx::query(
            "UPDATE source_schedule SET enabled = false WHERE enabled AND \
             (source_kind, source_instance) NOT IN \
             (SELECT k, i FROM unnest($1::text[], $2::text[]) AS s (k, i))",
        )
        .bind(&kinds)
        .bind(&instances)
        .execute(&mut *tx)
        .await
        .map_err(query_error("registering sources"))?;
        tx.commit().await.map_err(query_error("committing sources"))
    }

    /// Recomputes tiers: a source is active when someone acted on, or was
    /// recommended, one of its jobs within the window. A source that just
    /// became active is due no later than one active interval after its
    /// last read.
    pub async fn refresh_tiers(
        &self,
        settings: &ScheduleSettings,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let since = now - chrono(settings.active_window);
        sqlx::query(
            "UPDATE source_schedule s SET tier = t.tier, next_due_at = CASE \
               WHEN t.tier = 'active' AND s.tier = 'normal' AND s.last_finished_at IS NOT NULL \
               THEN LEAST(s.next_due_at, s.last_finished_at + $2::interval) \
               ELSE s.next_due_at END \
             FROM (SELECT source_kind, source_instance, CASE WHEN \
                     EXISTS (SELECT 1 FROM jobs j JOIN feedback f ON f.job_id = j.id \
                             WHERE j.source_kind = ss.source_kind \
                             AND j.source_instance = ss.source_instance \
                             AND f.recorded_at > $1) \
                     OR EXISTS (SELECT 1 FROM jobs j JOIN user_opportunities uo \
                                ON uo.opportunity_id = j.opportunity_id \
                                WHERE j.source_kind = ss.source_kind \
                                AND j.source_instance = ss.source_instance \
                                AND uo.last_shown_at > $1) \
                   THEN 'active' ELSE 'normal' END AS tier \
                   FROM source_schedule ss) t \
             WHERE s.source_kind = t.source_kind AND s.source_instance = t.source_instance \
             AND s.tier <> t.tier",
        )
        .bind(since)
        .bind(format!("{} seconds", settings.active_every.as_secs()))
        .execute(self.pool())
        .await
        .map_err(query_error("recomputing source tiers"))?;
        Ok(())
    }

    /// Claims up to `limit` due sources for `owner` (active ones first,
    /// then the longest overdue).
    pub async fn claim_due_sources(
        &self,
        owner: &str,
        limit: usize,
        lease: Duration,
        now: DateTime<Utc>,
    ) -> Result<Vec<ClaimedSource>, StorageError> {
        let rows = sqlx::query(
            "UPDATE source_schedule s SET lease_owner = $1, lease_expires_at = $2, \
             last_started_at = $3 \
             FROM (SELECT source_kind, source_instance FROM source_schedule \
                   WHERE enabled AND next_due_at <= $3 \
                   AND (lease_expires_at IS NULL OR lease_expires_at < $3) \
                   ORDER BY tier = 'active' DESC, next_due_at, source_kind, source_instance \
                   LIMIT $4 FOR UPDATE SKIP LOCKED) due \
             WHERE s.source_kind = due.source_kind AND s.source_instance = due.source_instance \
             RETURNING s.source_kind, s.source_instance, s.company, s.tier, \
             s.consecutive_failures",
        )
        .bind(owner)
        .bind(now + chrono(lease))
        .bind(now)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(self.pool())
        .await
        .map_err(query_error("claiming due sources"))?;
        let mut out = rows
            .iter()
            .map(|r| {
                let bad = |e: sqlx::Error| corrupt("<source_schedule>", e);
                let kind: String = r.try_get("source_kind").map_err(bad)?;
                let instance: String = r.try_get("source_instance").map_err(bad)?;
                let tier: String = r.try_get("tier").map_err(bad)?;
                let failures: i32 = r.try_get("consecutive_failures").map_err(bad)?;
                Ok(ClaimedSource {
                    key: SourceKey::new(&kind, &instance)
                        .map_err(|e| corrupt(&format!("{kind}:{instance}"), e))?,
                    company: r.try_get("company").map_err(bad)?,
                    tier: SourceTier::parse(&tier),
                    consecutive_failures: u32::try_from(failures).unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        out.sort_by(|a, b| a.key.to_string().cmp(&b.key.to_string()));
        Ok(out)
    }

    /// Records how reading a claimed source went, sets when it is next due
    /// and releases the lease. A lease that expired and was taken over by
    /// another worker is left to that worker.
    pub async fn finish_source(
        &self,
        owner: &str,
        source: &SourceKey,
        outcome: &SourceOutcome,
        settings: &ScheduleSettings,
        now: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let (status, error) = match outcome {
            SourceOutcome::Succeeded => ("succeeded", None),
            SourceOutcome::Failed { error } => {
                ("failed", Some(error.chars().take(500).collect::<String>()))
            }
        };
        let changed = sqlx::query(
            "UPDATE source_schedule SET \
               consecutive_failures = CASE WHEN $4 = 'succeeded' THEN 0 \
                                           ELSE consecutive_failures + 1 END, \
               next_due_at = $3 + CASE WHEN $4 = 'succeeded' \
                 THEN (CASE WHEN tier = 'active' THEN $6::interval ELSE $7::interval END) \
                 ELSE LEAST($10::interval * power(2, LEAST(consecutive_failures, 10)), \
                            $8::interval) END, \
               last_finished_at = $3, last_status = $4, last_error = $5, \
               lease_owner = NULL, lease_expires_at = NULL \
             WHERE source_kind = $1 AND source_instance = $2 AND lease_owner = $9",
        )
        .bind(source.kind())
        .bind(source.instance())
        .bind(now)
        .bind(status)
        .bind(error)
        .bind(format!("{} seconds", settings.active_every.as_secs()))
        .bind(format!("{} seconds", settings.normal_every.as_secs()))
        .bind(format!("{} seconds", settings.max_backoff.as_secs()))
        .bind(owner)
        .bind(format!("{} seconds", settings.retry_after.as_secs()))
        .execute(self.pool())
        .await
        .map_err(query_error("recording a source's outcome"))?
        .rows_affected();
        Ok(changed > 0)
    }

    /// Releases every lease `owner` holds without changing schedules (a
    /// worker shutting down early).
    pub async fn release_leases(&self, owner: &str) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE source_schedule SET lease_owner = NULL, lease_expires_at = NULL \
             WHERE lease_owner = $1",
        )
        .bind(owner)
        .execute(self.pool())
        .await
        .map_err(query_error("releasing leases"))?;
        sqlx::query("DELETE FROM verification_leases WHERE owner = $1")
            .bind(owner)
            .execute(self.pool())
            .await
            .map_err(query_error("releasing leases"))?;
        Ok(())
    }

    /// Records the start of a worker execution. Executions of the same kind
    /// still marked running after `abandon_after` are marked failed first
    /// (their worker died).
    pub async fn start_worker_run(
        &self,
        kind: WorkerKind,
        worker: &str,
        abandon_after: Duration,
        now: DateTime<Utc>,
    ) -> Result<WorkerRun, StorageError> {
        sqlx::query(
            "UPDATE worker_runs SET status = 'failed', finished_at = $3, \
             error = 'abandoned: the worker stopped without finishing' \
             WHERE kind = $1 AND status = 'running' AND started_at < $2",
        )
        .bind(kind.as_str())
        .bind(now - chrono(abandon_after))
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(query_error("closing abandoned runs"))?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO worker_runs (kind, worker, started_at, status) \
             VALUES ($1, $2, $3, 'running') RETURNING id",
        )
        .bind(kind.as_str())
        .bind(worker)
        .bind(now)
        .fetch_one(self.pool())
        .await
        .map_err(query_error("recording a worker run"))?;
        Ok(WorkerRun { id })
    }

    /// Records the end of a worker execution.
    pub async fn finish_worker_run(
        &self,
        run: WorkerRun,
        summary: &serde_json::Value,
        error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE worker_runs SET finished_at = $2, status = $3, summary = $4, error = $5 \
             WHERE id = $1",
        )
        .bind(run.id)
        .bind(now)
        .bind(if error.is_some() {
            "failed"
        } else {
            "succeeded"
        })
        .bind(sqlx::types::Json(summary))
        .bind(error)
        .execute(self.pool())
        .await
        .map_err(query_error("finishing a worker run"))?;
        Ok(())
    }

    /// Open jobs worth verifying now, most important first (see the module
    /// docs).
    pub async fn verification_candidates(
        &self,
        pick: &VerificationPick,
        now: DateTime<Utc>,
    ) -> Result<Vec<VerificationCandidate>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, priority, last_attempt FROM ( \
               SELECT j.id, l.attempted_at AS last_attempt, CASE \
                 WHEN EXISTS (SELECT 1 FROM feedback f JOIN jobs fj ON fj.id = f.job_id \
                              WHERE fj.opportunity_id = j.opportunity_id \
                              AND f.action IN ('save', 'applied', 'interview', 'offer')) THEN 1 \
                 WHEN EXISTS (SELECT 1 FROM user_opportunities uo \
                              WHERE uo.opportunity_id = j.opportunity_id \
                              AND uo.last_shown_at > $2) THEN 2 \
                 WHEN j.origin = 'sync' THEN 3 \
                 WHEN j.first_seen_at > $3 THEN 4 \
                 ELSE NULL END AS priority \
               FROM jobs j \
               LEFT JOIN LATERAL (SELECT attempted_at FROM job_verifications v \
                                  WHERE v.job_id = j.id \
                                  ORDER BY attempted_at DESC, id DESC LIMIT 1) l ON TRUE \
               WHERE j.status = 'open' AND (l.attempted_at IS NULL OR l.attempted_at < $1) \
             ) c WHERE priority IS NOT NULL \
             ORDER BY priority, last_attempt NULLS FIRST, id LIMIT $4",
        )
        .bind(now - chrono(pick.fresh_for))
        .bind(now - chrono(pick.recommended_within))
        .bind(now - chrono(pick.new_within))
        .bind(i64::try_from(pick.limit).unwrap_or(i64::MAX))
        .fetch_all(self.pool())
        .await
        .map_err(query_error("choosing jobs to verify"))?;
        rows.iter()
            .map(|r| {
                let id: String = r.try_get("id").map_err(|e| corrupt("<jobs>", e))?;
                let priority: i32 = r.try_get("priority").map_err(|e| corrupt(&id, e))?;
                Ok(VerificationCandidate {
                    job: id.parse().map_err(|e| corrupt(&id, e))?,
                    reason: match priority {
                        1 => VerifyReason::InPipeline,
                        2 => VerifyReason::Recommended,
                        3 => VerifyReason::Synced,
                        _ => VerifyReason::New,
                    },
                    last_attempt: r.try_get("last_attempt").map_err(|e| corrupt(&id, e))?,
                })
            })
            .collect()
    }

    /// Claims the jobs no other worker holds; returns the ones claimed.
    pub async fn claim_verifications(
        &self,
        owner: &str,
        jobs: &[JobId],
        lease: Duration,
        now: DateTime<Utc>,
    ) -> Result<Vec<JobId>, StorageError> {
        let ids: Vec<String> = jobs.iter().map(ToString::to_string).collect();
        let claimed: Vec<String> = sqlx::query_scalar(
            "INSERT INTO verification_leases (job_id, owner, expires_at) \
             SELECT j, $2, $3 FROM unnest($1::text[]) AS j \
             ON CONFLICT (job_id) DO UPDATE SET owner = excluded.owner, \
             expires_at = excluded.expires_at WHERE verification_leases.expires_at < $4 \
             RETURNING job_id",
        )
        .bind(&ids)
        .bind(owner)
        .bind(now + chrono(lease))
        .bind(now)
        .fetch_all(self.pool())
        .await
        .map_err(query_error("claiming jobs to verify"))?;
        let mut out: Vec<JobId> = claimed
            .iter()
            .map(|id| id.parse().map_err(|e| corrupt(id, e)))
            .collect::<Result<_, _>>()?;
        out.sort();
        Ok(out)
    }

    /// Releases verification claims.
    pub async fn release_verifications(
        &self,
        owner: &str,
        jobs: &[JobId],
    ) -> Result<(), StorageError> {
        let ids: Vec<String> = jobs.iter().map(ToString::to_string).collect();
        sqlx::query("DELETE FROM verification_leases WHERE owner = $1 AND job_id = ANY($2)")
            .bind(owner)
            .bind(&ids)
            .execute(self.pool())
            .await
            .map_err(query_error("releasing verification claims"))?;
        Ok(())
    }

    /// The schedule, for operations (`jobhunt admin status`).
    pub async fn schedule_overview(&self) -> Result<Vec<ScheduleRow>, StorageError> {
        sqlx::query(
            "SELECT source_kind, source_instance, tier, enabled, next_due_at, \
             consecutive_failures, last_finished_at, last_status, lease_owner \
             FROM source_schedule ORDER BY next_due_at, source_kind, source_instance",
        )
        .fetch_all(self.pool())
        .await
        .map_err(query_error("loading the schedule"))?
        .iter()
        .map(|r| {
            let bad = |e: sqlx::Error| corrupt("<source_schedule>", e);
            let kind: String = r.try_get("source_kind").map_err(bad)?;
            let instance: String = r.try_get("source_instance").map_err(bad)?;
            let tier: String = r.try_get("tier").map_err(bad)?;
            let failures: i32 = r.try_get("consecutive_failures").map_err(bad)?;
            Ok(ScheduleRow {
                source: format!("{kind}:{instance}"),
                tier: SourceTier::parse(&tier),
                enabled: r.try_get("enabled").map_err(bad)?,
                next_due_at: r.try_get("next_due_at").map_err(bad)?,
                consecutive_failures: u32::try_from(failures).unwrap_or(0),
                last_finished_at: r.try_get("last_finished_at").map_err(bad)?,
                last_status: r.try_get("last_status").map_err(bad)?,
                leased_by: r.try_get("lease_owner").map_err(bad)?,
            })
        })
        .collect()
    }
}

/// One source's schedule, for operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScheduleRow {
    pub source: String,
    pub tier: SourceTier,
    pub enabled: bool,
    pub next_due_at: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub last_finished_at: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
    pub leased_by: Option<String>,
}
