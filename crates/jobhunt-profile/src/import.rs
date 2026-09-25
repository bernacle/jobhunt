//! Resume import and re-import.
//!
//! [`merge_resume`] folds a parsed resume into an existing profile. It is
//! pure (no storage), so the rules are easy to test and every backend
//! applies the same ones. The rules:
//!
//! * **Identity.** Each resume record has an *import key*: experiences by
//!   company and title (plus an occurrence number when the same pair
//!   appears twice), projects by name, education by institution and degree,
//!   skills by normalized name. Claims are keyed by their subject's id plus
//!   what they say (the bullet's normalized words, or the technology,
//!   domain or role topic). Importing the same resume again therefore
//!   matches every record and claim instead of adding new ones.
//! * **Corrections.** An experience whose title changed is still matched
//!   when its company and start date are the same, so fixing a typo in the
//!   resume updates the record (and keeps its claims and decisions)
//!   instead of creating a duplicate.
//! * **Source facts update.** Matched resume records take the new values,
//!   except fields the user edited by hand, which are never overwritten.
//! * **Decisions survive.** Confirmed claims stay confirmed and rejected
//!   claims stay rejected (a rejected claim never comes back as trusted).
//!   The one exception: when the statement of a one-per-record claim (a
//!   title and period, a degree) changed, a confirmation of the old
//!   statement does not carry over to the new one.
//! * **Nothing is deleted.** Resume records and claims the new resume no
//!   longer contains are marked stale (`stale_since`). Stale claims need
//!   the user's review before they are used again, even if confirmed.
//!   A reworded bullet becomes a new claim that points to the one it
//!   `supersedes`. Records and claims the user entered are never touched.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;

use crate::aggregate::ProfileData;
use crate::date::Period;
use crate::evidence::{Claim, ClaimKind, Confidence, Provenance, Subject};
use crate::ids::{ClaimId, DocumentId, EducationId, ExperienceId, ProfileId, ProjectId, SkillId};
use crate::infer::{
    domains_in, is_accomplishment, known_technology, ownership_in, role_signals, technologies_in,
    title_level, topic_key,
};
use crate::model::{
    Education, EmploymentKind, Experience, Origin, Project, RecordMeta, Skill, SourceDocument,
    SourceRef, Verification,
};
use crate::resume::{ParsedEducation, ParsedExperience, ParsedProject, ParsedResume};

/// The id a document gets in a profile: the same file always maps to the
/// same document.
pub fn document_id(profile: ProfileId, sha256: &str) -> DocumentId {
    DocumentId::derive(&[&profile.to_string(), sha256])
}

/// Counts for one kind of record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// Newly marked stale: no longer in the resume.
    pub stale: usize,
    /// Were stale, and the resume contains them again.
    pub restored: usize,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub document: Option<DocumentId>,
    /// The profile had no resume before.
    pub first_import: bool,
    /// This exact file was imported before.
    pub same_file: bool,
    pub experiences: Tally,
    pub projects: Tally,
    pub education: Tally,
    pub skills: Tally,
    pub claims: Tally,
    /// Confirmed claims found again (still confirmed).
    pub kept_confirmed: usize,
    /// Rejected claims found again (still rejected).
    pub kept_rejected: usize,
    /// Confirmed claims whose statement changed; they need confirming again.
    pub reconfirm: usize,
    /// Confirmed claims whose source left the resume (now stale).
    pub stale_confirmed: usize,
    /// Records where the user's own edits were kept over the resume.
    pub preserved_edits: usize,
    /// Parser doubts, per record ("Freelance: no dates found").
    pub notes: Vec<String>,
    /// Lines the parser did not understand.
    pub ignored: Vec<String>,
}

struct Ctx<'a> {
    profile: ProfileId,
    document: DocumentId,
    now: DateTime<Utc>,
    claims: Vec<Claim>,
    report: &'a mut ImportReport,
}

