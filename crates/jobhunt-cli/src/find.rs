//! `jobhunt find`: discover jobs from the configured sources, store them, and
//! show the matching ones.

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::Utc;
use jobhunt_core::{ErrorChain, SourceKey};
use jobhunt_jobs::{
    Discovery, DiscoveryReport, JobQuery, JobRecord, JobRepository, JobStatus, OpportunityId,
    ScanKind,
};
use jobhunt_sources::{CareersPage, HttpClient, SourceSpec, careers};
use jobhunt_storage::SqliteJobStore;
use url::Url;

use crate::config::{AppConfig, LoadedConfig};
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

    /// Search only this source instead of the configured ones: KIND:NAME
    /// (ashby:linear, greenhouse:stripe, lever:spotify, yc:posthog), a job
    /// board URL, or a company careers page URL. Repeatable.
    #[arg(long = "source", value_name = "SOURCE")]
    pub sources: Vec<SourceArg>,

    /// Don't fetch anything; search the jobs already stored locally.
    #[arg(long)]
    pub offline: bool,
}

/// A `--source` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceArg {
    Key(SourceKey),
    Url(Url),
}

impl FromStr for SourceArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("://") {
            let url = Url::parse(s).map_err(|e| format!("invalid URL {s:?}: {e}"))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(format!("{s:?} is not an http(s) URL"));
            }
            return Ok(Self::Url(url));
        }
        s.parse().map(Self::Key).map_err(|e| e.to_string())
    }
}

