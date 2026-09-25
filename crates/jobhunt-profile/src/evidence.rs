//! The evidence graph: professional claims and why JobHunt believes them.
//!
//! A [`Claim`] is one statement about the user ("Designed a rules engine
//! for Travel Rule compliance checks", "Used Kubernetes at Ledgerly",
//! "Worked in payments at Ledgerly"). Each claim says:
//!
//! * what is claimed: [`Claim::kind`], [`Claim::text`] and, for technology,
//!   domain and role claims, a normalized [`Claim::topic`];
//! * what it is about: [`Claim::subject`] (an experience, project,
//!   education entry, or the profile as a whole);
//! * where it came from: [`Claim::provenance`] (read from the resume,
//!   inferred by JobHunt, or entered by the user) and [`Claim::source`]
//!   (the document and its verbatim snippet);
//! * how sure JobHunt is: [`Claim::confidence`], plus [`Claim::basis`] for
//!   inferences;
//! * what the user decided: [`Claim::verification`].
//!
//! [`Claim::standing`] turns that into the evidence policy: only claims
//! the user confirmed or entered, and claims quoted directly from the
//! resume with high confidence, are [`Standing::Usable`] for anything that
//! speaks for the user (application answers, later). Inferences are never
//! usable until confirmed, and rejected claims never are.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{ClaimId, EducationId, ExperienceId, ProjectId};
use crate::model::{SourceRef, Verification};

/// What a claim is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Subject {
    /// The user in general (skills lists, certifications, summary).
    Profile,
    Experience(ExperienceId),
    Project(ProjectId),
    Education(EducationId),
}

impl Subject {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Experience(_) => "experience",
            Self::Project(_) => "project",
            Self::Education(_) => "education",
        }
    }

    pub fn id_string(&self) -> Option<String> {
        match self {
            Self::Profile => None,
            Self::Experience(id) => Some(id.to_string()),
            Self::Project(id) => Some(id.to_string()),
            Self::Education(id) => Some(id.to_string()),
        }
    }

    pub fn from_parts(kind: &str, id: Option<&str>) -> Option<Self> {
        match (kind, id) {
            ("profile", None) => Some(Self::Profile),
            ("experience", Some(id)) => id.parse().ok().map(Self::Experience),
            ("project", Some(id)) => id.parse().ok().map(Self::Project),
            ("education", Some(id)) => id.parse().ok().map(Self::Education),
            _ => None,
        }
    }
}

/// What kind of statement a claim makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// Held a title at an organization over a period.
    Employment,
    /// Something the user did as part of a role.
    Responsibility,
    /// A result: shipped, reduced, grew, built something specific.
    Accomplishment,
    /// Used a technology in an experience or project.
    Technology,
    /// Named a skill (skills list).
    Skill,
    /// Worked in a business or problem domain (payments, security, ...).
    Domain,
    /// Has experience in a kind of engineering role (backend, platform, ...).
    Role,
    /// Seniority and ownership signals (led, owned, mentored, founding).
    Ownership,
    /// Studied at an institution.
    Education,
    /// Built or took part in a project.
    Project,
    /// Anything else (certifications, awards, a manual note).
    Other,
}

impl ClaimKind {
    pub const ALL: [ClaimKind; 11] = [
        Self::Employment,
        Self::Responsibility,
        Self::Accomplishment,
        Self::Technology,
        Self::Skill,
        Self::Domain,
        Self::Role,
        Self::Ownership,
        Self::Education,
        Self::Project,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Employment => "employment",
            Self::Responsibility => "responsibility",
            Self::Accomplishment => "accomplishment",
            Self::Technology => "technology",
            Self::Skill => "skill",
            Self::Domain => "domain",
            Self::Role => "role",
            Self::Ownership => "ownership",
            Self::Education => "education",
            Self::Project => "project",
            Self::Other => "other",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

/// How a claim entered the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Read directly from the resume; the snippet is the resume's words.
    Extracted,
    /// Concluded by JobHunt from other evidence (see [`Claim::basis`]).
    Inferred,
    /// Stated by the user.
    UserEntered,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
            Self::UserEntered => "user_entered",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "extracted" => Some(Self::Extracted),
            "inferred" => Some(Self::Inferred),
            "user_entered" => Some(Self::UserEntered),
            _ => None,
        }
    }
}

