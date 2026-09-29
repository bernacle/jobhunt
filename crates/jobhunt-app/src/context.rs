//! Application context: the evidence an assistant may use to help the
//! person apply to one opportunity. It prepares facts; it writes nothing
//! (no cover letter, no answers, no tailored resume).
//!
//! The evidence policy is the profile domain's
//! ([`jobhunt_profile::ProfileData::standing`]): a claim is included as a
//! fact only when its standing [`is_usable`](jobhunt_profile::Standing::is_usable)
//! (the person confirmed or entered it, or it is quoted directly from the
//! current resume with high confidence). Inferences, uncertain readings,
//! claims whose source left the resume, and rejected claims are never
//! included, not even their text: only how many were withheld, so the
//! person can review them. An experience or project appears only when the
//! claim that it exists (its employment or project claim) is itself
//! usable. Nothing is paraphrased or summarized: every fact is the claim's
//! text with the resume snippet it rests on.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use jobhunt_profile::infer::topic_key;
use jobhunt_profile::{Claim, ClaimKind, Origin, ProfileData, Standing, Subject, UsableBecause};
use jobhunt_ranking::facets::{Requirement, facets};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::inspect::DESCRIPTION_SUMMARY;
use crate::resolve::Opportunity;
use crate::views::{CompensationView, DecisionView, EligibilityBrief, VerificationBrief};

/// The rule the context follows, stated in every answer so a client knows
/// how to treat it.
pub const POLICY: &str = "Every fact below is usable under JobHunt's evidence policy: confirmed \
    or entered by the person, or quoted directly from their current resume with high confidence. \
    Use facts only as written; do not add metrics, responsibilities or accomplishments that are \
    not listed. Inferred, uncertain, outdated and rejected claims are withheld (see `withheld`).";

/// Experiences (and projects) included at most.
const MAX_SUBJECTS: usize = 6;

/// Why a fact may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsableBasis {
    /// The person confirmed it.
    ConfirmedByYou,
    /// The person entered it.
    EnteredByYou,
    /// Quoted directly from the current resume, with high confidence.
    QuotedFromResume,
}

/// Where a fact comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FactSource {
    /// `doc_…` of the resume.
    pub document: String,
    /// The resume's own words.
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// One usable fact about the person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fact {
    /// `clm_…`.
    pub claim_id: String,
    /// `employment`, `responsibility`, `accomplishment`, `technology`,
    /// `domain`, `role`, `ownership`, `project`, `skill`, `education`, `other`.
    pub kind: String,
    /// The claim, as written (the resume's words, or the person's).
    pub text: String,
    /// Normalized topic for technology, domain and role facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub usable_because: UsableBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<FactSource>,
}

/// An experience or project, with its usable facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceGroup {
    /// `exp_…` or `proj_…`.
    pub id: String,
    /// "Senior Engineer at Acme", or the project name.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    /// What connects it to this job ("uses PostgreSQL, which the job
    /// requires").
    pub relevance: Vec<String>,
    pub facts: Vec<Fact>,
}

/// A technology the job names, and the person's usable evidence for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TechnologyMatch {
    pub name: String,
    /// `required`, `preferred` or `mentioned`, as the posting says.
    pub job_requirement: String,
    /// Usable evidence (empty when there is none).
    pub evidence: Vec<Fact>,
}

/// What the job asks for, as read from the posting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobAsks {
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    pub technologies: Vec<TechnologyMatch>,
    pub domains: Vec<String>,
}

/// The job, briefly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextJob {
    /// `opp_…`.
    pub id: String,
    pub title: String,
    pub company: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_url: Option<String>,
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employment: Option<String>,
    pub compensation: CompensationView,
    /// The start of the posting's description, verbatim.
    pub description_excerpt: String,
}

/// Contact details, included only when explicitly requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContactDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub contacts: Vec<ContactView>,
}

/// One way to reach the person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContactView {
    /// `email`, `phone`, `linkedin`, `github` or `website`.
    pub kind: String,
    pub value: String,
}

