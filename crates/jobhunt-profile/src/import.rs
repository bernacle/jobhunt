//! Importing sources (a resume, a LinkedIn export, a GitHub account) and
//! importing them again.
//!
//! [`merge_resume`] folds a parsed resume into an existing profile;
//! [`crate::sources::merge_linkedin`] and [`crate::github::merge_github`]
//! fold their sources in with the same engine (`merge_source`), so every
//! source follows one set of rules.
//! It is pure (no storage), so the rules are easy to test and every backend
//! applies the same ones. The rules, written for a resume and true of every
//! source:
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
//!
//! Across sources ([`crate::support`]):
//!
//! * **One graph.** Records of each source have their own import keys
//!   (LinkedIn's start `li|`, GitHub's `gh|`). A record another source
//!   already has (same company and title with overlapping dates; same
//!   project URL or name; same institution and degree) is not duplicated:
//!   the new source is added to its `corroborations`. Claims are keyed by
//!   subject and statement, so the same statement from two sources is one
//!   claim with two sources. Ambiguous matches stay separate.
//! * **No precedence.** The source that created a record owns its fields;
//!   other sources only add evidence. When a source disagrees about a
//!   shared position's dates, its statement becomes a separate claim that
//!   needs review, with the difference as its basis.
//! * **Staleness per source.** A source no longer containing something is
//!   removed from it; it becomes stale only when no source is left.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;

use crate::aggregate::ProfileData;
use crate::basic_support;
use crate::date::{PartialDate, Period};
use crate::evidence::{Claim, ClaimKind, Confidence, Provenance, Subject};
use crate::field_support;
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
use crate::support::{Attached, Support};

/// The id a document gets in a profile: the same file always maps to the
/// same document.
pub fn document_id(profile: ProfileId, sha256: &str) -> DocumentId {
    DocumentId::derive(&[&profile.to_string(), sha256])
}

/// The one document a LinkedIn export or a GitHub account has in a
/// profile. Unlike resumes (one document per file), these sources are
/// updated in place: importing a newer export replaces what Narrow keeps
/// of the older one.
pub fn source_document_id(profile: ProfileId, origin: Origin) -> DocumentId {
    DocumentId::derive(&[&profile.to_string(), "source", origin.as_str()])
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
    /// Already in the profile from another source (or entered by the
    /// user): this source was added as evidence instead of a duplicate.
    pub corroborated: usize,
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
    /// Statements this source makes that disagree with another source
    /// about the same record (different dates for one position). Each is
    /// a separate claim waiting for review.
    pub conflicts: usize,
    /// Parser doubts, per record ("Freelance: no dates found").
    pub notes: Vec<String>,
    /// Lines the parser did not understand.
    pub ignored: Vec<String>,
}

struct Ctx<'a> {
    profile: ProfileId,
    /// The source being imported.
    origin: Origin,
    document: DocumentId,
    /// The profile's documents (without their text), to tell which source
    /// a reference belongs to.
    documents: Vec<SourceDocument>,
    now: DateTime<Utc>,
    claims: Vec<Claim>,
    report: &'a mut ImportReport,
}

/// The profile's documents without their text: enough to tell sources
/// apart, cheap to keep next to a mutable borrow of the records.
pub(crate) fn document_index(data: &ProfileData) -> Vec<SourceDocument> {
    data.documents
        .iter()
        .map(|d| SourceDocument {
            text: String::new(),
            ..d.clone()
        })
        .collect()
}

impl Ctx<'_> {
    /// A record import key, namespaced by source: a resume's keys are
    /// unchanged (`exp|…`), other sources' are prefixed (`li|exp|…`), so two
    /// sources never compete for one key.
    fn key(&self, base: String) -> String {
        match self.origin {
            Origin::Resume | Origin::User => base,
            Origin::Linkedin => format!("li|{base}"),
            Origin::Github => format!("gh|{base}"),
        }
    }

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
            corroborations: Vec::new(),
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
    let report = ImportReport {
        document: Some(document.id),
        first_import: !data
            .documents
            .iter()
            .any(|d| d.kind.origin() == Origin::Resume),
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
    merge_source(
        data,
        Origin::Resume,
        doc_id,
        parsed,
        Vec::new(),
        report,
        now,
    )
}

/// A conclusion about the whole profile a source supports without saying
/// it in so many words (GitHub's "recent hands-on Rust work"). Always an
/// inference: it waits for the user's review.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Inference {
    pub kind: ClaimKind,
    /// Identity across re-imports (prefixed with the source by the caller).
    pub key: String,
    pub text: String,
    pub topic: Option<String>,
    pub confidence: Confidence,
    /// The source's words it rests on, and where they are.
    pub snippet: String,
    pub section: String,
    pub basis: String,
}