/// How sure JobHunt is that the claim reflects its source. A coarse,
/// explained label, not a probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// One professional claim and its evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: ClaimId,
    pub kind: ClaimKind,
    /// The claim as a statement.
    pub text: String,
    /// Normalized topic for technology, skill, domain, role and ownership
    /// claims ("kubernetes", "payments", "backend", "mentorship").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub subject: Subject,
    pub provenance: Provenance,
    pub confidence: Confidence,
    #[serde(default)]
    pub verification: Verification,
    /// The document and verbatim snippet supporting the claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    /// For inferred claims: what the inference rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    /// For resume claims: identity across re-imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_key: Option<String>,
    /// An earlier claim this one replaced because the resume's wording
    /// changed (the earlier one is kept, stale).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<ClaimId>,
    /// Order within its subject.
    pub position: u32,
    /// Set when the latest resume import no longer supports the claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<DateTime<Utc>>,
    /// The user's note, such as why they rejected it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The user rewrote the text; re-imports keep their wording.
    #[serde(default)]
    pub edited: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Why a claim may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsableBecause {
    /// The user confirmed it.
    Confirmed,
    /// The user stated it.
    UserEntered,
    /// Quoted from the current resume with high confidence.
    Grounded,
}

/// Why a claim needs the user's review before it may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewReason {
    /// JobHunt concluded it; the resume does not say it in so many words.
    Inferred,
    /// Read from the resume, but the reading may be wrong.
    Uncertain,
    /// No source snippet backs it.
    NoSource,
    /// The latest resume no longer contains its source.
    SourceRemoved,
}

/// The evidence policy's verdict on a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Standing {
    Usable(UsableBecause),
    NeedsReview(ReviewReason),
    /// Rejected by the user; never used.
    Rejected,
}

impl Standing {
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Usable(_))
    }

    pub fn needs_review(&self) -> bool {
        matches!(self, Self::NeedsReview(_))
    }

    /// Short explanation for people.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Usable(UsableBecause::Confirmed) => "confirmed by you",
            Self::Usable(UsableBecause::UserEntered) => "entered by you",
            Self::Usable(UsableBecause::Grounded) => "quoted from your resume",
            Self::NeedsReview(ReviewReason::Inferred) => "inferred by JobHunt; confirm or reject",
            Self::NeedsReview(ReviewReason::Uncertain) => {
                "read from your resume, but the reading is uncertain"
            }
            Self::NeedsReview(ReviewReason::NoSource) => "no source backs it",
            Self::NeedsReview(ReviewReason::SourceRemoved) => {
                "no longer in your latest resume; confirm to keep using it"
            }
            Self::Rejected => "rejected by you; never used",
        }
    }
}

impl Claim {
    /// The evidence policy. In order:
    ///
    /// 1. rejected claims are never used;
    /// 2. a claim whose source left the resume needs review, unless the
    ///    user confirmed it again after that happened;
    /// 3. confirmed and user-entered claims are usable;
    /// 4. extracted claims are usable when they carry a snippet and high
    ///    confidence ("directly grounded");
    /// 5. everything else (inferences, uncertain readings) needs review.
    pub fn standing(&self) -> Standing {
        if self.verification == Verification::Rejected {
            return Standing::Rejected;
        }
        if let Some(stale) = self.stale_since {
            let reconfirmed = self.verification == Verification::Confirmed
                && self.verified_at.is_some_and(|at| at >= stale);
            if !reconfirmed && self.provenance != Provenance::UserEntered {
                return Standing::NeedsReview(ReviewReason::SourceRemoved);
            }
        }
        if self.verification == Verification::Confirmed {
            return Standing::Usable(UsableBecause::Confirmed);
        }
        match self.provenance {
            Provenance::UserEntered => Standing::Usable(UsableBecause::UserEntered),
            Provenance::Inferred => Standing::NeedsReview(ReviewReason::Inferred),
            Provenance::Extracted if self.source.is_none() => {
                Standing::NeedsReview(ReviewReason::NoSource)
            }
            Provenance::Extracted if self.confidence == Confidence::High => {
                Standing::Usable(UsableBecause::Grounded)
            }
            Provenance::Extracted => Standing::NeedsReview(ReviewReason::Uncertain),
        }
    }

    /// Whether the claim may be used in anything that speaks for the user.
    pub fn usable_for_applications(&self) -> bool {
        self.standing().is_usable()
    }

    /// One word for listings: `confirmed`, `rejected`, `stale`,
    /// `user-entered`, `extracted` or `inferred`.
    pub fn state_label(&self) -> &'static str {
        match (self.verification, self.stale_since, self.provenance) {
            (Verification::Rejected, _, _) => "rejected",
            (_, Some(_), _)
                if self.standing() == Standing::NeedsReview(ReviewReason::SourceRemoved) =>
            {
                "stale"
            }
            (Verification::Confirmed, _, _) => "confirmed",
            (_, _, Provenance::UserEntered) => "user-entered",
            (_, _, Provenance::Extracted) => "extracted",
            (_, _, Provenance::Inferred) => "inferred",
        }
    }
}

/// Which claims to list. Filters combine with AND; empty means any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimQuery {
    pub kinds: Vec<ClaimKind>,
    pub subject: Option<Subject>,
    pub provenance: Option<Provenance>,
    pub verification: Option<Verification>,
    /// Normalized topic (see [`crate::infer::topic_key`]).
    pub topic: Option<String>,
    /// Include claims whose source left the resume (default: yes).
    pub exclude_stale: bool,
    /// Only claims the evidence policy says need review.
    pub needs_review: bool,
    /// Only claims usable for applications.
    pub usable_only: bool,
}

