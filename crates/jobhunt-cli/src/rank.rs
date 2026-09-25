//! `jobhunt why`, `taste`, `pipeline`, `feedback`, and the feedback
//! commands (`save`, `unsave`, `reject`, `like`, `dislike`, `applied`,
//! `interview`, `offer`).
//!
//! Thin: arguments in, [`jobhunt_app::LocalApp`] use cases, [`rank_render`]
//! out.

use std::process::ExitCode;

use jobhunt_app::feedback::PipelineView;
use jobhunt_ranking::FeedbackAction;

use crate::config::LoadedConfig;
use crate::local::{finish, print_json, with_app};
use crate::rank_render;

/// Feedback on one opportunity.
#[derive(Debug, clap::Args)]
pub struct FeedbackArgs {
    /// An opportunity id (opp_…, as `find` prints it) or a job id (job_…);
    /// a unique prefix works too.
    #[arg(value_name = "ID")]
    pub id: String,

    /// Why, in your own words ("too corporate", "pure SRE", "love tiny
    /// founder-led teams"). Kept verbatim; the most useful feedback you can
    /// give.
    #[arg(long, short = 'r', value_name = "TEXT")]
    pub reason: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct WhyArgs {
    /// An opportunity id (opp_…) or a job id (job_…); a unique prefix
    /// works too.
    #[arg(value_name = "ID")]
    pub id: String,

    /// Every signal with its group, basis, weight and evidence.
    #[arg(long, short = 'd')]
    pub details: bool,
}

#[derive(Debug, clap::Args)]
pub struct TasteArgs {
    /// Every piece of evidence, and patterns with too little evidence to use.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, clap::Args)]
pub struct PipelineArgs {
    /// Also list the opportunities you rejected.
    #[arg(long)]
    pub all: bool,

    /// Print the pipeline as JSON (the structure the MCP get_pipeline tool
    /// returns).
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct LogArgs {
    /// Only the feedback on this opportunity (opp_… or job_…).
    #[arg(value_name = "ID")]
    pub id: Option<String>,
}

pub async fn feedback(
    action: FeedbackAction,
    args: FeedbackArgs,
    loaded: &LoadedConfig,
) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let opportunity = app.resolve(&args.id).await?;
        let outcome = app
            .record_feedback(
                &opportunity,
                action,
                args.reason.as_deref(),
                jobhunt_app::now(),
            )
            .await?;
        finish(
            rank_render::recorded(&mut anstream::stdout().lock(), &outcome),
            "the feedback",
        )
    })
}

pub async fn why(args: WhyArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let opportunity = app.resolve(&args.id).await?;
        let inspection = app.inspect(&opportunity, true, jobhunt_app::now()).await?;
        let Some(ranking) = &inspection.ranking else {
            return Err(jobhunt_app::AppError::NoProfile.into());
        };
        finish(
            rank_render::brief(
                &mut anstream::stdout().lock(),
                ranking,
                args.details,
                inspection.reused_ranking,
            ),
            "the brief",
        )
    })
}

pub async fn taste(args: TasteArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let service = app.ranking();
        let (_, person) = service.person().await?;
        let model = service.taste(&person).await?;
        finish(
            rank_render::taste(&mut anstream::stdout().lock(), &model, &person, args.all),
            "your taste",
        )
    })
}

pub async fn pipeline(args: PipelineArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let entries = app.pipeline(args.all).await?;
        if args.json {
            return print_json(&PipelineView::of(&entries));
        }
        finish(
            rank_render::pipeline(&mut anstream::stdout().lock(), &entries),
            "the pipeline",
        )
    })
}

pub async fn log(args: LogArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let service = app.ranking();
        let events = match &args.id {
            Some(id) => {
                let opportunity = app.resolve(id).await?;
                service.state(&opportunity.records).await?.events
            }
            None => service.feedback().await?,
        };
        finish(
            rank_render::feedback_log(&mut anstream::stdout().lock(), &events),
            "the feedback",
        )
    })
}
