//! Human-readable output for the profile commands.
//!
//! Everything writes to an `impl Write` so it can be tested; ids are
//! shortened to their prefix plus 8 hex characters (any unique prefix is
//! accepted back on the command line).

use std::fmt::Display;
use std::io::{self, Write};

use jobhunt_profile::{
    Certainty, Claim, ClaimKind, EmploymentKind, EvidenceStrength, Experience, ImportReport,
    LastSeen, Origin, PartialDate, Period, Preference, PreferenceCategory, PreferenceOrigin,
    PreferenceStatement, ProfileData, ProfileEvent, RecordMeta, Stance, Standing, StatementOutcome,
    StatementReading, Subject, Tally, Verification,
};

use crate::render::{DIM, TITLE, plural};

/// `exp_1a2b3c4d` for `exp_1a2b3c4d…` (prefix + 8 hex).
pub fn short(id: impl Display) -> String {
    let text = id.to_string();
    match text.find('_') {
        Some(at) => text.chars().take(at + 1 + 8).collect(),
        None => text,
    }
}

fn period(p: Period) -> String {
    p.display().unwrap_or_else(|| "dates unknown".to_owned())
}

fn experience_line(e: &Experience) -> String {
    let who = match (&e.company, &e.title) {
        (Some(c), Some(t)) => format!("{c} — {t}"),
        (None, Some(t)) if e.employment == Some(EmploymentKind::Freelance) => {
            format!("{t} (freelance)")
        }
        (None, Some(t)) => t.clone(),
        (Some(c), None) => c.clone(),
        (None, None) => "(untitled)".into(),
    };
    let mut parts = vec![who, period(e.period())];
    if let Some(location) = &e.location {
        parts.push(location.clone());
    }
    parts.join(" · ")
}

/// A label for what a claim is about.
pub fn subject_label(data: &ProfileData, subject: Subject) -> String {
    match subject {
        Subject::Profile => "Profile".to_owned(),
        Subject::Experience(id) => data
            .experience(id)
            .map_or_else(|| short(id), experience_line),
        Subject::Project(id) => data
            .projects
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| short(id), |p| format!("Project: {}", p.name)),
        Subject::Education(id) => data
            .education
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| short(id), |e| format!("Education: {}", e.institution)),
    }
}

fn record_flags(meta: &RecordMeta) -> String {
    let mut flags = Vec::new();
    if meta.origin == Origin::User {
        flags.push("added by you".to_owned());
    }
    if meta.verification == Verification::Confirmed && meta.origin == Origin::Resume {
        flags.push("confirmed".to_owned());
    }
    if !meta.edited_fields.is_empty() {
        flags.push(format!("edited: {}", meta.edited_fields.join(", ")));
    }
    if meta.is_stale() {
        flags.push("not in your latest resume".to_owned());
    }
    if flags.is_empty() {
        String::new()
    } else {
        format!(" ({})", flags.join("; "))
    }
}

fn marker(standing: Standing) -> &'static str {
    match standing {
        Standing::Usable(_) => "✓",
        Standing::NeedsReview(_) => "?",
        Standing::Rejected => "✗",
    }
}

