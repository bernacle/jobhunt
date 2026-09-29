//! Several sources, one evidence graph.
//!
//! A record or claim can be supported by more than one imported source: a
//! resume and a LinkedIn export both listing "Senior Engineer at Acme" are
//! one experience and one employment claim, not two. What supports it is
//! kept in two places:
//!
//! * `source`: one source's reference (document, verbatim snippet,
//!   section). While the record is not stale, that source contains it.
//! * `corroborations`: the other sources that contain it, at most one
//!   reference per source ([`Origin`]).
//!
//! Which source a reference belongs to is the [`Origin`] of its document
//! ([`ref_origin`]). A reference without a document in the profile (an old
//! resume import) counts as the resume.
//!
//! An import of one source tells, for every record and claim, whether that
//! source contains it (`Support::attach`) or not (`Support::detach`):
//!
//! * a source containing something stale restores it (the source becomes
//!   its `source`);
//! * a source containing something another source already backs is added
//!   to its corroborations (or refreshed there);
//! * a source no longer containing something is removed from it; if it
//!   was the `source`, a corroborating source takes its place, and only
//!   when none is left does the record become stale (kept, never deleted).
//!
//! With a single source this is exactly the resume re-import rule: present
//! means refreshed (or restored), absent means stale.

use chrono::{DateTime, Utc};

use crate::evidence::{Claim, Provenance};
use crate::ids::DocumentId;
use crate::model::{Origin, RecordMeta, SourceDocument, SourceRef};

/// The source a reference points into.
pub fn ref_origin(documents: &[SourceDocument], source: &SourceRef) -> Origin {
    documents
        .iter()
        .find(|d| d.id == source.document)
        .map_or(Origin::Resume, |d| d.kind.origin())
}

/// The sources supporting a record or claim now, `source` first. Empty
/// when it is stale or was entered by the user.
pub fn supporting_origins(
    documents: &[SourceDocument],
    source: Option<&SourceRef>,
    corroborations: &[SourceRef],
    fallback: Origin,
    stale: bool,
) -> Vec<Origin> {
    let mut out = Vec::new();
    if !stale && fallback.is_imported() {
        out.push(source.map_or(fallback, |s| ref_origin(documents, s)));
    }
    for c in corroborations {
        let origin = ref_origin(documents, c);
        if !out.contains(&origin) {
            out.push(origin);
        }
    }
    out
}

/// The support of one record or claim, borrowed for an import to update.
pub(crate) struct Support<'a> {
    source: &'a mut Option<SourceRef>,
    corroborations: &'a mut Vec<SourceRef>,
    stale_since: &'a mut Option<DateTime<Utc>>,
    /// Whose it is when `source` is empty: the record's origin (the user,
    /// for records they entered), or the resume for imported claims.
    fallback: Origin,
}

impl<'a> Support<'a> {
    pub(crate) fn of_meta(meta: &'a mut RecordMeta) -> Self {
        Self {
            source: &mut meta.source,
            corroborations: &mut meta.corroborations,
            stale_since: &mut meta.stale_since,
            fallback: meta.origin,
        }
    }

    pub(crate) fn of_claim(claim: &'a mut Claim) -> Self {
        let fallback = if claim.provenance == Provenance::UserEntered {
            Origin::User
        } else {
            Origin::Resume
        };
        Self {
            source: &mut claim.source,
            corroborations: &mut claim.corroborations,
            stale_since: &mut claim.stale_since,
            fallback,
        }
    }

    /// The source `source` belongs to.
    pub(crate) fn primary(&self, documents: &[SourceDocument]) -> Origin {
        self.source
            .as_ref()
            .map_or(self.fallback, |s| ref_origin(documents, s))
    }

    /// Whether `origin` currently supports it.
    pub(crate) fn supported_by(&self, origin: Origin, documents: &[SourceDocument]) -> bool {
        (self.stale_since.is_none()
            && self.fallback.is_imported()
            && self.primary(documents) == origin)
            || self
                .corroborations
                .iter()
                .any(|c| ref_origin(documents, c) == origin)
    }

