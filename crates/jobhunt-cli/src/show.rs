//! `jobhunt show`: everything stored about one job, including every source
//! that lists it and its history.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use jobhunt_eligibility::Assessment;
use jobhunt_jobs::verification::{OpportunityTrust, cached};
use jobhunt_jobs::{
    JobEvent, JobEventKind, JobId, JobRecord, JobRepository, JobStatus, OpportunityId,
};
use jobhunt_ranking::{Gate, OpportunityState, Ranking, RankingService, RuleReader, Sentiment};
use jobhunt_storage::SqliteJobStore;

use crate::config::LoadedConfig;
use crate::eligibility;
use crate::render::{DIM, compensation_text, employment_label, workplace_label};

#[derive(Debug, clap::Args)]
pub struct ShowArgs {
    /// A job id (job_…, printed by `find`) or an opportunity id (opp_…).
    #[arg(value_name = "ID")]
    pub id: String,
}

/// A job's records, the requested one first, then the rest of its
/// opportunity; or every record of an opportunity.
pub async fn load_records(store: &SqliteJobStore, id: &str) -> anyhow::Result<Vec<JobRecord>> {
    let records = if let Ok(job) = id.parse::<JobId>() {
        match store.get(job).await? {
            Some(record) => {
                // Put the requested record first, then the rest of its group.
                let mut members = store.opportunity_records(record.opportunity_id).await?;
                members.retain(|m| m.id != record.id);
                members.insert(0, record);
                members
            }
            None => Vec::new(),
        }
    } else if let Ok(opportunity) = id.parse::<OpportunityId>() {
        store.opportunity_records(opportunity).await?
    } else {
        bail!("{id:?} is not a job id (job_…) or opportunity id (opp_…)");
    };
    if records.is_empty() {
        bail!("no stored job has the id {id}");
    }
    Ok(records)
}