/// The overview printed by `narrow profile`.
pub fn profile(out: &mut impl Write, data: &ProfileData, all: bool) -> io::Result<()> {
    let p = &data.profile;
    let name = p.name.as_deref().unwrap_or("Your profile");
    match &p.headline {
        Some(headline) => writeln!(out, "{TITLE}{name}{TITLE:#} — {headline}")?,
        None => writeln!(out, "{TITLE}{name}{TITLE:#}")?,
    }
    let mut about = Vec::new();
    if let Some(location) = &p.location {
        about.push(location.clone());
    }
    if let Some(doc) = data.latest_document() {
        about.push(format!(
            "from {} (imported {})",
            doc.file_name.as_deref().unwrap_or("resume"),
            doc.last_imported_at.format("%Y-%m-%d")
        ));
    }
    if !about.is_empty() {
        writeln!(out, "{DIM}{}{DIM:#}", about.join(" · "))?;
    }

    section(out, "Experience")?;
    let experiences = data.visible_experiences();
    if experiences.is_empty() {
        writeln!(out, "  none yet")?;
    }
    for e in experiences {
        writeln!(out, "  {}{}", experience_line(e), record_flags(&e.meta))?;
        let subject = Subject::Experience(e.id);
        let bullets = data.claims_about(
            subject,
            &[ClaimKind::Accomplishment, ClaimKind::Responsibility],
        );
        let techs = data.technologies_of(subject);
        let mut details = vec![short(e.id)];
        if !bullets.is_empty() {
            details.push(plural(bullets.len() as u64, "bullet", "bullets"));
        }
        if !techs.is_empty() {
            details.push(techs.join(", "));
        }
        writeln!(out, "    {DIM}{}{DIM:#}", details.join(" · "))?;
        if all {
            for claim in bullets {
                writeln!(out, "    - {}", claim.text)?;
            }
        }
    }

    let projects = data.visible_projects();
    if !projects.is_empty() {
        section(out, "Projects")?;
        for x in projects {
            let mut line = x.name.clone();
            if let Some(description) = &x.description {
                line.push_str(&format!(" — {description}"));
            }
            line.push_str(&format!(" · {}", period(x.period())));
            writeln!(out, "  {line}{}", record_flags(&x.meta))?;
            let techs = data.technologies_of(Subject::Project(x.id));
            let mut details = vec![short(x.id)];
            if !techs.is_empty() {
                details.push(techs.join(", "));
            }
            writeln!(out, "    {DIM}{}{DIM:#}", details.join(" · "))?;
        }
    }

    let education = data.visible_education();
    if !education.is_empty() {
        section(out, "Education")?;
        for x in education {
            let what = match (&x.degree, &x.field) {
                (Some(d), Some(f)) => format!("{} — {d} in {f}", x.institution),
                (Some(d), None) => format!("{} — {d}", x.institution),
                (None, Some(f)) => format!("{} — {f}", x.institution),
                (None, None) => x.institution.clone(),
            };
            writeln!(
                out,
                "  {what} · {}{}  {DIM}{}{DIM:#}",
                period(x.period()),
                record_flags(&x.meta),
                short(x.id)
            )?;
        }
    }

    section(out, "Skills")?;
    let skills = data.skills_with_evidence();
    if skills.is_empty() {
        writeln!(out, "  none yet")?;
    }
    for (strength, label) in [
        (EvidenceStrength::Demonstrated, "used in your work"),
        (EvidenceStrength::UserStated, "added by you"),
        (EvidenceStrength::Listed, "listed only"),
    ] {
        let names: Vec<String> = skills
            .iter()
            .filter(|s| s.strength == strength)
            .map(|s| match (all, s.last_seen) {
                (true, Some(LastSeen::Current)) => format!("{} (now)", s.skill.name),
                (true, Some(LastSeen::At(d))) => format!("{} ({})", s.skill.name, d.year()),
                _ => s.skill.name.clone(),
            })
            .collect();
        if !names.is_empty() {
            writeln!(out, "  {label}: {}", names.join(", "))?;
        }
    }

    let domains = data.domains();
    if !domains.is_empty() {
        section(out, "Domains")?;
        let items: Vec<String> = domains
            .iter()
            .map(|d| {
                let confirmed = d
                    .claims
                    .iter()
                    .any(|c| c.verification == Verification::Confirmed);
                format!(
                    "{}{} ({})",
                    d.domain,
                    if confirmed { " ✓" } else { "" },
                    d.claims.len()
                )
            })
            .collect();
        writeln!(out, "  {}", items.join(", "))?;
        writeln!(
            out,
            "  {DIM}inferred from your resume; ✓ = confirmed by you{DIM:#}"
        )?;
    }
    let roles = data.signals(ClaimKind::Role);
    let ownership = data.signals(ClaimKind::Ownership);
    if !roles.is_empty() || !ownership.is_empty() {
        section(out, "Role signals")?;
        let fmt = |signals: &[(String, Vec<&Claim>)]| -> String {
            signals
                .iter()
                .map(|(topic, claims)| {
                    let confirmed = claims
                        .iter()
                        .any(|c| c.verification == Verification::Confirmed);
                    format!(
                        "{topic}{} ({})",
                        if confirmed { " ✓" } else { "" },
                        claims.len()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !roles.is_empty() {
            writeln!(out, "  kinds of work: {}", fmt(&roles))?;
        }
        if !ownership.is_empty() {
            writeln!(out, "  seniority and ownership: {}", fmt(&ownership))?;
        }
    }

    section(out, "Preferences")?;
    let active: Vec<&Preference> = data.preferences().active().collect();
    if active.is_empty() {
        writeln!(out, "  none yet")?;
    } else {
        for category in CATEGORIES {
            let items: Vec<String> = data
                .preferences()
                .in_category(category)
                .into_iter()
                .map(preference_short)
                .collect();
            if !items.is_empty() {
                writeln!(out, "  {}: {}", category_label(category), items.join("; "))?;
            }
        }
    }

    section(out, "Evidence")?;
    evidence_counts(out, data)?;

    let gaps = data.gaps();
    if !gaps.is_empty() {
        section(out, "Missing or uncertain")?;
        for gap in gaps.iter().take(if all { usize::MAX } else { 12 }) {
            writeln!(out, "  - {}", gap.message)?;
        }
        if !all && gaps.len() > 12 {
            writeln!(out, "  … {} more (narrow profile --all)", gaps.len() - 12)?;
        }
    }
    Ok(())
}

fn section(out: &mut impl Write, title: &str) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "{TITLE}{title}{TITLE:#}")
}

fn evidence_counts(out: &mut impl Write, data: &ProfileData) -> io::Result<()> {
    let total = data.claims.len();
    let (mut grounded, mut confirmed, mut user, mut review, mut rejected, mut stale) =
        (0, 0, 0, 0, 0, 0);
    for claim in &data.claims {
        match data.standing(claim) {
            Standing::Usable(jobhunt_profile::UsableBecause::Grounded) => grounded += 1,
            Standing::Usable(jobhunt_profile::UsableBecause::Confirmed) => confirmed += 1,
            Standing::Usable(jobhunt_profile::UsableBecause::UserEntered) => user += 1,
            Standing::NeedsReview(jobhunt_profile::ReviewReason::SourceRemoved) => {
                review += 1;
                stale += 1;
            }
            Standing::NeedsReview(_) => review += 1,
            Standing::Rejected => rejected += 1,
        }
    }
    writeln!(out, "  {}", plural(total as u64, "claim", "claims"))?;
    writeln!(out, "  {grounded} directly supported by your resume")?;
    if confirmed + user > 0 {
        writeln!(out, "  {} confirmed or entered by you", confirmed + user)?;
    }
    writeln!(out, "  {review} need review")?;
    if stale > 0 {
        writeln!(out, "  {stale} no longer in your latest resume")?;
    }
    if rejected > 0 {
        writeln!(out, "  {rejected} rejected")?;
    }
    Ok(())
}

const CATEGORIES: [PreferenceCategory; 6] = [
    PreferenceCategory::Role,
    PreferenceCategory::Compensation,
    PreferenceCategory::Location,
    PreferenceCategory::Company,
    PreferenceCategory::Domain,
    PreferenceCategory::WorkStyle,
];

fn category_label(category: PreferenceCategory) -> &'static str {
    match category {
        PreferenceCategory::Role => "Roles",
        PreferenceCategory::Compensation => "Compensation",
        PreferenceCategory::Location => "Location",
        PreferenceCategory::Company => "Company and team",
        PreferenceCategory::Domain => "Domains",
        PreferenceCategory::WorkStyle => "Work style",
    }
}

fn stance_word(stance: Stance) -> &'static str {
    match stance {
        Stance::Required => "must",
        Stance::Wanted => "want",
        Stance::Acceptable => "ok",
        Stance::Unwanted => "avoid",
    }
}

