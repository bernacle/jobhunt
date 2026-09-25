//! Scheduled work: discovery of the shared corpus and re-verification of
//! the jobs that matter.
//!
//! Both are single runs meant for a cron trigger (Railway cron services run
//! a command on a schedule, skip a run while the previous one is still
//! going, and need the process to exit). Both are safe to start twice at
//! once: they coordinate through Postgres leases (see
//! `jobhunt_storage::postgres::schedule`), never through process-local
//! state. Neither implements discovery or verification: they call the same
//! pipeline and service the local product uses, with the same lifecycle
//! (NEW / UPDATED / CLOSED / REOPENED with history), conditional requests
//! (ETags) and freshness policy.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use jobhunt_app::discover::{configured_sources, http_client, run_discovery};
use jobhunt_app::verify::http_verifier;
use jobhunt_core::ErrorChain;
use jobhunt_jobs::JobRepository;
use jobhunt_jobs::verification::{VerificationService, VerifyMode};
use jobhunt_sources::SourceSpec;
use jobhunt_storage::postgres::{
    PgStore, ScheduledSource, SourceOutcome, VerificationPick, WorkerKind,
};
use serde::Serialize;

use crate::config::CloudConfig;

/// A run's worker runs are closed as abandoned after this long.
const ABANDON_AFTER: Duration = Duration::from_secs(6 * 3600);

/// What a discovery run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DiscoverySummary {
    pub sources_due: usize,
    pub sources_read: usize,
    pub sources_failed: usize,
    pub new: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub closed: usize,
    pub reopened: usize,
    pub warnings: Vec<String>,
}

/// What a verification run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct VerificationSummary {
    pub candidates: usize,
    pub claimed: usize,
    pub verified: usize,
    pub reused: usize,
    pub failed: usize,
}

/// Reads every due source once (in batches of
/// [`CloudConfig::discovery_batch`]), until none is due or `budget` has
/// passed.
pub async fn discover(
    store: &PgStore,
    config: &CloudConfig,
    budget: Duration,
) -> Result<DiscoverySummary, jobhunt_app::AppError> {
    let owner = format!("discovery:{}", config.instance);
    let started = Instant::now();
    let now = Utc::now();
    let run = store
        .start_worker_run(WorkerKind::Discovery, &owner, ABANDON_AFTER, now)
        .await?;
    let result = discover_inner(store, config, &owner, started, budget).await;
    let (summary_json, error) = match &result {
        Ok(summary) => (serde_json::to_value(summary).unwrap_or_default(), None),
        Err(e) => (serde_json::json!({}), Some(e.public_message())),
    };
    if let Err(e) = store
        .finish_worker_run(run, &summary_json, error.as_deref(), Utc::now())
        .await
    {
        tracing::warn!(error = %e, "could not record the end of the run");
    }
    // Whatever happened, leave no lease behind for others to wait out.
    if let Err(e) = store.release_leases(&owner).await {
        tracing::warn!(error = %e, "could not release leases");
    }
    result
}

async fn discover_inner(
    store: &PgStore,
    config: &CloudConfig,
    owner: &str,
    started: Instant,
    budget: Duration,
) -> Result<DiscoverySummary, jobhunt_app::AppError> {
    let app_config = &config.app.config;
    let http = http_client(app_config)?;
    let mut summary = DiscoverySummary::default();
    // The configured corpus, careers pages resolved to their boards.
    let specs = configured_sources(app_config, &http, &mut summary.warnings).await?;
    let by_key: HashMap<String, SourceSpec> = specs
        .iter()
        .map(|s| (s.key().to_string(), s.clone()))
        .collect();
    let scheduled: Vec<ScheduledSource> = specs
        .iter()
        .map(|s| ScheduledSource {
            key: s.key().clone(),
            company: s.company().map(str::to_owned),
        })
        .collect();
    store.register_sources(&scheduled, Utc::now()).await?;
    store.refresh_tiers(&config.schedule, Utc::now()).await?;

    while started.elapsed() < budget {
        let claimed = store
            .claim_due_sources(
                owner,
                config.discovery_batch.max(1),
                config.schedule.lease,
                Utc::now(),
            )
            .await?;
        if claimed.is_empty() {
            break;
        }
        summary.sources_due += claimed.len();
        let batch: Vec<SourceSpec> = claimed
            .iter()
            .filter_map(|c| by_key.get(&c.key.to_string()).cloned())
            .collect();
        tracing::info!(sources = batch.len(), "discovering");
        let report = match run_discovery(store, app_config, &http, &batch).await {
            Ok(report) => report,
            Err(e) => {
                // Storage failed mid-run: give the sources back (with a
                // failure, so they back off) and stop.
                for c in &claimed {
                    let outcome = SourceOutcome::Failed {
                        error: e.public_message(),
                    };
                    let _ = store
                        .finish_source(owner, &c.key, &outcome, &config.schedule, Utc::now())
                        .await;
                }
                return Err(e);
            }
        };
        for source in &report.sources {
            let outcome = match &source.result {
                Ok(stats) => {
                    summary.sources_read += 1;
                    let c = &stats.counts;
                    summary.new += c.inserted;
                    summary.updated += c.updated;
                    summary.unchanged += c.unchanged;
                    summary.closed += c.closed;
                    summary.reopened += c.reopened;
                    SourceOutcome::Succeeded
                }
                Err(error) => {
                    summary.sources_failed += 1;
                    tracing::warn!(source = %source.source, error = %ErrorChain(error), "source failed");
                    SourceOutcome::Failed {
                        error: ErrorChain(error).to_string(),
                    }
                }
            };
            store
                .finish_source(
                    owner,
                    &source.source,
                    &outcome,
                    &config.schedule,
                    Utc::now(),
                )
                .await?;
        }
        // Sources claimed but no longer configured: release them.
        for c in claimed
            .iter()
            .filter(|c| !by_key.contains_key(&c.key.to_string()))
        {
            store
                .finish_source(
                    owner,
                    &c.key,
                    &SourceOutcome::Failed {
                        error: "no longer configured".into(),
                    },
                    &config.schedule,
                    Utc::now(),
                )
                .await?;
        }
    }
    tracing::info!(
        read = summary.sources_read,
        failed = summary.sources_failed,
        new = summary.new,
        updated = summary.updated,
        closed = summary.closed,
        "discovery finished"
    );
    Ok(summary)
}

