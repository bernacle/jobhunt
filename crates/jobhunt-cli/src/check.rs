//! `jobhunt check`: the full verification and eligibility report of one
//! opportunity, with the evidence behind every reason, from what is stored
//! (no network unless `--refresh`).

use std::process::ExitCode;

use jobhunt_jobs::verification::VerifyMode;

use crate::config::LoadedConfig;
use crate::local::{StderrProgress, with_app};
use crate::verify;

#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// An opportunity id (opp_…, as `find` prints it) or a job id (job_…);
    /// a unique prefix works too.
    #[arg(value_name = "ID")]
    pub id: String,

    /// Verify the job's sources first (`jobhunt verify --force`), so the
    /// answer rests on what the company publishes right now.
    #[arg(long)]
    pub refresh: bool,
}

pub async fn run(args: CheckArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let opportunity = app.resolve(&args.id).await?;
        let now = jobhunt_app::now();
        let checked = if args.refresh {
            app.verify(
                &opportunity,
                VerifyMode::Force,
                &StderrProgress::each(),
                now,
            )
            .await?
        } else {
            app.check(&opportunity, now).await?
        };
        verify::report(&checked, true)
    })
}