fn preference_short(p: &Preference) -> String {
    let doubt = if p.certainty == Certainty::Uncertain {
        " (?)"
    } else {
        ""
    };
    match p.value.category() {
        PreferenceCategory::Compensation | PreferenceCategory::Location
            if matches!(p.stance, Stance::Required | Stance::Wanted) =>
        {
            format!("{}{doubt}", p.value)
        }
        _ => format!("{} {}{doubt}", stance_word(p.stance), p.value),
    }
}

/// The summary printed by `narrow init`.
pub fn import_summary(
    out: &mut impl Write,
    data: &ProfileData,
    report: &ImportReport,
    pages: usize,
    removed_lines: usize,
) -> io::Result<()> {
    let doc = report
        .document
        .and_then(|id| data.documents.iter().find(|d| d.id == id));
    let name = doc
        .and_then(|d| d.file_name.clone())
        .unwrap_or_else(|| "resume".into());
    let what = if report.first_import {
        "Imported"
    } else if report.same_file {
        "Re-imported (same file)"
    } else {
        "Re-imported"
    };
    let pages = if pages > 1 {
        format!(" ({pages} pages)")
    } else {
        String::new()
    };
    writeln!(out, "{TITLE}{what} {name}{pages}{TITLE:#}")?;
    if removed_lines > 0 {
        writeln!(
            out,
            "{DIM}ignored {} repeated on every page (header, footer, page numbers){DIM:#}",
            plural(removed_lines as u64, "line", "lines")
        )?;
    }
    if let Some(name) = &data.profile.name {
        let headline = data
            .profile
            .headline
            .as_ref()
            .map(|h| format!(" — {h}"))
            .unwrap_or_default();
        writeln!(out, "{name}{headline}")?;
    }

    section(out, "Experience")?;
    let experiences = data.visible_experiences();
    if experiences.is_empty() {
        writeln!(out, "  none found")?;
    }
    for e in experiences.iter().filter(|e| !e.meta.is_stale()) {
        writeln!(out, "  {}", experience_line(e))?;
    }
    let projects: Vec<String> = data
        .visible_projects()
        .iter()
        .filter(|p| !p.meta.is_stale())
        .map(|p| p.name.clone())
        .collect();
    if !projects.is_empty() {
        section(out, "Projects")?;
        writeln!(out, "  {}", projects.join(", "))?;
    }
    let education: Vec<String> = data
        .visible_education()
        .iter()
        .filter(|e| !e.meta.is_stale())
        .map(|e| match (&e.degree, &e.field) {
            (Some(d), Some(f)) => format!("{d} in {f}, {}", e.institution),
            (Some(d), None) => format!("{d}, {}", e.institution),
            _ => e.institution.clone(),
        })
        .collect();
    if !education.is_empty() {
        section(out, "Education")?;
        for e in education {
            writeln!(out, "  {e}")?;
        }
    }
    section(out, "Skills")?;
    let skills: Vec<String> = data
        .skills_with_evidence()
        .iter()
        .filter(|s| s.strength != EvidenceStrength::Unsupported)
        .map(|s| s.skill.name.clone())
        .collect();
    if skills.is_empty() {
        writeln!(out, "  none found")?;
    } else {
        let shown = skills.len().min(12);
        let more = if skills.len() > shown {
            format!(" (+{} more)", skills.len() - shown)
        } else {
            String::new()
        };
        writeln!(out, "  {}{more}", skills[..shown].join(", "))?;
    }
    section(out, "Preferences")?;
    if data.preferences().active().next().is_none() {
        writeln!(out, "  none yet")?;
    } else {
        writeln!(
            out,
            "  {} set",
            plural(
                data.preferences().active().count() as u64,
                "preference",
                "preferences"
            )
        )?;
    }

    section(out, "Evidence")?;
    let live: Vec<&Claim> = data
        .claims
        .iter()
        .filter(|c| c.stale_since.is_none())
        .collect();
    let usable = live.iter().filter(|c| data.standing(c).is_usable()).count();
    let review = live
        .iter()
        .filter(|c| data.standing(c).needs_review())
        .count();
    writeln!(
        out,
        "  {} extracted",
        plural(live.len() as u64, "claim", "claims")
    )?;
    writeln!(out, "  {usable} directly supported")?;
    writeln!(out, "  {review} need review")?;
    if !report.first_import {
        section(out, "Changes since the last import")?;
        tally(out, "experiences", report.experiences)?;
        tally(out, "projects", report.projects)?;
        tally(out, "education", report.education)?;
        tally(out, "skills", report.skills)?;
        tally(out, "claims", report.claims)?;
        if report.kept_confirmed + report.kept_rejected > 0 {
            writeln!(
                out,
                "  kept your decisions: {} confirmed, {} rejected",
                report.kept_confirmed, report.kept_rejected
            )?;
        }
        if report.preserved_edits > 0 {
            writeln!(
                out,
                "  kept your edits on {}",
                plural(report.preserved_edits as u64, "record", "records")
            )?;
        }
        if report.reconfirm > 0 {
            writeln!(
                out,
                "  {} you confirmed changed and need confirming again",
                plural(report.reconfirm as u64, "claim", "claims")
            )?;
        }
        if report.stale_confirmed > 0 {
            writeln!(
                out,
                "  {} you confirmed left the resume; review them (narrow claims review)",
                plural(report.stale_confirmed as u64, "claim", "claims")
            )?;
        }
    }
    let mut uncertain: Vec<String> = report.notes.clone();
    uncertain.extend(
        report
            .ignored
            .iter()
            .take(5)
            .map(|line| format!("not understood: {line}")),
    );
    if report.ignored.len() > 5 {
        uncertain.push(format!(
            "… {} more lines not understood",
            report.ignored.len() - 5
        ));
    }
    if !uncertain.is_empty() {
        section(out, "Uncertain")?;
        for note in uncertain {
            writeln!(out, "  - {note}")?;
        }
    }
    section(out, "Next")?;
    writeln!(out, "  narrow profile")?;
    writeln!(out, "  narrow claims review")?;
    writeln!(
        out,
        "  narrow preferences add \"I want … and at least …; avoid …\""
    )?;
    Ok(())
}