    /// `origin` (read from `document`) contains it, with `reference` as the
    /// words behind it when there are any. Returns [`Attached::Restored`]
    /// when it was stale.
    pub(crate) fn attach(
        &mut self,
        origin: Origin,
        document: DocumentId,
        reference: Option<SourceRef>,
        documents: &[SourceDocument],
    ) -> Attached {
        self.corroborations
            .retain(|c| ref_origin(documents, c) != origin);
        if self.stale_since.is_some() && self.fallback.is_imported() {
            *self.stale_since = None;
            *self.source = reference;
            return Attached::Restored;
        }
        if self.fallback.is_imported() && self.primary(documents) == origin {
            *self.source = reference;
            return Attached::Primary;
        }
        // Another source (or the user) backs it already: corroborate. A
        // reading without a snippet is still recorded, with an empty one.
        self.corroborations
            .push(reference.unwrap_or_else(|| SourceRef {
                document,
                snippet: String::new(),
                section: None,
            }));
        Attached::Corroborated
    }

    /// `origin` does not contain it (any more). Returns whether it became
    /// stale because no source is left.
    pub(crate) fn detach(
        &mut self,
        origin: Origin,
        documents: &[SourceDocument],
        now: DateTime<Utc>,
    ) -> bool {
        self.corroborations
            .retain(|c| ref_origin(documents, c) != origin);
        if self.stale_since.is_some()
            || !self.fallback.is_imported()
            || self.primary(documents) != origin
        {
            return false;
        }
        if !self.corroborations.is_empty() {
            // The next source takes over; nothing became stale.
            *self.source = Some(self.corroborations.remove(0));
            return false;
        }
        *self.stale_since = Some(now);
        true
    }

    /// Drops every reference into `origin`'s documents (the source is being
    /// removed). Returns whether anything else still supports it.
    pub(crate) fn forget(
        &mut self,
        origin: Origin,
        documents: &[SourceDocument],
        now: DateTime<Utc>,
    ) -> bool {
        self.corroborations
            .retain(|c| ref_origin(documents, c) != origin);
        let primary_removed = self.source.is_some() && self.primary(documents) == origin;
        if primary_removed {
            *self.source = if self.corroborations.is_empty() {
                None
            } else {
                Some(self.corroborations.remove(0))
            };
            if self.source.is_none() && self.stale_since.is_none() {
                *self.stale_since = Some(now);
            }
        }
        // What is left: the user's own, another source's live reference (or
        // a resume reading without a snippet), or a corroboration.
        !self.fallback.is_imported()
            || self.stale_since.is_none()
            || !self.corroborations.is_empty()
    }
}

