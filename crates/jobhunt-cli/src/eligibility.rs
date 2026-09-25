//! Verification and eligibility output shared by `verify`, `check`, `show`
//! and `find`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use chrono::{DateTime, Utc};
use jobhunt_eligibility::decision::{Eligibility, EligibilityDecision, Reason, Verdict};
use jobhunt_eligibility::describe::{self, Fact};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Assessment, JobRequirements};
use jobhunt_jobs::JobRecord;
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationStatus, Authority, CompensationChange, CompensationStatus,
    OpportunityTrust, RecordTrust, Standing, TrustState, VerificationAge, VerificationRecord, ago,
};
use jobhunt_profile::ProfileService;
use jobhunt_storage::SqliteJobStore;

use crate::render::{DIM, compensation_text};

const GOOD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
const OPEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
const BAD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
const HEADING: Style = Style::new().bold();

/// The person's facts, or `None` when there is no profile yet.
pub async fn profile_facts(store: &SqliteJobStore) -> anyhow::Result<Option<ProfileFacts>> {
    let data = ProfileService::new(store).load().await?;
    Ok(data.as_ref().map(ProfileFacts::from_profile))
}

fn style(status: Eligibility) -> Style {
    match status {
        Eligibility::Eligible => GOOD,
        Eligibility::Conditional | Eligibility::Uncertain => OPEN,
        Eligibility::Ineligible => BAD,
    }
}

fn marker(status: Eligibility) -> &'static str {
    match status {
        Eligibility::Eligible => "✓",
        Eligibility::Conditional => "~",
        Eligibility::Uncertain => "?",
        Eligibility::Ineligible => "✗",
    }
}

fn verdict_marker(verdict: Verdict) -> (&'static str, Style) {
    match verdict {
        Verdict::Pass => ("✓", GOOD),
        Verdict::Conditional => ("~", OPEN),
        Verdict::Unknown => ("?", OPEN),
        Verdict::Fail => ("✗", BAD),
        Verdict::NotApplicable => ("•", Style::new()),
    }
}

/// `✓ ELIGIBLE: Brazil is within the listed Americas region`
pub fn verdict(d: &EligibilityDecision) -> String {
    let s = style(d.status);
    format!(
        "{s}{} {}{s:#}: {}",
        marker(d.status),
        d.status.label(),
        d.headline
    )
}

fn heading(out: &mut impl Write, text: &str) -> io::Result<()> {
    writeln!(out, "{HEADING}{text}{HEADING:#}")
}

fn family(kind: &str) -> &'static str {
    match kind {
        "ashby" => "Ashby",
        "greenhouse" => "Greenhouse",
        "lever" => "Lever",
        "yc" => "Work at a Startup",
        _ => "an unknown source",
    }
}

/// "First-party Ashby listing", "Work at a Startup listing (a platform the
/// employer posts to itself)".
fn listing_name(r: &RecordTrust) -> String {
    let kind = family(r.source.kind());
    match r.authority {
        Authority::EmployerFirstParty => {
            format!("First-party listing on the employer's own site (via {kind})")
        }
        Authority::EmployerConfiguredAts => format!("First-party {kind} listing"),
        Authority::TrustedSource => {
            format!("{kind} listing (a platform the employer posts to itself, not its own site)")
        }
        Authority::SecondarySource => format!("Secondary listing on {kind} (a reposting)"),
        Authority::Unknown => format!("Listing on {} (authority unknown)", r.source),
    }
}

fn failure_text(v: &VerificationRecord) -> String {
    v.failure
        .as_ref()
        .map(|f| f.detail.clone())
        .unwrap_or_else(|| v.listing.as_str().to_owned())
}