fn tally(out: &mut impl Write, what: &str, t: Tally) -> io::Result<()> {
    if t == Tally::default() {
        return Ok(());
    }
    let mut parts = vec![
        format!("{} new", t.added),
        format!("{} updated", t.updated),
        format!("{} unchanged", t.unchanged),
    ];
    if t.stale > 0 {
        parts.push(format!("{} no longer in the resume", t.stale));
    }
    if t.restored > 0 {
        parts.push(format!("{} back in the resume", t.restored));
    }
    writeln!(out, "  {what}: {}", parts.join(", "))
}

/// One line per claim, grouped by what it is about.
pub fn claim_list(out: &mut impl Write, data: &ProfileData, claims: &[&Claim]) -> io::Result<()> {
    if claims.is_empty() {
        return writeln!(out, "No claims match.");
    }
    let mut current: Option<Subject> = None;
    for claim in claims {
        if current != Some(claim.subject) {
            if current.is_some() {
                writeln!(out)?;
            }
            writeln!(
                out,
                "{TITLE}{}{TITLE:#}",
                subject_label(data, claim.subject)
            )?;
            current = Some(claim.subject);
        }
        let standing = data.standing(claim);
        writeln!(
            out,
            "  {} {DIM}{}{DIM:#} {:<12} {DIM}{:<14}{DIM:#} {}",
            marker(standing),
            short(claim.id),
            claim.state_label(),
            claim.kind.as_str(),
            claim.text
        )?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "{DIM}✓ usable · ? needs review · ✗ rejected — narrow claims show <id> for the evidence{DIM:#}"
    )
}

