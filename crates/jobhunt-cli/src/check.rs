//! `jobhunt check`: the full verification and eligibility report of one
//! job, with the evidence behind every reason, from what is stored (no
//! network unless `--refresh`).

use std::process::ExitCode;

use anyhow::Context;
use chrono::Utc;
use jobhunt_jobs::verification::{VerifyMode, cached};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::show::load_records;
use crate::verify;

#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// A job id (job_…, printed by `find`) or an opportunity id (opp_…).
    #[arg(value_name = "ID")]
    pub id: String,

    /// Verify the job's sources first (`jobhunt verify --force`), so the
    /// answer rests on what the company publishes right now.
    #[arg(long)]
    pub refresh: bool,
}

pub async fn run(args: CheckArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let store = SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")?;
    let result = execute(&args, loaded, &store).await;
    store.close().await;
    result
}

async fn execute(
    args: &CheckArgs,
    loaded: &LoadedConfig,
    store: &SqliteJobStore,
) -> anyhow::Result<ExitCode> {
    let records = load_records(store, &args.id).await?;
    let now = Utc::now();
    let verified = if args.refresh {
        verify::verify(loaded, store, &records, VerifyMode::Force, now).await?
    } else {
        cached(store, &records).await?
    };
    verify::report(loaded, store, &verified, now, true).await
}