/// The trust state of one record as one line.
fn record_line(r: &RecordTrust) -> (String, Style) {
    let name = listing_name(r);
    match r.state {
        TrustState::VerifiedActive => (format!("✓ {name} active"), GOOD),
        TrustState::VerifiedClosed => (
            format!(
                "✗ {name} closed: {}",
                r.latest
                    .as_ref()
                    .and_then(|v| v
                        .authority_chain
                        .iter()
                        .rev()
                        .find(|l| l.note.starts_with("no longer")))
                    .map_or_else(|| "not found at the source".to_owned(), |l| l.note.clone())
            ),
            BAD,
        ),
        TrustState::ClosedByDiscovery => (
            format!("✗ {name} closed: missing from the source's last complete listing"),
            BAD,
        ),
        TrustState::CouldNotVerify => (
            format!(
                "! Could not verify the {}: {}",
                name.to_lowercase()
                    .trim_start_matches("first-party ")
                    .to_owned(),
                r.latest.as_ref().map(failure_text).unwrap_or_default()
            ),
            OPEN,
        ),
        TrustState::NotVerified => (format!("? {name}: not verified yet"), OPEN),
    }
}

fn application_line(v: &VerificationRecord) -> (String, Style) {
    let a = &v.application;
    let how = match a.basis {
        ApplicationBasis::Probed => match a.http_status {
            Some(status) => format!(" (checked: HTTP {status})"),
            None => " (checked)".to_owned(),
        },
        ApplicationBasis::Published => " (published by the board's API)".to_owned(),
        ApplicationBasis::ListingPage => " (on the listing page)".to_owned(),
        ApplicationBasis::NotChecked => String::new(),
    };
    match a.status {
        ApplicationStatus::Active => (format!("✓ Application path active{how}"), GOOD),
        ApplicationStatus::Closed => (
            format!(
                "✗ Application path closed{}",
                a.detail
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
            BAD,
        ),
        ApplicationStatus::Unavailable => (
            format!(
                "! Application path could not be checked{}",
                a.detail
                    .as_deref()
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default()
            ),
            OPEN,
        ),
        ApplicationStatus::Unknown => (
            format!(
                "? Application path unknown{}",
                a.detail
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
            OPEN,
        ),
    }
}

/// "Verified 12 seconds ago", "Could not verify just now; last successfully
/// verified 3 days ago".
fn age_line(r: &RecordTrust, now: DateTime<Utc>) -> String {
    let success = r.last_success.as_ref().map(|v| ago(now - v.attempted_at));
    let stale = match r.age {
        Some(VerificationAge::Stale) => " (stale: verify again)",
        Some(VerificationAge::Aging) => " (aging)",
        _ => "",
    };
    match (&r.latest, success) {
        (None, _) => format!(
            "Never verified; discovery last saw it {}",
            ago(now - r.last_seen_at)
        ),
        (Some(latest), Some(success)) if latest.succeeded() => {
            format!("Verified {success}{stale}")
        }
        (Some(latest), Some(success)) => format!(
            "Last attempt {} failed; last successfully verified {success}{stale}",
            ago(now - latest.attempted_at)
        ),
        (Some(latest), None) => format!(
            "Last attempt {} failed; never successfully verified",
            ago(now - latest.attempted_at)
        ),
    }
}

/// The verification section.
pub fn verification(
    out: &mut impl Write,
    trust: &OpportunityTrust,
    now: DateTime<Utc>,
    indent: &str,
    detail: bool,
) -> io::Result<()> {
    let Some(best) = trust.best() else {
        return writeln!(out, "{indent}No source records");
    };
    let (line, s) = record_line(best);
    writeln!(out, "{indent}{s}{line}{s:#}")?;
    if let Some(latest) = best
        .latest
        .as_ref()
        .filter(|v| v.succeeded() && best.state == TrustState::VerifiedActive)
    {
        let (line, s) = application_line(latest);
        writeln!(out, "{indent}{s}{line}{s:#}")?;
    }
    writeln!(out, "{indent}{}", age_line(best, now))?;
    if let Standing::NotTrusted(why) = &trust.standing {
        writeln!(
            out,
            "{indent}{OPEN}Not trusted enough to recommend: {why}{OPEN:#}"
        )?;
    }
    for c in &trust.conflicts {
        writeln!(out, "{indent}{OPEN}! {c}{OPEN:#}")?;
    }
    for (i, r) in trust.records.iter().enumerate() {
        if Some(i) == trust.best {
            continue;
        }
        let (line, _) = record_line(r);
        writeln!(out, "{indent}{DIM}Also: {} — {line}{DIM:#}", r.source)?;
    }
    if detail {
        for r in &trust.records {
            record_details(out, r, now, &format!("{indent}  "))?;
        }
    }
    Ok(())
}

fn record_details(
    out: &mut impl Write,
    r: &RecordTrust,
    now: DateTime<Utc>,
    indent: &str,
) -> io::Result<()> {
    writeln!(
        out,
        "{indent}{} · {} · {}",
        r.source,
        r.job_id,
        r.authority.as_str()
    )?;
    let Some(v) = &r.latest else {
        return writeln!(out, "{indent}  {DIM}not verified{DIM:#}");
    };
    writeln!(
        out,
        "{indent}  {DIM}{} · attempted {} ({}){DIM:#}",
        v.method.as_str(),
        v.attempted_at.format("%Y-%m-%d %H:%M:%S UTC"),
        ago(now - v.attempted_at)
    )?;
    if let Some(url) = &v.checked_url {
        writeln!(out, "{indent}  {DIM}checked {url}{DIM:#}")?;
    }
    for link in &v.authority_chain {
        writeln!(
            out,
            "{indent}  {DIM}{} {}: {} ({}){DIM:#}",
            if link.checked { "→" } else { "·" },
            link.kind.as_str(),
            link.url,
            link.note
        )?;
    }
    if let Some(lifecycle) = &v.lifecycle {
        writeln!(out, "{indent}  {DIM}stored record: {lifecycle}{DIM:#}")?;
    }
    if !v.changed_fields.is_empty() {
        writeln!(
            out,
            "{indent}  {DIM}changed since the stored version: {}{DIM:#}",
            v.changed_fields.join(", ")
        )?;
    }
    match v.changed_since_last_verification {
        Some(true) => writeln!(
            out,
            "{indent}  {DIM}changed since the previous verification{DIM:#}"
        )?,
        Some(false) => writeln!(
            out,
            "{indent}  {DIM}unchanged since the previous verification{DIM:#}"
        )?,
        None => {}
    }
    for u in &v.unknowns {
        writeln!(out, "{indent}  {DIM}unknown: {u}{DIM:#}")?;
    }
    Ok(())
}

fn facts(out: &mut impl Write, facts: &[Fact], indent: &str) -> io::Result<()> {
    for f in facts {
        writeln!(out, "{indent}{}: {}", f.label, f.value)?;
    }
    Ok(())
}

/// Location facts, and where the person is.
pub fn location(
    out: &mut impl Write,
    job: &JobRequirements,
    profile: Option<&ProfileFacts>,
    indent: &str,
) -> io::Result<()> {
    facts(out, &describe::location(job), indent)?;
    if let Some(p) = profile {
        match &p.location {
            Some(l) => {
                let mut notes: Vec<String> = Vec::new();
                match l.area {
                    Some(a) if !a.to_string().eq_ignore_ascii_case(l.raw.trim()) => {
                        notes.push(format!("read as {a}"));
                    }
                    Some(_) => {}
                    None => notes.push("not recognized".to_owned()),
                }
                if l.basis == jobhunt_eligibility::profile::FactBasis::Resume {
                    notes.push("from your resume".to_owned());
                }
                let notes = if notes.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", notes.join(", "))
                };
                writeln!(out, "{indent}{DIM}You: {}{notes}{DIM:#}", l.raw)?;
            }
            None => writeln!(out, "{indent}{DIM}You: location not set{DIM:#}")?,
        }
    }
    Ok(())
}

/// Employment and time-zone facts.
pub fn employment(out: &mut impl Write, job: &JobRequirements, indent: &str) -> io::Result<()> {
    facts(out, &describe::employment(job), indent)?;
    facts(out, &describe::timezone(job), indent)
}

/// Compensation as verified (or, when not verified, as discovered).
pub fn compensation(
    out: &mut impl Write,
    trust: &OpportunityTrust,
    record: &JobRecord,
    indent: &str,
) -> io::Result<()> {
    let verified = trust
        .best()
        .filter(|r| r.state == TrustState::VerifiedActive)
        .and_then(|r| r.latest.as_ref());
    let Some(v) = verified else {
        return match record
            .posting
            .compensation
            .as_ref()
            .and_then(compensation_text)
        {
            Some(text) => writeln!(
                out,
                "{indent}{text} {DIM}(from discovery; not verified){DIM:#}"
            ),
            None => writeln!(out, "{indent}Not published {DIM}(not verified){DIM:#}"),
        };
    };
    let c = &v.compensation;
    match c.status {
        CompensationStatus::Published => {
            if c.ranges.is_empty()
                && let Some(summary) = &c.summary
            {
                writeln!(out, "{indent}{summary}")?;
            }
            for r in &c.ranges {
                writeln!(out, "{indent}{}", r.describe())?;
            }
        }
        CompensationStatus::NotPublished => writeln!(out, "{indent}Not published")?,
        CompensationStatus::NotObserved => writeln!(out, "{indent}Not observed")?,
    }
    let change = match c.change {
        CompensationChange::FirstVerification => "first verification",
        CompensationChange::Unchanged => "unchanged since the previous verification",
        CompensationChange::Changed => "changed since the previous verification",
        CompensationChange::NewlyPublished => "newly published",
        CompensationChange::Removed => "removed since the previous verification",
        CompensationChange::NotObserved => "not observed",
    };
    let from = if v.authority.is_employer() {
        "the first-party listing"
    } else {
        "a listing that isn't the employer's own"
    };
    writeln!(out, "{indent}{DIM}Verified from {from} · {change}{DIM:#}")?;
    if let Some(previous) = c.previous.as_ref().and_then(compensation_text) {
        writeln!(out, "{indent}{DIM}Previously: {previous}{DIM:#}")?;
    }
    if c.ambiguous_currency {
        writeln!(
            out,
            "{indent}{DIM}The currency is only a symbol several currencies share; JobHunt doesn't guess it{DIM:#}"
        )?;
    }
    Ok(())
}

fn reason_line(out: &mut impl Write, r: &Reason, indent: &str, detail: bool) -> io::Result<()> {
    let (m, s) = verdict_marker(r.verdict);
    writeln!(out, "{indent}{s}{m}{s:#} {}", r.conclusion)?;
    if !detail {
        return Ok(());
    }
    for e in &r.evidence {
        writeln!(
            out,
            "{indent}    {DIM}“{}” ({} {}){DIM:#}",
            clip(&e.text, 160),
            e.source,
            e.field
        )?;
    }
    for f in &r.profile {
        writeln!(
            out,
            "{indent}    {DIM}your {}: {} ({}){DIM:#}",
            f.what, f.value, f.basis
        )?;
    }
    Ok(())
}

/// The eligibility section: the status and why.
pub fn eligibility(
    out: &mut impl Write,
    assessment: &Assessment,
    indent: &str,
    detail: bool,
) -> io::Result<()> {
    let d = &assessment.decision;
    let s = style(d.status);
    writeln!(out, "{indent}{s}{}{s:#}", d.status.label())?;
    if let Some(option) = &d.option
        && (detail || !d.other_options.is_empty())
    {
        writeln!(out, "{indent}{DIM}For: {option}{DIM:#}")?;
    }
    writeln!(out)?;
    heading(out, "Why")?;
    if !assessment.recommendable() {
        let gate = assessment.listing_reason();
        if gate.verdict != Verdict::Pass {
            reason_line(out, &gate, indent, false)?;
        }
    }
    for r in &d.reasons {
        reason_line(out, r, indent, detail)?;
    }
    if !d.other_options.is_empty() {
        writeln!(out)?;
        heading(out, "Other ways to do this job")?;
        for o in &d.other_options {
            let s = style(o.status);
            writeln!(
                out,
                "{indent}{s}{} {}{s:#} {}: {}",
                marker(o.status),
                o.status.label(),
                o.option,
                o.headline()
            )?;
            if detail {
                for r in &o.reasons {
                    reason_line(out, r, &format!("{indent}    "), true)?;
                }
            }
        }
    }
    if !d.conflicts.is_empty() {
        writeln!(out)?;
        heading(out, "Conflicting information")?;
        for c in &d.conflicts {
            writeln!(out, "{indent}{OPEN}!{OPEN:#} {}", c.summary)?;
            if detail {
                for e in &c.evidence {
                    writeln!(
                        out,
                        "{indent}    {DIM}“{}” ({} {}){DIM:#}",
                        clip(&e.text, 160),
                        e.source,
                        e.field
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub fn section(out: &mut impl Write, name: &str) -> io::Result<()> {
    heading(out, name)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}
