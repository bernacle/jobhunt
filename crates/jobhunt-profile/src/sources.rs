//! The sources of a profile beyond the resume: importing a LinkedIn
//! export's document, and removing a LinkedIn or GitHub source again.
//!
//! A LinkedIn export and a GitHub account each have one document per
//! profile ([`crate::import::source_document_id`]), updated in place on
//! re-import. Removing one ([`remove_source`]) takes out its document and
//! everything only it supported, unless the user decided about it:
//!
//! * records and claims another source (or the user) also supports stay,
//!   and their provenance moves to what is left;
//! * records and claims only this source supported, that the user never
//!   confirmed, rejected or edited, are deleted;
//! * those the user decided about are kept without a source (stale): a
//!   confirmation needs renewing before use, and a rejection keeps
//!   protecting against a later re-import;
//! * the user's own records and claims are never touched.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::aggregate::ProfileData;
use crate::evidence::{ClaimKind, Provenance, Subject};
use crate::ids::{ClaimId, EducationId, ExperienceId, ProjectId};
use crate::import::{ImportReport, document_index, merge_source, source_document_id};
use crate::model::{Origin, RecordMeta, SourceDocument, Verification};
use crate::resume::ParsedResume;
use crate::support::{Support, ref_origin};

/// Stores `document` as the profile's one document of `origin` (replacing
/// the text of an earlier import) and starts the report.
pub(crate) fn upsert_source_document(
    data: &mut ProfileData,
    origin: Origin,
    mut document: SourceDocument,
    now: DateTime<Utc>,
) -> ImportReport {
    document.id = source_document_id(data.id(), origin);
    let existing = data.documents.iter_mut().find(|d| d.id == document.id);
    let report = ImportReport {
        document: Some(document.id),
        first_import: existing.is_none(),
        same_file: existing
            .as_ref()
            .is_some_and(|d| d.sha256 == document.sha256),
        ..ImportReport::default()
    };
    match existing {
        Some(d) => {
            d.kind = document.kind;
            d.file_name = document.file_name.or(d.file_name.take());
            d.sha256 = document.sha256;
            d.pages = document.pages;
            d.text = document.text;
            d.parser = document.parser;
            d.last_imported_at = now;
        }
        None => {
            document.first_imported_at = now;
            document.last_imported_at = now;
            data.documents.push(document);
        }
    }
    report
}

/// Folds a LinkedIn data export (its career files, read into `parsed`,
/// rendered as `document`'s text) into `data`. See [`crate::import`] for
/// the rules; LinkedIn only fills basics the profile does not have.
pub fn merge_linkedin(
    data: &mut ProfileData,
    document: SourceDocument,
    parsed: &ParsedResume,
    now: DateTime<Utc>,
) -> ImportReport {
    let mut report = upsert_source_document(data, Origin::Linkedin, document, now);
    report.notes.clone_from(&parsed.notes);
    report.ignored.clone_from(&parsed.ignored);
    let document = report
        .document
        .unwrap_or_else(|| source_document_id(data.id(), Origin::Linkedin));
    merge_source(
        data,
        Origin::Linkedin,
        document,
        parsed,
        Vec::new(),
        report,
        now,
    )
}

/// What removing a source did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRemoval {
    /// Its documents, deleted.
    pub documents: usize,
    /// Records only it supported, deleted.
    pub records_deleted: usize,
    /// Records other sources (or the user) still support; kept.
    pub records_kept: usize,
    /// Claims only it supported, deleted.
    pub claims_deleted: usize,
    /// Claims other sources still support; kept, with their provenance
    /// moved to those sources.
    pub claims_kept: usize,
    /// Claims only it supported that the user confirmed, rejected or
    /// rewrote: kept without a source (confirmations need renewing).
    pub decisions_kept: usize,
}

/// Whether a record's or claim's evidence mentions a document of `origin`.
fn cites(
    source: Option<&crate::model::SourceRef>,
    corroborations: &[crate::model::SourceRef],
    origin: Origin,
    documents: &[SourceDocument],
) -> bool {
    source
        .into_iter()
        .chain(corroborations)
        .any(|r| ref_origin(documents, r) == origin)
}