/// Folds one source's parsed content (already stored as `document`) into
/// `data`, with the rules in the module docs. `inferences` are profile-level
/// conclusions the source supports.
pub(crate) fn merge_source(
    data: &mut ProfileData,
    origin: Origin,
    document: DocumentId,
    parsed: &ParsedResume,
    inferences: Vec<Inference>,
    mut report: ImportReport,
    now: DateTime<Utc>,
) -> ImportReport {
    merge_basics(data, parsed, origin, &mut report, now);

    let mut ctx = Ctx {
        profile: data.id(),
        origin,
        document,
        documents: document_index(data),
        now,
        claims: Vec::new(),
        report: &mut report,
    };
    let experience_ids = merge_experiences(data, &parsed.experiences, &mut ctx);
    merge_projects(data, &parsed.projects, &experience_ids, &mut ctx);
    merge_education(data, &parsed.education, &mut ctx);
    profile_claims(parsed, &mut ctx);
    for inference in inferences {
        let source = ctx.source(&inference.snippet, &inference.section);
        ctx.claim(
            Subject::Profile,
            inference.kind,
            inference.key,
            inference.text,
            inference.topic,
            Provenance::Inferred,
            inference.confidence,
            source,
            Some(inference.basis),
        );
    }
    let candidates = std::mem::take(&mut ctx.claims);
    let documents = std::mem::take(&mut ctx.documents);
    let report = ctx.report;
    merge_claims(data, candidates, origin, document, &documents, report, now);
    merge_skills(data, parsed, origin, report, now);

    data.profile.updated_at = now;
    std::mem::take(report)
}