/// Verifies up to [`CloudConfig::verification_batch`] jobs that matter
/// (see `jobhunt_storage::postgres::schedule`), reusing the product's
/// freshness policy: a job verified within its fresh window is not asked
/// again.
pub async fn verify(
    store: &PgStore,
    config: &CloudConfig,
) -> Result<VerificationSummary, jobhunt_app::AppError> {
    let owner = format!("verification:{}", config.instance);
    let now = Utc::now();
    let run = store
        .start_worker_run(WorkerKind::Verification, &owner, ABANDON_AFTER, now)
        .await?;
    let result = verify_inner(store, config, &owner).await;
    let (summary_json, error) = match &result {
        Ok(summary) => (serde_json::to_value(summary).unwrap_or_default(), None),
        Err(e) => (serde_json::json!({}), Some(e.public_message())),
    };
    if let Err(e) = store
        .finish_worker_run(run, &summary_json, error.as_deref(), Utc::now())
        .await
    {
        tracing::warn!(error = %e, "could not record the end of the run");
    }
    if let Err(e) = store.release_leases(&owner).await {
        tracing::warn!(error = %e, "could not release leases");
    }
    result
}

async fn verify_inner(
    store: &PgStore,
    config: &CloudConfig,
    owner: &str,
) -> Result<VerificationSummary, jobhunt_app::AppError> {
    let app_config = &config.app.config;
    let policy = app_config.verification.policy();
    let pick = VerificationPick {
        fresh_for: policy
            .fresh_for
            .to_std()
            .unwrap_or(Duration::from_secs(86_400)),
        recommended_within: Duration::from_secs(7 * 86_400),
        new_within: Duration::from_secs(2 * 86_400),
        limit: config.verification_batch.max(1),
    };
    let now = Utc::now();
    let candidates = store.verification_candidates(&pick, now).await?;
    let mut summary = VerificationSummary {
        candidates: candidates.len(),
        ..VerificationSummary::default()
    };
    if candidates.is_empty() {
        return Ok(summary);
    }
    let ids: Vec<_> = candidates.iter().map(|c| c.job).collect();
    let claimed = store
        .claim_verifications(owner, &ids, Duration::from_secs(15 * 60), now)
        .await?;
    summary.claimed = claimed.len();
    let mut records = Vec::with_capacity(claimed.len());
    for id in &claimed {
        if let Some(record) = store.get(*id).await? {
            records.push(record);
        }
    }
    let verifier = http_verifier(app_config)?;
    let service = VerificationService::new(store, &verifier)
        .with_policy(policy)
        .with_concurrency(app_config.verification.concurrency);
    let verified = service
        .verify(&records, VerifyMode::IfDue, Utc::now())
        .await
        .map_err(jobhunt_app::AppError::from)?;
    for v in &verified {
        if v.reused {
            summary.reused += 1;
        } else if v.latest.as_ref().is_some_and(|l| l.succeeded()) {
            summary.verified += 1;
        } else {
            summary.failed += 1;
        }
    }
    store.release_verifications(owner, &claimed).await?;
    tracing::info!(
        candidates = summary.candidates,
        verified = summary.verified,
        failed = summary.failed,
        "verification finished"
    );
    Ok(summary)
}