impl ClaimQuery {
    pub fn matches(&self, claim: &Claim) -> bool {
        let standing = claim.standing();
        (self.kinds.is_empty() || self.kinds.contains(&claim.kind))
            && self.subject.is_none_or(|s| s == claim.subject)
            && self.provenance.is_none_or(|p| p == claim.provenance)
            && self.verification.is_none_or(|v| v == claim.verification)
            && self
                .topic
                .as_deref()
                .is_none_or(|t| claim.topic.as_deref() == Some(t))
            && !(self.exclude_stale && claim.stale_since.is_some())
            && (!self.needs_review || standing.needs_review())
            && (!self.usable_only || standing.is_usable())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::ids::DocumentId;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
    }

    fn claim(provenance: Provenance, confidence: Confidence) -> Claim {
        Claim {
            id: ClaimId::derive(&["x"]),
            kind: ClaimKind::Accomplishment,
            text: "Designed a rules engine".into(),
            topic: None,
            subject: Subject::Profile,
            provenance,
            confidence,
            verification: Verification::Unverified,
            source: Some(SourceRef {
                document: DocumentId::derive(&["d"]),
                snippet: "Designed a rules engine".into(),
                section: None,
            }),
            basis: None,
            import_key: None,
            supersedes: None,
            position: 0,
            stale_since: None,
            verified_at: None,
            note: None,
            edited: false,
            created_at: at(0),
            updated_at: at(0),
        }
    }

    #[test]
    fn grounded_extractions_are_usable_inferences_are_not() {
        let direct = claim(Provenance::Extracted, Confidence::High);
        assert_eq!(direct.standing(), Standing::Usable(UsableBecause::Grounded));
        assert_eq!(direct.state_label(), "extracted");

        let uncertain = claim(Provenance::Extracted, Confidence::Medium);
        assert_eq!(
            uncertain.standing(),
            Standing::NeedsReview(ReviewReason::Uncertain)
        );

        let mut unsourced = claim(Provenance::Extracted, Confidence::High);
        unsourced.source = None;
        assert_eq!(
            unsourced.standing(),
            Standing::NeedsReview(ReviewReason::NoSource)
        );

        let inferred = claim(Provenance::Inferred, Confidence::High);
        assert_eq!(
            inferred.standing(),
            Standing::NeedsReview(ReviewReason::Inferred),
            "inference is never promoted to truth on its own"
        );
        assert!(!inferred.usable_for_applications());

        let manual = claim(Provenance::UserEntered, Confidence::High);
        assert_eq!(
            manual.standing(),
            Standing::Usable(UsableBecause::UserEntered)
        );
    }

    #[test]
    fn decisions_override_provenance() {
        let mut inferred = claim(Provenance::Inferred, Confidence::Low);
        inferred.verification = Verification::Confirmed;
        inferred.verified_at = Some(at(1));
        assert_eq!(
            inferred.standing(),
            Standing::Usable(UsableBecause::Confirmed)
        );
        assert_eq!(inferred.state_label(), "confirmed");

        let mut rejected = claim(Provenance::Extracted, Confidence::High);
        rejected.verification = Verification::Rejected;
        assert_eq!(rejected.standing(), Standing::Rejected);
        assert_eq!(rejected.state_label(), "rejected");
    }

    #[test]
    fn stale_claims_need_reconfirmation() {
        let mut confirmed = claim(Provenance::Extracted, Confidence::High);
        confirmed.verification = Verification::Confirmed;
        confirmed.verified_at = Some(at(1));
        confirmed.stale_since = Some(at(2));
        assert_eq!(
            confirmed.standing(),
            Standing::NeedsReview(ReviewReason::SourceRemoved)
        );
        assert_eq!(confirmed.state_label(), "stale");
        assert_eq!(
            confirmed.verification,
            Verification::Confirmed,
            "the decision is kept"
        );

        confirmed.verified_at = Some(at(3));
        assert_eq!(
            confirmed.standing(),
            Standing::Usable(UsableBecause::Confirmed)
        );
        assert_eq!(confirmed.state_label(), "confirmed");
    }

    #[test]
    fn queries_filter_by_policy() {
        let direct = claim(Provenance::Extracted, Confidence::High);
        let inferred = claim(Provenance::Inferred, Confidence::Medium);
        let review = ClaimQuery {
            needs_review: true,
            ..ClaimQuery::default()
        };
        assert!(!review.matches(&direct));
        assert!(review.matches(&inferred));
        let usable = ClaimQuery {
            usable_only: true,
            kinds: vec![ClaimKind::Accomplishment],
            ..ClaimQuery::default()
        };
        assert!(usable.matches(&direct));
        assert!(!usable.matches(&inferred));
    }
}