/// What [`Support::attach`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attached {
    /// The source is (still) its main source.
    Primary,
    /// It was stale; the source brought it back.
    Restored,
    /// Another source backs it; this one was added as corroboration.
    Corroborated,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::{DocumentKind, Verification};

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
    }

    fn doc(name: &str, kind: DocumentKind) -> SourceDocument {
        SourceDocument {
            id: DocumentId::derive(&[name]),
            kind,
            file_name: None,
            sha256: "0".repeat(64),
            pages: None,
            text: String::new(),
            parser: "test".into(),
            first_imported_at: at(0),
            last_imported_at: at(0),
        }
    }

    fn reference(document: &SourceDocument, snippet: &str) -> SourceRef {
        SourceRef {
            document: document.id,
            snippet: snippet.into(),
            section: None,
        }
    }

    fn meta(origin: Origin, source: Option<SourceRef>) -> RecordMeta {
        RecordMeta {
            origin,
            source,
            import_key: None,
            verification: Verification::Unverified,
            stale_since: None,
            edited_fields: Vec::new(),
            notes: Vec::new(),
            corroborations: Vec::new(),
            source_snapshots: Vec::new(),
            created_at: at(0),
            updated_at: at(0),
        }
    }

    #[test]
    fn one_source_behaves_like_resume_reimports() {
        let resume = doc("r", DocumentKind::Pdf);
        let docs = vec![resume.clone()];
        let mut m = meta(Origin::Resume, Some(reference(&resume, "a")));
        let mut s = Support::of_meta(&mut m);
        assert!(s.supported_by(Origin::Resume, &docs));
        assert!(s.detach(Origin::Resume, &docs, at(1)), "absent: stale");
        assert!(!s.detach(Origin::Resume, &docs, at(2)), "already stale");
        assert_eq!(
            s.attach(
                Origin::Resume,
                resume.id,
                Some(reference(&resume, "b")),
                &docs
            ),
            Attached::Restored
        );
        assert_eq!(m.stale_since, None);
        assert_eq!(m.source.as_ref().unwrap().snippet, "b");
    }

    #[test]
    fn a_second_source_corroborates_and_takes_over() {
        let resume = doc("r", DocumentKind::Pdf);
        let linkedin = doc("l", DocumentKind::Linkedin);
        let docs = vec![resume.clone(), linkedin.clone()];
        let mut m = meta(Origin::Resume, Some(reference(&resume, "resume words")));
        let mut s = Support::of_meta(&mut m);
        assert_eq!(
            s.attach(
                Origin::Linkedin,
                linkedin.id,
                Some(reference(&linkedin, "li")),
                &docs
            ),
            Attached::Corroborated
        );
        assert_eq!(
            supporting_origins(
                &docs,
                s.source.as_ref(),
                s.corroborations,
                Origin::Resume,
                false
            ),
            vec![Origin::Resume, Origin::Linkedin]
        );
        // Again: refreshed, not duplicated.
        s.attach(
            Origin::Linkedin,
            linkedin.id,
            Some(reference(&linkedin, "li 2")),
            &docs,
        );
        assert_eq!(s.corroborations.len(), 1);
        // The resume drops it: LinkedIn still backs it, so it is not stale.
        assert!(!s.detach(Origin::Resume, &docs, at(1)));
        assert_eq!(m.stale_since, None);
        assert_eq!(m.source.as_ref().unwrap().snippet, "li 2");
        assert!(m.corroborations.is_empty());
        // The resume lists it again: it corroborates LinkedIn now.
        let mut s = Support::of_meta(&mut m);
        assert_eq!(
            s.attach(
                Origin::Resume,
                resume.id,
                Some(reference(&resume, "back")),
                &docs
            ),
            Attached::Corroborated
        );
        // LinkedIn drops it: the resume takes over.
        assert!(!s.detach(Origin::Linkedin, &docs, at(2)));
        assert_eq!(m.source.as_ref().unwrap().snippet, "back");
    }

    #[test]
    fn user_records_are_corroborated_but_never_stale() {
        let linkedin = doc("l", DocumentKind::Linkedin);
        let docs = vec![linkedin.clone()];
        let mut m = meta(Origin::User, None);
        let mut s = Support::of_meta(&mut m);
        assert!(!s.supported_by(Origin::Linkedin, &docs));
        assert_eq!(
            s.attach(
                Origin::Linkedin,
                linkedin.id,
                Some(reference(&linkedin, "li")),
                &docs
            ),
            Attached::Corroborated
        );
        assert!(s.supported_by(Origin::Linkedin, &docs));
        assert!(!s.detach(Origin::Linkedin, &docs, at(1)));
        assert_eq!(m.stale_since, None);
        assert!(m.corroborations.is_empty());
    }

    #[test]
    fn forgetting_a_source_keeps_what_others_support() {
        let resume = doc("r", DocumentKind::Pdf);
        let github = doc("g", DocumentKind::Github);
        let docs = vec![resume.clone(), github.clone()];
        let mut both = meta(Origin::Github, Some(reference(&github, "gh")));
        both.corroborations.push(reference(&resume, "resume"));
        assert!(Support::of_meta(&mut both).forget(Origin::Github, &docs, at(1)));
        assert_eq!(both.source.as_ref().unwrap().snippet, "resume");

        let mut only = meta(Origin::Github, Some(reference(&github, "gh")));
        assert!(!Support::of_meta(&mut only).forget(Origin::Github, &docs, at(1)));
        assert_eq!(only.source, None, "no reference into a removed document");
        assert_eq!(only.stale_since, Some(at(1)));
    }
}
