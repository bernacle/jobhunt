//! Eligibility output shared by `check`, `show` and `find`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use jobhunt_eligibility::{Assessment, Fit, UserConstraints, Verification, assess_record};
use jobhunt_jobs::JobRecord;
use jobhunt_profile::ProfileService;
use jobhunt_storage::SqliteJobStore;

use crate::render::DIM;

const GOOD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
const OPEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
const BAD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));

/// The user's constraints, or `None` when there is no profile yet.
pub async fn user_constraints(store: &SqliteJobStore) -> anyhow::Result<Option<UserConstraints>> {
    let data = ProfileService::new(store).load().await?;
    Ok(data.as_ref().map(UserConstraints::from_profile))
}

/// The best assessment across a job's records (the first wins ties), with
/// the index of the record it is about.
pub fn best(records: &[JobRecord], user: &UserConstraints) -> Option<(usize, Assessment)> {
    let mut best: Option<(usize, Assessment)> = None;
    for (i, record) in records.iter().enumerate() {
        let a = assess_record(record, user);
        if best.as_ref().is_none_or(|(_, b)| a.fit > b.fit) {
            best = Some((i, a));
        }
    }
    best
}

fn style(fit: Fit) -> Style {
    match fit {
        Fit::Yes | Fit::Likely => GOOD,
        Fit::Unknown => OPEN,
        Fit::Unlikely | Fit::No => BAD,
    }
}

fn marker(fit: Fit) -> &'static str {
    match fit {
        Fit::Yes => "✓",
        Fit::Likely => "~",
        Fit::Unknown => "?",
        Fit::Unlikely => "!",
        Fit::No => "✗",
    }
}

/// `✓ yes: Brazil eligible: remote in Latin America`
pub fn verdict(a: &Assessment) -> String {
    let s = style(a.fit);
    format!("{s}{} {}{s:#}: {}", marker(a.fit), a.fit, a.headline())
}

/// One line per check; with `detail`, notes and evidence under each.
pub fn checks(out: &mut impl Write, a: &Assessment, indent: &str, detail: bool) -> io::Result<()> {
    let width = a
        .checks
        .iter()
        .map(|c| c.dimension.label().len())
        .max()
        .unwrap_or(0);
    for c in &a.checks {
        let s = style(c.fit);
        writeln!(
            out,
            "{indent}{s}{} {:<8}{s:#} {:<width$}  {}",
            marker(c.fit),
            c.fit.label(),
            c.dimension.label(),
            c.summary
        )?;
        if !detail {
            continue;
        }
        let pad = format!("{indent}{}", " ".repeat(11 + width + 2));
        for note in &c.notes {
            writeln!(out, "{pad}{note}")?;
        }
        for e in &c.evidence {
            writeln!(
                out,
                "{pad}{DIM}“{}” ({} {}){DIM:#}",
                clip(&e.text, 160),
                e.source,
                e.field
            )?;
        }
    }
    Ok(())
}

/// The verification summary and one line per record.
pub fn verification(out: &mut impl Write, v: &Verification, indent: &str) -> io::Result<()> {
    let s = if v.first_party() && v.freshness() >= jobhunt_eligibility::Freshness::Aging {
        GOOD
    } else {
        OPEN
    };
    writeln!(out, "{indent}{s}{}{s:#}", v.summary())?;
    if v.sources.len() > 1 {
        for source in &v.sources {
            writeln!(
                out,
                "{indent}  {DIM}{} · {} · {} · last seen {}{DIM:#}",
                source.source,
                source.status.as_str(),
                source.freshness,
                jobhunt_eligibility::verify::ago(v.now - source.last_seen_at)
            )?;
        }
    }
    Ok(())
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}
