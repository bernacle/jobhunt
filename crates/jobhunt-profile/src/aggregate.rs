//! [`ProfileData`]: one profile with everything that belongs to it, and the
//! read-side questions later features will ask of it.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};

use crate::date::PartialDate;
use crate::evidence::{Claim, ClaimKind, ClaimQuery, Provenance, Standing, Subject};
use crate::ids::{ClaimId, ExperienceId, ProfileId};
use crate::infer::topic_key;
use crate::model::{
    Education, EvidenceStrength, Experience, Profile, Project, Skill, SourceDocument, Verification,
};
use crate::preferences::{Preference, PreferenceCategory, PreferenceStatement, PreferencesView};

/// A profile and all its records.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileData {
    pub profile: Profile,
    pub documents: Vec<SourceDocument>,
    pub experiences: Vec<Experience>,
    pub projects: Vec<Project>,
    pub education: Vec<Education>,
    pub skills: Vec<Skill>,
    pub claims: Vec<Claim>,
    pub preferences: Vec<Preference>,
    pub statements: Vec<PreferenceStatement>,
}

/// A skill with what backs it.
#[derive(Debug, Clone)]
pub struct SkillEvidence<'a> {
    pub skill: &'a Skill,
    pub strength: EvidenceStrength,
    /// Technology and skill claims about it that are not rejected.
    pub claims: Vec<&'a Claim>,
    /// The latest time it was used, from its experiences and projects.
    pub last_seen: Option<LastSeen>,
}

/// When a skill was last used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastSeen {
    /// In a current position or project.
    Current,
    /// In one that ended (or, lacking an end, started) then.
    At(PartialDate),
}

/// A domain and the experiences that evidence it.
#[derive(Debug, Clone)]
pub struct DomainEvidence<'a> {
    pub domain: String,
    pub claims: Vec<&'a Claim>,
}

/// Listing order of claim kinds: what the record is, its bullets in the
/// resume's order, then what they show.
fn kind_rank(kind: ClaimKind) -> u8 {
    match kind {
        ClaimKind::Employment | ClaimKind::Education | ClaimKind::Project => 0,
        ClaimKind::Responsibility | ClaimKind::Accomplishment => 1,
        ClaimKind::Technology => 2,
        ClaimKind::Skill => 3,
        ClaimKind::Domain => 4,
        ClaimKind::Role => 5,
        ClaimKind::Ownership => 6,
        ClaimKind::Other => 7,
    }
}

/// Something the profile does not know, or is unsure about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub area: &'static str,
    pub message: String,
}

impl ProfileData {
    /// An empty profile.
    pub fn new(id: ProfileId, now: DateTime<Utc>) -> Self {
        Self {
            profile: Profile::new(id, now),
            documents: Vec::new(),
            experiences: Vec::new(),
            projects: Vec::new(),
            education: Vec::new(),
            skills: Vec::new(),
            claims: Vec::new(),
            preferences: Vec::new(),
            statements: Vec::new(),
        }
    }

    pub fn id(&self) -> ProfileId {
        self.profile.id
    }

