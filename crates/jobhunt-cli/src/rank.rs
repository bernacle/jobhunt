//! `jobhunt rank`, `why`, `taste`, `pipeline`, `feedback`, and the
//! feedback commands (`save`, `unsave`, `reject`, `like`, `dislike`,
//! `applied`, `interview`, `offer`).
//!
//! Thin: arguments in, [`RankingService`] use cases, [`rank_render`] out.

use std::io;
use std::process::ExitCode;

use anyhow::Context;
use chrono::Utc;
use jobhunt_ranking::{FeedbackAction, RankQuery, RankingService, RuleReader};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::rank_render;
use crate::show::load_records;

/// Feedback on one job.
#[derive(Debug, clap::Args)]
pub struct FeedbackArgs {
    /// A job id (job_…) or an opportunity id (opp_…).
    #[arg(value_name = "ID")]
    pub id: String,

    /// Why, in your own words ("too corporate", "pure SRE", "love tiny
    /// founder-led teams"). Kept verbatim; the most useful feedback you can
    /// give.
    #[arg(long, short = 'r', value_name = "TEXT")]
    pub reason: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct RankArgs {
    /// Only rank jobs whose title, company, location, department or team
    /// contain every one of these words.
    #[arg(value_name = "WORDS")]
    pub query: Vec<String>,

    /// Maximum number of jobs to show.
    #[arg(short = 'n', long, default_value_t = 10, value_name = "N")]
    pub limit: usize,

    /// Also show jobs that rank as maybe or low priority.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, clap::Args)]
pub struct WhyArgs {
    /// A job id (job_…) or an opportunity id (opp_…).
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
    /// Also list the jobs you rejected.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, clap::Args)]
pub struct LogArgs {
    /// Only the feedback on this job (job_… or opp_…).
    #[arg(value_name = "ID")]
    pub id: Option<String>,
}

async fn open(loaded: &LoadedConfig) -> anyhow::Result<SqliteJobStore> {
    SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")
}

fn service<'a>(
    store: &'a SqliteJobStore,
    loaded: &LoadedConfig,
) -> RankingService<'a, SqliteJobStore> {
    RankingService::new(store, &RuleReader).with_policy(loaded.config.verification.policy())
}

fn finish(result: io::Result<()>, what: &str) -> anyhow::Result<ExitCode> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.with_context(|| format!("could not write {what}"))?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Runs a use case against the store, closing it afterwards.
macro_rules! with_store {
    ($loaded:expr, |$store:ident| $body:expr) => {{
        let $store = open($loaded).await?;
        let result: anyhow::Result<ExitCode> = async { $body }.await;
        $store.close().await;
        result
    }};
}

pub async fn feedback(
    action: FeedbackAction,
    args: FeedbackArgs,
    loaded: &LoadedConfig,
) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let records = load_records(&store, &args.id).await?;
        let recorded = service(&store, loaded)
            .record(&records, action, args.reason.as_deref(), Utc::now())
            .await
            .context("could not save the feedback")?;
        finish(
            rank_render::recorded(&mut anstream::stdout().lock(), &recorded),
            "the feedback",
        )
    })
}

pub async fn rank(args: RankArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let query = RankQuery {
            text: args.query.join(" "),
            store_top: args.limit,
            all: args.all,
        };
        let report = service(&store, loaded).rank(&query, Utc::now()).await?;
        finish(
            rank_render::rank_list(
                &mut anstream::stdout().lock(),
                &report,
                args.limit,
                args.all,
            ),
            "the ranking",
        )
    })
}

pub async fn why(args: WhyArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let records = load_records(&store, &args.id).await?;
        let now = Utc::now();
        let service = service(&store, loaded);
        let explained = service.explain(&records, now).await?;
        service.mark_seen(&records, now).await?;
        finish(
            rank_render::brief(
                &mut anstream::stdout().lock(),
                &explained.ranking,
                args.details,
                explained.reused,
            ),
            "the brief",
        )
    })
}

pub async fn taste(args: TasteArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let service = service(&store, loaded);
        let (_, person) = service.person().await?;
        let model = service.taste(&person).await?;
        finish(
            rank_render::taste(&mut anstream::stdout().lock(), &model, &person, args.all),
            "your taste",
        )
    })
}

pub async fn pipeline(args: PipelineArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let entries = service(&store, loaded).pipeline(args.all).await?;
        finish(
            rank_render::pipeline(&mut anstream::stdout().lock(), &entries),
            "the pipeline",
        )
    })
}

pub async fn log(args: LogArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_store!(loaded, |store| {
        let service = service(&store, loaded);
        let events = match &args.id {
            Some(id) => {
                let records = load_records(&store, id).await?;
                service.state(&records).await?.events
            }
            None => service.feedback().await?,
        };
        finish(
            rank_render::feedback_log(&mut anstream::stdout().lock(), &events),
            "the feedback",
        )
    })
}