/// Claims left out, by why. Their text is never included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Withheld {
    /// Inferred, uncertain, or no longer in the resume: usable once the
    /// person confirms them.
    pub needs_review: usize,
    /// Rejected by the person: never used.
    pub rejected: usize,
    /// How the person can review them.
    pub how_to_review: String,
}

/// The answer of `prepare_application_context`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApplicationContext {
    /// How to use this context.
    pub policy: String,
    pub job: ContextJob,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionView>,
    pub verification: VerificationBrief,
    pub eligibility: EligibilityBrief,
    pub job_asks: JobAsks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// Where the person lives, as they wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Most relevant first.
    pub relevant_experience: Vec<EvidenceGroup>,
    pub relevant_projects: Vec<EvidenceGroup>,
    /// Usable facts about the person as a whole (skills lists,
    /// certifications) that bear on the job.
    pub general: Vec<Fact>,
    /// What the job asks for that no usable evidence covers.
    pub missing_evidence: Vec<String>,
    /// What to be careful about: the brief's caveats and unknowns,
    /// eligibility conditions, verification state.
    pub caveats: Vec<String>,
    pub withheld: Withheld,
    /// Only when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<ContactDetails>,
}

fn basis(standing: Standing) -> Option<UsableBasis> {
    match standing {
        Standing::Usable(UsableBecause::Confirmed) => Some(UsableBasis::ConfirmedByYou),
        Standing::Usable(UsableBecause::UserEntered) => Some(UsableBasis::EnteredByYou),
        Standing::Usable(UsableBecause::Grounded) => Some(UsableBasis::QuotedFromResume),
        Standing::NeedsReview(_) | Standing::Rejected => None,
    }
}

/// The claim as a fact, when the evidence policy allows it.
fn usable(data: &ProfileData, claim: &Claim) -> Option<Fact> {
    let because = basis(data.standing(claim))?;
    Some(Fact {
        claim_id: claim.id.to_string(),
        kind: claim.kind.as_str().to_owned(),
        text: claim.text.clone(),
        topic: claim.topic.clone(),
        usable_because: because,
        source: claim.source.as_ref().map(|s| FactSource {
            document: s.document.to_string(),
            snippet: s.snippet.clone(),
            section: s.section.clone(),
        }),
    })
}

/// Every claim about a subject (the policy decides later which are used).
fn about(data: &ProfileData, subject: Subject) -> Vec<&Claim> {
    let mut claims: Vec<&Claim> = data
        .claims
        .iter()
        .filter(|c| c.subject == subject)
        .collect();
    claims.sort_by_key(|c| c.position);
    claims
}

struct Wanted {
    technologies: BTreeSet<String>,
    domains: BTreeSet<String>,
    roles: BTreeSet<String>,
}

