//! `jobhunt find`: the few opportunities worth your time.
//!
//! With a profile, `find` is the personalized shortlist
//! ([`jobhunt_app::LocalApp::find`]): it refreshes job boards only when the
//! stored jobs are stale, ranks every open opportunity, verifies the best
//! candidates, and prints a handful with why. `--raw` (or `--source`,
//! `--eligible`, `--possible`) lists the matching stored jobs instead, the
//! inventory `find` printed before it was personalized. Without a profile
//! the inventory is shown, with how to start one.
//!
//! `jobhunt rank` is the shortlist from stored jobs only (`find --offline`).

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::bail;
use chrono::{DateTime, Utc};
use jobhunt_app::discover::SourceArg;
use jobhunt_app::shortlist::{MAX_LIMIT, SearchResults, ShortlistItem};
use jobhunt_app::views::{EligibilityStatus, VerificationState};
use jobhunt_app::{AppError, FindRequest, LocalApp, RefreshMode, RefreshReason};
use jobhunt_core::ErrorChain;
use jobhunt_eligibility::{Eligibility, EligibilityDecision, evaluate_record};
use jobhunt_jobs::{
    DiscoveryReport, JobId, JobQuery, JobRecord, JobStatus, OpportunityId, ScanKind,
};
use jobhunt_storage::Store;

use crate::config::LoadedConfig;
use crate::local::{StderrProgress, finish, print_json, with_app};
use crate::rank_render::{GOOD, HEADING, OPEN, tier_style};
use crate::render::{self, DIM, TITLE, plural};

#[derive(Debug, clap::Args)]
pub struct FindArgs {
    /// Only consider jobs whose title, company, location, department or
    /// team contain every one of these words.
    #[arg(value_name = "WORDS")]
    pub query: Vec<String>,

    /// How many to show (default 5; 20 with --raw).
    #[arg(short = 'n', long, value_name = "N")]
    pub limit: Option<usize>,

    /// Also show opportunities that rank as maybe or low priority.
    #[arg(long)]
    pub all: bool,

    /// Read every configured job board first, even if the stored jobs are
    /// fresh.
    #[arg(long, conflicts_with = "offline")]
    pub refresh: bool,

    /// Don't touch the network: no refresh, no verification. Work from
    /// what is stored.
    #[arg(long)]
    pub offline: bool,

    /// List every matching stored job (unranked, with source details)
    /// instead of the personalized shortlist.
    #[arg(long)]
    pub raw: bool,

    /// Read only this source (implies --raw): KIND:NAME (ashby:linear,
    /// greenhouse:stripe, lever:spotify, yc:posthog), a job board URL, or a
    /// company careers page URL. Repeatable.
    #[arg(long = "source", value_name = "SOURCE")]
    pub sources: Vec<SourceArg>,

    /// Only jobs your profile says you can take (implies --raw): eligible,
    /// or conditionally eligible (relocating, or sponsorship the posting
    /// offers).
    #[arg(long, conflicts_with = "possible")]
    pub eligible: bool,

    /// Hide jobs your profile rules out; keep the uncertain ones (implies
    /// --raw).
    #[arg(long)]
    pub possible: bool,

    /// Print the shortlist as JSON (the structure the MCP search_jobs tool
    /// returns).
    #[arg(long, conflicts_with = "raw")]
    pub json: bool,
}

impl FindArgs {
    fn raw_mode(&self) -> bool {
        self.raw || !self.sources.is_empty() || self.eligible || self.possible
    }

    fn refresh_mode(&self) -> RefreshMode {
        if self.offline {
            RefreshMode::Never
        } else if self.refresh {
            RefreshMode::Always
        } else {
            RefreshMode::Auto
        }
    }

    /// The lowest decision to show, when filtering by eligibility.
    fn min_status(&self) -> Option<Eligibility> {
        if self.eligible {
            Some(Eligibility::Conditional)
        } else if self.possible {
            Some(Eligibility::Uncertain)
        } else {
            None
        }
    }
}