pub async fn run(args: ShowArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let store = SqliteJobStore::open(&loaded.database)
        .await
        .context("could not open the local job database")?;
    let records = match load_records(&store, &args.id).await {
        Ok(records) => records,
        Err(error) => {
            store.close().await;
            return Err(error);
        }
    };
    let now = Utc::now();
    let verified = cached(&store, &records).await?;
    let (_, assessment, trust) =
        crate::verify::assessment(&store, &verified, &loaded.config.verification.policy(), now)
            .await?;
    let mut histories = Vec::with_capacity(records.len());
    for record in &records {
        histories.push(store.history(record.id).await?);
    }
    let ranking =
        RankingService::new(&store, &RuleReader).with_policy(loaded.config.verification.policy());
    let fit = match assessment {
        Some(_) => Some(ranking.explain(&records, now).await?.ranking),
        None => None,
    };
    let state = ranking.state(&records).await?;
    ranking.mark_seen(&records, now).await?;
    store.close().await;

    let mut out = anstream::stdout().lock();
    match write_details(
        &mut out,
        &records,
        &histories,
        &trust,
        assessment.as_ref(),
        (fit.as_ref(), &state),
        now,
    ) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write the job")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn write_details(
    out: &mut impl Write,
    records: &[JobRecord],
    histories: &[Vec<JobEvent>],
    trust: &OpportunityTrust,
    assessment: Option<&Assessment>,
    (fit, state): (Option<&Ranking>, &OpportunityState),
    now: DateTime<Utc>,
) -> io::Result<()> {
    let main = &records[0];
    let job = &main.posting;
    writeln!(out, "{}", job.title)?;
    writeln!(out, "{}", job.company)?;
    writeln!(out)?;
    let mut field = |name: &str, value: Option<String>| -> io::Result<()> {
        if let Some(value) = value {
            writeln!(out, "{name:<13} {value}")?;
        }
        Ok(())
    };
    field("Location", job.location.clone())?;
    let others: Vec<String> = job
        .locations
        .iter()
        .skip(1)
        .filter_map(|l| l.name.clone())
        .collect();
    field("Also in", (!others.is_empty()).then(|| others.join("; ")))?;
    field(
        "Workplace",
        job.workplace_type
            .as_ref()
            .map(|w| workplace_label(w).to_owned()),
    )?;
    field(
        "Remote",
        job.is_remote
            .map(|r| if r { "yes" } else { "no" }.to_owned()),
    )?;
    field(
        "Employment",
        job.employment_type
            .as_ref()
            .map(|e| employment_label(e).to_owned()),
    )?;
    field("Department", job.department.clone())?;
    field("Team", job.team.clone())?;
    field("Pay", job.compensation.as_ref().and_then(compensation_text))?;
    field("Posted", job.posted_at.map(date))?;
    field("URL", Some(job.url.to_string()))?;
    field("Apply", job.apply_url.as_ref().map(ToString::to_string))?;
    field("Opportunity", Some(main.opportunity_id.to_string()))?;

    writeln!(out)?;
    writeln!(out, "Verification:")?;
    eligibility::verification(out, trust, now, "  ", false)?;
    writeln!(out)?;
    writeln!(out, "Eligibility:")?;
    match assessment {
        Some(a) => {
            writeln!(out, "  {}", eligibility::verdict(&a.decision))?;
            if !a.recommendable()
                && a.decision.status >= jobhunt_eligibility::Eligibility::Conditional
            {
                writeln!(out, "  {DIM}{}{DIM:#}", a.listing_reason().conclusion)?;
            }
            writeln!(
                out,
                "  {DIM}Why, with evidence: jobhunt check {id} · verify again: jobhunt verify {id}{DIM:#}",
                id = main.id
            )?;
        }
        None => writeln!(
            out,
            "  No career profile yet: run `jobhunt init <resume>` or `jobhunt preferences set location <place>`."
        )?,
    }

    writeln!(out)?;
    writeln!(out, "Fit:")?;
    match fit {
        Some(r) => {
            let label = match &r.gate {
                Gate::Excluded { .. } => "Not recommended",
                _ => r.tier.label(),
            };
            writeln!(out, "  {label}: {}", r.brief.verdict)?;
            writeln!(
                out,
                "  {DIM}Why, caveats and unknowns: jobhunt why {}{DIM:#}",
                main.opportunity_id
            )?;
        }
        None => writeln!(out, "  Needs a career profile, like eligibility.")?,
    }
    if state.has_feedback() {
        let since = state
            .since()
            .map(|at| format!(" since {}", at.format("%Y-%m-%d")))
            .unwrap_or_default();
        let sentiment = match state.sentiment {
            Some(Sentiment::Liked) => ", liked",
            Some(Sentiment::Disliked) => ", disliked",
            None => "",
        };
        writeln!(
            out,
            "  Your status: {}{sentiment}{since} {DIM}(jobhunt feedback {}){DIM:#}",
            state.stage.as_str(),
            main.opportunity_id
        )?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "{}:",
        if records.len() > 1 {
            "Listed by these sources"
        } else {
            "Source"
        }
    )?;
    for (record, history) in records.iter().zip(histories) {
        let status = match record.status {
            JobStatus::Open => "open".to_owned(),
            JobStatus::Closed => {
                format!("closed {}", record.closed_at.map(date).unwrap_or_default())
            }
        };
        let provenance = &record.posting.provenance;
        writeln!(
            out,
            "  {} · {} · {status}",
            provenance.source,
            provenance
                .source_record_id
                .as_deref()
                .unwrap_or("(no source id)")
        )?;
        writeln!(out, "    {}", record.id)?;
        writeln!(out, "    {}", record.posting.url)?;
        writeln!(
            out,
            "    first seen {} · last seen {} · content changed {}",
            date(record.first_seen_at),
            date(record.last_seen_at),
            date(record.content_updated_at)
        )?;
        if let Some(from) = &provenance.fetched_from {
            writeln!(out, "    fetched from {from}")?;
        }
        for event in history {
            writeln!(out, "    {}", event_line(event))?;
        }
    }

    if let Some(text) = &job.description_text {
        writeln!(out)?;
        writeln!(out, "{text}")?;
    }
    out.flush()
}

fn event_line(event: &JobEvent) -> String {
    let what = match event.kind {
        JobEventKind::New => "new".to_owned(),
        JobEventKind::Closed => "closed (missing from a complete listing)".to_owned(),
        JobEventKind::Updated => format!("updated: {}", event.changed_fields.join(", ")),
        JobEventKind::Reopened if event.changed_fields.is_empty() => "reopened".to_owned(),
        JobEventKind::Reopened => {
            format!("reopened, changed: {}", event.changed_fields.join(", "))
        }
    };
    format!("{} {what}", date(event.at))
}

fn date(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M UTC").to_string()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn describes_history_events() {
        let at = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        let event = |kind, fields: &[&str]| JobEvent {
            kind,
            at,
            run: None,
            changed_fields: fields.iter().map(|f| (*f).to_owned()).collect(),
            previous: None,
        };
        assert_eq!(
            event_line(&event(JobEventKind::Updated, &["title", "compensation"])),
            "2026-09-25 12:00 UTC updated: title, compensation"
        );
        assert_eq!(
            event_line(&event(JobEventKind::Reopened, &[])),
            "2026-09-25 12:00 UTC reopened"
        );
        assert!(event_line(&event(JobEventKind::Closed, &[])).contains("complete listing"));
    }
}
