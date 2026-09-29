//! The profile as the MCP `get_profile` tool returns it: professional
//! context, never contact details or the resume's raw text.

use jobhunt_profile::{Claim, ClaimKind, EvidenceStrength, LastSeen, ProfileData, Subject};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::preferences::{PreferenceView, StatementView};

/// One experience.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExperienceSummary {
    /// `exp_…`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    /// "Mar 2021 – Present".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    pub current: bool,
    /// Technologies used there (evidence that is not rejected).
    pub technologies: Vec<String>,
    /// Domains worked in there.
    pub domains: Vec<String>,
    /// No longer in the latest resume.
    pub stale: bool,
}

/// A technology or skill and what backs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TechnologyEvidence {
    pub name: String,
    /// `demonstrated` (used in an experience or project), `user_stated`,
    /// `listed` (only in a skills list) or `unsupported`.
    pub strength: String,
    /// Claims backing it (not rejected).
    pub evidence: usize,
    /// Where it was used ("Senior Engineer at Acme").
    pub used_at: Vec<String>,
    /// `current`, or the last year it was used ("2023").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
}

/// A domain or role kind and how much evidence backs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Signal {
    pub name: String,
    pub evidence: usize,
}

/// A claim waiting for the person's decision. Not usable as a fact until
/// confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnresolvedClaim {
    /// `clm_…` (confirm with `narrow claims confirm <id>`).
    pub id: String,
    pub kind: String,
    pub text: String,
    /// Why it needs review.
    pub why: String,
    /// What it is about ("Senior Engineer at Acme", a project's name);
    /// absent for the profile in general.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// `exp_…`, `proj_…` or `edu_…` it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about_id: Option<String>,
    /// `extracted` (read from the resume), `inferred` (concluded by
    /// JobHunt) or `user_entered`.
    #[serde(default)]
    pub provenance: String,
    /// How sure the reading is: `high`, `medium` or `low`.
    #[serde(default)]
    pub confidence: String,
    /// The resume's own words behind it, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// The resume section the words are from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The file the words are from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// For inferred claims: what the inference rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
}

impl UnresolvedClaim {
    pub fn of(data: &ProfileData, c: &Claim) -> Self {
        let (about, about_id) = match c.subject {
            Subject::Profile => (None, None),
            Subject::Experience(id) => {
                (data.experience(id).map(|e| e.label()), Some(id.to_string()))
            }
            Subject::Project(id) => (
                data.projects
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone()),
                Some(id.to_string()),
            ),
            Subject::Education(id) => (
                data.education
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.institution.clone()),
                Some(id.to_string()),
            ),
        };
        let source = c.source.as_ref();
        Self {
            id: c.id.to_string(),
            kind: c.kind.as_str().to_owned(),
            text: c.text.clone(),
            why: data.standing(c).describe().to_owned(),
            about,
            about_id,
            provenance: c.provenance.as_str().to_owned(),
            confidence: c.confidence.as_str().to_owned(),
            snippet: source.map(|s| s.snippet.clone()),
            section: source.and_then(|s| s.section.clone()),
            document: source.and_then(|s| {
                data.documents
                    .iter()
                    .find(|d| d.id == s.document)
                    .and_then(|d| d.file_name.clone())
            }),
            basis: c.basis.clone(),
        }
    }
}

/// A project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectSummary {
    /// `proj_…`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub technologies: Vec<String>,
    pub stale: bool,
}

/// An education entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EducationSummary {
    /// `edu_…`.
    pub id: String,
    pub institution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    pub stale: bool,
}

/// A resume imported into the profile (never its text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSummary {
    /// `doc_…`.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// `pdf`, `text` or `markdown`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    pub first_imported_at: String,
    pub last_imported_at: String,
    /// The most recently imported one: the resume the profile follows.
    pub current: bool,
}

/// The answer of `get_profile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// Where the person lives, as they wrote it (eligibility depends on it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub experiences: Vec<ExperienceSummary>,
    /// Strongest evidence first.
    pub technologies: Vec<TechnologyEvidence>,
    pub domains: Vec<Signal>,
    /// Kinds of engineering role the experience shows (backend, platform).
    pub role_signals: Vec<Signal>,
    /// Seniority and ownership signals (led, mentored, founding).
    pub ownership_signals: Vec<Signal>,
    #[serde(default)]
    pub projects: Vec<ProjectSummary>,
    #[serde(default)]
    pub education: Vec<EducationSummary>,
    /// Imported resumes, the current one first.
    #[serde(default)]
    pub documents: Vec<DocumentSummary>,
    /// Preferences in effect.
    pub preferences: Vec<PreferenceView>,
    /// What the person said, verbatim.
    pub statements: Vec<StatementView>,
    /// Claims needing the person's review (the first few).
    pub needs_review: Vec<UnresolvedClaim>,
    pub needs_review_total: usize,
    /// What is missing or uncertain.
    pub gaps: Vec<String>,
    /// Always true: names and contact details are not part of this view.
    pub contact_details_omitted: bool,
}

const REVIEW_SHOWN: usize = 10;