/// `jobhunt rank`: the shortlist from stored jobs.
#[derive(Debug, clap::Args)]
pub struct RankArgs {
    #[arg(value_name = "WORDS")]
    pub query: Vec<String>,
    #[arg(short = 'n', long, default_value_t = 10, value_name = "N")]
    pub limit: usize,
    #[arg(long)]
    pub all: bool,
}

pub async fn rank(args: RankArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    execute(
        FindArgs {
            query: args.query,
            limit: Some(args.limit),
            all: args.all,
            refresh: false,
            offline: true,
            raw: false,
            sources: Vec::new(),
            eligible: false,
            possible: false,
            json: false,
        },
        loaded,
        0,
        true,
    )
    .await
}

pub async fn run(args: FindArgs, loaded: &LoadedConfig, verbosity: u8) -> anyhow::Result<ExitCode> {
    execute(args, loaded, verbosity, false).await
}

async fn execute(
    args: FindArgs,
    loaded: &LoadedConfig,
    verbosity: u8,
    personal_only: bool,
) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let has_profile = app.profile_facts().await?.is_some();
        if !has_profile && (personal_only || args.json) {
            return Err(AppError::NoProfile.into());
        }
        if args.raw_mode() || !has_profile {
            return raw(&app, &args, verbosity, has_profile).await;
        }
        shortlist(&app, &args, verbosity).await
    })
}

async fn shortlist(app: &LocalApp, args: &FindArgs, verbosity: u8) -> anyhow::Result<ExitCode> {
    let limit = args.limit.unwrap_or(5);
    if limit == 0 || limit > MAX_LIMIT {
        bail!("--limit must be between 1 and {MAX_LIMIT}");
    }
    let request = FindRequest {
        text: args.query.join(" "),
        limit,
        all_tiers: args.all,
        refresh: args.refresh_mode(),
        verify: !args.offline,
    };
    let now = jobhunt_app::now();
    let found = app.find(&request, &StderrProgress::search(), now).await?;
    match &found.refresh.report {
        Some(report) => {
            report_problems(report, &found.refresh.warnings);
            if verbosity > 0 {
                let _ = render::scan_table(&mut anstream::stderr().lock(), report);
            }
        }
        None => {
            for warning in &found.refresh.warnings {
                eprintln!("warning: {warning}");
            }
        }
    }
    let results = SearchResults::of(&found, now);
    if args.json {
        return print_json(&results);
    }
    finish(
        write_shortlist(
            &mut anstream::stdout().lock(),
            &results,
            &found.refresh.reason,
            found.refresh.report.as_ref(),
            now,
        ),
        "the shortlist",
    )
}

fn verification_line(item: &ShortlistItem, now: DateTime<Utc>) -> String {
    let v = &item.verification;
    let when = v
        .verified_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| jobhunt_jobs::verification::ago(now - t.with_timezone(&Utc)));
    let (mark, style, text) = match (v.state, v.trusted) {
        (VerificationState::VerifiedActive, true) => (
            "✓",
            GOOD,
            format!("Verified open {}", when.unwrap_or_default()),
        ),
        (VerificationState::VerifiedActive, false) => (
            "~",
            OPEN,
            format!(
                "Verified open {} ({})",
                when.unwrap_or_default(),
                v.not_trusted_because.clone().unwrap_or_default()
            ),
        ),
        (VerificationState::CouldNotVerify, _) => ("?", OPEN, "Could not verify yet".to_owned()),
        (VerificationState::NotVerified, _) => ("?", OPEN, "Not verified yet".to_owned()),
        (VerificationState::VerifiedClosed | VerificationState::ClosedByDiscovery, _) => {
            ("✗", crate::rank_render::BAD, "Closed".to_owned())
        }
    };
    let (emark, estyle, elabel) = match item.eligibility.status {
        EligibilityStatus::Eligible => ("✓", GOOD, "Eligible"),
        EligibilityStatus::Conditional => ("~", OPEN, "Eligible if"),
        EligibilityStatus::Uncertain => ("?", OPEN, "Eligibility unclear"),
        EligibilityStatus::Ineligible => ("✗", crate::rank_render::BAD, "Not eligible"),
        EligibilityStatus::NotChecked => ("?", OPEN, "Eligibility not checked"),
    };
    format!(
        "{style}{mark} {text}{style:#} · {estyle}{emark} {elabel}{estyle:#}{DIM}: {}{DIM:#}",
        item.eligibility.headline
    )
}

