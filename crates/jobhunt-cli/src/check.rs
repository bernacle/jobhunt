//! `jobhunt check`: can you take this job, and is what JobHunt knows about
//! it first-party and current? Every answer comes with the posting's words.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Context;
use chrono::Utc;
use jobhunt_eligibility::verify;
use jobhunt_jobs::{Discovery, JobRecord};
use jobhunt_sources::{HttpClient, SourceSpec};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::eligibility;
use crate::find::report_problems;
use crate::render::{DIM, TITLE, plural};
use crate::show::load_records;

#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// A job id (job_…, printed by `find`) or an opportunity id (opp_…).
    #[arg(value_name = "ID")]
    pub id: String,

    /// Re-read the job's sources first, so the answer rests on what the
    /// company publishes right now (and a job taken down shows as closed).
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
    let mut records = load_records(store, &args.id).await?;
    if args.refresh {
        refresh(&records, loaded, store).await?;
        records = load_records(store, &args.id).await?;
    }
    let user = eligibility::user_constraints(store).await?;
    let mut out = anstream::stdout().lock();
    match write(&mut out, &records, user.as_ref()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write the check")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Re-reads every source listing the job.
async fn refresh(
    records: &[JobRecord],
    loaded: &LoadedConfig,
    store: &SqliteJobStore,
) -> anyhow::Result<()> {
    let config = &loaded.config;
    let configured = config
        .sources
        .specs()
        .context("invalid source in configuration")?;
    let specs = sources_of(records, &configured)?;
    let http = HttpClient::new(config.discovery.http_settings())?;
    let sources = specs
        .iter()
        .map(|spec| spec.build(&http))
        .collect::<Result<Vec<_>, _>>()?;
    eprintln!(
        "Re-reading {}…",
        plural(sources.len() as u64, "source", "sources")
    );
    let report = Discovery::new(store)
        .with_concurrency(config.discovery.concurrency)
        .with_validator_max_age(config.discovery.validator_max_age())
        .run(&sources)
        .await
        .context("discovery failed while saving jobs")?;
    report_problems(&report);
    Ok(())
}

/// Each source listing the job once, with its configured details (such as
/// the company name) when it is configured.
fn sources_of(records: &[JobRecord], configured: &[SourceSpec]) -> anyhow::Result<Vec<SourceSpec>> {
    let mut specs: Vec<SourceSpec> = Vec::new();
    for record in records {
        let key = &record.posting.provenance.source;
        if specs.iter().any(|s| s.key() == key) {
            continue;
        }
        let spec = match configured.iter().find(|c| c.key() == key) {
            Some(spec) => spec.clone(),
            None => SourceSpec::from_key(key)?,
        };
        specs.push(spec);
    }
    Ok(specs)
}

fn write(
    out: &mut impl Write,
    records: &[JobRecord],
    user: Option<&jobhunt_eligibility::UserConstraints>,
) -> io::Result<()> {
    let main = &records[0];
    writeln!(out, "{TITLE}{}{TITLE:#}", main.posting.title)?;
    writeln!(out, "{} · {}", main.posting.company, main.id)?;
    writeln!(out)?;
    match user.and_then(|u| eligibility::best(records, u)) {
        Some((index, assessment)) => {
            writeln!(out, "{}", eligibility::verdict(&assessment))?;
            writeln!(out)?;
            eligibility::checks(out, &assessment, "  ", true)?;
            if records.len() > 1 {
                writeln!(
                    out,
                    "  {DIM}Answered from {}'s listing.{DIM:#}",
                    records[index].posting.provenance.source
                )?;
            }
        }
        None => writeln!(
            out,
            "No career profile yet, so there is nothing to check the job against.\n\
             Run `jobhunt init <resume>` or `jobhunt preferences set location <place>`."
        )?,
    }
    writeln!(out)?;
    writeln!(out, "Source:")?;
    eligibility::verification(out, &verify(records, Utc::now()), "  ")?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use jobhunt_core::{CanonicalUrl, Provenance, SourceKey};
    use jobhunt_jobs::{JobPosting, JobStatus, OpportunityId};

    use super::*;

    fn record(source: &str, id: &str) -> JobRecord {
        let key: SourceKey = source.parse().unwrap();
        let posting = JobPosting {
            provenance: Provenance {
                source: key,
                source_record_id: Some(id.into()),
                fetched_from: None,
            },
            url: CanonicalUrl::parse(&format!("https://jobs.example.com/{id}")).unwrap(),
            apply_url: None,
            company: "Acme".into(),
            title: "Engineer".into(),
            department: None,
            team: None,
            location: None,
            locations: Vec::new(),
            employment_type: None,
            workplace_type: None,
            is_remote: None,
            compensation: None,
            work_authorization: None,
            description_text: None,
            description_html: None,
            posted_at: None,
            source_updated_at: None,
        };
        let at = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        JobRecord {
            id: posting.id(),
            opportunity_id: OpportunityId::founded_by(posting.id()),
            posting,
            first_seen_at: at,
            last_seen_at: at,
            content_updated_at: at,
            status: JobStatus::Open,
            closed_at: None,
        }
    }

    #[test]
    fn refresh_rereads_each_listing_source_once() {
        let configured = crate::config::AppConfig::default().sources.specs().unwrap();
        let records = [
            record("ashby:linear", "1"),
            record("yc:linear", "2"),
            record("ashby:linear", "3"),
        ];
        let specs = sources_of(&records, &configured).unwrap();
        let keys: Vec<String> = specs.iter().map(|s| s.key().to_string()).collect();
        assert_eq!(keys, ["ashby:linear", "yc:linear"]);
        let SourceSpec::Ashby { board, .. } = &specs[0] else {
            panic!("expected ashby");
        };
        assert_eq!(
            board.company.as_deref(),
            Some("Linear"),
            "configured details kept"
        );
        assert!(sources_of(&[record("workday:acme", "1")], &configured).is_err());
    }
}
