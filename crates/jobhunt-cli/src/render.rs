//! Human-readable output.

use std::collections::HashMap;
use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use jobhunt_eligibility::EligibilityDecision;
use jobhunt_jobs::{
    Compensation, CompensationKind, DiscoveryReport, EmploymentType, JobId, JobRecord,
    OpportunityId, PayInterval, ScanKind, WorkplaceType,
};

pub(crate) const TITLE: Style = Style::new().bold();
pub(crate) const DIM: Style = Style::new().dimmed();
pub(crate) const LINK: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Cyan)));

/// Writes one block per job:
///
/// ```text
///  1. Design Engineer (Web & Brand)
///     Linear · North America (+1 more) · Remote · Full-time
///     ✓ yes: Germany eligible: remote in Europe
///     $180K – $250K • Offers Equity
///     https://jobs.ashbyhq.com/linear/f04f398b-…
///     ashby:linear · posted 9 days ago · job_02e5…
///     also listed on yc:linear
/// ```
///
/// The eligibility line appears when `verdicts` has the job (there is a
/// career profile to check it against). A job whose latest verification
/// found it closed (after discovery last saw it) says so.
///
/// `also_listed` holds, per opportunity, the other sources' records of the
/// same job (cross-source duplicates), which are named on the last line.
pub fn jobs(
    out: &mut impl Write,
    records: &[JobRecord],
    also_listed: &HashMap<OpportunityId, Vec<JobRecord>>,
    verdicts: &HashMap<JobId, EligibilityDecision>,
    closed: &HashMap<JobId, DateTime<Utc>>,
    now: DateTime<Utc>,
) -> io::Result<()> {
    let width = records.len().to_string().len();
    for (index, record) in records.iter().enumerate() {
        let job = &record.posting;
        let indent = " ".repeat(width + 3);
        writeln!(
            out,
            " {DIM}{:>width$}.{DIM:#} {TITLE}{}{TITLE:#}",
            index + 1,
            job.title
        )?;
        writeln!(out, "{indent}{}", details_line(record))?;
        if let Some(at) = closed.get(&record.id) {
            let red = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Red.into()));
            writeln!(
                out,
                "{indent}{red}✗ Verified closed at its source {}{red:#}",
                jobhunt_jobs::verification::ago(now - *at)
            )?;
        }
        if let Some(a) = verdicts.get(&record.id) {
            writeln!(out, "{indent}{}", crate::eligibility::verdict(a))?;
        }
        if let Some(pay) = job.compensation.as_ref().and_then(compensation_text) {
            writeln!(out, "{indent}{pay}")?;
        }
        writeln!(out, "{indent}{LINK}{}{LINK:#}", job.url)?;
        writeln!(out, "{indent}{DIM}{}{DIM:#}", provenance_line(record, now))?;
        if let Some(others) = also_listed.get(&record.opportunity_id) {
            let sources: Vec<String> = others
                .iter()
                .map(|o| o.posting.provenance.source.to_string())
                .collect();
            writeln!(
                out,
                "{indent}{DIM}also listed on {}{DIM:#}",
                sources.join(", ")
            )?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// `Company · Location (+N more) · Workplace · Employment type`, skipping
/// whatever is unknown.
fn details_line(record: &JobRecord) -> String {
    let job = &record.posting;
    let mut parts = vec![job.company.clone()];
    if let Some(location) = &job.location {
        let extra = job.locations.len().saturating_sub(1);
        if extra > 0 {
            parts.push(format!("{location} (+{extra} more)"));
        } else {
            parts.push(location.clone());
        }
    }
    let workplace = match &job.workplace_type {
        Some(workplace) => Some(workplace_label(workplace)),
        None if job.is_remote == Some(true) => Some("Remote"),
        None => None,
    };
    // Skip it when the location already says the same thing ("Remote").
    if let Some(workplace) = workplace
        && job.location.as_deref().map(search_key) != Some(search_key(workplace))
    {
        parts.push(workplace.to_owned());
    }
    if let Some(employment) = &job.employment_type {
        parts.push(employment_label(employment).to_owned());
    }
    parts.join(" · ")
}

fn provenance_line(record: &JobRecord, now: DateTime<Utc>) -> String {
    let source = &record.posting.provenance.source;
    let when = match record.posting.posted_at {
        Some(posted) => format!("posted {}", relative_date(posted, now)),
        None => format!("found {}", relative_date(record.first_seen_at, now)),
    };
    format!("{source} · {when} · {}", record.id)
}

pub fn employment_label(value: &EmploymentType) -> &str {
    match value {
        EmploymentType::FullTime => "Full-time",
        EmploymentType::PartTime => "Part-time",
        EmploymentType::Contract => "Contract",
        EmploymentType::Temporary => "Temporary",
        EmploymentType::Internship => "Internship",
        EmploymentType::Other(raw) => raw,
    }
}

pub fn workplace_label(value: &WorkplaceType) -> &str {
    match value {
        WorkplaceType::Remote => "Remote",
        WorkplaceType::Hybrid => "Hybrid",
        WorkplaceType::OnSite => "On-site",
        WorkplaceType::Other(raw) => raw,
    }
}

/// The source's own summary when it has one; otherwise a salary range built
/// from the structured components.
pub fn compensation_text(comp: &Compensation) -> Option<String> {
    if let Some(summary) = &comp.summary {
        return Some(summary.clone());
    }
    let salaries: Vec<_> = comp
        .components
        .iter()
        .filter(|c| c.kind == CompensationKind::Salary && (c.min.is_some() || c.max.is_some()))
        .collect();
    let salary = salaries.first()?;
    let amount = |v: f64| format_amount(v);
    let range = match (salary.min, salary.max) {
        (Some(min), Some(max)) if min == max => amount(min),
        (Some(min), Some(max)) => format!("{} – {}", amount(min), amount(max)),
        (Some(min), None) => format!("from {}", amount(min)),
        (None, Some(max)) => format!("up to {}", amount(max)),
        (None, None) => return None,
    };
    let currency = salary
        .currency
        .as_deref()
        .map(|c| format!("{c} "))
        .unwrap_or_default();
    let interval = match salary.interval {
        Some(PayInterval::Hour) => " per hour",
        Some(PayInterval::Day) => " per day",
        Some(PayInterval::Week) => " per week",
        Some(PayInterval::Month) => " per month",
        Some(PayInterval::Year) => " per year",
        Some(PayInterval::OneTime) | None => "",
    };
    let label = match &salary.label {
        Some(label) if salaries.len() > 1 => format!(" ({label}; +{} more)", salaries.len() - 1),
        Some(label) => format!(" ({label})"),
        None if salaries.len() > 1 => format!(" (+{} more)", salaries.len() - 1),
        None => String::new(),
    };
    Some(format!("{currency}{range}{interval}{label}"))
}

/// The last line of `find`: what the run did, in lifecycle terms.
///
/// `Checked 14 sources in 3.2s: 2,431 open jobs (12 new, 3 updated, 1 closed).`
pub fn run_summary(report: &DiscoveryReport) -> String {
    let totals = report.totals();
    let open = totals.inserted + totals.updated + totals.unchanged + totals.reopened;
    let mut changes = vec![
        format!("{} new", totals.inserted),
        format!("{} updated", totals.updated),
    ];
    if totals.reopened > 0 {
        changes.push(format!("{} reopened", totals.reopened));
    }
    if totals.closed > 0 {
        changes.push(format!("{} closed", totals.closed));
    }
    let mut line = format!(
        "Checked {} in {:.1}s: {} ({}).",
        plural(report.sources.len() as u64, "source", "sources"),
        report.elapsed.as_secs_f64(),
        plural(open as u64, "open job", "open jobs"),
        changes.join(", ")
    );
    let failed = report.failures().count();
    if failed > 0 {
        line.push_str(&format!(" {failed} failed."));
    }
    if report.dedupe.multi_source_opportunities > 0 {
        line.push_str(&format!(
            " {} listed by more than one source.",
            plural(
                report.dedupe.multi_source_opportunities as u64,
                "job is",
                "jobs are"
            )
        ));
    }
    line
}

/// Per-source statistics of a run, one row per source.
pub fn scan_table(out: &mut impl Write, report: &DiscoveryReport) -> io::Result<()> {
    let width = report
        .sources
        .iter()
        .map(|s| s.source.to_string().len())
        .max()
        .unwrap_or(6)
        .max(6);
    writeln!(
        out,
        "{:<width$}  {:>6}  {:>8}  {:>5}  {:>5}  {:>7}  {:>9}  {:>8}  {:>6}  {:>6}",
        "source",
        "time",
        "received",
        "rej.",
        "new",
        "updated",
        "unchanged",
        "reopened",
        "closed",
        "scan"
    )?;
    for source in &report.sources {
        let time = format!("{:.1}s", source.elapsed.as_secs_f64());
        match &source.result {
            Ok(stats) => {
                let c = &stats.counts;
                let scan = match (stats.kind, stats.closing_withheld) {
                    (ScanKind::Complete, None) => "full",
                    (ScanKind::Complete, Some(_)) => "held",
                    (ScanKind::Partial, _) => "partial",
                    (ScanKind::NotModified, _) => "same",
                };
                writeln!(
                    out,
                    "{:<width$}  {:>6}  {:>8}  {:>5}  {:>5}  {:>7}  {:>9}  {:>8}  {:>6}  {:>6}",
                    source.source.to_string(),
                    time,
                    c.received,
                    c.rejected,
                    c.inserted,
                    c.updated,
                    c.unchanged,
                    c.reopened,
                    c.closed,
                    scan
                )?;
            }
            Err(_) => writeln!(
                out,
                "{:<width$}  {:>6}  failed",
                source.source.to_string(),
                time
            )?,
        }
    }
    let t = report.totals();
    writeln!(
        out,
        "{:<width$}  {:>6}  {:>8}  {:>5}  {:>5}  {:>7}  {:>9}  {:>8}  {:>6}",
        "total",
        format!("{:.1}s", report.elapsed.as_secs_f64()),
        t.received,
        t.rejected,
        t.inserted,
        t.updated,
        t.unchanged,
        t.reopened,
        t.closed
    )?;
    writeln!(
        out,
        "scan: full = complete listing, same = source reported no change, \
         partial = incomplete listing (nothing closed), held = complete but closing withheld"
    )?;
    writeln!(
        out,
        "cross-source: {} links, {} jobs listed by more than one source, {} look-alikes kept \
         apart (same company and title but no shared URL or ATS id)",
        report.dedupe.links, report.dedupe.multi_source_opportunities, report.dedupe.look_alikes
    )
}

/// Formats with thousands separators, keeping up to two decimals when present.
fn format_amount(value: f64) -> String {
    let whole = value.trunc() as i64;
    let digits = whole.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if whole < 0 {
        grouped.insert(0, '-');
    }
    let cents = ((value - value.trunc()).abs() * 100.0).round() as i64;
    if cents > 0 {
        format!("{grouped}.{cents:02}")
    } else {
        grouped
    }
}

pub fn relative_date(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let days = now.signed_duration_since(then).num_days();
    match days {
        d if d < 0 => then.format("%Y-%m-%d").to_string(),
        0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        2..=59 => format!("{days} days ago"),
        _ => then.format("%Y-%m-%d").to_string(),
    }
}

pub fn plural(count: impl Into<u64>, singular: &str, plural: &str) -> String {
    let count = count.into();
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use jobhunt_core::{CanonicalUrl, Provenance, SourceKey};
    use jobhunt_jobs::{CompensationComponent, JobPosting, JobStatus, SourceLocation};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 24, 12, 0, 0).unwrap()
    }

    fn record() -> JobRecord {
        let posting = JobPosting {
            provenance: Provenance {
                source: SourceKey::new("ashby", "linear").unwrap(),
                source_record_id: Some("f04f398b".into()),
                fetched_from: None,
            },
            url: CanonicalUrl::parse("https://jobs.ashbyhq.com/linear/f04f398b").unwrap(),
            apply_url: None,
            company: "Linear".into(),
            title: "Design Engineer (Web & Brand)".into(),
            department: None,
            team: None,
            location: Some("North America".into()),
            locations: vec![
                SourceLocation {
                    name: Some("North America".into()),
                    ..Default::default()
                },
                SourceLocation {
                    name: Some("Europe".into()),
                    ..Default::default()
                },
            ],
            employment_type: Some(EmploymentType::FullTime),
            workplace_type: Some(WorkplaceType::Remote),
            is_remote: Some(true),
            compensation: Some(Compensation {
                summary: Some("$180K – $250K • Offers Equity".into()),
                components: vec![],
            }),
            work_authorization: None,
            description_text: None,
            description_html: None,
            posted_at: Some(Utc.with_ymd_and_hms(2026, 9, 15, 18, 5, 31).unwrap()),
            source_updated_at: None,
        };
        JobRecord {
            id: posting.id(),
            opportunity_id: OpportunityId::founded_by(posting.id()),
            posting,
            first_seen_at: now(),
            last_seen_at: now(),
            content_updated_at: now(),
            status: JobStatus::Open,
            closed_at: None,
        }
    }

    #[test]
    fn renders_a_job_block() {
        let mut out = Vec::new();
        let r = record();
        let id = r.id;
        jobs(
            &mut out,
            &[r],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            now(),
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        // anstyle writes escape codes; strip them for a stable comparison.
        let plain = strip_ansi(&text);
        assert_eq!(
            plain,
            format!(
                " 1. Design Engineer (Web & Brand)\n\
                \x20   Linear · North America (+1 more) · Remote · Full-time\n\
                \x20   $180K – $250K • Offers Equity\n\
                \x20   https://jobs.ashbyhq.com/linear/f04f398b\n\
                \x20   ashby:linear · posted 8 days ago · {id}\n\n"
            )
        );
    }

    #[test]
    fn names_other_sources_of_the_same_job() {
        let r = record();
        let mut twin = record();
        twin.posting.provenance.source = SourceKey::new("yc", "linear").unwrap();
        let also: HashMap<_, _> = [(r.opportunity_id, vec![twin])].into();
        let mut out = Vec::new();
        jobs(
            &mut out,
            &[r],
            &also,
            &HashMap::new(),
            &HashMap::new(),
            now(),
        )
        .unwrap();
        let plain = strip_ansi(&String::from_utf8(out).unwrap());
        assert!(
            plain.contains("\n    also listed on yc:linear\n"),
            "{plain}"
        );
    }

    #[test]
    fn unknown_fields_are_left_out() {
        let mut r = record();
        r.posting.location = None;
        r.posting.locations.clear();
        r.posting.workplace_type = None;
        r.posting.is_remote = None;
        r.posting.employment_type = None;
        r.posting.compensation = None;
        r.posting.posted_at = None;
        assert_eq!(details_line(&r), "Linear");
        assert_eq!(
            provenance_line(&r, now()),
            format!("ashby:linear · found today · {}", r.id)
        );
    }

    #[test]
    fn does_not_repeat_remote() {
        let mut r = record();
        r.posting.location = Some("Remote".into());
        r.posting.locations.truncate(1);
        assert_eq!(details_line(&r), "Linear · Remote · Full-time");
    }

    #[test]
    fn compensation_falls_back_to_structured_salary() {
        let comp = Compensation {
            summary: None,
            components: vec![
                CompensationComponent {
                    kind: CompensationKind::EquityPercentage,
                    label: None,
                    currency: None,
                    min: None,
                    max: None,
                    interval: None,
                },
                CompensationComponent {
                    kind: CompensationKind::Salary,
                    label: None,
                    currency: Some("USD".into()),
                    min: Some(211_400.0),
                    max: Some(290_600.5),
                    interval: Some(PayInterval::Year),
                },
            ],
        };
        assert_eq!(
            compensation_text(&comp).as_deref(),
            Some("USD 211,400 – 290,600.50 per year")
        );
        let empty = Compensation {
            summary: None,
            components: vec![],
        };
        assert_eq!(compensation_text(&empty), None);

        let range = |label: &str, currency: &str| CompensationComponent {
            kind: CompensationKind::Salary,
            label: Some(label.into()),
            currency: Some(currency.into()),
            min: Some(100.0),
            max: Some(200.0),
            interval: Some(PayInterval::Year),
        };
        let regional = Compensation {
            summary: None,
            components: vec![
                range("US Annual Pay Range", "USD"),
                range("Canada Annual Pay Range", "CAD"),
            ],
        };
        assert_eq!(
            compensation_text(&regional).as_deref(),
            Some("USD 100 – 200 per year (US Annual Pay Range; +1 more)")
        );
    }

    #[test]
    fn relative_dates() {
        let now = now();
        assert_eq!(relative_date(now, now), "today");
        assert_eq!(
            relative_date(now - chrono::Duration::days(1), now),
            "yesterday"
        );
        assert_eq!(
            relative_date(now - chrono::Duration::days(12), now),
            "12 days ago"
        );
        assert_eq!(
            relative_date(Utc.with_ymd_and_hms(2021, 4, 27, 0, 0, 0).unwrap(), now),
            "2021-04-27"
        );
    }

    #[test]
    fn formats_amounts_and_plurals() {
        assert_eq!(format_amount(1_234_567.0), "1,234,567");
        assert_eq!(format_amount(999.0), "999");
        assert_eq!(format_amount(45.5), "45.50");
        assert_eq!(plural(1u64, "job", "jobs"), "1 job");
        assert_eq!(plural(3u64, "job", "jobs"), "3 jobs");
    }

    pub(crate) fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