impl Ctx<'_> {
    fn source(&self, snippet: &str, section: &str) -> Option<SourceRef> {
        (!snippet.trim().is_empty()).then(|| SourceRef {
            document: self.document,
            snippet: snippet.trim().to_owned(),
            section: Some(section.to_owned()),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn claim(
        &mut self,
        subject: Subject,
        kind: ClaimKind,
        key: String,
        text: String,
        topic: Option<String>,
        provenance: Provenance,
        confidence: Confidence,
        source: Option<SourceRef>,
        basis: Option<String>,
    ) {
        let import_key = format!(
            "{}|{}",
            subject.id_string().unwrap_or_else(|| "profile".to_owned()),
            key
        );
        if self
            .claims
            .iter()
            .any(|c| c.import_key.as_deref() == Some(import_key.as_str()))
        {
            return;
        }
        let position = u32::try_from(self.claims.iter().filter(|c| c.subject == subject).count())
            .unwrap_or(u32::MAX);
        self.claims.push(Claim {
            id: ClaimId::derive(&[&self.profile.to_string(), &import_key]),
            kind,
            text,
            topic,
            subject,
            provenance,
            confidence,
            verification: Verification::Unverified,
            source,
            basis,
            import_key: Some(import_key),
            supersedes: None,
            position,
            stale_since: None,
            verified_at: None,
            note: None,
            edited: false,
            created_at: self.now,
            updated_at: self.now,
        });
    }
}

fn opt_key(value: Option<&str>) -> String {
    value
        .map(search_key)
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "?".to_owned())
}

fn period_text(period: Period) -> String {
    period
        .display()
        .map(|p| format!(" ({p})"))
        .unwrap_or_default()
}

/// Folds `parsed` (read from `document`) into `data`. See the module docs
/// for the rules. `document.id` is recomputed from the profile and hash.
pub fn merge_resume(
    data: &mut ProfileData,
    mut document: SourceDocument,
    parsed: &ParsedResume,
    now: DateTime<Utc>,
) -> ImportReport {
    let profile = data.id();
    document.id = document_id(profile, &document.sha256);
    let mut report = ImportReport {
        document: Some(document.id),
        first_import: data.documents.is_empty(),
        same_file: data.documents.iter().any(|d| d.id == document.id),
        notes: parsed.notes.clone(),
        ignored: parsed.ignored.clone(),
        ..ImportReport::default()
    };
    let doc_id = document.id;
    match data.documents.iter_mut().find(|d| d.id == doc_id) {
        Some(existing) => {
            existing.last_imported_at = now;
            existing.text = document.text;
            existing.parser = document.parser;
            existing.pages = document.pages;
            existing.file_name = document.file_name.or(existing.file_name.take());
        }
        None => {
            document.first_imported_at = now;
            document.last_imported_at = now;
            data.documents.push(document);
        }
    }

    merge_basics(data, parsed, &mut report, now);

    let mut ctx = Ctx {
        profile,
        document: doc_id,
        now,
        claims: Vec::new(),
        report: &mut report,
    };
    let experience_ids = merge_experiences(data, &parsed.experiences, &mut ctx);
    merge_projects(data, &parsed.projects, &experience_ids, &mut ctx);
    merge_education(data, &parsed.education, &mut ctx);
    profile_claims(parsed, &mut ctx);
    let candidates = std::mem::take(&mut ctx.claims);
    let report = ctx.report;
    merge_claims(data, candidates, report, now);
    merge_skills(data, parsed, report, now);

    data.profile.updated_at = now;
    std::mem::take(report)
}

fn merge_basics(
    data: &mut ProfileData,
    parsed: &ParsedResume,
    report: &mut ImportReport,
    now: DateTime<Utc>,
) {
    let basics = &parsed.basics;
    let profile = &mut data.profile;
    let mut preserved = false;
    let edited: HashSet<String> = profile.edited_fields.iter().cloned().collect();
    let mut assign = |field: &str, target: &mut Option<String>, value: &Option<String>| {
        if value.is_none() {
            return;
        }
        if edited.contains(field) {
            if target != value {
                preserved = true;
            }
            return;
        }
        *target = value.clone();
    };
    assign("name", &mut profile.name, &basics.name);
    assign("headline", &mut profile.headline, &basics.headline);
    assign("location", &mut profile.location, &basics.location);
    assign("summary", &mut profile.summary, &basics.summary);
    if !basics.contacts.is_empty() {
        if edited.contains("contacts") {
            preserved |= profile.contacts != basics.contacts;
        } else {
            profile.contacts = basics.contacts.clone();
        }
    }
    if !basics.languages.is_empty() {
        if edited.contains("languages") {
            preserved |= profile.languages != basics.languages;
        } else {
            profile.languages = basics.languages.clone();
        }
    }
    if preserved {
        report.preserved_edits += 1;
    }
    profile.updated_at = now;
}

/// Assigns `value` to `field` unless the user edited it. Returns whether
/// the stored value changed, and records preserved edits.
fn apply_field<T: PartialEq + Clone>(
    meta: &RecordMeta,
    field: &str,
    target: &mut T,
    value: &T,
    preserved: &mut bool,
) -> bool {
    if meta.is_edited(field) {
        if target != value {
            *preserved = true;
        }
        return false;
    }
    if target != value {
        *target = value.clone();
        return true;
    }
    false
}

fn new_meta(
    key: String,
    source: Option<SourceRef>,
    notes: Vec<String>,
    now: DateTime<Utc>,
) -> RecordMeta {
    RecordMeta {
        origin: Origin::Resume,
        source,
        import_key: Some(key),
        verification: Verification::Unverified,
        stale_since: None,
        edited_fields: Vec::new(),
        notes,
        created_at: now,
        updated_at: now,
    }
}

/// Refreshes the bookkeeping of a matched record. Returns whether anything
/// besides the timestamps changed.
fn refresh_meta(
    meta: &mut RecordMeta,
    key: String,
    source: Option<SourceRef>,
    notes: &[String],
    tally: &mut Tally,
) -> bool {
    let mut changed = false;
    if meta.stale_since.take().is_some() {
        tally.restored += 1;
        changed = true;
    }
    if meta.import_key.as_deref() != Some(key.as_str()) {
        meta.import_key = Some(key);
        changed = true;
    }
    if meta.source != source {
        // A new document or snippet is provenance, not a content change.
        meta.source = source;
    }
    let notes = if meta.edited_fields.is_empty() {
        notes.to_vec()
    } else {
        Vec::new()
    };
    if meta.notes != notes {
        meta.notes = notes;
        changed = true;
    }
    changed
}

fn tally_outcome(tally: &mut Tally, changed: bool) {
    if changed {
        tally.updated += 1;
    } else {
        tally.unchanged += 1;
    }
}

fn mark_stale<'a>(
    metas: impl Iterator<Item = &'a mut RecordMeta>,
    seen: &HashSet<String>,
    tally: &mut Tally,
    now: DateTime<Utc>,
) {
    for meta in metas {
        let from_resume = meta.origin == Origin::Resume;
        let key = meta.import_key.clone().unwrap_or_default();
        if from_resume && !seen.contains(&key) && meta.stale_since.is_none() {
            meta.stale_since = Some(now);
            meta.updated_at = now;
            tally.stale += 1;
        }
    }
}