/// The review queue: each claim with why JobHunt believes it.
pub fn review(
    out: &mut impl Write,
    data: &ProfileData,
    claims: &[&Claim],
    total: usize,
) -> io::Result<()> {
    if claims.is_empty() {
        return writeln!(
            out,
            "Nothing to review: every claim is either quoted from your resume, confirmed, or rejected."
        );
    }
    writeln!(
        out,
        "{} need review. Confirm what is true, reject what is not; rejected claims are never used.",
        plural(total as u64, "claim", "claims")
    )?;
    for (i, claim) in claims.iter().enumerate() {
        writeln!(out)?;
        writeln!(out, "{:>2}. {TITLE}{}{TITLE:#}", i + 1, claim.text)?;
        writeln!(
            out,
            "    {DIM}{} · {} · {} · {} confidence · {}{DIM:#}",
            short(claim.id),
            claim.state_label(),
            claim.kind.as_str(),
            claim.confidence.as_str(),
            subject_label(data, claim.subject)
        )?;
        writeln!(out, "    status: {}", data.standing(claim).describe())?;
        if let Some(basis) = &claim.basis {
            writeln!(out, "    why: {basis}")?;
        }
        if let Some(source) = &claim.source {
            writeln!(out, "    resume: “{}”", one_line(&source.snippet))?;
        }
        if let Some(note) = &claim.note {
            writeln!(out, "    note: {note}")?;
        }
    }
    if total > claims.len() {
        writeln!(out)?;
        writeln!(
            out,
            "… {} more (narrow claims review --all)",
            total - claims.len()
        )?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "{DIM}narrow claims confirm <id>… · narrow claims reject <id>… [--reason …]{DIM:#}"
    )
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Everything about one claim.
pub fn claim_detail(out: &mut impl Write, data: &ProfileData, claim: &Claim) -> io::Result<()> {
    writeln!(out, "{TITLE}{}{TITLE:#}", claim.text)?;
    writeln!(out)?;
    let mut field = |name: &str, value: String| writeln!(out, "{name:<13} {value}");
    field("Id", claim.id.to_string())?;
    field("Kind", claim.kind.as_str().to_owned())?;
    if let Some(topic) = &claim.topic {
        field("Topic", topic.clone())?;
    }
    field("About", subject_label(data, claim.subject))?;
    field(
        "Provenance",
        match claim.provenance {
            jobhunt_profile::Provenance::Extracted => "extracted from your resume".to_owned(),
            jobhunt_profile::Provenance::Inferred => "inferred by JobHunt".to_owned(),
            jobhunt_profile::Provenance::UserEntered => "entered by you".to_owned(),
        },
    )?;
    field("Confidence", claim.confidence.as_str().to_owned())?;
    let verification = match (claim.verification, claim.verified_at) {
        (Verification::Unverified, _) => "not reviewed".to_owned(),
        (v, Some(at)) => format!("{} on {}", v.as_str(), at.format("%Y-%m-%d")),
        (v, None) => v.as_str().to_owned(),
    };
    field("Verification", verification)?;
    field("Status", data.standing(claim).describe().to_owned())?;
    if let Some(basis) = &claim.basis {
        field("Why", basis.clone())?;
    }
    if let Some(source) = &claim.source {
        let doc = data
            .documents
            .iter()
            .find(|d| d.id == source.document)
            .and_then(|d| d.file_name.clone())
            .unwrap_or_else(|| short(source.document));
        let section = source
            .section
            .as_ref()
            .map(|s| format!(", {s}"))
            .unwrap_or_default();
        field("Source", format!("{doc}{section}"))?;
        for (i, line) in source.snippet.lines().enumerate() {
            let label = if i == 0 { "Resume text" } else { "" };
            field(label, format!("“{}”", line.trim()))?;
        }
    }
    if let Some(stale) = claim.stale_since {
        field("Stale since", stale.format("%Y-%m-%d").to_string())?;
    }
    if let Some(old) = claim.supersedes {
        let text = data.claim(old).map(|c| c.text.clone()).unwrap_or_default();
        field("Replaces", format!("{} “{text}”", short(old)))?;
    }
    if claim.edited {
        field("Edited", "you rewrote this claim".to_owned())?;
    }
    if let Some(note) = &claim.note {
        field("Note", note.clone())?;
    }
    field(
        "Created",
        claim.created_at.format("%Y-%m-%d %H:%M UTC").to_string(),
    )?;
    field(
        "Updated",
        claim.updated_at.format("%Y-%m-%d %H:%M UTC").to_string(),
    )?;
    Ok(())
}

/// `narrow preferences`.
pub fn preferences(out: &mut impl Write, data: &ProfileData) -> io::Result<()> {
    let view = data.preferences();
    if view.active().next().is_none() {
        writeln!(
            out,
            "No preferences yet. Tell JobHunt what you want, in your own words:"
        )?;
        writeln!(
            out,
            "  narrow preferences add \"I want small product teams and at least $120k. Avoid pure SRE roles.\""
        )?;
        writeln!(out, "or set them one by one: narrow preferences set --help")?;
    }
    for category in CATEGORIES {
        let items = view.in_category(category);
        if items.is_empty() {
            continue;
        }
        writeln!(out, "{TITLE}{}{TITLE:#}", category_label(category))?;
        for p in items {
            let origin = match (p.origin, &p.snippet) {
                (PreferenceOrigin::Statement, Some(snippet)) => format!("from “{snippet}”"),
                (PreferenceOrigin::Statement, None) => "from a statement".to_owned(),
                (PreferenceOrigin::UserEntered, _) => "set by you".to_owned(),
            };
            let doubt = if p.certainty == Certainty::Uncertain {
                " · uncertain, check it"
            } else {
                ""
            };
            writeln!(
                out,
                "  {:<6} {}  {DIM}{} · {origin}{doubt}{DIM:#}",
                stance_word(p.stance),
                p.value,
                short(p.id)
            )?;
            if let Some(note) = &p.note {
                writeln!(out, "         {DIM}{note}{DIM:#}")?;
            }
        }
        writeln!(out)?;
    }
    if !data.statements.is_empty() {
        writeln!(out, "{TITLE}In your words{TITLE:#}")?;
        for s in &data.statements {
            statement_line(out, s)?;
        }
    }
    Ok(())
}

fn statement_line(out: &mut impl Write, s: &PreferenceStatement) -> io::Result<()> {
    let reading = match s.reading {
        StatementReading::Understood => "understood",
        StatementReading::Partial => "partly understood",
        StatementReading::NotUnderstood => "kept as written; not understood",
    };
    writeln!(
        out,
        "  “{}”  {DIM}{} · {} · {reading}{DIM:#}",
        s.text,
        short(s.id),
        s.created_at.format("%Y-%m-%d")
    )?;
    for part in &s.unparsed {
        writeln!(out, "    {DIM}not understood: “{part}”{DIM:#}")?;
    }
    Ok(())
}

/// What `narrow preferences add` understood.
pub fn statement_outcome(out: &mut impl Write, outcome: &StatementOutcome) -> io::Result<()> {
    if outcome.repeated {
        writeln!(
            out,
            "Already saved ({}), and still in effect: nothing changed.",
            short(outcome.statement.id)
        )?;
    } else {
        writeln!(
            out,
            "Saved your statement ({}).",
            short(outcome.statement.id)
        )?;
    }
    if outcome.preferences.is_empty() {
        writeln!(
            out,
            "JobHunt could not read a structured preference from it; it is kept as written."
        )?;
    } else {
        writeln!(out, "Understood:")?;
        for p in &outcome.preferences {
            let doubt = if p.certainty == Certainty::Uncertain {
                "  (uncertain; check it)"
            } else {
                ""
            };
            writeln!(out, "  {:<6} {}{doubt}", stance_word(p.stance), p.value)?;
            if let Some(note) = &p.note {
                writeln!(out, "         {note}")?;
            }
        }
    }
    for part in &outcome.statement.unparsed {
        writeln!(out, "Not understood (kept): “{part}”")?;
    }
    for old in &outcome.replaced {
        writeln!(out, "Replaces: {} {}", stance_word(old.stance), old.value)?;
    }
    Ok(())
}

pub fn history(out: &mut impl Write, events: &[ProfileEvent]) -> io::Result<()> {
    if events.is_empty() {
        return writeln!(out, "No changes recorded yet.");
    }
    for e in events {
        let record = e.record.as_deref().map(short).unwrap_or_default();
        writeln!(
            out,
            "{} {:<18} {DIM}{record}{DIM:#} {}",
            e.at.format("%Y-%m-%d %H:%M"),
            e.kind.as_str(),
            e.detail
        )?;
    }
    Ok(())
}

/// For `profile add`/`edit` confirmations.
pub fn date_or_unknown(date: Option<PartialDate>) -> String {
    date.map_or_else(|| "unknown".to_owned(), |d| d.display_short())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_ids() {
        assert_eq!(
            short("clm_0123456789abcdef0123456789abcdef"),
            "clm_01234567"
        );
        assert_eq!(short("x"), "x");
    }
}