/// Takes `origin` (LinkedIn or GitHub) out of the profile; see the module
/// docs. Returns `None` when the profile has no such source.
pub fn remove_source(
    data: &mut ProfileData,
    origin: Origin,
    now: DateTime<Utc>,
) -> Option<SourceRemoval> {
    let documents = document_index(data);
    let gone: Vec<_> = documents
        .iter()
        .filter(|d| d.kind.origin() == origin)
        .map(|d| d.id)
        .collect();
    if gone.is_empty() || origin == Origin::Resume || origin == Origin::User {
        return None;
    }
    let mut out = SourceRemoval {
        documents: gone.len(),
        ..SourceRemoval::default()
    };

    // Claims first: whether a record may go depends on the decisions about
    // its claims.
    let mut deleted_claims: HashSet<ClaimId> = HashSet::new();
    let mut decided_subjects: HashSet<Subject> = HashSet::new();
    for c in &mut data.claims {
        if c.provenance == Provenance::UserEntered
            || c.verification != Verification::Unverified
            || c.edited
        {
            decided_subjects.insert(c.subject);
        }
        if !cites(c.source.as_ref(), &c.corroborations, origin, &documents) {
            continue;
        }
        let decided = c.verification != Verification::Unverified || c.edited;
        let elsewhere = Support::of_claim(c).forget(origin, &documents, now);
        c.updated_at = now;
        if elsewhere || c.provenance == Provenance::UserEntered {
            out.claims_kept += 1;
        } else if decided {
            out.decisions_kept += 1;
        } else {
            deleted_claims.insert(c.id);
        }
    }

    let mut deleted_subjects: HashSet<Subject> = HashSet::new();
    let mut release = |meta: &mut RecordMeta, subject: Subject| -> bool {
        if !cites(
            meta.source.as_ref(),
            &meta.corroborations,
            origin,
            &documents,
        ) {
            return false;
        }
        let elsewhere = Support::of_meta(meta).forget(origin, &documents, now);
        meta.updated_at = now;
        let untouched = meta.verification == Verification::Unverified
            && meta.edited_fields.is_empty()
            && !decided_subjects.contains(&subject);
        if meta.origin == origin && !elsewhere && untouched {
            deleted_subjects.insert(subject);
            true
        } else {
            false
        }
    };
    let before = data.experiences.len() + data.projects.len() + data.education.len();
    let mut deleted_experiences: HashSet<ExperienceId> = HashSet::new();
    let mut deleted_projects: HashSet<ProjectId> = HashSet::new();
    let mut deleted_education: HashSet<EducationId> = HashSet::new();
    let mut touched = 0usize;
    for e in &mut data.experiences {
        let cited = cites(
            e.meta.source.as_ref(),
            &e.meta.corroborations,
            origin,
            &documents,
        );
        touched += usize::from(cited);
        if release(&mut e.meta, Subject::Experience(e.id)) {
            deleted_experiences.insert(e.id);
        }
    }
    for p in &mut data.projects {
        let cited = cites(
            p.meta.source.as_ref(),
            &p.meta.corroborations,
            origin,
            &documents,
        );
        touched += usize::from(cited);
        if release(&mut p.meta, Subject::Project(p.id)) {
            deleted_projects.insert(p.id);
        }
    }
    for x in &mut data.education {
        let cited = cites(
            x.meta.source.as_ref(),
            &x.meta.corroborations,
            origin,
            &documents,
        );
        touched += usize::from(cited);
        if release(&mut x.meta, Subject::Education(x.id)) {
            deleted_education.insert(x.id);
        }
    }
    data.experiences
        .retain(|e| !deleted_experiences.contains(&e.id));
    data.projects.retain(|p| !deleted_projects.contains(&p.id));
    data.education
        .retain(|x| !deleted_education.contains(&x.id));
    for p in &mut data.projects {
        if p.experience
            .is_some_and(|x| deleted_experiences.contains(&x))
        {
            p.experience = None;
        }
    }
    let after = data.experiences.len() + data.projects.len() + data.education.len();
    out.records_deleted = before - after;
    out.records_kept = touched - out.records_deleted;

    // Claims about deleted records go with them (none was decided, or the
    // record would have been kept).
    let claims_before = data.claims.len();
    data.claims
        .retain(|c| !deleted_claims.contains(&c.id) && !deleted_subjects.contains(&c.subject));
    out.claims_deleted = claims_before - data.claims.len();
    let ids: HashSet<ClaimId> = data.claims.iter().map(|c| c.id).collect();
    for c in &mut data.claims {
        if c.supersedes.is_some_and(|old| !ids.contains(&old)) {
            c.supersedes = None;
        }
    }

    // Skills: one no remaining source evidences is stale; one only this
    // source named, and the user never touched, goes.
    let live: HashSet<String> = data
        .claims
        .iter()
        .filter(|c| {
            c.stale_since.is_none() && matches!(c.kind, ClaimKind::Technology | ClaimKind::Skill)
        })
        .filter_map(|c| c.topic.clone())
        .collect();
    data.skills.retain(|s| {
        !(s.meta.origin == origin
            && !live.contains(&s.key)
            && s.meta.verification == Verification::Unverified
            && s.meta.edited_fields.is_empty())
    });
    for s in &mut data.skills {
        if s.meta.origin.is_imported() && !live.contains(&s.key) && s.meta.stale_since.is_none() {
            s.meta.stale_since = Some(now);
            s.meta.updated_at = now;
        }
    }

    data.documents.retain(|d| !gone.contains(&d.id));
    data.profile.updated_at = now;
    Some(out)
}