fn names(claims: &[&Claim]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in claims {
        let name = c.topic.clone().unwrap_or_else(|| c.text.clone());
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl ProfileView {
    pub fn of(data: &ProfileData) -> Self {
        let not_rejected = |c: &&Claim| data.standing(c) != jobhunt_profile::Standing::Rejected;
        let experiences = data
            .visible_experiences()
            .into_iter()
            .map(|e| {
                let about = |kind| -> Vec<&Claim> {
                    data.claims_about(Subject::Experience(e.id), &[kind])
                        .into_iter()
                        .filter(not_rejected)
                        .collect()
                };
                ExperienceSummary {
                    id: e.id.to_string(),
                    title: e.title.clone(),
                    company: e.company.clone(),
                    period: e.period().display(),
                    current: e.current,
                    technologies: names(&about(ClaimKind::Technology)),
                    domains: names(&about(ClaimKind::Domain)),
                    stale: e.meta.is_stale(),
                }
            })
            .collect();
        let technologies = data
            .skills_with_evidence()
            .into_iter()
            .filter(|s| s.strength > EvidenceStrength::Unsupported)
            .map(|s| {
                let mut used_at: Vec<String> = Vec::new();
                for c in s.claims.iter().filter(|c| c.kind == ClaimKind::Technology) {
                    let label = match c.subject {
                        Subject::Experience(id) => data.experience(id).map(|e| e.label()),
                        Subject::Project(id) => data
                            .projects
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| p.name.clone()),
                        _ => None,
                    };
                    if let Some(label) = label
                        && !used_at.contains(&label)
                    {
                        used_at.push(label);
                    }
                }
                TechnologyEvidence {
                    name: s.skill.name.clone(),
                    strength: s.strength.as_str().to_owned(),
                    evidence: s.claims.len(),
                    used_at,
                    last_used: s.last_seen.map(|l| match l {
                        LastSeen::Current => "current".to_owned(),
                        LastSeen::At(d) => d.year().to_string(),
                    }),
                }
            })
            .collect();
        let signal = |(name, claims): (String, Vec<&Claim>)| Signal {
            name,
            evidence: claims.len(),
        };
        let review = data.review_queue();
        Self {
            headline: data.profile.headline.clone(),
            location: data.profile.location.clone(),
            summary: data.profile.summary.clone(),
            experiences,
            technologies,
            domains: data
                .domains()
                .into_iter()
                .map(|d| Signal {
                    name: d.domain,
                    evidence: d.claims.len(),
                })
                .collect(),
            role_signals: data
                .signals(ClaimKind::Role)
                .into_iter()
                .map(signal)
                .collect(),
            ownership_signals: data
                .signals(ClaimKind::Ownership)
                .into_iter()
                .map(signal)
                .collect(),
            projects: data
                .visible_projects()
                .into_iter()
                .map(|p| ProjectSummary {
                    id: p.id.to_string(),
                    name: p.name.clone(),
                    role: p.role.clone(),
                    period: p.period().display(),
                    description: p.description.clone(),
                    technologies: names(
                        &data
                            .claims_about(Subject::Project(p.id), &[ClaimKind::Technology])
                            .into_iter()
                            .filter(not_rejected)
                            .collect::<Vec<_>>(),
                    ),
                    stale: p.meta.is_stale(),
                })
                .collect(),
            education: data
                .visible_education()
                .into_iter()
                .map(|e| EducationSummary {
                    id: e.id.to_string(),
                    institution: e.institution.clone(),
                    degree: e.degree.clone(),
                    field: e.field.clone(),
                    period: e.period().display(),
                    stale: e.meta.is_stale(),
                })
                .collect(),
            documents: {
                let current = data.latest_document().map(|d| d.id);
                let mut docs: Vec<DocumentSummary> = data
                    .documents
                    .iter()
                    .map(|d| DocumentSummary {
                        id: d.id.to_string(),
                        file_name: d.file_name.clone(),
                        kind: d.kind.as_str().to_owned(),
                        pages: d.pages,
                        first_imported_at: crate::views::time(d.first_imported_at),
                        last_imported_at: crate::views::time(d.last_imported_at),
                        current: Some(d.id) == current,
                    })
                    .collect();
                docs.sort_by(|a, b| {
                    b.current
                        .cmp(&a.current)
                        .then(b.last_imported_at.cmp(&a.last_imported_at))
                });
                docs
            },
            preferences: data
                .preferences
                .iter()
                .filter(|p| p.active)
                .map(PreferenceView::of)
                .collect(),
            statements: data.statements.iter().map(StatementView::of).collect(),
            needs_review: review
                .iter()
                .take(REVIEW_SHOWN)
                .map(|c| UnresolvedClaim::of(data, c))
                .collect(),
            needs_review_total: review.len(),
            gaps: data.gaps().into_iter().map(|g| g.message).collect(),
            contact_details_omitted: true,
        }
    }
}

impl LocalApp {
    /// The profile's professional context (see [`ProfileView`]).
    pub async fn profile_view(&self) -> Result<ProfileView, AppError> {
        let data = self.profiles().require().await?;
        Ok(ProfileView::of(&data))
    }
}