fn experience_key(parsed: &ParsedExperience) -> String {
    format!(
        "exp|{}|{}",
        opt_key(parsed.company.as_deref()),
        opt_key(parsed.title.as_deref())
    )
}

fn merge_experiences(
    data: &mut ProfileData,
    parsed: &[ParsedExperience],
    ctx: &mut Ctx<'_>,
) -> Vec<ExperienceId> {
    // Keys, with an occurrence number for repeated company + title.
    let mut counts: HashMap<String, usize> = HashMap::new();
    let keys: Vec<String> = parsed
        .iter()
        .map(|p| {
            let base = experience_key(p);
            let n = counts.entry(base.clone()).or_insert(0);
            *n += 1;
            if *n == 1 { base } else { format!("{base}#{n}") }
        })
        .collect();

    // Pass 1: exact keys. Pass 2: same company and start date (a corrected
    // title), or the title the user already corrected by hand.
    let mut matched: Vec<Option<usize>> = vec![None; parsed.len()];
    let mut taken: HashSet<usize> = HashSet::new();
    for (i, key) in keys.iter().enumerate() {
        if let Some(idx) = data.experiences.iter().position(|e| {
            e.meta.origin == Origin::Resume && e.meta.import_key.as_deref() == Some(key.as_str())
        }) && taken.insert(idx)
        {
            matched[i] = Some(idx);
        }
    }
    for (i, p) in parsed.iter().enumerate() {
        if matched[i].is_some() {
            continue;
        }
        let company = opt_key(p.company.as_deref());
        let found = data.experiences.iter().enumerate().position(|(idx, e)| {
            e.meta.origin == Origin::Resume
                && !taken.contains(&idx)
                && opt_key(e.company.as_deref()) == company
                && ((e.start.is_some() && e.start == p.start)
                    || (e.meta.is_edited("title")
                        && opt_key(e.title.as_deref()) == opt_key(p.title.as_deref())))
        });
        if let Some(idx) = found {
            taken.insert(idx);
            matched[i] = Some(idx);
        }
    }

    let mut ids = Vec::with_capacity(parsed.len());
    let mut seen_keys = HashSet::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = keys[i].clone();
        seen_keys.insert(key.clone());
        let source = ctx.source(&p.header, "Experience");
        let label = p
            .title
            .clone()
            .or_else(|| p.company.clone())
            .unwrap_or_else(|| "(untitled)".into());
        ctx.report
            .notes
            .extend(p.notes.iter().map(|n| format!("{label}: {n}")));
        let position = u32::try_from(i).unwrap_or(u32::MAX);
        let id = match matched[i] {
            Some(idx) => {
                let e = &mut data.experiences[idx];
                let mut preserved = false;
                let mut changed = false;
                let meta = e.meta.clone();
                changed |=
                    apply_field(&meta, "company", &mut e.company, &p.company, &mut preserved);
                changed |= apply_field(&meta, "title", &mut e.title, &p.title, &mut preserved);
                changed |= apply_field(
                    &meta,
                    "employment",
                    &mut e.employment,
                    &p.employment,
                    &mut preserved,
                );
                changed |= apply_field(&meta, "start", &mut e.start, &p.start, &mut preserved);
                changed |= apply_field(&meta, "end", &mut e.end, &p.end, &mut preserved);
                changed |=
                    apply_field(&meta, "current", &mut e.current, &p.current, &mut preserved);
                changed |= apply_field(
                    &meta,
                    "location",
                    &mut e.location,
                    &p.location,
                    &mut preserved,
                );
                changed |=
                    apply_field(&meta, "summary", &mut e.summary, &p.summary, &mut preserved);
                changed |= refresh_meta(
                    &mut e.meta,
                    key.clone(),
                    source,
                    &p.notes,
                    &mut ctx.report.experiences,
                );
                e.position = position;
                if changed {
                    e.meta.updated_at = ctx.now;
                }
                if preserved {
                    ctx.report.preserved_edits += 1;
                }
                tally_outcome(&mut ctx.report.experiences, changed);
                e.id
            }
            None => {
                let id = ExperienceId::derive(&[&ctx.profile.to_string(), &key]);
                data.experiences.push(Experience {
                    id,
                    company: p.company.clone(),
                    title: p.title.clone(),
                    employment: p.employment.clone(),
                    start: p.start,
                    end: p.end,
                    current: p.current,
                    location: p.location.clone(),
                    summary: p.summary.clone(),
                    position,
                    meta: new_meta(key, source, p.notes.clone(), ctx.now),
                });
                ctx.report.experiences.added += 1;
                id
            }
        };
        ids.push(id);
    }
    mark_stale(
        data.experiences.iter_mut().map(|e| &mut e.meta),
        &seen_keys,
        &mut ctx.report.experiences,
        ctx.now,
    );

    for (p, id) in parsed.iter().zip(&ids) {
        let Some(experience) = data.experience(*id).cloned() else {
            continue;
        };
        experience_claims(&experience, p, ctx);
    }
    ids
}

