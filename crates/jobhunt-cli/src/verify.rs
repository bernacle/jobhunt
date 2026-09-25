//! `jobhunt verify`: ask a job's authoritative sources whether it is still
//! open, record what they say, and check it against your profile.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Context;
use chrono::{DateTime, Utc};
use jobhunt_eligibility::evaluate::trust;
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Assessment, cached_assess, requirements};
use jobhunt_jobs::verification::{
    FreshnessPolicy, OpportunityTrust, RecordVerification, VerificationService, VerifyMode,
};
use jobhunt_sources::{HttpClient, HttpVerifier, VerifierHosts};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::eligibility;
use crate::render::{DIM, TITLE, plural};
use crate::show::load_records;

/// Sends every verification request to this base URL instead of the real
/// hosts. For offline tests against a local server; not a user setting.
const ENDPOINT_OVERRIDE: &str = "JOBHUNT_VERIFY_ENDPOINT";

#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// A job id (job_…, printed by `find`) or an opportunity id (opp_…).
    #[arg(value_name = "ID")]
    pub id: String,

    /// Ask the sources even if a recent verification exists.
    #[arg(long)]
    pub force: bool,

    /// Show provenance: every source record, the URLs checked, the
    /// authority chain, and the evidence behind every reason.
    #[arg(long, short = 'd')]
    pub details: bool,
}

pub async fn run(args: VerifyArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let store = SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")?;
    let result = execute(&args, loaded, &store).await;
    store.close().await;
    result
}

async fn execute(
    args: &VerifyArgs,
    loaded: &LoadedConfig,
    store: &SqliteJobStore,
) -> anyhow::Result<ExitCode> {
    let records = load_records(store, &args.id).await?;
    let now = Utc::now();
    let mode = if args.force {
        VerifyMode::Force
    } else {
        VerifyMode::IfDue
    };
    let verified = verify(loaded, store, &records, mode, now).await?;
    let reused = verified.iter().filter(|v| v.reused).count();
    if reused > 0 && !args.force {
        eprintln!(
            "Reused {} from the last {} minutes (--force asks again).",
            plural(reused as u64, "recent verification", "recent verifications"),
            loaded.config.verification.reuse_minutes
        );
    }
    report(loaded, store, &verified, now, args.details).await
}

/// Verifies records against their sources.
pub async fn verify(
    loaded: &LoadedConfig,
    store: &SqliteJobStore,
    records: &[jobhunt_jobs::JobRecord],
    mode: VerifyMode,
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<RecordVerification>> {
    let config = &loaded.config;
    let http = HttpClient::new(config.discovery.http_settings())?;
    let verifier = match std::env::var(ENDPOINT_OVERRIDE) {
        Ok(base) if !base.trim().is_empty() => {
            HttpVerifier::with_hosts(http, VerifierHosts::all_at(&base))
        }
        _ => HttpVerifier::new(http),
    };
    let fetching = records.len();
    {
        eprintln!(
            "Verifying {} at {}…",
            plural(fetching as u64, "source record", "source records"),
            plural(
                records
                    .iter()
                    .map(|r| r.posting.provenance.source.to_string())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len() as u64,
                "source",
                "sources"
            )
        );
    }
    VerificationService::new(store, &verifier)
        .with_policy(config.verification.policy())
        .with_concurrency(config.verification.concurrency)
        .verify(records, mode, now)
        .await
        .context("could not save the verification")
}

/// The assessment of verified records against the profile (reusing a
/// stored decision when nothing it depends on changed).
pub async fn assessment(
    store: &SqliteJobStore,
    verified: &[RecordVerification],
    policy: &FreshnessPolicy,
    now: DateTime<Utc>,
) -> anyhow::Result<(Option<ProfileFacts>, Option<Assessment>, OpportunityTrust)> {
    let profile = eligibility::profile_facts(store).await?;
    match &profile {
        Some(p) => {
            let (a, _) = cached_assess(store, verified, p, policy, now)
                .await
                .context("could not store the eligibility decision")?;
            let trust = a.trust.clone();
            Ok((profile, Some(a), trust))
        }
        None => Ok((None, None, trust(verified, policy, now))),
    }
}

/// Prints the full report.
pub async fn report(
    loaded: &LoadedConfig,
    store: &SqliteJobStore,
    verified: &[RecordVerification],
    now: DateTime<Utc>,
    detail: bool,
) -> anyhow::Result<ExitCode> {
    let policy = loaded.config.verification.policy();
    let (profile, assessment, trust) = assessment(store, verified, &policy, now).await?;
    let mut out = anstream::stdout().lock();
    match write(
        &mut out,
        verified,
        &trust,
        profile.as_ref(),
        assessment.as_ref(),
        now,
        detail,
    ) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write the verification")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn write(
    out: &mut impl Write,
    verified: &[RecordVerification],
    trust: &OpportunityTrust,
    profile: Option<&ProfileFacts>,
    assessment: Option<&Assessment>,
    now: DateTime<Utc>,
    detail: bool,
) -> io::Result<()> {
    let main = trust
        .best
        .and_then(|i| verified.get(i))
        .or_else(|| verified.first())
        .map(|v| &v.record);
    let Some(main) = main else {
        return writeln!(out, "No source records.");
    };
    let job = requirements(main);
    writeln!(
        out,
        "{TITLE}{} — {}{TITLE:#}",
        main.posting.title, main.posting.company
    )?;
    writeln!(out, "{DIM}{} · {}{DIM:#}", main.id, main.opportunity_id)?;
    writeln!(out)?;
    eligibility::section(out, "Verification")?;
    eligibility::verification(out, trust, now, "  ", detail)?;
    writeln!(out)?;
    eligibility::section(out, "Location")?;
    eligibility::location(out, &job, profile, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Employment")?;
    eligibility::employment(out, &job, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Compensation")?;
    eligibility::compensation(out, trust, main, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Eligibility")?;
    match assessment {
        Some(a) => eligibility::eligibility(out, a, "  ", detail)?,
        None => writeln!(
            out,
            "  No career profile yet, so there is nothing to check the job against.\n  \
             Run `jobhunt init <resume>` or `jobhunt preferences set location <place>`."
        )?,
    }
    if !detail {
        writeln!(out)?;
        writeln!(
            out,
            "{DIM}Evidence and provenance: jobhunt verify --details {}{DIM:#}",
            main.id
        )?;
    }
    writeln!(
        out,
        "{DIM}A compatibility signal from the posting and your profile, not legal advice about work authorization.{DIM:#}"
    )?;
    out.flush()
}
