//! `narrow verify`: ask a job's authoritative sources whether it is still
//! open, record what they say, and check it against your profile.

use std::io::{self, Write};
use std::process::ExitCode;

use chrono::{DateTime, Utc};
use jobhunt_app::inspect::VerificationReport;
use jobhunt_app::verify::Checked;
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Assessment, requirements};
use jobhunt_jobs::verification::{OpportunityTrust, RecordVerification, VerifyMode};

use crate::config::LoadedConfig;
use crate::eligibility;
use crate::local::{StderrProgress, finish, print_json, with_app};
use crate::render::{DIM, TITLE, plural};

#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// An opportunity id (opp_…, as `find` prints it) or a job id (job_…);
    /// a unique prefix works too.
    #[arg(value_name = "ID")]
    pub id: String,

    /// Ask the sources even if a recent verification exists.
    #[arg(long)]
    pub force: bool,

    /// Show provenance: every source record, the URLs checked, the
    /// authority chain, and the evidence behind every reason.
    #[arg(long, short = 'd')]
    pub details: bool,

    /// Print the result as JSON (the structure the MCP verify_job tool
    /// returns).
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: VerifyArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let opportunity = app.resolve(&args.id).await?;
        let mode = if args.force {
            VerifyMode::Force
        } else {
            VerifyMode::IfDue
        };
        let checked = app
            .verify(
                &opportunity,
                mode,
                &StderrProgress::each(),
                jobhunt_app::now(),
            )
            .await?;
        let reused = checked.reused();
        if reused > 0 && !args.force {
            eprintln!(
                "Reused {} from the last {} minutes (--force asks again).",
                plural(reused as u64, "recent verification", "recent verifications"),
                loaded.config.verification.reuse_minutes
            );
        }
        if args.json {
            return print_json(&VerificationReport::of(&opportunity, &checked));
        }
        report(&checked, args.details)
    })
}

/// Prints the full report.
pub fn report(checked: &Checked, detail: bool) -> anyhow::Result<ExitCode> {
    finish(
        write(
            &mut anstream::stdout().lock(),
            &checked.verified,
            &checked.trust,
            checked.profile.as_ref(),
            checked.assessment.as_ref(),
            jobhunt_app::now(),
            detail,
        ),
        "the verification",
    )
}

fn write(
    out: &mut impl Write,
    verified: &[RecordVerification],
    trust: &OpportunityTrust,
    profile: Option<&ProfileFacts>,
    assessment: Option<&Assessment>,
    now: DateTime<Utc>,
    detail: bool,
) -> io::Result<()> {
    let main = trust
        .best
        .and_then(|i| verified.get(i))
        .or_else(|| verified.first())
        .map(|v| &v.record);
    let Some(main) = main else {
        return writeln!(out, "No source records.");
    };
    let job = requirements(main);
    writeln!(
        out,
        "{TITLE}{} — {}{TITLE:#}",
        main.posting.title, main.posting.company
    )?;
    writeln!(out, "{DIM}{} · {}{DIM:#}", main.id, main.opportunity_id)?;
    writeln!(out)?;
    eligibility::section(out, "Verification")?;
    eligibility::verification(out, trust, now, "  ", detail)?;
    writeln!(out)?;
    eligibility::section(out, "Location")?;
    eligibility::location(out, &job, profile, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Employment")?;
    eligibility::employment(out, &job, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Compensation")?;
    eligibility::compensation(out, trust, main, "  ")?;
    writeln!(out)?;
    eligibility::section(out, "Eligibility")?;
    match assessment {
        Some(a) => eligibility::eligibility(out, a, "  ", detail)?,
        None => writeln!(
            out,
            "  No career profile yet, so there is nothing to check the job against.\n  \
             Run `narrow init <resume>` or `narrow preferences set location <place>`."
        )?,
    }
    if !detail {
        writeln!(out)?;
        writeln!(
            out,
            "{DIM}Evidence and provenance: narrow verify --details {}{DIM:#}",
            main.id
        )?;
    }
    writeln!(
        out,
        "{DIM}A compatibility signal from the posting and your profile, not legal advice about work authorization.{DIM:#}"
    )?;
    out.flush()
}