fn write_shortlist(
    out: &mut impl Write,
    r: &SearchResults,
    reason: &RefreshReason,
    report: Option<&DiscoveryReport>,
    now: DateTime<Utc>,
) -> io::Result<()> {
    let f = &r.funnel;
    writeln!(
        out,
        "Checked {}",
        plural(f.checked as u64, "open job", "open jobs")
    )?;
    writeln!(
        out,
        "{} passed basic eligibility",
        group(f.passed_eligibility)
    )?;
    writeln!(out, "{} looked plausible", group(f.plausible))?;
    writeln!(
        out,
        "{} worth reviewing",
        match f.worth_reviewing {
            1 => "1 is".to_owned(),
            n => format!("{} are", group(n)),
        }
    )?;
    writeln!(out)?;
    if r.results.is_empty() {
        if f.checked == 0 {
            writeln!(
                out,
                "No stored job matches{}.",
                r.query
                    .as_deref()
                    .map(|q| format!(" “{q}”"))
                    .unwrap_or_default()
            )?;
        } else if f.passed_eligibility == 0 {
            writeln!(
                out,
                "Nothing to recommend: every match is ruled out (see below)."
            )?;
        } else {
            writeln!(
                out,
                "No opportunity stands out yet: {} rank as maybe or low priority (--all shows them).",
                plural(
                    (f.passed_eligibility - f.worth_reviewing) as u64,
                    "job",
                    "jobs"
                )
            )?;
        }
        writeln!(out)?;
    }
    for (i, item) in r.results.iter().enumerate() {
        let tier = match item.tier {
            jobhunt_app::views::FitTier::StrongFit => jobhunt_ranking::Tier::StrongFit,
            jobhunt_app::views::FitTier::WorthReviewing => jobhunt_ranking::Tier::WorthReviewing,
            jobhunt_app::views::FitTier::Maybe => jobhunt_ranking::Tier::Maybe,
            jobhunt_app::views::FitTier::LowPriority => jobhunt_ranking::Tier::LowPriority,
        };
        let s = tier_style(tier);
        writeln!(
            out,
            " {DIM}{:>2}.{DIM:#} {TITLE}{}{TITLE:#} — {}   {s}{}{s:#}",
            i + 1,
            item.title,
            item.company,
            tier.label()
        )?;
        writeln!(out, "     {DIM}{}{DIM:#}", item.summary)?;
        writeln!(out, "     {}", verification_line(item, now))?;
        if !item.why.is_empty() {
            writeln!(
                out,
                "     {HEADING}Why this may be worth your time{HEADING:#}"
            )?;
            for line in item.why.iter().take(3) {
                writeln!(out, "       {GOOD}+{GOOD:#} {line}")?;
            }
        }
        if !item.consider.is_empty() {
            writeln!(out, "     {HEADING}Things to consider{HEADING:#}")?;
            for line in &item.consider {
                writeln!(out, "       {OPEN}-{OPEN:#} {line}")?;
            }
        }
        writeln!(
            out,
            "     {DIM}{} · {}{DIM:#}",
            item.short_id, item.next_step
        )?;
        writeln!(out)?;
    }
    let n = &r.not_shown;
    let mut not_shown: Vec<String> = Vec::new();
    let mut add = |count: usize, one: &str, many: &str| {
        if count > 0 {
            not_shown.push(plural(count as u64, one, many));
        }
    };
    add(n.ineligible, "job you can't take", "jobs you can't take");
    add(
        n.below_pay_minimum,
        "job below your pay minimum",
        "jobs below your pay minimum",
    );
    add(
        n.unmet_requirement,
        "job against what you require",
        "jobs against what you require",
    );
    add(
        n.pay_unknown,
        "job without published pay (hidden)",
        "jobs without published pay (hidden)",
    );
    add(
        n.eligibility_unconfirmed,
        "job with unconfirmed eligibility (hidden)",
        "jobs with unconfirmed eligibility (hidden)",
    );
    add(n.rejected, "you rejected", "you rejected");
    add(n.in_pipeline, "in your pipeline", "in your pipeline");
    add(n.closed, "closed", "closed");
    add(
        n.lower_tiers,
        "maybe or low priority (--all)",
        "maybe or low priority (--all)",
    );
    add(n.beyond_limit, "more beyond --limit", "more beyond --limit");
    if !not_shown.is_empty() {
        writeln!(out, "{DIM}Not shown: {}.{DIM:#}", not_shown.join(" · "))?;
    }
    let l = &r.learning;
    let freshness = match report {
        Some(report) => render::run_summary(report),
        None => match reason {
            RefreshReason::Disabled => {
                "Offline: used stored jobs without refreshing or verifying them.".to_owned()
            }
            other => format!(
                "Used stored jobs ({}; --refresh reads the job boards now).",
                other.describe(now)
            ),
        },
    };
    writeln!(out, "{DIM}{freshness}{DIM:#}")?;
    if r.verified_now > 0 {
        writeln!(
            out,
            "{DIM}Verified {} at {} now.{DIM:#}",
            plural(r.verified_now as u64, "listing", "listings"),
            if r.verified_now == 1 {
                "its source"
            } else {
                "their sources"
            }
        )?;
    }
    writeln!(
        out,
        "{DIM}Ranked against your profile{}. Tiers are coarse on purpose; \
         `jobhunt why <id>` shows every reason.{DIM:#}",
        if l.feedback > 0 {
            format!(
                ", {} and {} learned from it",
                plural(l.feedback as u64, "piece of feedback", "pieces of feedback"),
                plural(l.active_patterns as u64, "pattern", "patterns")
            )
        } else {
            String::new()
        }
    )?;
    if !l.has_preferences {
        writeln!(
            out,
            "{DIM}Ranking improves with preferences and feedback: jobhunt preferences add \"…\", \
             then jobhunt save|reject <id> --reason \"…\".{DIM:#}"
        )?;
    }
    out.flush()
}