pub async fn run(args: FindArgs, loaded: &LoadedConfig, verbosity: u8) -> anyhow::Result<ExitCode> {
    let config = &loaded.config;
    let http = HttpClient::new(config.discovery.http_settings())?;
    let specs = select_sources(&args.sources, config, &http, args.offline).await?;
    let store = SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")?;

    let mut query = JobQuery {
        // Offline without --source searches everything stored.
        sources: if args.offline && args.sources.is_empty() {
            Vec::new()
        } else {
            specs.iter().map(|s| s.key().clone()).collect()
        },
        status: Some(JobStatus::Open),
        distinct_opportunities: true,
        limit: Some(args.limit),
        ..JobQuery::default()
    }
    .with_text(&args.query.join(" "));

    let report = if args.offline {
        None
    } else {
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
            .with_validator_max_age(config.discovery.validator_max_age())
            .run(&sources)
            .await
            .context("discovery failed while saving jobs")?;
        report_problems(&report);
        if verbosity > 0 {
            // Diagnostics go to stderr, next to the logs.
            let _ = render::scan_table(&mut anstream::stderr().lock(), &report);
        }
        if report.succeeded() == 0 && !report.sources.is_empty() {
            store.close().await;
            bail!(
                "could not reach any source ({} failed)",
                report.sources.len()
            );
        }
        // Only show jobs listed at their source during this run.
        query.seen_since = Some(report.started_at);
        Some(report)
    };

    let records = store.search(&query).await?;
    let total = store.count(&query).await?;
    let also_listed = other_listings(&store, &records).await?;
    store.close().await;

    match print_results(&records, &also_listed, total, report.as_ref(), &args) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write results")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Other open source records of each shown opportunity.
async fn other_listings(
    store: &SqliteJobStore,
    records: &[JobRecord],
) -> anyhow::Result<HashMap<OpportunityId, Vec<JobRecord>>> {
    let mut others = HashMap::new();
    for record in records {
        let members = store.opportunity_records(record.opportunity_id).await?;
        let rest: Vec<JobRecord> = members
            .into_iter()
            .filter(|m| m.id != record.id && m.status == JobStatus::Open)
            .collect();
        if !rest.is_empty() {
            others.insert(record.opportunity_id, rest);
        }
    }
    Ok(others)
}

/// The sources to read: the ones named with `--source` (using configured
/// details such as the company name when available), else all configured
/// ones, including boards found on configured careers pages.
async fn select_sources(
    requested: &[SourceArg],
    config: &AppConfig,
    http: &HttpClient,
    offline: bool,
) -> anyhow::Result<Vec<SourceSpec>> {
    let configured = config
        .sources
        .specs()
        .context("invalid source in configuration")?;
    let mut selected: Vec<SourceSpec> = Vec::new();
    let add = |spec: SourceSpec, selected: &mut Vec<SourceSpec>| {
        if !selected.iter().any(|s| s.key() == spec.key()) {
            let spec = configured
                .iter()
                .find(|c| c.key() == spec.key())
                .cloned()
                .unwrap_or(spec);
            selected.push(spec);
        }
    };

    if requested.is_empty() {
        for spec in configured.iter().cloned() {
            add(spec, &mut selected);
        }
        if !offline {
            let resolved = futures::future::join_all(
                config
                    .sources
                    .careers
                    .iter()
                    .map(|page| async move { (page, resolve_page(http, page).await) }),
            )
            .await;
            for (page, result) in resolved {
                match result {
                    Ok(specs) => specs.into_iter().for_each(|s| add(s, &mut selected)),
                    Err(error) => {
                        eprintln!("warning: skipped careers page {}: {error:#}", page.url)
                    }
                }
            }
        }
        return Ok(selected);
    }

    for arg in requested {
        match arg {
            SourceArg::Key(key) => add(SourceSpec::from_key(key)?, &mut selected),
            SourceArg::Url(url) => {
                let specs = match careers::board_for_url(url) {
                    Some(board) => vec![SourceSpec::from_board(&board, None)?],
                    None if offline => {
                        bail!("can't find the job board behind {url} while offline")
                    }
                    None => {
                        let page = CareersPage {
                            url: url.to_string(),
                            company: None,
                        };
                        resolve_page(http, &page).await?
                    }
                };
                specs.into_iter().for_each(|s| add(s, &mut selected));
            }
        }
    }
    Ok(selected)
}

/// The supported boards a careers page uses.
async fn resolve_page(http: &HttpClient, page: &CareersPage) -> anyhow::Result<Vec<SourceSpec>> {
    let url = Url::parse(&page.url).with_context(|| format!("invalid URL {:?}", page.url))?;
    let boards = careers::detect(http, &url)
        .await
        .with_context(|| format!("could not read {url}"))?;
    if boards.is_empty() {
        bail!(
            "found no Ashby, Greenhouse, Lever or YC job board on {url} \
             (pages that load jobs with JavaScript can't be read; configure the board instead)"
        );
    }
    let specs = boards
        .iter()
        .map(|board| SourceSpec::from_board(board, page.company.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    tracing::info!(
        page = %url,
        boards = ?specs.iter().map(|s| s.key().to_string()).collect::<Vec<_>>(),
        "careers page resolved"
    );
    Ok(specs)
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
    let partial = report
        .sources
        .iter()
        .filter(|s| matches!(&s.result, Ok(stats) if stats.kind == ScanKind::Partial))
        .count();
    if partial > 0 {
        eprintln!(
            "note: {} returned an incomplete listing; jobs missing from it were kept open",
            plural(partial as u64, "source", "sources")
        );
    }
}

fn print_results(
    records: &[JobRecord],
    also_listed: &HashMap<OpportunityId, Vec<JobRecord>>,
    total: u64,
    report: Option<&DiscoveryReport>,
    args: &FindArgs,
) -> io::Result<()> {
    let mut out = anstream::stdout().lock();
    let now = Utc::now();

    if records.is_empty() && total > 0 {
        // `--limit 0`: nothing to list, but say how many matched.
        writeln!(
            out,
            "Showing 0 of {}.",
            plural(total, "matching job", "matching jobs")
        )?;
    } else if records.is_empty() {
        let hint = match (report, args.query.is_empty()) {
            (None, _) => "No stored jobs match. Run without --offline to fetch fresh jobs.",
            (Some(_), false) => "No jobs match those words. Try fewer or different words.",
            (Some(_), true) => "The sources returned no open jobs.",
        };
        writeln!(out, "{hint}")?;
    } else {
        render::jobs(&mut out, records, also_listed, now)?;
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
        Some(report) => writeln!(out, "{}", render::run_summary(report))?,
        None => writeln!(out, "Offline: showing stored jobs without refreshing them.")?,
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http() -> HttpClient {
        HttpClient::new(jobhunt_sources::HttpSettings::default()).unwrap()
    }

    fn keys(specs: &[SourceSpec]) -> Vec<String> {
        specs.iter().map(|s| s.key().to_string()).collect()
    }

    #[tokio::test]
    async fn selects_configured_sources_by_default() {
        let config = AppConfig::default();
        let selected = select_sources(&[], &config, &http(), true).await.unwrap();
        assert_eq!(selected.len(), config.sources.specs().unwrap().len());
    }

    #[tokio::test]
    async fn requested_sources_reuse_configured_details() {
        let config = AppConfig::default();
        let requested: Vec<SourceArg> = [
            "ashby:linear",
            "ashby:posthog",
            "ashby:linear",
            "greenhouse:somethingelse",
            "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1",
            "https://www.ycombinator.com/companies/posthog/jobs",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        let selected = select_sources(&requested, &config, &http(), true)
            .await
            .unwrap();
        assert_eq!(
            keys(&selected),
            [
                "ashby:linear",
                "ashby:posthog",
                "greenhouse:somethingelse",
                "lever:spotify",
                "yc:posthog"
            ]
        );
        let SourceSpec::Ashby { board, .. } = &selected[0] else {
            panic!("expected ashby");
        };
        assert_eq!(board.company.as_deref(), Some("Linear"));
        let SourceSpec::Lever { site, .. } = &selected[3] else {
            panic!("expected lever");
        };
        assert_eq!(
            site.company.as_deref(),
            Some("Spotify"),
            "configured name reused"
        );
    }

    #[tokio::test]
    async fn unknown_kinds_and_unrecognized_urls_offline_are_rejected() {
        let config = AppConfig::default();
        let unknown = vec!["workday:acme".parse().unwrap()];
        assert!(
            select_sources(&unknown, &config, &http(), true)
                .await
                .is_err()
        );
        let page = vec!["https://example.com/careers".parse().unwrap()];
        let error = select_sources(&page, &config, &http(), true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("offline"));
    }

    #[test]
    fn parses_source_arguments() {
        assert_eq!(
            "lever:spotify".parse::<SourceArg>().unwrap(),
            SourceArg::Key("lever:spotify".parse().unwrap())
        );
        assert!(matches!(
            "https://jobs.lever.co/spotify"
                .parse::<SourceArg>()
                .unwrap(),
            SourceArg::Url(_)
        ));
        assert!("ftp://example.com".parse::<SourceArg>().is_err());
        assert!("spotify".parse::<SourceArg>().is_err());
    }
}
