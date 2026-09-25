//! The cloud process modes of the same binary: `jobhunt server`,
//! `jobhunt worker discovery|verification`, `jobhunt migrate`, and
//! `jobhunt admin …` for operators. Their configuration comes from the
//! environment (see `jobhunt_cloud::config`), not from the local config
//! file.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use jobhunt_cloud::config::{CloudConfig, process_env};
use jobhunt_cloud::{Role, server};

use crate::local::{finish, print_json};

#[derive(Debug, clap::Args)]
pub struct WorkerArgs {
    #[command(subcommand)]
    pub kind: WorkerKind,
}

#[derive(Debug, clap::Subcommand)]
pub enum WorkerKind {
    /// Read the sources that are due (a Railway cron run), then exit.
    Discovery {
        /// Stop claiming new sources after this many minutes.
        #[arg(long, default_value_t = 20)]
        budget_minutes: u64,
    },
    /// Re-verify the jobs that matter and are not fresh, then exit.
    Verification,
}

#[derive(Debug, clap::Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum AdminCommand {
    /// The cloud configuration (without secret values), schema, discovery
    /// schedule and recent usage.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Re-encrypt private data under the active key (after adding a new
    /// key first in JOBHUNT_ENCRYPTION_KEYS).
    Reencrypt,
}

/// The configuration from the environment.
pub fn cloud_config() -> CloudConfig {
    CloudConfig::from_env(&process_env)
}

pub async fn serve(config: CloudConfig) -> anyhow::Result<ExitCode> {
    server::serve(config).await?;
    Ok(ExitCode::SUCCESS)
}

pub async fn migrate(config: CloudConfig) -> anyhow::Result<ExitCode> {
    let status = server::migrate(config).await?;
    println!(
        "Schema current: {} of {} migrations applied.",
        status.applied, status.known
    );
    Ok(ExitCode::SUCCESS)
}

pub async fn worker(args: WorkerArgs, config: CloudConfig) -> anyhow::Result<ExitCode> {
    match args.kind {
        WorkerKind::Discovery { budget_minutes } => {
            let summary = server::discovery_worker(
                config,
                Duration::from_secs(budget_minutes.saturating_mul(60)),
            )
            .await?;
            print_json(&summary)
        }
        WorkerKind::Verification => {
            let summary = server::verification_worker(config).await?;
            print_json(&summary)
        }
    }
}

pub async fn admin(args: AdminArgs, config: CloudConfig) -> anyhow::Result<ExitCode> {
    match args.command {
        AdminCommand::Status { json } => status(config, json).await,
        AdminCommand::Reencrypt => {
            let config = config.require(Role::Server)?;
            let keys = config.keyring()?;
            let store = server::connect(&config, keys).await?;
            let report = store.reencrypt().await?;
            store.close().await;
            println!(
                "Examined {} sealed values; re-encrypted {} under the active key.",
                report.examined, report.rewritten
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn status(config: CloudConfig, json: bool) -> anyhow::Result<ExitCode> {
    let report = config.report();
    let problems = config.problems(Role::Server);
    let keys = config
        .keyring()
        .unwrap_or_else(|_| jobhunt_storage::postgres::Keyring::ephemeral());
    let store = match config.database_url() {
        Some(_) => server::connect(&config, keys).await.ok(),
        None => None,
    };
    let (schema, schedule, usage) = match &store {
        Some(s) => (
            s.schema().await.ok(),
            s.schedule_overview().await.unwrap_or_default(),
            s.usage_summary(chrono::Utc::now() - chrono::Duration::days(7))
                .await
                .unwrap_or_default(),
        ),
        None => (None, Vec::new(), Vec::new()),
    };
    if let Some(s) = &store {
        s.close().await;
    }
    if json {
        return print_json(&serde_json::json!({
            "settings": report.iter().map(|s| serde_json::json!({
                "name": s.name, "status": s.status, "required": s.required,
            })).collect::<Vec<_>>(),
            "problems": problems,
            "schema": schema.map(|s| serde_json::json!({"applied": s.applied, "known": s.known})),
            "schedule": schedule,
            "usage_7d": usage.iter().map(|(e, n, u)| serde_json::json!({
                "event": e, "count": n, "accounts": u,
            })).collect::<Vec<_>>(),
        }));
    }
    let mut out = anstream::stdout().lock();
    let result = (|| -> std::io::Result<()> {
        writeln!(out, "Configuration (values of secrets are never shown):")?;
        for s in &report {
            writeln!(out, "  {:<50} {}", s.name, s.status)?;
        }
        for p in &problems {
            writeln!(out, "  ! {p}")?;
        }
        match &schema {
            Some(s) => writeln!(
                out,
                "Schema: {} of {} migrations applied",
                s.applied, s.known
            )?,
            None => writeln!(out, "Schema: database not reachable")?,
        }
        writeln!(out, "Discovery schedule ({} sources):", schedule.len())?;
        for row in schedule.iter().take(50) {
            writeln!(
                out,
                "  {:<32} {:?} next {} failures {}{}{}",
                row.source,
                row.tier,
                row.next_due_at.format("%Y-%m-%d %H:%M"),
                row.consecutive_failures,
                if row.enabled { "" } else { " (disabled)" },
                row.leased_by
                    .as_ref()
                    .map_or_else(String::new, |w| format!(" (held by {w})"))
            )?;
        }
        writeln!(out, "Usage, last 7 days:")?;
        for (event, count, accounts) in &usage {
            writeln!(out, "  {event:<20} {count} events, {accounts} accounts")?;
        }
        Ok(())
    })();
    finish(result, "the status")
}