fn merge_basics(
    data: &mut ProfileData,
    parsed: &ParsedResume,
    origin: Origin,
    report: &mut ImportReport,
    now: DateTime<Utc>,
) {
    // GitHub's public API has no career basics to import. In particular,
    // an empty GitHub snapshot must not become a purported source for them.
    if origin == Origin::Github {
        return;
    }
    let basics = &parsed.basics;
    let has_resume = data
        .documents
        .iter()
        .any(|d| d.kind.origin() == Origin::Resume);
    let profile = &mut data.profile;
    let mut preserved = false;
    let edited: HashSet<String> = profile.edited_fields.iter().cloned().collect();
    // A resume states the basics; other sources only fill what nothing
    // else stated (no source precedence, no back-and-forth between them).
    let fill_only = origin != Origin::Resume;
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
        if fill_only && target.is_some() {
            return;
        }
        *target = value.clone();
    };
    assign("name", &mut profile.name, &basics.name);
    let managed = origin == Origin::Linkedin || !profile.basic_sources.is_empty();
    if managed {
        preserved |= basic_support::import(profile, basics, origin, has_resume);
    } else {
        assign("headline", &mut profile.headline, &basics.headline);
        assign("location", &mut profile.location, &basics.location);
        assign("summary", &mut profile.summary, &basics.summary);
    }
    if !basics.contacts.is_empty() && !(fill_only && !profile.contacts.is_empty()) {
        if edited.contains("contacts") {
            preserved |= profile.contacts != basics.contacts;
        } else {
            profile.contacts = basics.contacts.clone();
        }
    }
    if !managed && !basics.languages.is_empty() && !(fill_only && !profile.languages.is_empty()) {
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
    origin: Origin,
    key: String,
    source: Option<SourceRef>,
    notes: Vec<String>,
    now: DateTime<Utc>,
) -> RecordMeta {
    RecordMeta {
        origin,
        source,
        import_key: Some(key),
        verification: Verification::Unverified,
        stale_since: None,
        edited_fields: Vec::new(),
        notes,
        corroborations: Vec::new(),
        source_snapshots: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

/// Refreshes the bookkeeping of a record the source created and still
/// contains. Returns whether anything besides the timestamps changed.
fn refresh_meta(
    meta: &mut RecordMeta,
    key: String,
    source: Option<SourceRef>,
    notes: &[String],
    tally: &mut Tally,
    ctx: &Ctx<'_>,
) -> bool {
    let mut changed = false;
    // A new document or snippet is provenance, not a content change.
    if Support::of_meta(meta).attach(ctx.origin, ctx.document, source, &ctx.documents)
        == Attached::Restored
    {
        tally.restored += 1;
        changed = true;
    }
    if meta.import_key.as_deref() != Some(key.as_str()) {
        meta.import_key = Some(key);
        changed = true;
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

/// Adds the source as evidence for a record another source (or the user)
/// has. Nothing of the record changes but its provenance.
fn corroborate(meta: &mut RecordMeta, source: Option<SourceRef>, tally: &mut Tally, ctx: &Ctx<'_>) {
    let mut support = Support::of_meta(meta);
    let already = support.supported_by(ctx.origin, &ctx.documents);
    match support.attach(ctx.origin, ctx.document, source, &ctx.documents) {
        Attached::Restored => {
            tally.restored += 1;
            meta.updated_at = ctx.now;
        }
        _ if already => tally.unchanged += 1,
        _ => tally.corroborated += 1,
    }
}

fn tally_outcome(tally: &mut Tally, changed: bool) {
    if changed {
        tally.updated += 1;
    } else {
        tally.unchanged += 1;
    }
}

/// The source no longer contains these records (they were not `seen`):
/// it stops supporting them, and those it alone supported become stale.
fn release<'a, I>(records: I, seen: &HashSet<String>, tally: &mut Tally, ctx: &Ctx<'_>)
where
    I: Iterator<Item = (String, &'a mut RecordMeta)>,
{
    for (id, meta) in records {
        if seen.contains(&id) {
            continue;
        }
        let mut support = Support::of_meta(meta);
        if !support.supported_by(ctx.origin, &ctx.documents) {
            continue;
        }
        if support.detach(ctx.origin, &ctx.documents, ctx.now) {
            tally.stale += 1;
        }
        meta.updated_at = ctx.now;
    }
}

/// Which records of other origins an import may match: a resume matches
/// what LinkedIn or GitHub created (never the user's own records, as
/// before); LinkedIn and GitHub also add evidence to what the user entered.
fn may_match(importing: Origin, existing: Origin) -> bool {
    existing != importing
        && match importing {
            Origin::Resume => matches!(existing, Origin::Linkedin | Origin::Github),
            Origin::User => false,
            Origin::Linkedin | Origin::Github => true,
        }
}

/// Whether two dates can be the same date: equal at the precision both
/// have. An unknown date agrees with anything (it is not a conflict).
fn dates_agree(a: Option<PartialDate>, b: Option<PartialDate>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.year() == b.year()
                && match (a.month(), b.month()) {
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                }
        }
        _ => true,
    }
}

/// Whether two sources give the same period for one position.
fn periods_agree(a: Period, b: Period) -> bool {
    let ends = match (a.current, b.current) {
        (true, true) => true,
        (true, false) => b.end.is_none(),
        (false, true) => a.end.is_none(),
        (false, false) => dates_agree(a.end, b.end),
    };
    dates_agree(a.start, b.start) && ends
}

/// Whether an existing experience and a source's entry are clearly the
/// same position: same company and title (normalized), and periods that
/// are not known to be disjoint (the same title twice, years apart, is two
/// positions).
fn same_position(e: &Experience, p: &ParsedExperience, today: PartialDate) -> bool {
    let company = opt_key(p.company.as_deref());
    let title = opt_key(p.title.as_deref());
    if company == "?" || title == "?" {
        return false;
    }
    if opt_key(e.company.as_deref()) != company || opt_key(e.title.as_deref()) != title {
        return false;
    }
    let known =
        |period: &Period| period.start.is_some() && (period.current || period.end.is_some());
    let theirs = Period {
        start: p.start,
        end: p.end,
        current: p.current,
    };
    !(known(&e.period()) && known(&theirs)) || e.period().overlaps(&theirs, today)
}

fn experience_key(parsed: &ParsedExperience) -> String {
    format!(
        "exp|{}|{}",
        opt_key(parsed.company.as_deref()),
        opt_key(parsed.title.as_deref())
    )
}

/// How a source's entry relates to the record it was matched with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Match {
    /// A record this source created.
    Own,
    /// Another source's (or the user's) record, and the source agrees with
    /// it.
    Agrees,
    /// Another source's record for the same position, with different
    /// dates: the source's statement is kept apart, for review.
    Disagrees,
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
            ctx.key(if *n == 1 { base } else { format!("{base}#{n}") })
        })
        .collect();

    // Pass 1: exact keys. Pass 2: same company and start date (a corrected
    // title), or the title the user already corrected by hand. Pass 3: the
    // same position another source (or the user) already has.
    let origin = ctx.origin;
    let mut matched: Vec<Option<(usize, Match)>> = vec![None; parsed.len()];
    let mut taken: HashSet<usize> = HashSet::new();
    for (i, key) in keys.iter().enumerate() {
        if let Some(idx) = data.experiences.iter().position(|e| {
            e.meta.origin == origin && e.meta.import_key.as_deref() == Some(key.as_str())
        }) && taken.insert(idx)
        {
            matched[i] = Some((idx, Match::Own));
        }
    }
    for (i, p) in parsed.iter().enumerate() {
        if matched[i].is_some() {
            continue;
        }
        let company = opt_key(p.company.as_deref());
        let found = data.experiences.iter().enumerate().position(|(idx, e)| {
            e.meta.origin == origin
                && !taken.contains(&idx)
                && opt_key(e.company.as_deref()) == company
                && ((e.start.is_some() && e.start == p.start)
                    || (e.meta.is_edited("title")
                        && opt_key(e.title.as_deref()) == opt_key(p.title.as_deref())))
        });
        if let Some(idx) = found {
            taken.insert(idx);
            matched[i] = Some((idx, Match::Own));
        }
    }
    let today = PartialDate::of(ctx.now);
    for (i, p) in parsed.iter().enumerate() {
        if matched[i].is_some() {
            continue;
        }
        let candidates: Vec<usize> = data
            .experiences
            .iter()
            .enumerate()
            .filter(|(idx, e)| {
                !taken.contains(idx)
                    && may_match(origin, e.meta.origin)
                    && same_position(e, p, today)
            })
            .map(|(idx, _)| idx)
            .collect();
        match candidates.as_slice() {
            [idx] => {
                let e = &data.experiences[*idx];
                let theirs = Period {
                    start: p.start,
                    end: p.end,
                    current: p.current,
                };
                let how = if periods_agree(e.period(), theirs) {
                    Match::Agrees
                } else {
                    Match::Disagrees
                };
                taken.insert(*idx);
                matched[i] = Some((*idx, how));
            }
            [] => {}
            _ => ctx.report.notes.push(format!(
                "{}: matches {} positions already in the profile; kept separate",
                p.title.as_deref().unwrap_or("(untitled)"),
                candidates.len()
            )),
        }
    }

    let mut ids = Vec::with_capacity(parsed.len());
    let mut how_matched = Vec::with_capacity(parsed.len());
    let mut seen_ids = HashSet::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = keys[i].clone();
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
        let parsed_fields = field_support::experience(p);
        let (id, how) = match matched[i] {
            Some((idx, Match::Own)) => {
                let e = &mut data.experiences[idx];
                if !e.meta.source_snapshots.is_empty() {
                    field_support::put(&mut e.meta, origin, key.clone(), parsed_fields.clone());
                }
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
                let mut tally = ctx.report.experiences;
                changed |=
                    refresh_meta(&mut e.meta, key.clone(), source, &p.notes, &mut tally, ctx);
                ctx.report.experiences = tally;
                e.position = position;
                if changed {
                    e.meta.updated_at = ctx.now;
                }
                if preserved {
                    ctx.report.preserved_edits += 1;
                }
                tally_outcome(&mut ctx.report.experiences, changed);
                (e.id, Match::Own)
            }
            Some((idx, how)) => {
                let previous = data.experiences[idx].clone();
                field_support::backfill(&previous, &mut data.experiences[idx].meta);
                field_support::put(
                    &mut data.experiences[idx].meta,
                    origin,
                    key.clone(),
                    parsed_fields.clone(),
                );
                let mut tally = ctx.report.experiences;
                corroborate(&mut data.experiences[idx].meta, source, &mut tally, ctx);
                ctx.report.experiences = tally;
                (data.experiences[idx].id, how)
            }
            None => {
                let id = ExperienceId::derive(&[&ctx.profile.to_string(), &key]);
                let mut experience = Experience {
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
                    meta: new_meta(origin, key.clone(), source, p.notes.clone(), ctx.now),
                };
                if origin != Origin::Resume {
                    field_support::put(&mut experience.meta, origin, key, parsed_fields);
                }
                data.experiences.push(experience);
                ctx.report.experiences.added += 1;
                (id, Match::Own)
            }
        };
        seen_ids.insert(id.to_string());
        ids.push(id);
        how_matched.push(how);
    }
    let mut tally = ctx.report.experiences;
    release(
        data.experiences
            .iter_mut()
            .map(|e| (e.id.to_string(), &mut e.meta)),
        &seen_ids,
        &mut tally,
        ctx,
    );
    ctx.report.experiences = tally;
    for e in &mut data.experiences {
        if !seen_ids.contains(&e.id.to_string())
            && e.meta.source_snapshots.iter().any(|s| s.origin == origin)
        {
            field_support::forget(&mut e.meta, origin);
            let meta = e.meta.clone();
            field_support::reconcile(e, &meta, &ctx.documents, field_support::EXPERIENCE_FIELDS);
        }
    }

    for ((p, id), how) in parsed.iter().zip(&ids).zip(how_matched) {
        let Some(experience) = data.experience(*id).cloned() else {
            continue;
        };
        experience_claims(&experience, p, how, &data.documents, ctx);
    }
    ids
}