fn relevance(facts: &[Fact], wanted: &Wanted) -> Vec<String> {
    let mut out = Vec::new();
    for f in facts {
        let Some(topic) = &f.topic else { continue };
        let line = match f.kind.as_str() {
            "technology" | "skill" if wanted.technologies.contains(topic) => {
                format!("uses {topic}, which the job asks for")
            }
            "domain" if wanted.domains.contains(topic) => format!("{topic} domain, as the job"),
            "role" if wanted.roles.contains(topic) => format!("{topic} work, as the job"),
            _ => continue,
        };
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

impl ApplicationContext {
    fn build(data: &ProfileData, i: &crate::inspect::Inspection, include_contact: bool) -> Self {
        let record = i
            .checked
            .trust
            .best
            .and_then(|b| i.checked.verified.get(b))
            .map_or_else(|| i.opportunity.main(), |v| &v.record);
        let job = facets(record);
        let wanted = Wanted {
            technologies: job
                .technologies
                .iter()
                .map(|t| topic_key(&t.name))
                .collect(),
            domains: job.domains.iter().map(|d| d.key.value.clone()).collect(),
            roles: job.roles.iter().map(|r| r.key.value.clone()).collect(),
        };

        // Withheld counts cover every claim of the profile.
        let mut withheld = Withheld {
            needs_review: 0,
            rejected: 0,
            how_to_review: "narrow claims --state review, then narrow claims confirm <id>".into(),
        };
        for c in &data.claims {
            match data.standing(c) {
                Standing::NeedsReview(_) => withheld.needs_review += 1,
                Standing::Rejected => withheld.rejected += 1,
                Standing::Usable(_) => {}
            }
        }

        // Experiences: only when the employment claim itself is usable
        // (or the person added the experience), with their usable facts.
        let mut experiences: Vec<(usize, u32, EvidenceGroup)> = Vec::new();
        for e in data.visible_experiences() {
            let claims = about(data, Subject::Experience(e.id));
            let employment_usable = claims
                .iter()
                .filter(|c| c.kind == ClaimKind::Employment)
                .any(|c| data.standing(c).is_usable())
                || (e.meta.origin == Origin::User && !e.meta.is_stale());
            if !employment_usable {
                continue;
            }
            let facts: Vec<Fact> = claims.iter().filter_map(|c| usable(data, c)).collect();
            let relevance = relevance(&facts, &wanted);
            experiences.push((
                relevance.len(),
                e.position,
                EvidenceGroup {
                    id: e.id.to_string(),
                    label: e.label(),
                    period: e.period().display(),
                    relevance,
                    facts,
                },
            ));
        }
        // Most relevant first; resume order (most recent first) otherwise.
        experiences.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        let mut projects: Vec<(usize, u32, EvidenceGroup)> = Vec::new();
        for p in data.visible_projects() {
            let claims = about(data, Subject::Project(p.id));
            let exists = claims
                .iter()
                .filter(|c| c.kind == ClaimKind::Project)
                .any(|c| data.standing(c).is_usable())
                || (p.meta.origin == Origin::User && !p.meta.is_stale());
            if !exists {
                continue;
            }
            let facts: Vec<Fact> = claims.iter().filter_map(|c| usable(data, c)).collect();
            let relevance = relevance(&facts, &wanted);
            projects.push((
                relevance.len(),
                p.position,
                EvidenceGroup {
                    id: p.id.to_string(),
                    label: p.name.clone(),
                    period: p.period().display(),
                    relevance,
                    facts,
                },
            ));
        }
        projects.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        // Irrelevant projects add noise; keep the ones that connect.
        projects.retain(|(score, _, _)| *score > 0);

        // Usable facts from the included subjects and the profile itself.
        let included: Vec<&EvidenceGroup> = experiences
            .iter()
            .map(|(_, _, g)| g)
            .chain(projects.iter().map(|(_, _, g)| g))
            .collect();
        let general: Vec<Fact> = about(data, Subject::Profile)
            .into_iter()
            .filter_map(|c| usable(data, c))
            .filter(|f| {
                f.topic.as_ref().is_some_and(|t| {
                    wanted.technologies.contains(t)
                        || wanted.domains.contains(t)
                        || wanted.roles.contains(t)
                })
            })
            .collect();

        let evidence_for = |topic: &str, kinds: &[&str]| -> Vec<Fact> {
            included
                .iter()
                .flat_map(|g| g.facts.iter())
                .chain(general.iter())
                .filter(|f| f.topic.as_deref() == Some(topic) && kinds.contains(&f.kind.as_str()))
                .cloned()
                .collect()
        };
        let mut missing = Vec::new();
        let technologies: Vec<TechnologyMatch> = job
            .technologies
            .iter()
            .map(|t| {
                let evidence = evidence_for(&topic_key(&t.name), &["technology", "skill"]);
                if evidence.is_empty() && t.requirement >= Requirement::Preferred {
                    missing.push(format!(
                        "No usable evidence of {} ({} by the job)",
                        t.name,
                        t.requirement.as_str()
                    ));
                }
                TechnologyMatch {
                    name: t.name.clone(),
                    job_requirement: t.requirement.as_str().to_owned(),
                    evidence,
                }
            })
            .collect();
        for d in &job.domains {
            if evidence_for(&d.key.value, &["domain"]).is_empty() {
                missing.push(format!(
                    "No usable evidence of {} domain experience",
                    d.key.value_label()
                ));
            }
        }
        if withheld.needs_review > 0 && !missing.is_empty() {
            missing.push(format!(
                "{} claims await review and may cover some of this once confirmed",
                withheld.needs_review
            ));
        }

        let mut caveats: Vec<String> = Vec::new();
        if let Some(r) = &i.ranking {
            caveats.extend(r.brief.caveats.iter().cloned());
            caveats.extend(r.brief.unknowns.iter().cloned());
        }
        if let Some(a) = &i.checked.assessment
            && a.decision.status < jobhunt_eligibility::Eligibility::Eligible
        {
            caveats.push(format!("Eligibility: {}", a.decision.headline));
        }
        if let jobhunt_jobs::verification::Standing::NotTrusted(why) = &i.checked.trust.standing {
            caveats.push(format!("Verification: {why}"));
        }

        let p = &record.posting;
        let mut locations: Vec<String> = p.location.iter().cloned().collect();
        for l in &p.locations {
            if let Some(name) = &l.name
                && !locations.contains(name)
            {
                locations.push(name.clone());
            }
        }
        let excerpt: String = p
            .description_text
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(DESCRIPTION_SUMMARY / 2)
            .collect();
        Self {
            policy: POLICY.to_owned(),
            job: ContextJob {
                id: record.opportunity_id.to_string(),
                title: p.title.clone(),
                company: p.company.clone(),
                url: p.url.to_string(),
                apply_url: p.apply_url.as_ref().map(ToString::to_string),
                locations,
                workplace: p.workplace_type.as_ref().map(|w| w.as_str().to_owned()),
                employment: p.employment_type.as_ref().map(|e| e.as_str().to_owned()),
                compensation: CompensationView::best(record, &i.checked.trust),
                description_excerpt: excerpt,
            },
            decision: i.ranking.as_ref().map(DecisionView::of),
            verification: VerificationBrief::of(&i.checked.trust),
            eligibility: EligibilityBrief::of(i.checked.assessment.as_ref().map(|a| &a.decision)),
            job_asks: JobAsks {
                roles: job.roles.iter().map(|r| r.key.value_label()).collect(),
                level: job.level.as_ref().map(|(l, _)| l.as_str().to_owned()),
                technologies,
                domains: job.domains.iter().map(|d| d.key.value_label()).collect(),
            },
            headline: data.profile.headline.clone(),
            location: data.profile.location.clone(),
            relevant_experience: experiences
                .into_iter()
                .take(MAX_SUBJECTS)
                .map(|(_, _, g)| g)
                .collect(),
            relevant_projects: projects
                .into_iter()
                .take(MAX_SUBJECTS)
                .map(|(_, _, g)| g)
                .collect(),
            general,
            missing_evidence: missing,
            caveats,
            withheld,
            contact: include_contact.then(|| ContactDetails {
                name: data.profile.name.clone(),
                contacts: data
                    .profile
                    .contacts
                    .iter()
                    .map(|c| ContactView {
                        kind: c.kind.as_str().to_owned(),
                        value: c.value.clone(),
                    })
                    .collect(),
            }),
        }
    }
}

impl LocalApp {
    /// The evidence-backed context for applying to one opportunity (see
    /// the module docs). Contact details only with `include_contact`.
    pub async fn application_context(
        &self,
        opportunity: &Opportunity,
        include_contact: bool,
        now: DateTime<Utc>,
    ) -> Result<ApplicationContext, AppError> {
        let data = self.profiles().require().await?;
        let inspection = self.inspect(opportunity, false, now).await?;
        Ok(ApplicationContext::build(
            &data,
            &inspection,
            include_contact,
        ))
    }
}