fn experience_claims(e: &Experience, p: &ParsedExperience, ctx: &mut Ctx<'_>) {
    let subject = Subject::Experience(e.id);
    let at = e
        .company
        .clone()
        .or_else(|| e.employment.as_ref().map(|k| k.label().to_lowercase()))
        .or_else(|| e.title.clone())
        .unwrap_or_else(|| "this position".into());
    let section = "Experience";

    let what = match (&p.title, &p.company) {
        (Some(t), Some(c)) => Some(format!("{t} at {c}")),
        (Some(t), None) if p.employment == Some(EmploymentKind::Freelance) => {
            Some(format!("{t} (freelance)"))
        }
        (Some(t), None) => Some(t.clone()),
        (None, Some(c)) => Some(format!("Worked at {c}")),
        (None, None) => None,
    };
    if let Some(what) = what {
        let period = Period {
            start: p.start,
            end: p.end,
            current: p.current,
        };
        let confident = p.title.is_some()
            && (p.company.is_some() || p.employment == Some(EmploymentKind::Freelance))
            && !p.ambiguous_header;
        ctx.claim(
            subject,
            ClaimKind::Employment,
            "employment".into(),
            format!("{what}{}", period_text(period)),
            None,
            Provenance::Extracted,
            if confident {
                Confidence::High
            } else {
                Confidence::Medium
            },
            ctx.source(&p.header, section),
            None,
        );
    }

    bullet_claims(subject, &p.bullets, section, ctx);
    let mut texts: Vec<&str> = p.bullets.iter().map(String::as_str).collect();
    if let Some(summary) = &p.summary {
        texts.push(summary);
    }
    let techs = technology_claims(
        subject,
        &at,
        p.tech_line.as_deref(),
        &p.technologies,
        &texts,
        section,
        ctx,
    );

    // Domains: what the company, title and bullets talk about.
    let mut domain_texts: Vec<&str> = Vec::new();
    if let Some(company) = &p.company {
        domain_texts.push(company);
    }
    domain_texts.push(&p.header);
    domain_texts.extend(texts.iter().copied());
    domain_claims(subject, &at, &domain_texts, section, ctx);

    // Role kinds.
    let tech_names: Vec<&str> = techs.iter().map(String::as_str).collect();
    for signal in role_signals(p.title.as_deref(), &tech_names, &texts) {
        let snippet = if signal.from_title {
            Some(p.header.as_str())
        } else {
            p.tech_line.as_deref().or(texts.first().copied())
        };
        let confidence = if signal.from_title || signal.reasons.len() >= 2 {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        ctx.claim(
            subject,
            ClaimKind::Role,
            format!("role|{}", signal.role),
            format!("{} engineering experience at {at}", capitalize(signal.role)),
            Some(signal.role.to_owned()),
            Provenance::Inferred,
            confidence,
            snippet.and_then(|s| ctx.source(s, section)),
            Some(signal.reasons.join("; ")),
        );
    }

    // Seniority from the title, ownership from the bullets.
    if let Some(title) = &p.title
        && let Some(level) = title_level(title)
    {
        ctx.claim(
            subject,
            ClaimKind::Ownership,
            format!("ownership|{}", level.topic),
            match level.topic {
                "founding" => format!("Founding-stage role at {at}"),
                "management" => format!("Management role at {at}"),
                topic => format!("{}-level role at {at}", capitalize(topic)),
            },
            Some(level.topic.to_owned()),
            Provenance::Inferred,
            Confidence::Medium,
            ctx.source(&p.header, section),
            Some(format!("title “{title}” says “{}”", level.phrase)),
        );
    }
    ownership_claims(subject, &at, &p.bullets, section, ctx);
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn bullet_claims(subject: Subject, bullets: &[String], section: &str, ctx: &mut Ctx<'_>) {
    for bullet in bullets {
        let key = search_key(bullet);
        if key.is_empty() {
            continue;
        }
        let kind = if is_accomplishment(bullet) {
            ClaimKind::Accomplishment
        } else {
            ClaimKind::Responsibility
        };
        ctx.claim(
            subject,
            kind,
            format!("bullet|{key}"),
            bullet.trim().to_owned(),
            None,
            Provenance::Extracted,
            Confidence::High,
            ctx.source(bullet, section),
            None,
        );
    }
}

/// Technology claims for one record. Returns the canonical names found.
fn technology_claims(
    subject: Subject,
    at: &str,
    tech_line: Option<&str>,
    listed: &[String],
    texts: &[&str],
    section: &str,
    ctx: &mut Ctx<'_>,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut add = |name: String, snippet: &str, ctx: &mut Ctx<'_>| {
        let key = topic_key(&name);
        if names.iter().any(|n| topic_key(n) == key) {
            return;
        }
        ctx.claim(
            subject,
            ClaimKind::Technology,
            format!("technology|{key}"),
            format!("Used {name} at {at}"),
            Some(key),
            Provenance::Extracted,
            Confidence::High,
            ctx.source(snippet, section),
            None,
        );
        names.push(name);
    };
    if let Some(line) = tech_line {
        for item in listed {
            let name = known_technology(item).map_or_else(|| item.clone(), |t| t.name.to_owned());
            add(name, line, ctx);
        }
    }
    for text in texts {
        for mention in technologies_in(text, false) {
            add(mention.technology.to_owned(), text, ctx);
        }
    }
    names
}

fn domain_claims(subject: Subject, at: &str, texts: &[&str], section: &str, ctx: &mut Ctx<'_>) {
    // domain → (phrases, strong, first text)
    let mut found: Vec<(&'static str, Vec<String>, bool, &str)> = Vec::new();
    for text in texts {
        for hit in domains_in(text) {
            match found.iter_mut().find(|(d, ..)| *d == hit.domain) {
                Some((_, phrases, strong, _)) => {
                    if !phrases.iter().any(|p| p.eq_ignore_ascii_case(&hit.phrase)) {
                        phrases.push(hit.phrase);
                    }
                    *strong |= hit.strong;
                }
                None => found.push((hit.domain, vec![hit.phrase], hit.strong, text)),
            }
        }
    }
    for (domain, phrases, strong, first) in found {
        // One weak phrase is not evidence of a domain.
        if !strong && phrases.len() < 2 {
            continue;
        }
        let quoted: Vec<String> = phrases.iter().take(4).map(|p| format!("“{p}”")).collect();
        ctx.claim(
            subject,
            ClaimKind::Domain,
            format!("domain|{domain}"),
            format!("Worked in {domain} at {at}"),
            Some(domain.to_owned()),
            Provenance::Inferred,
            if strong {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            ctx.source(first, section),
            Some(format!("mentions {}", quoted.join(", "))),
        );
    }
}

fn ownership_claims(
    subject: Subject,
    at: &str,
    bullets: &[String],
    section: &str,
    ctx: &mut Ctx<'_>,
) {
    let mut found: Vec<(&'static str, Vec<String>, &str)> = Vec::new();
    for bullet in bullets {
        for signal in ownership_in(bullet) {
            match found.iter_mut().find(|(t, ..)| *t == signal.topic) {
                Some((_, quotes, _)) => quotes.push(signal.phrase),
                None => found.push((signal.topic, vec![signal.phrase], bullet)),
            }
        }
    }
    for (topic, quotes, first) in found {
        let text = match topic {
            "technical leadership" => format!("Led technical work at {at}"),
            "mentorship" => format!("Mentored or hired engineers at {at}"),
            "founding" => format!("Founding-stage work at {at}"),
            other => format!("{} at {at}", capitalize(other)),
        };
        let quoted: Vec<String> = quotes.iter().take(3).map(|q| format!("“{q}”")).collect();
        ctx.claim(
            subject,
            ClaimKind::Ownership,
            format!("ownership|{topic}"),
            text,
            Some(topic.to_owned()),
            Provenance::Inferred,
            Confidence::Medium,
            ctx.source(first, section),
            Some(format!("bullets say {}", quoted.join(", "))),
        );
    }
}

fn merge_projects(
    data: &mut ProfileData,
    parsed: &[ParsedProject],
    experiences: &[ExperienceId],
    ctx: &mut Ctx<'_>,
) {
    let mut seen_keys = HashSet::new();
    let mut ids = Vec::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = format!("proj|{}", opt_key(Some(&p.name)));
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        ctx.report
            .notes
            .extend(p.notes.iter().map(|n| format!("{}: {n}", p.name)));
        let source = ctx.source(&p.header, "Projects");
        let position = u32::try_from(i).unwrap_or(u32::MAX);
        // A project naming the company of an imported experience belongs to it.
        let text = format!("{} {}", p.header, p.bullets.join(" "));
        let experience = experiences.iter().copied().find(|id| {
            data.experience(*id)
                .and_then(|e| e.company.as_deref())
                .is_some_and(|company| contains_words(&text, company))
        });
        let id = match data.projects.iter().position(|x| {
            x.meta.origin == Origin::Resume && x.meta.import_key.as_deref() == Some(key.as_str())
        }) {
            Some(idx) => {
                let x = &mut data.projects[idx];
                let meta = x.meta.clone();
                let mut preserved = false;
                let mut changed = false;
                changed |= apply_field(&meta, "name", &mut x.name, &p.name, &mut preserved);
                changed |= apply_field(
                    &meta,
                    "description",
                    &mut x.description,
                    &p.description,
                    &mut preserved,
                );
                changed |= apply_field(&meta, "role", &mut x.role, &p.role, &mut preserved);
                changed |= apply_field(&meta, "url", &mut x.url, &p.url, &mut preserved);
                changed |= apply_field(&meta, "start", &mut x.start, &p.start, &mut preserved);
                changed |= apply_field(&meta, "end", &mut x.end, &p.end, &mut preserved);
                changed |=
                    apply_field(&meta, "current", &mut x.current, &p.current, &mut preserved);
                changed |= apply_field(
                    &meta,
                    "experience",
                    &mut x.experience,
                    &experience,
                    &mut preserved,
                );
                changed |= refresh_meta(
                    &mut x.meta,
                    key.clone(),
                    source,
                    &p.notes,
                    &mut ctx.report.projects,
                );
                x.position = position;
                if changed {
                    x.meta.updated_at = ctx.now;
                }
                if preserved {
                    ctx.report.preserved_edits += 1;
                }
                tally_outcome(&mut ctx.report.projects, changed);
                x.id
            }
            None => {
                let id = ProjectId::derive(&[&ctx.profile.to_string(), &key]);
                data.projects.push(Project {
                    id,
                    name: p.name.clone(),
                    description: p.description.clone(),
                    role: p.role.clone(),
                    url: p.url.clone(),
                    start: p.start,
                    end: p.end,
                    current: p.current,
                    experience,
                    position,
                    meta: new_meta(key, source, p.notes.clone(), ctx.now),
                });
                ctx.report.projects.added += 1;
                id
            }
        };
        ids.push((id, p));
    }
    mark_stale(
        data.projects.iter_mut().map(|x| &mut x.meta),
        &seen_keys,
        &mut ctx.report.projects,
        ctx.now,
    );
    for (id, p) in ids {
        let subject = Subject::Project(id);
        let section = "Projects";
        let what = match &p.description {
            Some(d) => format!("Built {} ({d})", p.name),
            None => format!("Built {}", p.name),
        };
        let period = Period {
            start: p.start,
            end: p.end,
            current: p.current,
        };
        ctx.claim(
            subject,
            ClaimKind::Project,
            "project".into(),
            format!("{what}{}", period_text(period)),
            None,
            Provenance::Extracted,
            Confidence::High,
            ctx.source(&p.header, section),
            None,
        );
        bullet_claims(subject, &p.bullets, section, ctx);
        let mut texts: Vec<&str> = vec![p.header.as_str()];
        texts.extend(p.bullets.iter().map(String::as_str));
        technology_claims(
            subject,
            &p.name,
            p.tech_line.as_deref(),
            &p.technologies,
            &texts,
            section,
            ctx,
        );
        domain_claims(subject, &p.name, &texts, section, ctx);
    }
}

fn contains_words(text: &str, phrase: &str) -> bool {
    let key = search_key(phrase);
    !key.is_empty() && format!(" {} ", search_key(text)).contains(&format!(" {key} "))
}

fn merge_education(data: &mut ProfileData, parsed: &[ParsedEducation], ctx: &mut Ctx<'_>) {
    let mut seen_keys = HashSet::new();
    let mut ids = Vec::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = format!(
            "edu|{}|{}",
            opt_key(Some(&p.institution)),
            opt_key(p.degree.as_deref())
        );
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        ctx.report
            .notes
            .extend(p.notes.iter().map(|n| format!("{}: {n}", p.institution)));
        let source = ctx.source(&p.header, "Education");
        let position = u32::try_from(i).unwrap_or(u32::MAX);
        let institution = opt_key(Some(&p.institution));
        let found = data
            .education
            .iter()
            .position(|x| {
                x.meta.origin == Origin::Resume
                    && x.meta.import_key.as_deref() == Some(key.as_str())
            })
            .or_else(|| {
                // Same institution, degree corrected.
                let candidates: Vec<usize> = data
                    .education
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| {
                        x.meta.origin == Origin::Resume
                            && opt_key(Some(&x.institution)) == institution
                            && !x
                                .meta
                                .import_key
                                .as_deref()
                                .is_some_and(|k| seen_keys.contains(k))
                    })
                    .map(|(idx, _)| idx)
                    .collect();
                (candidates.len() == 1).then(|| candidates[0])
            });
        let id = match found {
            Some(idx) => {
                let x = &mut data.education[idx];
                let meta = x.meta.clone();
                let mut preserved = false;
                let mut changed = false;
                changed |= apply_field(
                    &meta,
                    "institution",
                    &mut x.institution,
                    &p.institution,
                    &mut preserved,
                );
                changed |= apply_field(&meta, "degree", &mut x.degree, &p.degree, &mut preserved);
                changed |= apply_field(&meta, "field", &mut x.field, &p.field, &mut preserved);
                changed |= apply_field(&meta, "start", &mut x.start, &p.start, &mut preserved);
                changed |= apply_field(&meta, "end", &mut x.end, &p.end, &mut preserved);
                changed |=
                    apply_field(&meta, "current", &mut x.current, &p.current, &mut preserved);
                changed |= refresh_meta(
                    &mut x.meta,
                    key.clone(),
                    source,
                    &p.notes,
                    &mut ctx.report.education,
                );
                x.position = position;
                if changed {
                    x.meta.updated_at = ctx.now;
                }
                if preserved {
                    ctx.report.preserved_edits += 1;
                }
                tally_outcome(&mut ctx.report.education, changed);
                x.id
            }
            None => {
                let id = EducationId::derive(&[&ctx.profile.to_string(), &key]);
                data.education.push(Education {
                    id,
                    institution: p.institution.clone(),
                    degree: p.degree.clone(),
                    field: p.field.clone(),
                    start: p.start,
                    end: p.end,
                    current: p.current,
                    position,
                    meta: new_meta(key, source, p.notes.clone(), ctx.now),
                });
                ctx.report.education.added += 1;
                id
            }
        };
        ids.push((id, p));
    }
    mark_stale(
        data.education.iter_mut().map(|x| &mut x.meta),
        &seen_keys,
        &mut ctx.report.education,
        ctx.now,
    );
    for (id, p) in ids {
        let what = match (&p.degree, &p.field) {
            (Some(d), Some(f)) => format!("{d} in {f}, {}", p.institution),
            (Some(d), None) => format!("{d}, {}", p.institution),
            (None, Some(f)) => format!("Studied {f} at {}", p.institution),
            (None, None) => format!("Studied at {}", p.institution),
        };
        let period = Period {
            start: p.start,
            end: p.end,
            current: p.current,
        };
        ctx.claim(
            Subject::Education(id),
            ClaimKind::Education,
            "education".into(),
            format!("{what}{}", period_text(period)),
            None,
            Provenance::Extracted,
            if p.degree.is_some() {
                Confidence::High
            } else {
                Confidence::Medium
            },
            ctx.source(&p.header, "Education"),
            None,
        );
    }
}

fn profile_claims(parsed: &ParsedResume, ctx: &mut Ctx<'_>) {
    for line in &parsed.skills {
        for skill in &line.skills {
            let name = known_technology(skill).map_or_else(|| skill.clone(), |t| t.name.to_owned());
            let key = topic_key(&name);
            let text = match &line.category {
                Some(category) => format!("Lists {name} as a skill ({category})"),
                None => format!("Lists {name} as a skill"),
            };
            ctx.claim(
                Subject::Profile,
                ClaimKind::Skill,
                format!("skill|{key}"),
                text,
                Some(key),
                Provenance::Extracted,
                Confidence::High,
                ctx.source(&line.line, "Skills"),
                None,
            );
        }
    }
    for other in &parsed.other {
        let key = search_key(&other.line);
        if key.is_empty() {
            continue;
        }
        ctx.claim(
            Subject::Profile,
            ClaimKind::Other,
            format!("other|{}|{key}", search_key(&other.section)),
            format!("{}: {}", other.section, other.line),
            None,
            Provenance::Extracted,
            Confidence::High,
            ctx.source(&other.line, &other.section),
            None,
        );
    }
}

/// Word-overlap similarity of two texts (Jaccard over word sets).
fn similarity(a: &str, b: &str) -> f64 {
    let wa: HashSet<String> = search_key(a).split(' ').map(str::to_owned).collect();
    let wb: HashSet<String> = search_key(b).split(' ').map(str::to_owned).collect();
    let inter = wa.intersection(&wb).count() as f64;
    let union = wa.union(&wb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

/// Claims whose identity key does not include their text: the statement
/// can change while the claim stays the same.
fn keyed_by_subject(kind: ClaimKind) -> bool {
    matches!(
        kind,
        ClaimKind::Employment | ClaimKind::Education | ClaimKind::Project
    )
}

fn merge_claims(
    data: &mut ProfileData,
    candidates: Vec<Claim>,
    report: &mut ImportReport,
    now: DateTime<Utc>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut new_claims: Vec<Claim> = Vec::new();
    for candidate in candidates {
        let Some(key) = candidate.import_key.clone() else {
            continue;
        };
        seen.insert(key.clone());
        let existing = data.claims.iter_mut().find(|c| {
            c.provenance != Provenance::UserEntered && c.import_key.as_deref() == Some(key.as_str())
        });
        let Some(c) = existing else {
            new_claims.push(candidate);
            report.claims.added += 1;
            continue;
        };
        let mut changed = false;
        if c.stale_since.take().is_some() {
            report.claims.restored += 1;
            changed = true;
        }
        if !c.edited && c.text != candidate.text {
            if keyed_by_subject(c.kind) && c.verification == Verification::Confirmed {
                // The user confirmed a different statement.
                c.note = Some(format!("You had confirmed: “{}”", c.text));
                c.verification = Verification::Unverified;
                c.verified_at = None;
                report.reconfirm += 1;
            }
            c.text = candidate.text.clone();
            changed = true;
        }
        match c.verification {
            Verification::Confirmed => report.kept_confirmed += 1,
            Verification::Rejected => report.kept_rejected += 1,
            Verification::Unverified => {}
        }
        if c.kind != candidate.kind
            || c.confidence != candidate.confidence
            || c.basis != candidate.basis
            || c.topic != candidate.topic
        {
            c.kind = candidate.kind;
            c.confidence = candidate.confidence;
            c.basis = candidate.basis.clone();
            c.topic = candidate.topic.clone();
            changed = true;
        }
        // New document or snippet: provenance refresh, not a change.
        c.source = candidate.source.clone();
        c.position = candidate.position;
        if changed {
            c.updated_at = now;
        }
        tally_outcome(&mut report.claims, changed);
    }

    // Resume claims the new resume no longer supports become stale.
    let mut newly_stale: Vec<ClaimId> = Vec::new();
    for c in &mut data.claims {
        let from_resume = c.provenance != Provenance::UserEntered && c.import_key.is_some();
        let key = c.import_key.clone().unwrap_or_default();
        if from_resume && !seen.contains(&key) && c.stale_since.is_none() {
            c.stale_since = Some(now);
            c.updated_at = now;
            report.claims.stale += 1;
            if c.verification == Verification::Confirmed {
                report.stale_confirmed += 1;
            }
            newly_stale.push(c.id);
        }
    }

    // A reworded bullet points to the claim it replaced.
    for claim in &mut new_claims {
        if !matches!(
            claim.kind,
            ClaimKind::Accomplishment | ClaimKind::Responsibility
        ) {
            continue;
        }
        let best = data
            .claims
            .iter()
            .filter(|old| {
                newly_stale.contains(&old.id)
                    && old.subject == claim.subject
                    && matches!(
                        old.kind,
                        ClaimKind::Accomplishment | ClaimKind::Responsibility
                    )
            })
            .map(|old| (old.id, similarity(&old.text, &claim.text)))
            .filter(|(_, score)| *score >= 0.5)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        claim.supersedes = best.map(|(id, _)| id);
    }
    data.claims.extend(new_claims);
}

fn merge_skills(
    data: &mut ProfileData,
    parsed: &ParsedResume,
    report: &mut ImportReport,
    now: DateTime<Utc>,
) {
    let profile = data.id();
    // name, category, from the skills section first, then technologies.
    let mut wanted: Vec<(String, String, Option<String>)> = Vec::new();
    let mut push = |name: String, category: Option<String>| {
        let key = topic_key(&name);
        if key.is_empty() || wanted.iter().any(|(_, k, _)| *k == key) {
            return;
        }
        wanted.push((name, key, category));
    };
    for line in &parsed.skills {
        for skill in &line.skills {
            match known_technology(skill) {
                Some(t) => push(
                    t.name.to_owned(),
                    line.category.clone().or(Some(t.category.to_owned())),
                ),
                None => push(skill.clone(), line.category.clone()),
            }
        }
    }
    let live_topics: Vec<(String, Option<String>)> = data
        .claims
        .iter()
        .filter(|c| c.kind == ClaimKind::Technology && c.stale_since.is_none())
        .filter_map(|c| c.topic.clone().map(|t| (t, None)))
        .collect();
    for (topic, _) in live_topics {
        let (name, category) = match known_technology(&topic) {
            Some(t) => (t.name.to_owned(), Some(t.category.to_owned())),
            None => {
                // Keep the spelling from the claim text ("Used Foo at ...").
                let name = data
                    .claims
                    .iter()
                    .find(|c| c.topic.as_deref() == Some(topic.as_str()))
                    .and_then(|c| c.text.strip_prefix("Used "))
                    .and_then(|t| t.split(" at ").next())
                    .map_or_else(|| topic.clone(), str::to_owned);
                (name, None)
            }
        };
        push(name, category);
    }

    let mut seen: HashSet<String> = HashSet::new();
    for (name, key, category) in wanted {
        let import_key = format!("skill|{key}");
        seen.insert(import_key.clone());
        // A skill the user added keeps being the user's.
        if data
            .skills
            .iter()
            .any(|s| s.key == key && s.meta.origin == Origin::User)
        {
            continue;
        }
        match data.skills.iter_mut().find(|s| s.key == key) {
            Some(s) => {
                let meta = s.meta.clone();
                let mut preserved = false;
                let mut changed = apply_field(&meta, "name", &mut s.name, &name, &mut preserved);
                changed |= apply_field(
                    &meta,
                    "category",
                    &mut s.category,
                    &category,
                    &mut preserved,
                );
                changed |= refresh_meta(&mut s.meta, import_key, None, &[], &mut report.skills);
                if changed {
                    s.meta.updated_at = now;
                }
                if preserved {
                    report.preserved_edits += 1;
                }
                tally_outcome(&mut report.skills, changed);
            }
            None => {
                data.skills.push(Skill {
                    id: SkillId::derive(&[&profile.to_string(), &import_key]),
                    name,
                    key,
                    category,
                    meta: new_meta(import_key, None, Vec::new(), now),
                });
                report.skills.added += 1;
            }
        }
    }
    mark_stale(
        data.skills.iter_mut().map(|s| &mut s.meta),
        &seen,
        &mut report.skills,
        now,
    );
}
