//! `jobhunt context <id>`: the evidence-backed application context the MCP
//! `prepare_application_context` tool returns, as JSON.

use std::process::ExitCode;

use crate::config::LoadedConfig;
use crate::local::{print_json, with_app};

#[derive(Debug, clap::Args)]
pub struct ContextArgs {
    /// An opportunity id (opp_…, as `find` prints it) or a job id (job_…);
    /// a unique prefix works too.
    #[arg(value_name = "ID")]
    pub id: String,

    /// Include your name and contact details.
    #[arg(long)]
    pub contact: bool,
}

pub async fn run(args: ContextArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let opportunity = app.resolve(&args.id).await?;
        let context = app
            .application_context(&opportunity, args.contact, jobhunt_app::now())
            .await?;
        print_json(&context)
    })
}