fn experience_claims(
    e: &Experience,
    p: &ParsedExperience,
    how: Match,
    documents: &[SourceDocument],
    ctx: &mut Ctx<'_>,
) {
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
        if how == Match::Disagrees {
            // The same position, but this source gives other dates: its
            // statement is its own claim, for the user to settle.
            let theirs = e
                .meta
                .source
                .as_ref()
                .map_or(e.meta.origin, |s| crate::support::ref_origin(documents, s));
            ctx.report.conflicts += 1;
            ctx.claim(
                subject,
                ClaimKind::Employment,
                format!("employment|{}", ctx.origin.as_str()),
                format!(
                    "{}: {what}{}",
                    capitalize(ctx.origin.label()),
                    period_text(period)
                ),
                None,
                Provenance::Extracted,
                Confidence::Medium,
                ctx.source(&p.header, section),
                Some(format!(
                    "dates differ from your {}: “{}{}”",
                    theirs.label(),
                    e.label(),
                    period_text(e.period())
                )),
            );
        } else {
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
    let mut add = |name: String, snippet: &str, listed: bool, ctx: &mut Ctx<'_>| {
        let key = topic_key(&name);
        if names.iter().any(|n| topic_key(n) == key) {
            return;
        }
        let (text, confidence) = match ctx.origin {
            // GitHub's language statistics are a fact about the code; a name
            // in a description or topic is only a mention.
            Origin::Github if listed => (format!("{name} code in {at} (GitHub)"), Confidence::High),
            Origin::Github => (
                format!("{at}'s description or topics mention {name}"),
                Confidence::Medium,
            ),
            _ => (format!("Used {name} at {at}"), Confidence::High),
        };
        ctx.claim(
            subject,
            ClaimKind::Technology,
            match ctx.origin {
                Origin::Github if listed => format!("repository-code|{key}"),
                Origin::Github => format!("repository-mention|{key}"),
                _ => format!("technology|{key}"),
            },
            text,
            Some(key),
            Provenance::Extracted,
            confidence,
            ctx.source(snippet, section),
            None,
        );
        names.push(name);
    };
    if let Some(line) = tech_line {
        for item in listed {
            let name = known_technology(item).map_or_else(|| item.clone(), |t| t.name.to_owned());
            add(name, line, true, ctx);
        }
    }
    for text in texts {
        for mention in technologies_in(text, false) {
            add(mention.technology.to_owned(), text, false, ctx);
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
        let text = match ctx.origin {
            Origin::Github => format!("Public project about {domain}: {at}"),
            _ => format!("Worked in {domain} at {at}"),
        };
        ctx.claim(
            subject,
            ClaimKind::Domain,
            if ctx.origin == Origin::Github {
                format!("repository-domain|{domain}")
            } else {
                format!("domain|{domain}")
            },
            text,
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

/// A URL reduced to what identifies it (`github.com/alice/lox`), for
/// matching the same project across sources.
fn url_key(url: &str) -> Option<String> {
    let lower = url.trim().to_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    (!rest.is_empty()).then(|| rest.to_owned())
}

/// Whether an existing project and a source's entry are clearly the same
/// project: the same URL, or the same name (as a whole, or the repository
/// part of `owner/name`).
fn same_project(
    x: &Project,
    p: &ParsedProject,
    experience: Option<ExperienceId>,
    today: PartialDate,
) -> bool {
    if let (Some(a), Some(b)) = (
        x.url.as_deref().and_then(url_key),
        p.url.as_deref().and_then(url_key),
    ) {
        return a == b;
    }
    let short = |name: &str| {
        let key = opt_key(Some(name.rsplit('/').next().unwrap_or(name)));
        (key != "?").then_some(key)
    };
    if short(&x.name).is_none() || short(&x.name) != short(&p.name) {
        return false;
    }
    // A company conflict is decisive. Otherwise the same linked position
    // or an exact, substantial description supplies the missing context;
    // name alone never does.
    if x.experience.is_some() && experience.is_some() && x.experience != experience {
        return false;
    }
    let same_description = x
        .description
        .as_deref()
        .zip(p.description.as_deref())
        .is_some_and(|(a, b)| {
            let a = search_key(a);
            a.len() >= 20 && a == search_key(b)
        });
    if x.experience.is_none_or(|linked| Some(linked) != experience) && !same_description {
        return false;
    }
    let theirs = Period {
        start: p.start,
        end: p.end,
        current: p.current,
    };
    !(x.period().start.is_some()
        && (x.period().current || x.period().end.is_some())
        && theirs.start.is_some()
        && (theirs.current || theirs.end.is_some()))
        || x.period().overlaps(&theirs, today)
}

fn merge_projects(
    data: &mut ProfileData,
    parsed: &[ParsedProject],
    experiences: &[ExperienceId],
    ctx: &mut Ctx<'_>,
) {
    let origin = ctx.origin;
    let mut seen_keys = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut taken: HashSet<usize> = HashSet::new();
    let mut ids = Vec::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = ctx.key(match &p.key {
            Some(key) => format!("proj|{key}"),
            None => format!("proj|{}", opt_key(Some(&p.name))),
        });
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
        let parsed_fields = field_support::project(p, experience);
        let own = data.projects.iter().position(|x| {
            x.meta.origin == origin && x.meta.import_key.as_deref() == Some(key.as_str())
        });
        let other = || -> Result<Option<usize>, usize> {
            if let Some(idx) = data
                .projects
                .iter()
                .enumerate()
                .find(|(idx, x)| {
                    !taken.contains(idx) && field_support::has_key(&x.meta, origin, &key)
                })
                .map(|(idx, _)| idx)
            {
                return Ok(Some(idx));
            }
            let candidates: Vec<usize> = data
                .projects
                .iter()
                .enumerate()
                .filter(|(idx, x)| {
                    !taken.contains(idx)
                        && may_match(origin, x.meta.origin)
                        && same_project(x, p, experience, PartialDate::of(ctx.now))
                })
                .map(|(idx, _)| idx)
                .collect();
            match candidates.as_slice() {
                [] => Ok(None),
                [idx] => Ok(Some(*idx)),
                many => Err(many.len()),
            }
        };
        let id = match (own, other()) {
            (Some(idx), _) => {
                taken.insert(idx);
                let x = &mut data.projects[idx];
                if !x.meta.source_snapshots.is_empty() {
                    field_support::put(&mut x.meta, origin, key.clone(), parsed_fields.clone());
                }
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
                let mut tally = ctx.report.projects;
                changed |=
                    refresh_meta(&mut x.meta, key.clone(), source, &p.notes, &mut tally, ctx);
                ctx.report.projects = tally;
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
            (None, Ok(Some(idx))) => {
                taken.insert(idx);
                let previous = data.projects[idx].clone();
                field_support::backfill(&previous, &mut data.projects[idx].meta);
                if origin == Origin::Github && !data.projects[idx].meta.is_edited("url") {
                    let prior_url = field_support::snapshot(&data.projects[idx].meta, origin)
                        .and_then(|s| s.fields.get("url"))
                        .and_then(serde_json::Value::as_str);
                    if prior_url == data.projects[idx].url.as_deref()
                        && prior_url != p.url.as_deref()
                    {
                        data.projects[idx].url = p.url.clone();
                    }
                }
                field_support::put(
                    &mut data.projects[idx].meta,
                    origin,
                    key.clone(),
                    parsed_fields.clone(),
                );
                let mut tally = ctx.report.projects;
                corroborate(&mut data.projects[idx].meta, source, &mut tally, ctx);
                ctx.report.projects = tally;
                data.projects[idx].id
            }
            (None, found) => {
                if let Err(many) = found {
                    ctx.report.notes.push(format!(
                        "{}: matches {many} projects already in the profile; kept separate",
                        p.name
                    ));
                }
                let id = ProjectId::derive(&[&ctx.profile.to_string(), &key]);
                let mut project = Project {
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
                    meta: new_meta(origin, key.clone(), source, p.notes.clone(), ctx.now),
                };
                if origin != Origin::Resume {
                    field_support::put(&mut project.meta, origin, key, parsed_fields);
                }
                data.projects.push(project);
                ctx.report.projects.added += 1;
                id
            }
        };
        seen_ids.insert(id.to_string());
        ids.push((id, p));
    }
    let mut tally = ctx.report.projects;
    release(
        data.projects
            .iter_mut()
            .map(|x| (x.id.to_string(), &mut x.meta)),
        &seen_ids,
        &mut tally,
        ctx,
    );
    ctx.report.projects = tally;
    for project in &mut data.projects {
        if !seen_ids.contains(&project.id.to_string())
            && project
                .meta
                .source_snapshots
                .iter()
                .any(|s| s.origin == origin)
        {
            field_support::forget(&mut project.meta, origin);
            let meta = project.meta.clone();
            field_support::reconcile(
                project,
                &meta,
                &ctx.documents,
                field_support::PROJECT_FIELDS,
            );
        }
    }
    for (id, p) in ids {
        let subject = Subject::Project(id);
        let section = "Projects";
        let period = Period {
            start: p.start,
            end: p.end,
            current: p.current,
        };
        if origin == Origin::Github {
            // A fact about public code, never about skill: no dates in the
            // statement (activity changes with every push).
            let archived = if p.notes.iter().any(|n| n == "archived") {
                " (archived)"
            } else {
                ""
            };
            ctx.claim(
                subject,
                ClaimKind::Project,
                "repository".into(),
                format!("Owns the public GitHub repository {}{archived}", p.name),
                None,
                Provenance::Extracted,
                Confidence::High,
                ctx.source(&p.header, section),
                None,
            );
        } else {
            let what = match &p.description {
                Some(d) => format!("Built {} ({d})", p.name),
                None => format!("Built {}", p.name),
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
        }
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
    let origin = ctx.origin;
    let mut seen_keys = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut taken: HashSet<usize> = HashSet::new();
    let mut ids = Vec::new();
    for (i, p) in parsed.iter().enumerate() {
        let key = ctx.key(format!(
            "edu|{}|{}",
            opt_key(Some(&p.institution)),
            opt_key(p.degree.as_deref())
        ));
        if !seen_keys.insert(key.clone()) {
            continue;
        }
        ctx.report
            .notes
            .extend(p.notes.iter().map(|n| format!("{}: {n}", p.institution)));
        let source = ctx.source(&p.header, "Education");
        let position = u32::try_from(i).unwrap_or(u32::MAX);
        let institution = opt_key(Some(&p.institution));
        let degree = opt_key(p.degree.as_deref());
        let parsed_fields = field_support::education(p);
        let found = data
            .education
            .iter()
            .position(|x| {
                x.meta.origin == origin && x.meta.import_key.as_deref() == Some(key.as_str())
            })
            .or_else(|| {
                // Same institution, degree corrected.
                let candidates: Vec<usize> = data
                    .education
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| {
                        x.meta.origin == origin
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
        // The same degree another source (or the user) already has.
        let other = found.is_none().then(|| {
            data.education
                .iter()
                .enumerate()
                .filter(|(idx, x)| {
                    !taken.contains(idx)
                        && may_match(origin, x.meta.origin)
                        && opt_key(Some(&x.institution)) == institution
                        && opt_key(x.degree.as_deref()) == degree
                })
                .map(|(idx, _)| idx)
                .collect::<Vec<_>>()
        });
        let id = match (found, other.as_deref()) {
            (Some(idx), _) => {
                taken.insert(idx);
                let x = &mut data.education[idx];
                if !x.meta.source_snapshots.is_empty() {
                    field_support::put(&mut x.meta, origin, key.clone(), parsed_fields.clone());
                }
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
                let mut tally = ctx.report.education;
                changed |=
                    refresh_meta(&mut x.meta, key.clone(), source, &p.notes, &mut tally, ctx);
                ctx.report.education = tally;
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
            (None, Some([idx])) => {
                taken.insert(*idx);
                let previous = data.education[*idx].clone();
                field_support::backfill(&previous, &mut data.education[*idx].meta);
                field_support::put(
                    &mut data.education[*idx].meta,
                    origin,
                    key.clone(),
                    parsed_fields.clone(),
                );
                let mut tally = ctx.report.education;
                corroborate(&mut data.education[*idx].meta, source, &mut tally, ctx);
                ctx.report.education = tally;
                data.education[*idx].id
            }
            (None, other) => {
                if let Some(many) = other.filter(|c| c.len() > 1) {
                    ctx.report.notes.push(format!(
                        "{}: matches {} education entries already in the profile; kept separate",
                        p.institution,
                        many.len()
                    ));
                }
                let id = EducationId::derive(&[&ctx.profile.to_string(), &key]);
                let mut education = Education {
                    id,
                    institution: p.institution.clone(),
                    degree: p.degree.clone(),
                    field: p.field.clone(),
                    start: p.start,
                    end: p.end,
                    current: p.current,
                    position,
                    meta: new_meta(origin, key.clone(), source, p.notes.clone(), ctx.now),
                };
                if origin != Origin::Resume {
                    field_support::put(&mut education.meta, origin, key, parsed_fields);
                }
                data.education.push(education);
                ctx.report.education.added += 1;
                id
            }
        };
        seen_ids.insert(id.to_string());
        ids.push((id, p));
    }
    let mut tally = ctx.report.education;
    release(
        data.education
            .iter_mut()
            .map(|x| (x.id.to_string(), &mut x.meta)),
        &seen_ids,
        &mut tally,
        ctx,
    );
    ctx.report.education = tally;
    for education in &mut data.education {
        if !seen_ids.contains(&education.id.to_string())
            && education
                .meta
                .source_snapshots
                .iter()
                .any(|s| s.origin == origin)
        {
            field_support::forget(&mut education.meta, origin);
            let meta = education.meta.clone();
            field_support::reconcile(
                education,
                &meta,
                &ctx.documents,
                field_support::EDUCATION_FIELDS,
            );
        }
    }
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

/// Whether a candidate's key is a record's one statement of what it is
/// (`exp_…|employment`), as opposed to another source's differing version
/// of it (`exp_…|employment|linkedin`).
fn states_the_record(claim: &Claim) -> bool {
    keyed_by_subject(claim.kind)
        && claim.subject != Subject::Profile
        && claim
            .import_key
            .as_deref()
            .and_then(|k| k.rsplit('|').next())
            .is_some_and(|last| matches!(last, "employment" | "education" | "project"))
}

#[allow(clippy::too_many_arguments)]
fn merge_claims(
    data: &mut ProfileData,
    candidates: Vec<Claim>,
    origin: Origin,
    document: DocumentId,
    documents: &[SourceDocument],
    report: &mut ImportReport,
    now: DateTime<Utc>,
) {
    let mut seen: HashSet<ClaimId> = HashSet::new();
    let mut new_claims: Vec<Claim> = Vec::new();
    for mut candidate in candidates {
        let Some(key) = candidate.import_key.clone() else {
            continue;
        };
        // A hand-corrected assertion replaces the imported wording for
        // this subject and topic. Re-import must not attach the old source
        // to the user's new words or resurrect the superseded assertion.
        if data.claims.iter().any(|c| {
            c.provenance == Provenance::UserEntered
                && c.edited
                && c.import_key.as_deref() == Some(key.as_str())
                && (c.text != candidate.text || !states_the_record(&candidate))
        }) {
            continue;
        }
        // A shared logical record can have different source statements
        // (for example, project descriptions). One source must never take
        // over another source's words merely because the claim key matches.
        let distinct_key = format!("{key}|assertion|{}", origin.as_str());
        let already_distinct = data
            .claims
            .iter()
            .any(|c| c.import_key.as_deref() == Some(distinct_key.as_str()));
        let differing_shared = data.claims.iter().any(|c| {
            c.import_key.as_deref() == Some(key.as_str()) && c.text != candidate.text && {
                let sources = crate::support::supporting_origins(
                    documents,
                    c.source.as_ref(),
                    &c.corroborations,
                    if c.provenance == Provenance::UserEntered {
                        Origin::User
                    } else {
                        Origin::Resume
                    },
                    c.stale_since.is_some(),
                );
                sources.iter().any(|source| *source != origin)
            }
        });
        if already_distinct || differing_shared {
            if let Some(base) = data
                .claims
                .iter_mut()
                .find(|c| c.import_key.as_deref() == Some(key.as_str()))
            {
                Support::of_claim(base).detach(origin, documents, now);
            }
            candidate.import_key = Some(distinct_key.clone());
            candidate.id = ClaimId::derive(&[&data.id().to_string(), &distinct_key]);
        }
        let Some(key) = candidate.import_key.clone() else {
            continue;
        };
        let existing = data.claims.iter_mut().find(|c| {
            c.provenance != Provenance::UserEntered && c.import_key.as_deref() == Some(key.as_str())
        });
        let Some(c) = existing else {
            // The user's own statement of the same record (an experience
            // they added, now in a LinkedIn export too): evidence for it,
            // not a second statement.
            if states_the_record(&candidate)
                && let Some(user) = data.claims.iter_mut().find(|c| {
                    c.provenance == Provenance::UserEntered
                        && c.subject == candidate.subject
                        && c.kind == candidate.kind
                        && c.text == candidate.text
                })
            {
                seen.insert(user.id);
                let mut support = Support::of_claim(user);
                if support.supported_by(origin, documents) {
                    report.claims.unchanged += 1;
                } else {
                    report.claims.corroborated += 1;
                }
                support.attach(origin, document, candidate.source, documents);
                continue;
            }
            seen.insert(candidate.id);
            new_claims.push(candidate);
            report.claims.added += 1;
            continue;
        };
        seen.insert(c.id);
        let mut support = Support::of_claim(c);
        let already = support.supported_by(origin, documents);
        let attached = support.attach(origin, document, candidate.source.clone(), documents);
        match c.verification {
            Verification::Confirmed => report.kept_confirmed += 1,
            Verification::Rejected => report.kept_rejected += 1,
            Verification::Unverified => {}
        }
        if attached == Attached::Corroborated {
            // Another source's claim: this one adds evidence, and never
            // rewrites what that source says.
            if already {
                report.claims.unchanged += 1;
            } else {
                report.claims.corroborated += 1;
            }
            continue;
        }
        let mut changed = false;
        if attached == Attached::Restored {
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
                report.kept_confirmed = report.kept_confirmed.saturating_sub(1);
            }
            c.text = candidate.text.clone();
            changed = true;
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
        c.position = candidate.position;
        if changed {
            c.updated_at = now;
        }
        tally_outcome(&mut report.claims, changed);
    }

    // Claims this source no longer supports lose it; those it alone
    // supported become stale.
    let mut newly_stale: Vec<ClaimId> = Vec::new();
    for c in &mut data.claims {
        if seen.contains(&c.id) || (c.import_key.is_none() && c.corroborations.is_empty()) {
            continue;
        }
        let id = c.id;
        let confirmed = c.verification == Verification::Confirmed;
        let mut support = Support::of_claim(c);
        if !support.supported_by(origin, documents) {
            continue;
        }
        if support.detach(origin, documents, now) {
            report.claims.stale += 1;
            if confirmed {
                report.stale_confirmed += 1;
            }
            newly_stale.push(id);
        }
        c.updated_at = now;
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

/// The display name of a technology a claim is about, from its wording
/// ("Used Foo at …", "Foo code in …").
fn technology_name(text: &str) -> Option<&str> {
    text.strip_prefix("Used ")
        .and_then(|t| t.split(" at ").next())
        .or_else(|| text.split(" code in ").next().filter(|n| *n != text))
}

fn merge_skills(
    data: &mut ProfileData,
    parsed: &ParsedResume,
    origin: Origin,
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
    // Whatever any source still evidences: a technology used somewhere, or
    // a skill another source lists.
    let live_topics: Vec<String> = data
        .claims
        .iter()
        .filter(|c| {
            c.stale_since.is_none()
                && (c.kind == ClaimKind::Technology
                    || (c.kind == ClaimKind::Skill && c.provenance != Provenance::UserEntered))
        })
        .filter_map(|c| c.topic.clone())
        .collect();
    for topic in live_topics {
        let (name, category) = match known_technology(&topic) {
            Some(t) => (t.name.to_owned(), Some(t.category.to_owned())),
            None => {
                // Keep the spelling from the claim text ("Used Foo at ...").
                let name = data
                    .claims
                    .iter()
                    .find(|c| c.topic.as_deref() == Some(topic.as_str()))
                    .and_then(|c| technology_name(&c.text))
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
            Some(s) if s.meta.origin == origin => {
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
                if s.meta.stale_since.take().is_some() {
                    report.skills.restored += 1;
                    changed = true;
                }
                if s.meta.import_key.as_deref() != Some(import_key.as_str()) {
                    s.meta.import_key = Some(import_key);
                    changed = true;
                }
                if !s.meta.notes.is_empty() {
                    s.meta.notes.clear();
                    changed = true;
                }
                if changed {
                    s.meta.updated_at = now;
                }
                if preserved {
                    report.preserved_edits += 1;
                }
                tally_outcome(&mut report.skills, changed);
            }
            Some(s) => {
                // Another source's skill: this one evidences it too.
                if s.meta.stale_since.take().is_some() {
                    report.skills.restored += 1;
                    s.meta.updated_at = now;
                } else {
                    report.skills.unchanged += 1;
                }
            }
            None => {
                data.skills.push(Skill {
                    id: SkillId::derive(&[&profile.to_string(), &import_key]),
                    name,
                    key,
                    category,
                    meta: new_meta(origin, import_key, None, Vec::new(), now),
                });
                report.skills.added += 1;
            }
        }
    }
    // A skill no source evidences any more becomes stale, whichever source
    // first named it.
    for s in &mut data.skills {
        let key = s.meta.import_key.clone().unwrap_or_default();
        if s.meta.origin.is_imported() && !seen.contains(&key) && s.meta.stale_since.is_none() {
            s.meta.stale_since = Some(now);
            s.meta.updated_at = now;
            report.skills.stale += 1;
        }
    }
}