fn group(n: usize) -> String {
    jobhunt_profile::preferences::group_thousands(n as u64)
}

/// The inventory: every matching stored job, unranked.
async fn raw(
    app: &LocalApp,
    args: &FindArgs,
    verbosity: u8,
    has_profile: bool,
) -> anyhow::Result<ExitCode> {
    let now = jobhunt_app::now();
    let mode = if args.offline {
        RefreshMode::Never
    } else if args.refresh {
        RefreshMode::Always
    } else {
        RefreshMode::Auto
    };
    let refresh = app
        .refresh(mode, &args.sources, &StderrProgress::search(), now)
        .await?;
    if let Some(report) = &refresh.report {
        report_problems(report, &refresh.warnings);
        if verbosity > 0 {
            let _ = render::scan_table(&mut anstream::stderr().lock(), report);
        }
    }
    let store = app.store();
    let limit = args.limit.unwrap_or(20);
    let requested = if args.sources.is_empty() {
        Vec::new()
    } else {
        app.selected_sources(&args.sources)
            .await?
            .iter()
            .map(|s| s.key().clone())
            .collect()
    };
    let mut query = JobQuery {
        sources: requested,
        status: Some(JobStatus::Open),
        distinct_opportunities: true,
        limit: Some(limit),
        ..JobQuery::default()
    }
    .with_text(&args.query.join(" "));
    if let Some(report) = &refresh.report {
        // Only jobs listed at their source during this run.
        query.seen_since = Some(report.started_at);
    }

    let user = app.profile_facts().await?;
    let (records, total, verdicts) = match (args.min_status(), &user) {
        (Some(_), None) => {
            bail!(
                "--eligible and --possible need a career profile: run `jobhunt init <resume>` \
                 or `jobhunt preferences set location <place>`"
            );
        }
        (Some(min), Some(user)) => {
            // Eligibility is not a stored column: read every match, then cut.
            query.limit = None;
            let mut kept = Vec::new();
            let mut verdicts = HashMap::new();
            for record in store.search(&query).await? {
                let a = evaluate_record(&record, user);
                // A job its source says is gone is never offered as a match.
                if a.status >= min && verified_closed(store, &record).await?.is_none() {
                    verdicts.insert(record.id, a);
                    kept.push(record);
                }
            }
            let total = kept.len() as u64;
            kept.truncate(limit);
            (kept, total, verdicts)
        }
        (None, user) => {
            let records = store.search(&query).await?;
            let total = store.count(&query).await?;
            let verdicts = user
                .iter()
                .flat_map(|u| records.iter().map(move |r| (r.id, evaluate_record(r, u))))
                .collect();
            (records, total, verdicts)
        }
    };
    let also_listed = other_listings(store, &records).await?;
    let mut closed = HashMap::new();
    for record in &records {
        if let Some(at) = verified_closed(store, record).await? {
            closed.insert(record.id, at);
        }
    }
    finish(
        print_results(
            &records,
            &also_listed,
            &verdicts,
            &closed,
            total,
            refresh.report.as_ref(),
            args,
            has_profile,
        ),
        "results",
    )
}

