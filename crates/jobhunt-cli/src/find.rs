//! `jobhunt find`: discover jobs from the configured sources, store them, and
//! show the matching ones.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, bail};
use chrono::Utc;
use jobhunt_core::{ErrorChain, SourceKey};
use jobhunt_jobs::{Discovery, DiscoveryReport, JobQuery, JobRepository};
use jobhunt_sources::{HttpClient, SourceSpec};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::render::{self, plural};

#[derive(Debug, clap::Args)]
pub struct FindArgs {
    /// Only show jobs whose title, company, location, department or team
    /// contain every one of these words.
    #[arg(value_name = "WORDS")]
    pub query: Vec<String>,

    /// Maximum number of jobs to show.
    #[arg(short = 'n', long, default_value_t = 20, value_name = "N")]
    pub limit: usize,

    /// Search only this source instead of the configured ones, written
    /// KIND:NAME (for example ashby:posthog). Repeatable.
    #[arg(long = "source", value_name = "KIND:NAME")]
    pub sources: Vec<SourceKey>,

    /// Don't fetch anything; search the jobs already stored locally.
    #[arg(long)]
    pub offline: bool,
}

pub async fn run(args: FindArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let config = &loaded.config;
    let specs = select_sources(&args.sources, config)?;
    let store = SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")?;

    let mut query = JobQuery {
        sources: specs.iter().map(|s| s.key().clone()).collect(),
        limit: Some(args.limit),
        ..JobQuery::default()
    }
    .with_text(&args.query.join(" "));

    let report = if args.offline {
        None
    } else {
        let http = HttpClient::new(config.discovery.http_settings())?;
        let sources = specs
            .iter()
            .map(|spec| spec.build(&http))
            .collect::<Result<Vec<_>, _>>()?;
        eprintln!(
            "Searching {}…",
            plural(sources.len() as u64, "source", "sources")
        );
        let report = Discovery::new(&store)
            .with_concurrency(config.discovery.concurrency)
            .run(&sources)
            .await
            .context("discovery failed while saving jobs")?;
        report_problems(&report);
        if report.succeeded() == 0 && !report.sources.is_empty() {
            store.close().await;
            bail!(
                "could not reach any source ({} failed)",
                report.sources.len()
            );
        }
        // Only show jobs that are currently listed at their source.
        query.seen_since = Some(report.started_at);
        Some(report)
    };

    let records = store.search(&query).await?;
    let total = store.count(&query).await?;
    store.close().await;

    match print_results(&records, total, report.as_ref(), &args) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write results")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The sources to read: the ones named with `--source` (using configured
/// details such as the company name when available), else all configured.
fn select_sources(
    requested: &[SourceKey],
    config: &crate::config::AppConfig,
) -> anyhow::Result<Vec<SourceSpec>> {
    let configured = config
        .sources
        .specs()
        .context("invalid source in configuration")?;
    if requested.is_empty() {
        return Ok(configured);
    }
    let mut selected: Vec<SourceSpec> = Vec::with_capacity(requested.len());
    for key in requested {
        if selected.iter().any(|s| s.key() == key) {
            continue;
        }
        let spec = match configured.iter().find(|s| s.key() == key) {
            Some(spec) => spec.clone(),
            None => SourceSpec::from_key(key)?,
        };
        selected.push(spec);
    }
    Ok(selected)
}

fn report_problems(report: &DiscoveryReport) {
    for (source, error) in report.failures() {
        eprintln!("warning: skipped {source}: {}", ErrorChain(error));
    }
    let rejected = report.totals().rejected;
    if rejected > 0 {
        eprintln!(
            "warning: {} could not be read (run with -v for details)",
            plural(rejected as u64, "posting", "postings")
        );
    }
}

fn print_results(
    records: &[jobhunt_jobs::JobRecord],
    total: u64,
    report: Option<&DiscoveryReport>,
    args: &FindArgs,
) -> io::Result<()> {
    let mut out = anstream::stdout().lock();
    let now = Utc::now();

    if records.is_empty() {
        let hint = match (report, args.query.is_empty()) {
            (None, _) => "No stored jobs match. Run without --offline to fetch fresh jobs.",
            (Some(_), false) => "No jobs match those words. Try fewer or different words.",
            (Some(_), true) => "The sources returned no open jobs.",
        };
        writeln!(out, "{hint}")?;
    } else {
        render::jobs(&mut out, records, now)?;
        let shown = records.len() as u64;
        if shown < total {
            writeln!(
                out,
                "Showing {shown} of {}. Use --limit to see more or add words to narrow the search.",
                plural(total, "matching job", "matching jobs")
            )?;
        } else {
            writeln!(
                out,
                "Showing {}.",
                plural(total, "matching job", "matching jobs")
            )?;
        }
    }

    match report {
        Some(report) => {
            let totals = report.totals();
            let open = totals.normalized - totals.duplicates;
            writeln!(
                out,
                "Checked {} in {:.1}s: {} ({} new, {} updated).",
                plural(report.sources.len() as u64, "source", "sources"),
                report.elapsed.as_secs_f64(),
                plural(open as u64, "open job", "open jobs"),
                totals.inserted,
                totals.updated,
            )?;
        }
        None => writeln!(out, "Offline: showing stored jobs without refreshing them.")?,
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn selects_configured_sources_by_default() {
        let config = AppConfig::default();
        let selected = select_sources(&[], &config).unwrap();
        assert_eq!(selected.len(), config.sources.ashby.len());
    }

    #[test]
    fn requested_sources_reuse_configured_details() {
        let config = AppConfig::default();
        let requested: Vec<SourceKey> = vec![
            "ashby:linear".parse().unwrap(),
            "ashby:posthog".parse().unwrap(),
            "ashby:linear".parse().unwrap(),
            "ashby:somethingelse".parse().unwrap(),
        ];
        let selected = select_sources(&requested, &config).unwrap();
        let keys: Vec<String> = selected.iter().map(|s| s.key().to_string()).collect();
        assert_eq!(
            keys,
            ["ashby:linear", "ashby:posthog", "ashby:somethingelse"]
        );
        let SourceSpec::Ashby { board, .. } = &selected[0];
        assert_eq!(board.company.as_deref(), Some("Linear"));
        let SourceSpec::Ashby { board, .. } = &selected[2];
        assert_eq!(board.company, None);
    }

    #[test]
    fn unknown_source_kinds_are_rejected() {
        let requested: Vec<SourceKey> = vec!["workday:acme".parse().unwrap()];
        assert!(select_sources(&requested, &AppConfig::default()).is_err());
    }
}