    /// Whether nothing has been imported or entered yet.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
            && self.experiences.is_empty()
            && self.projects.is_empty()
            && self.education.is_empty()
            && self.skills.is_empty()
            && self.claims.is_empty()
            && self.preferences.is_empty()
            && self.statements.is_empty()
    }

    /// The most recently imported document.
    pub fn latest_document(&self) -> Option<&SourceDocument> {
        self.documents.iter().max_by_key(|d| d.last_imported_at)
    }

    /// Experiences not rejected, most recent first: current positions,
    /// then by end (or start) date, undated ones last, then resume order.
    pub fn visible_experiences(&self) -> Vec<&Experience> {
        let mut out: Vec<&Experience> = self
            .experiences
            .iter()
            .filter(|e| !e.meta.is_rejected())
            .collect();
        out.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then(b.end.or(b.start).cmp(&a.end.or(a.start)))
                .then(a.position.cmp(&b.position))
        });
        out
    }

    pub fn visible_projects(&self) -> Vec<&Project> {
        let mut out: Vec<&Project> = self
            .projects
            .iter()
            .filter(|p| !p.meta.is_rejected())
            .collect();
        out.sort_by_key(|p| p.position);
        out
    }

    pub fn visible_education(&self) -> Vec<&Education> {
        let mut out: Vec<&Education> = self
            .education
            .iter()
            .filter(|e| !e.meta.is_rejected())
            .collect();
        out.sort_by_key(|e| e.position);
        out
    }

    pub fn experience(&self, id: ExperienceId) -> Option<&Experience> {
        self.experiences.iter().find(|e| e.id == id)
    }

    pub fn claim(&self, id: ClaimId) -> Option<&Claim> {
        self.claims.iter().find(|c| c.id == id)
    }

    /// Whether the claim's subject was rejected (which excludes the claim
    /// from use as well).
    pub fn subject_rejected(&self, subject: Subject) -> bool {
        match subject {
            Subject::Profile => false,
            Subject::Experience(id) => self
                .experiences
                .iter()
                .any(|e| e.id == id && e.meta.is_rejected()),
            Subject::Project(id) => self
                .projects
                .iter()
                .any(|p| p.id == id && p.meta.is_rejected()),
            Subject::Education(id) => self
                .education
                .iter()
                .any(|e| e.id == id && e.meta.is_rejected()),
        }
    }

    /// The evidence policy for a claim in context: [`Claim::standing`],
    /// except that a claim about a rejected record counts as rejected.
    pub fn standing(&self, claim: &Claim) -> Standing {
        if self.subject_rejected(claim.subject) {
            Standing::Rejected
        } else {
            claim.standing()
        }
    }

    /// Claims matching `query`, in subject order then position.
    pub fn claims(&self, query: &ClaimQuery) -> Vec<&Claim> {
        let mut out: Vec<&Claim> = self
            .claims
            .iter()
            .filter(|c| query.matches(c))
            .filter(|c| {
                // Policy filters must also respect rejected subjects.
                (!query.usable_only || self.standing(c).is_usable())
                    && (!query.needs_review || self.standing(c).needs_review())
            })
            .collect();
        let order = self.subject_order();
        out.sort_by_key(|c| {
            (
                order.get(&c.subject).copied().unwrap_or(u32::MAX),
                kind_rank(c.kind),
                c.position,
                c.id,
            )
        });
        out
    }

    fn subject_order(&self) -> HashMap<Subject, u32> {
        let mut order = HashMap::new();
        order.insert(Subject::Profile, 0);
        for e in &self.experiences {
            order.insert(Subject::Experience(e.id), 1_000 + e.position);
        }
        for p in &self.projects {
            order.insert(Subject::Project(p.id), 100_000 + p.position);
        }
        for e in &self.education {
            order.insert(Subject::Education(e.id), 200_000 + e.position);
        }
        order
    }

    /// Claims about one subject, of the given kinds (all kinds if empty),
    /// excluding rejected ones.
    pub fn claims_about(&self, subject: Subject, kinds: &[ClaimKind]) -> Vec<&Claim> {
        self.claims(&ClaimQuery {
            subject: Some(subject),
            kinds: kinds.to_vec(),
            ..ClaimQuery::default()
        })
        .into_iter()
        .filter(|c| c.verification != Verification::Rejected)
        .collect()
    }

    /// Claims about a subject the user confirmed.
    pub fn confirmed_claims(&self, subject: Subject) -> Vec<&Claim> {
        self.claims(&ClaimQuery {
            subject: Some(subject),
            verification: Some(Verification::Confirmed),
            ..ClaimQuery::default()
        })
    }

    /// Topics of non-rejected claims of `kind` about `subject`, in order.
    pub fn topics(&self, subject: Subject, kind: ClaimKind) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for claim in self.claims_about(subject, &[kind]) {
            if let Some(topic) = claim.topic.as_deref()
                && !out.contains(&topic)
            {
                out.push(topic);
            }
        }
        out
    }

    /// Display names of the technologies used in one experience or project.
    pub fn technologies_of(&self, subject: Subject) -> Vec<&str> {
        self.topics(subject, ClaimKind::Technology)
            .into_iter()
            .map(|topic| {
                self.skills
                    .iter()
                    .find(|s| s.key == topic)
                    .map_or(topic, |s| s.name.as_str())
            })
            .collect()
    }

    /// Experiences with evidence of working in `domain` (not rejected).
    pub fn experiences_in_domain(&self, domain: &str) -> Vec<&Experience> {
        let key = topic_key(domain);
        self.visible_experiences()
            .into_iter()
            .filter(|e| {
                self.claims_about(Subject::Experience(e.id), &[ClaimKind::Domain])
                    .iter()
                    .any(|c| c.topic.as_deref() == Some(key.as_str()))
            })
            .collect()
    }

    /// Domains with their evidence, most evidenced first.
    pub fn domains(&self) -> Vec<DomainEvidence<'_>> {
        let mut by_domain: BTreeMap<String, Vec<&Claim>> = BTreeMap::new();
        for claim in &self.claims {
            if claim.kind == ClaimKind::Domain
                && self.standing(claim) != Standing::Rejected
                && let Some(topic) = &claim.topic
            {
                by_domain.entry(topic.clone()).or_default().push(claim);
            }
        }
        let mut out: Vec<DomainEvidence<'_>> = by_domain
            .into_iter()
            .map(|(domain, claims)| DomainEvidence { domain, claims })
            .collect();
        out.sort_by(|a, b| {
            b.claims
                .len()
                .cmp(&a.claims.len())
                .then(a.domain.cmp(&b.domain))
        });
        out
    }

    /// Role kinds (backend, platform, ...) or ownership topics (staff,
    /// mentorship, ...) with their evidence.
    pub fn signals(&self, kind: ClaimKind) -> Vec<(String, Vec<&Claim>)> {
        let mut by_topic: BTreeMap<String, Vec<&Claim>> = BTreeMap::new();
        for claim in &self.claims {
            if claim.kind == kind
                && self.standing(claim) != Standing::Rejected
                && let Some(topic) = &claim.topic
            {
                by_topic.entry(topic.clone()).or_default().push(claim);
            }
        }
        let mut out: Vec<(String, Vec<&Claim>)> = by_topic.into_iter().collect();
        out.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        out
    }

    /// Technology and skill claims about a skill (by its key), including
    /// rejected ones.
    pub fn skill_claims<'a>(&'a self, skill: &Skill) -> Vec<&'a Claim> {
        self.claims
            .iter()
            .filter(|c| {
                matches!(c.kind, ClaimKind::Technology | ClaimKind::Skill)
                    && c.topic.as_deref() == Some(skill.key.as_str())
            })
            .collect()
    }

    /// What backs a skill.
    pub fn skill_evidence<'a>(&'a self, skill: &'a Skill) -> SkillEvidence<'a> {
        let claims: Vec<&Claim> = self
            .skill_claims(skill)
            .into_iter()
            .filter(|c| self.standing(c) != Standing::Rejected)
            .collect();
        let live = |c: &&&Claim| {
            !matches!(
                self.standing(c),
                Standing::NeedsReview(crate::ReviewReason::SourceRemoved)
            )
        };
        let demonstrated = claims
            .iter()
            .filter(live)
            .any(|c| c.kind == ClaimKind::Technology && c.subject != Subject::Profile);
        let user = skill.meta.origin == crate::Origin::User
            || claims
                .iter()
                .any(|c| c.provenance == Provenance::UserEntered);
        let listed = claims
            .iter()
            .filter(live)
            .any(|c| c.kind == ClaimKind::Skill);
        let strength = if skill.meta.is_rejected() {
            EvidenceStrength::Unsupported
        } else if demonstrated {
            EvidenceStrength::Demonstrated
        } else if user {
            EvidenceStrength::UserStated
        } else if listed {
            EvidenceStrength::Listed
        } else {
            EvidenceStrength::Unsupported
        };
        let mut last_seen: Option<LastSeen> = None;
        for claim in claims.iter().filter(|c| c.kind == ClaimKind::Technology) {
            let period = match claim.subject {
                Subject::Experience(id) => self.experience(id).map(Experience::period),
                Subject::Project(id) => self
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map(Project::period),
                _ => None,
            };
            let Some(period) = period else { continue };
            let seen = if period.current {
                Some(LastSeen::Current)
            } else {
                period.end.or(period.start).map(LastSeen::At)
            };
            last_seen = match (last_seen, seen) {
                (Some(LastSeen::Current), _) | (_, Some(LastSeen::Current)) => {
                    Some(LastSeen::Current)
                }
                (Some(LastSeen::At(a)), Some(LastSeen::At(b))) => Some(LastSeen::At(a.max(b))),
                (a, b) => a.or(b),
            };
        }
        SkillEvidence {
            skill,
            strength,
            claims,
            last_seen,
        }
    }

    /// Skills that are not rejected, strongest evidence first, then by how
    /// many claims back them.
    pub fn skills_with_evidence(&self) -> Vec<SkillEvidence<'_>> {
        let mut out: Vec<SkillEvidence<'_>> = self
            .skills
            .iter()
            .filter(|s| !s.meta.is_rejected())
            .map(|s| self.skill_evidence(s))
            .collect();
        out.sort_by(|a, b| {
            b.strength
                .cmp(&a.strength)
                .then(b.claims.len().cmp(&a.claims.len()))
                .then(
                    a.skill
                        .name
                        .to_lowercase()
                        .cmp(&b.skill.name.to_lowercase()),
                )
        });
        out
    }

    /// Technologies used in at least one experience or project.
    pub fn technologies_with_evidence(&self) -> Vec<SkillEvidence<'_>> {
        self.skills_with_evidence()
            .into_iter()
            .filter(|s| s.strength == EvidenceStrength::Demonstrated)
            .collect()
    }

    pub fn preferences(&self) -> PreferencesView<'_> {
        PreferencesView::new(&self.preferences)
    }

    /// Claims that need the user's review, in profile order.
    pub fn review_queue(&self) -> Vec<&Claim> {
        self.claims(&ClaimQuery {
            needs_review: true,
            ..ClaimQuery::default()
        })
    }

    /// What is missing or uncertain, for "profile completeness".
    pub fn gaps(&self) -> Vec<Gap> {
        let mut gaps = Vec::new();
        let mut add = |area: &'static str, message: String| gaps.push(Gap { area, message });
        if self.documents.is_empty() && self.experiences.is_empty() {
            add(
                "profile",
                "no resume imported yet (jobhunt init <resume>)".into(),
            );
        }
        for e in self.visible_experiences() {
            let label = e.label();
            // The parser's notes say it best; add what they do not cover.
            let noted = |word: &str| e.meta.notes.iter().any(|n| n.contains(word));
            for note in &e.meta.notes {
                add("experience", format!("{label}: {note}"));
            }
            if e.title.is_none() && !noted("title") {
                add("experience", format!("{label}: no title"));
            }
            if e.company.is_none()
                && e.employment != Some(crate::EmploymentKind::Freelance)
                && !noted("company")
            {
                add("experience", format!("{label}: no company"));
            }
            if e.start.is_none() && !noted("date") {
                add("experience", format!("{label}: no start date"));
            }
            if e.end.is_none() && !e.current && e.start.is_some() && !noted("date") {
                add("experience", format!("{label}: no end date"));
            }
            if e.meta.is_stale() {
                add("experience", format!("{label}: not in your latest resume"));
            }
        }
        let review = self.review_queue().len();
        if review > 0 {
            add(
                "evidence",
                format!("{review} claims need review (jobhunt claims review)"),
            );
        }
        let categories = [
            (PreferenceCategory::Role, "roles you want"),
            (PreferenceCategory::Compensation, "compensation"),
            (
                PreferenceCategory::Location,
                "location and remote constraints",
            ),
            (PreferenceCategory::Company, "company and team preferences"),
        ];
        let view = self.preferences();
        for (category, what) in categories {
            if view.in_category(category).is_empty() {
                add("preferences", format!("no {what} set"));
            }
        }
        for p in view
            .compensation()
            .minimum
            .iter()
            .chain(&view.compensation().target)
        {
            if let crate::PreferenceValue::Compensation { currency: None, .. } = p.value {
                add(
                    "preferences",
                    format!(
                        "compensation “{}” has no currency; set it with jobhunt preferences set compensation --currency …",
                        p.value
                    ),
                );
            }
        }
        let uncertain = view
            .active()
            .filter(|p| p.certainty == crate::Certainty::Uncertain)
            .count();
        if uncertain > 0 {
            add(
                "preferences",
                format!("{uncertain} preferences were read with doubts; check them"),
            );
        }
        gaps
    }
}