/// When the job's latest verification found it closed, if that is newer
/// than discovery's last sighting.
async fn verified_closed(
    store: &dyn Store,
    record: &JobRecord,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    use jobhunt_jobs::verification::ListingStatus;
    Ok(store
        .latest_verification(record.id)
        .await?
        .filter(|v| v.listing == ListingStatus::Closed && v.attempted_at >= record.last_seen_at)
        .map(|v| v.attempted_at))
}

/// Other open source records of each shown opportunity.
async fn other_listings(
    store: &dyn Store,
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

pub(crate) fn report_problems(report: &DiscoveryReport, warnings: &[String]) {
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
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

#[allow(clippy::too_many_arguments)]
fn print_results(
    records: &[JobRecord],
    also_listed: &HashMap<OpportunityId, Vec<JobRecord>>,
    verdicts: &HashMap<JobId, EligibilityDecision>,
    closed: &HashMap<JobId, DateTime<Utc>>,
    total: u64,
    report: Option<&DiscoveryReport>,
    args: &FindArgs,
    has_profile: bool,
) -> io::Result<()> {
    let mut out = anstream::stdout().lock();
    let now = Utc::now();

    if !has_profile {
        writeln!(
            out,
            "No profile found, so these are not personalized. Start with:\n  jobhunt init resume.pdf\n"
        )?;
    }
    if records.is_empty() && total > 0 {
        // `--limit 0`: nothing to list, but say how many matched.
        writeln!(
            out,
            "Showing 0 of {}.",
            plural(total, "matching job", "matching jobs")
        )?;
    } else if records.is_empty() {
        let hint = match (report, args.query.is_empty()) {
            _ if args.eligible => {
                "No matching jobs fit your profile. Try --possible to include the uncertain ones."
            }
            _ if args.possible => "Your profile rules out every matching job.",
            (None, true) if args.offline => {
                "No discovered opportunities yet. Run:\n  jobhunt find --refresh"
            }
            (None, _) => "No stored jobs match. Run with --refresh to read the job boards now.",
            (Some(_), false) => "No jobs match those words. Try fewer or different words.",
            (Some(_), true) => "The sources returned no open jobs.",
        };
        writeln!(out, "{hint}")?;
    } else {
        render::jobs(&mut out, records, also_listed, verdicts, closed, now)?;
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
        None if args.offline => {
            writeln!(out, "Offline: showing stored jobs without refreshing them.")?
        }
        None => writeln!(
            out,
            "Showing stored jobs (read recently; --refresh reads the job boards now)."
        )?,
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_mode_flags() {
        let args = |raw, source: bool, eligible| FindArgs {
            query: Vec::new(),
            limit: None,
            all: false,
            refresh: false,
            offline: false,
            raw,
            sources: if source {
                vec!["ashby:linear".parse().unwrap()]
            } else {
                Vec::new()
            },
            eligible,
            possible: false,
            json: false,
        };
        assert!(!args(false, false, false).raw_mode());
        assert!(args(true, false, false).raw_mode());
        assert!(args(false, true, false).raw_mode());
        assert!(args(false, false, true).raw_mode());
    }
}
