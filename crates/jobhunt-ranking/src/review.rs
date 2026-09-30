//! Semantic review: a model reads the shortlist more closely than the
//! rules can, and its structured answer is validated and folded into the
//! fit by deterministic rules.
//!
//! The rules ([`crate::fit`]) read every job; they are cheap, offline and
//! explainable, and they decide the recommendation on their own when no
//! reviewer is configured. A reviewer ([`FitReviewer`], a model in
//! `jobhunt-ai`) reads only the **shortlist**: jobs the rules found strong
//! or plausible, best first, a bounded number per ranking, and each
//! (candidate, posting, reviewer) at most once ([`review_key`], stored by
//! [`crate::cache::RankingRepository`]).
//!
//! The model never decides the ranking. It answers in JSON of a fixed
//! schema ([`review_schema`]); [`parse_review`] rejects malformed answers
//! whole and drops any point whose quote isn't in the posting (so it can't
//! invent company facts), any aspect outside the vocabulary, and anything
//! about pay, visas or relocation (practicalities, never fit). Then
//! [`apply`] combines:
//!
//! | Rules | Review | Result |
//! | --- | --- | --- |
//! | poor (a material contradiction) | anything | poor |
//! | any | poor, with a quoted contradiction | poor |
//! | strong | plausible or insufficient | plausible (held back) |
//! | strong | strong | strong |
//! | plausible | strong, with quoted role fit and another quoted reason | strong |
//! | plausible | otherwise | plausible |
//! | insufficient | strong or plausible | plausible at most |
//!
//! A review can hold a job back or confirm it; it can raise a job by one
//! step at most, and only on quoted evidence. When the reviewer fails
//! (unavailable, a timeout, a rate limit, a malformed or refused answer),
//! the rules' assessment stands and says so ([`ReviewState::Unavailable`]):
//! a model outage never fills Today with weaker jobs.
//!
//! ## Exactly what is sent
//!
//! [`FitReviewRequest::build`] is the only place a request is assembled:
//!
//! * the candidate ([`CandidateBrief`]): their taste statements (the
//!   composed profile's dimension, value, polarity and whose it is: "said",
//!   "read from their words", "earlier setting", "inferred", "learned"),
//!   the level of their latest title, specialties their career evidence
//!   shows, and for up to 6 visible experiences the title, the years, and
//!   the topics of role and domain claims and technologies (up to 8);
//! * the job ([`JobBrief`]): title, company name, location and workplace
//!   fields, Narrow's rule reading (level, shapes, specialties, company,
//!   team, culture), and the description's content sentences without
//!   benefits and pay boilerplate (at most [`MAX_SENTENCES`] and
//!   [`MAX_JOB_CHARS`] characters).
//!
//! Never sent: the person's name, email, phone, contacts, location,
//! employers' names, experience summaries, resume, LinkedIn or GitHub
//! text, education, feedback reasons, pay preferences, or ids.

use async_trait::async_trait;
use jobhunt_core::StableId;
use jobhunt_profile::taste::TasteProfile;
use jobhunt_profile::{ClaimKind, Polarity, ProfileData, TasteOrigin, TasteReview};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::facets::JobFacets;
use crate::fit::{
    Aspect, ContradictionKind, Firmness, FitAssessment, FitContradiction, FitLevel, FitReason,
    ReviewState, Severity,
};
use crate::person::Person;

/// Revision of the prompt, schema and combination rules. Part of every
/// review's key.
pub const REVIEW_VERSION: &str = "fit-review/1";

/// Content sentences of a posting sent at most.
pub const MAX_SENTENCES: usize = 60;
/// Characters of a posting's content sent at most.
pub const MAX_JOB_CHARS: usize = 7_000;
const MAX_EXPERIENCES: usize = 6;
const MAX_TECHNOLOGIES: usize = 8;
/// Longest reason or quote kept from an answer.
const MAX_POINT_CHARS: usize = 300;
/// Points kept per list.
const MAX_POINTS: usize = 6;

/// The fixed instructions.
pub const SYSTEM_PROMPT: &str = "You assess whether a software engineering job is unusually right \
for one candidate, as a thoughtful recruiter who knows the candidate well would. You are given the \
candidate's taste (what they said they want and avoid, with whose statement it is), their career \
evidence (titles, years, the kind of work, technologies), and one job posting.\n\n\
Judge FIT only: would this person genuinely want this company and role? Consider the shape of \
the work (what they would actually do), its depth (using a technology is not building its \
internals: an application using PostgreSQL is not a PostgreSQL storage engine; running services \
on Kubernetes is not writing its control plane; calling LLM APIs is not training models), the \
seniority the role asks for, and the company and team as the posting describes them.\n\n\
Rules:\n\
- Pay, visas, relocation, time zones and where the job may be done are NOT fit. Never mention \
them.\n\
- Use only what the posting says. Never use what you know about the company from elsewhere; if \
the posting doesn't say how big or how old the company is, that is unknown.\n\
- Every affirmative point and every contradiction must quote the posting verbatim (a short \
exact phrase from the posting text you were given).\n\
- The candidate's own statements (\"said\", \"confirmed\") outweigh Narrow's readings, inferences \
and learned patterns.\n\
- fit is \"strong\" only when the work itself fits and something more does (level, company, \
team, ownership, depth), with nothing that goes against what they want; \"plausible\" when \
something points to it but not enough; \"insufficient\" when little does; \"poor\" when \
something material contradicts what they want (the wrong seniority, a specialization well \
beyond or outside their evidence, a kind of work or company they avoid).\n\
- Write reasons for the candidate, specific to this job, in one sentence each: why they would \
actually want it, not which keywords overlap.";

/// Aspects a review point may be about.
const ASPECTS: [&str; 9] = [
    "role",
    "specialization",
    "seniority",
    "company",
    "team",
    "culture",
    "ownership",
    "domain",
    "work_style",
];

/// Words that make a point about practicalities, never fit.
const PRACTICAL_WORDS: [&str; 14] = [
    "salary",
    "pay",
    "compensation",
    "equity",
    "visa",
    "sponsor",
    "relocat",
    "time zone",
    "timezone",
    "remote",
    "on-site",
    "onsite",
    "hybrid",
    "benefits",
];

/// The candidate, as a reviewer reads them: taste and career evidence,
/// never identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBrief {
    /// "prefer · work_shape: backend (Backend engineering) · said".
    pub taste: Vec<String>,
    /// "senior (latest title)".
    pub level: Option<String>,
    /// "database-engine internals".
    pub specialties: Vec<String>,
    /// "Senior Software Engineer · 2022–present · work: backend, platform
    /// · technologies: Go, PostgreSQL".
    pub experience: Vec<String>,
}

fn whose(a: &jobhunt_profile::taste::ComposedAssertion) -> &'static str {
    if a.origin == TasteOrigin::Stated {
        "said"
    } else if matches!(a.review, TasteReview::Confirmed | TasteReview::Corrected) {
        "confirmed"
    } else {
        match a.origin {
            TasteOrigin::Interpreted => "read from their words",
            TasteOrigin::Legacy => "earlier setting",
            TasteOrigin::Profile => "inferred from their profile",
            TasteOrigin::Learned => "learned from feedback",
            TasteOrigin::Stated => "said",
        }
    }
}

impl CandidateBrief {
    pub fn build(data: &ProfileData, profile: &TasteProfile, person: &Person) -> Self {
        let taste = profile
            .assertions
            .iter()
            .filter(|a| a.polarity != Polarity::Neutral)
            .map(|a| {
                format!(
                    "{} · {}: {} ({}) · {}",
                    a.polarity.as_str(),
                    a.dimension.as_str(),
                    a.value,
                    a.text,
                    whose(a)
                )
            })
            .collect();
        let level = person.level.as_ref().and_then(|(l, _)| {
            crate::facets::Seniority::of(*l).map(|s| format!("{} (latest title)", s.as_str()))
        });
        let specialties = person
            .specialties
            .iter()
            .map(|s| s.specialty.label().to_owned())
            .collect();
        let mut experience = Vec::new();
        for e in data.visible_experiences().into_iter().take(MAX_EXPERIENCES) {
            let subject = jobhunt_profile::Subject::Experience(e.id);
            let topics = |kind: ClaimKind| -> Vec<String> {
                let mut out: Vec<String> = Vec::new();
                for c in data.claims_about(subject, &[kind]) {
                    if !data.standing(c).is_usable() {
                        continue;
                    }
                    if let Some(t) = &c.topic
                        && !out.contains(t)
                    {
                        out.push(t.clone());
                    }
                }
                out
            };
            let mut line = e.title.clone().unwrap_or_else(|| "(untitled role)".into());
            let years = match (e.start, e.end, e.current) {
                (Some(s), _, true) => Some(format!("{}–present", s.year())),
                (Some(s), Some(end), false) => Some(format!("{}–{}", s.year(), end.year())),
                _ => None,
            };
            if let Some(y) = years {
                line.push_str(&format!(" · {y}"));
            }
            let technologies: Vec<String> = data
                .technologies_of(subject)
                .into_iter()
                .take(MAX_TECHNOLOGIES)
                .map(str::to_owned)
                .collect();
            for (label, values) in [
                ("work", topics(ClaimKind::Role)),
                ("domains", topics(ClaimKind::Domain)),
                ("technologies", technologies),
            ] {
                if !values.is_empty() {
                    line.push_str(&format!(" · {label}: {}", values.join(", ")));
                }
            }
            experience.push(line);
        }
        Self {
            taste,
            level,
            specialties,
            experience,
        }
    }

    /// Identifies what a review of this candidate rests on.
    pub fn digest(&self) -> String {
        let text = serde_json::to_string(self).unwrap_or_default();
        StableId::derive("jobhunt.review.candidate", &[&text]).to_hex()
    }
}

/// The job, as a reviewer reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobBrief {
    pub title: String,
    pub company: String,
    /// Location and workplace fields, as published.
    pub setup: String,
    /// Narrow's rule reading, as hints ("level: senior (title)").
    pub reading: Vec<String>,
    /// The description's content sentences.
    pub text: Vec<String>,
}

impl JobBrief {
    pub fn build(record: &jobhunt_jobs::JobRecord, facets: &JobFacets) -> Self {
        let p = &record.posting;
        let mut setup: Vec<String> = Vec::new();
        setup.extend(p.location.clone());
        if let Some(w) = &p.workplace_type {
            setup.push(w.as_str().to_owned());
        }
        let w = &facets.work;
        let mut reading = Vec::new();
        if let Some(l) = &w.level {
            reading.push(format!("level: {} ({:?})", l.seniority.as_str(), l.basis).to_lowercase());
        }
        if !w.shapes.is_empty() {
            let shapes: Vec<String> = w
                .shapes
                .iter()
                .map(|s| format!("{} ({:?})", s.shape, s.basis).to_lowercase())
                .collect();
            reading.push(format!("work: {}", shapes.join(", ")));
        }
        if !w.specialties.is_empty() {
            let s: Vec<&str> = w.specialties.iter().map(|s| s.specialty.label()).collect();
            reading.push(format!("specialized: {}", s.join(", ")));
        }
        if !w.company.is_empty() {
            let c: Vec<&str> = w.company.iter().map(|t| t.value.as_str()).collect();
            reading.push(format!("company: {}", c.join(", ")));
        }
        if let Some(t) = &w.team {
            reading.push(format!("team: {}", t.value));
        }
        let mut text = Vec::new();
        let mut chars = 0;
        for s in crate::facets::content_sentences(p.description_text.as_deref().unwrap_or_default())
            .into_iter()
            .take(MAX_SENTENCES)
        {
            chars += s.chars().count();
            if chars > MAX_JOB_CHARS {
                break;
            }
            text.push(s);
        }
        Self {
            title: p.title.trim().to_owned(),
            company: p.company.trim().to_owned(),
            setup: setup.join(" · "),
            reading,
            text,
        }
    }

    /// Everything a quote may come from, normalized.
    fn quotable(&self) -> String {
        normalize(&format!("{}\n{}", self.title, self.text.join("\n")))
    }
}

/// One review request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitReviewRequest {
    pub candidate: CandidateBrief,
    pub job: JobBrief,
}

impl FitReviewRequest {
    pub fn build(
        candidate: &CandidateBrief,
        record: &jobhunt_jobs::JobRecord,
        facets: &JobFacets,
    ) -> Self {
        Self {
            candidate: candidate.clone(),
            job: JobBrief::build(record, facets),
        }
    }
}

/// The text a model reads after [`SYSTEM_PROMPT`].
pub fn render(request: &FitReviewRequest) -> String {
    let c = &request.candidate;
    let j = &request.job;
    let mut out = String::new();
    out.push_str("<candidate>\n<taste>\n");
    for t in &c.taste {
        out.push_str(&format!("- {t}\n"));
    }
    out.push_str("</taste>\n");
    if let Some(l) = &c.level {
        out.push_str(&format!("<level>{l}</level>\n"));
    }
    if !c.specialties.is_empty() {
        out.push_str(&format!(
            "<demonstrated_specialties>{}</demonstrated_specialties>\n",
            c.specialties.join(", ")
        ));
    }
    out.push_str("<experience>\n");
    for e in &c.experience {
        out.push_str(&format!("- {e}\n"));
    }
    out.push_str("</experience>\n</candidate>\n\n<job>\n");
    out.push_str(&format!(
        "<title>{}</title>\n<company>{}</company>\n",
        j.title, j.company
    ));
    if !j.setup.is_empty() {
        out.push_str(&format!("<setup>{}</setup>\n", j.setup));
    }
    if !j.reading.is_empty() {
        out.push_str("<rule_reading>\n");
        for r in &j.reading {
            out.push_str(&format!("- {r}\n"));
        }
        out.push_str("</rule_reading>\n");
    }
    out.push_str("<posting>\n");
    for s in &j.text {
        out.push_str(s);
        out.push('\n');
    }
    out.push_str("</posting>\n</job>\n");
    out
}

/// The JSON schema of an answer.
pub fn review_schema() -> Value {
    let verdict = json!({"type": "string", "enum": ["match", "partial", "mismatch", "unknown"]});
    let point = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["aspect", "reason", "quote"],
        "properties": {
            "aspect": {"type": "string", "enum": ASPECTS},
            "reason": {"type": "string"},
            "quote": {"type": "string"}
        }
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "fit", "role_fit", "company_fit", "seniority", "specialization",
            "affirmative", "contradictions", "uncertainties"
        ],
        "properties": {
            "fit": {"type": "string", "enum": ["strong", "plausible", "insufficient", "poor"]},
            "role_fit": verdict,
            "company_fit": verdict,
            "seniority": verdict,
            "specialization": verdict,
            "affirmative": {"type": "array", "items": point},
            "contradictions": {"type": "array", "items": point},
            "uncertainties": {"type": "array", "items": {"type": "string"}}
        }
    })
}

/// A quoted point of a review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPoint {
    pub aspect: String,
    pub reason: String,
    pub quote: String,
}

/// A validated review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitReview {
    pub fit: FitLevel,
    pub role_fit: String,
    pub company_fit: String,
    pub seniority: String,
    pub specialization: String,
    pub affirmative: Vec<ReviewPoint>,
    pub contradictions: Vec<ReviewPoint>,
    pub uncertainties: Vec<String>,
    /// `model/anthropic:claude-opus-5-5`.
    pub reviewer: String,
    /// Points dropped in validation.
    pub rejected: usize,
    /// Tokens the model read and wrote, when the provider says.
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// Why a review couldn't be had.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("the reviewer couldn't be reached: {0}")]
    Transport(String),
    #[error("the reviewer answered HTTP {status}")]
    Status { status: u16, retryable: bool },
    #[error("the reviewer declined")]
    Refused,
    #[error("the reviewer's answer was cut off")]
    Truncated,
    #[error("the reviewer's answer is not usable: {0}")]
    Malformed(String),
}

impl ReviewError {
    /// Worth another attempt (the rules' assessment stands meanwhile).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) | Self::Malformed(_) => true,
            Self::Status { retryable, .. } => *retryable,
            Self::Refused | Self::Truncated => false,
        }
    }

    /// A few words for the assessment and the logs (never the answer).
    pub fn short(&self) -> String {
        match self {
            Self::Transport(why) => format!("unreachable ({why})"),
            Self::Status { status: 429, .. } => "rate limited".into(),
            Self::Status { status, .. } => format!("HTTP {status}"),
            Self::Refused => "declined".into(),
            Self::Truncated => "cut off".into(),
            Self::Malformed(_) => "malformed answer".into(),
        }
    }
}

/// Reviews fit with a model (or anything else that can).
#[async_trait]
pub trait FitReviewer: Send + Sync {
    /// `model/anthropic:claude-opus-5-5`: part of every review's key.
    fn name(&self) -> String;

    async fn review(&self, request: &FitReviewRequest) -> Result<FitReview, ReviewError>;
}

/// The key a review is stored under: the reviewer, the prompt and rules
/// revision, what was sent about the candidate and about the job. Change
/// any of them and the job is reviewed again; otherwise never twice.
pub fn review_key(request: &FitReviewRequest, reviewer: &str) -> String {
    let job = serde_json::to_string(&request.job).unwrap_or_default();
    format!(
        "rev_{}",
        StableId::derive(
            "jobhunt.review",
            &[
                REVIEW_VERSION,
                crate::fit::FIT_RULES,
                reviewer,
                &request.candidate.digest(),
                &job
            ]
        )
    )
}

fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        let c = match c {
            '’' | '‘' => '\'',
            '“' | '”' => '"',
            '–' | '—' => '-',
            c => c,
        };
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.extend(c.to_lowercase());
            space = false;
        }
    }
    out.trim().to_owned()
}

fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect::<String>().trim().to_owned()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPoint {
    aspect: String,
    reason: String,
    quote: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReview {
    fit: String,
    role_fit: String,
    company_fit: String,
    seniority: String,
    specialization: String,
    affirmative: Vec<RawPoint>,
    contradictions: Vec<RawPoint>,
    uncertainties: Vec<String>,
}

/// Validates an answer against its request. Malformed JSON, unknown
/// fields or values reject the answer whole; a point without a verbatim
/// quote from the posting, outside the vocabulary, or about practicalities
/// is dropped and counted.
pub fn parse_review(
    text: &str,
    request: &FitReviewRequest,
    reviewer: &str,
) -> Result<FitReview, ReviewError> {
    let raw: RawReview = serde_json::from_str(text.trim())
        .map_err(|e| ReviewError::Malformed(format!("not the expected JSON: {e}")))?;
    let mut fit = FitLevel::parse(&raw.fit)
        .ok_or_else(|| ReviewError::Malformed(format!("unknown fit {:?}", raw.fit)))?;
    let verdicts = ["match", "partial", "mismatch", "unknown"];
    for v in [
        &raw.role_fit,
        &raw.company_fit,
        &raw.seniority,
        &raw.specialization,
    ] {
        if !verdicts.contains(&v.as_str()) {
            return Err(ReviewError::Malformed(format!("unknown verdict {v:?}")));
        }
    }
    let quotable = request.job.quotable();
    let mut rejected = 0;
    let mut keep = |points: Vec<RawPoint>| -> Vec<ReviewPoint> {
        let mut out = Vec::new();
        for p in points {
            let quote = normalize(&p.quote);
            let reason = clip_chars(p.reason.trim(), MAX_POINT_CHARS);
            let lower = reason.to_lowercase();
            let ok = ASPECTS.contains(&p.aspect.as_str())
                && quote.chars().count() >= 4
                && quotable.contains(&quote)
                && !reason.is_empty()
                && !PRACTICAL_WORDS.iter().any(|w| lower.contains(w))
                && out.len() < MAX_POINTS;
            if ok {
                out.push(ReviewPoint {
                    aspect: p.aspect,
                    reason,
                    quote: clip_chars(p.quote.trim(), MAX_POINT_CHARS),
                });
            } else {
                rejected += 1;
            }
        }
        out
    };
    let affirmative = keep(raw.affirmative);
    let contradictions = keep(raw.contradictions);
    // A verdict needs its evidence: no strong fit without a quoted role
    // fit and another quoted reason, no poor fit without a quoted
    // contradiction.
    let role = affirmative.iter().any(|p| p.aspect == "role");
    let other = affirmative.iter().any(|p| p.aspect != "role");
    if fit == FitLevel::Strong && !(role && other) {
        fit = FitLevel::Plausible;
    }
    if fit == FitLevel::Poor && contradictions.is_empty() {
        fit = FitLevel::Insufficient;
    }
    let uncertainties = raw
        .uncertainties
        .into_iter()
        .map(|u| clip_chars(u.trim(), MAX_POINT_CHARS))
        .filter(|u| {
            let l = u.to_lowercase();
            !u.is_empty() && !PRACTICAL_WORDS.iter().any(|w| l.contains(w))
        })
        .take(MAX_POINTS)
        .collect();
    Ok(FitReview {
        fit,
        role_fit: raw.role_fit,
        company_fit: raw.company_fit,
        seniority: raw.seniority,
        specialization: raw.specialization,
        affirmative,
        contradictions,
        uncertainties,
        reviewer: reviewer.to_owned(),
        rejected,
        input_tokens: 0,
        output_tokens: 0,
    })
}

fn aspect_of(name: &str) -> Aspect {
    match name {
        "role" => Aspect::Role,
        "specialization" => Aspect::Specialization,
        "seniority" => Aspect::Seniority,
        "company" => Aspect::Company,
        "team" => Aspect::Team,
        "culture" => Aspect::Culture,
        "ownership" => Aspect::Ownership,
        "domain" => Aspect::Domain,
        _ => Aspect::WorkStyle,
    }
}

fn kind_of(name: &str) -> ContradictionKind {
    match name {
        "seniority" => ContradictionKind::Seniority,
        "specialization" => ContradictionKind::RoleDepth,
        "role" => ContradictionKind::WorkShape,
        "company" => ContradictionKind::CompanyShape,
        "team" => ContradictionKind::TeamShape,
        "culture" | "ownership" => ContradictionKind::Culture,
        "domain" => ContradictionKind::Domain,
        _ => ContradictionKind::WorkStyle,
    }
}

fn point_text(p: &ReviewPoint) -> String {
    let reason = p.reason.trim_end_matches('.');
    format!("{reason}: “{}”", p.quote)
}

/// Folds a validated review into the rules' assessment (see the module
/// docs). Idempotent: an assessment already reviewed is left alone.
pub fn apply(fit: &mut FitAssessment, review: &FitReview) {
    if matches!(fit.review, ReviewState::Reviewed { .. }) {
        return;
    }
    let before = fit.level;
    let mut level = before;
    match (before, review.fit) {
        (FitLevel::Poor, _) => {}
        (_, FitLevel::Poor) => {
            for p in &review.contradictions {
                fit.contradictions.push(FitContradiction {
                    kind: kind_of(&p.aspect),
                    severity: Severity::Material,
                    text: point_text(p),
                    evidence: vec![format!("“{}”", p.quote)],
                    firmness: Firmness::Soft,
                });
            }
            level = FitLevel::Poor;
        }
        (FitLevel::Strong, FitLevel::Plausible | FitLevel::Insufficient) => {
            let why = review
                .contradictions
                .first()
                .map(point_text)
                .or_else(|| review.uncertainties.first().cloned())
                .unwrap_or_else(|| "A closer reading found too little that fits".into());
            fit.contradictions.push(FitContradiction {
                kind: review
                    .contradictions
                    .first()
                    .map_or(ContradictionKind::WorkShape, |p| kind_of(&p.aspect)),
                severity: Severity::Holding,
                text: why,
                evidence: Vec::new(),
                firmness: Firmness::Soft,
            });
            level = FitLevel::Plausible;
        }
        (FitLevel::Plausible, FitLevel::Strong)
            if !fit
                .contradictions
                .iter()
                .any(|c| c.severity >= Severity::Holding) =>
        {
            for p in &review.affirmative {
                let aspect = aspect_of(&p.aspect);
                if fit
                    .reasons
                    .iter()
                    .any(|r| r.aspect == aspect && r.weight > 0.0)
                {
                    continue;
                }
                fit.reasons.push(FitReason {
                    aspect,
                    text: point_text(p),
                    evidence: vec![format!("“{}”", p.quote)],
                    firmness: Firmness::Soft,
                    weight: Firmness::Soft.weight(),
                    folded: false,
                });
            }
            level = FitLevel::Strong;
        }
        (FitLevel::Insufficient, FitLevel::Strong | FitLevel::Plausible) => {
            level = FitLevel::Plausible;
        }
        _ => {}
    }
    for u in &review.uncertainties {
        if !fit.uncertainties.contains(u) {
            fit.uncertainties.push(u.clone());
        }
    }
    fit.contradictions.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.firmness.cmp(&a.firmness))
    });
    fit.level = level;
    fit.assessor = format!("{} + {}", crate::fit::FIT_RULES, review.reviewer);
    fit.review = ReviewState::Reviewed {
        by: review.reviewer.clone(),
        changed: level != before,
    };
}

/// How much semantic review a ranking may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewBudget {
    /// Shortlisted jobs looked up (strong and plausible, best first).
    pub shortlist: usize,
    /// New reviews (model calls) per ranking; the rest wait for the next.
    pub new_reviews: usize,
    /// Calls at a time.
    pub concurrency: usize,
    /// Stop starting new calls after this long.
    pub deadline: std::time::Duration,
}

impl Default for ReviewBudget {
    fn default() -> Self {
        Self {
            shortlist: 30,
            new_reviews: 12,
            concurrency: 4,
            deadline: std::time::Duration::from_secs(45),
        }
    }
}

/// What the semantic stage did in one ranking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStats {
    /// On the shortlist.
    pub shortlisted: usize,
    /// Answered from stored reviews.
    pub cache_hits: usize,
    /// Model calls made.
    pub calls: usize,
    /// Calls that failed (the rules' assessment stood).
    pub failures: usize,
    /// Shortlisted but left for a later ranking (budget or deadline).
    pub deferred: usize,
    /// Reviews that moved a fit level.
    pub changed: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> FitReviewRequest {
        FitReviewRequest {
            candidate: CandidateBrief {
                taste: vec!["prefer · work_shape: backend (Backend engineering) · said".into()],
                level: Some("senior (latest title)".into()),
                specialties: Vec::new(),
                experience: vec!["Senior Software Engineer · 2022–present · work: backend".into()],
            },
            job: JobBrief {
                title: "Software Engineer II".into(),
                company: "Acme".into(),
                setup: "Remote".into(),
                reading: Vec::new(),
                text: vec![
                    "You’ll own our billing services end to end.".into(),
                    "We are a team of 8 engineers.".into(),
                ],
            },
        }
    }

    fn answer(fit: &str, affirmative: Value, contradictions: Value) -> String {
        json!({
            "fit": fit,
            "role_fit": "match",
            "company_fit": "unknown",
            "seniority": "unknown",
            "specialization": "unknown",
            "affirmative": affirmative,
            "contradictions": contradictions,
            "uncertainties": ["The posting doesn't say the salary", "Level isn't stated"]
        })
        .to_string()
    }

    #[test]
    fn quotes_must_come_from_the_posting() {
        let text = answer(
            "strong",
            json!([
                {"aspect": "role", "reason": "Backend ownership", "quote": "you'll own our billing services end to end"},
                {"aspect": "team", "reason": "A small team", "quote": "team of 8 engineers"},
                {"aspect": "company", "reason": "A famous unicorn", "quote": "valued at $10B"},
                {"aspect": "role", "reason": "Great pay for backend", "quote": "billing services"}
            ]),
            json!([]),
        );
        let r = parse_review(&text, &request(), "test").unwrap();
        assert_eq!(r.fit, FitLevel::Strong);
        assert_eq!(r.affirmative.len(), 2, "{:?}", r.affirmative);
        assert_eq!(r.rejected, 2, "an invented fact and a pay remark");
        assert_eq!(r.uncertainties, ["Level isn't stated"], "pay is not fit");
    }

    #[test]
    fn a_verdict_needs_its_evidence() {
        let only_role = answer(
            "strong",
            json!([{"aspect": "role", "reason": "Backend", "quote": "billing services"}]),
            json!([]),
        );
        assert_eq!(
            parse_review(&only_role, &request(), "t").unwrap().fit,
            FitLevel::Plausible
        );
        let unquoted = answer(
            "poor",
            json!([]),
            json!([{"aspect": "company", "reason": "Huge company", "quote": "a Fortune 500 company"}]),
        );
        assert_eq!(
            parse_review(&unquoted, &request(), "t").unwrap().fit,
            FitLevel::Insufficient
        );
    }

    #[test]
    fn malformed_answers_are_rejected_whole() {
        for bad in [
            "not json",
            "{}",
            &answer("excellent", json!([]), json!([])),
            &json!({"fit": "strong", "role_fit": "match", "company_fit": "match", "seniority": "match",
                    "specialization": "match", "affirmative": [], "contradictions": [],
                    "uncertainties": [], "score": 0.9}).to_string(),
        ] {
            assert!(
                matches!(parse_review(bad, &request(), "t"), Err(ReviewError::Malformed(_))),
                "{bad}"
            );
        }
    }

    fn assessment(level: FitLevel) -> FitAssessment {
        FitAssessment {
            level,
            role_fit: 1.0,
            support: 1.0,
            ..FitAssessment::default()
        }
    }

    fn review(fit: FitLevel) -> FitReview {
        FitReview {
            fit,
            role_fit: "match".into(),
            company_fit: "unknown".into(),
            seniority: "unknown".into(),
            specialization: "unknown".into(),
            affirmative: vec![
                ReviewPoint {
                    aspect: "role".into(),
                    reason: "You'd own billing end to end".into(),
                    quote: "own our billing services end to end".into(),
                },
                ReviewPoint {
                    aspect: "team".into(),
                    reason: "A small team".into(),
                    quote: "team of 8 engineers".into(),
                },
            ],
            contradictions: vec![ReviewPoint {
                aspect: "specialization".into(),
                reason: "Storage-engine internals".into(),
                quote: "storage engine".into(),
            }],
            uncertainties: Vec::new(),
            reviewer: "model/test:m".into(),
            rejected: 0,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    #[test]
    fn the_combination_table() {
        use FitLevel::*;
        for (rules, model, expected) in [
            (Poor, Strong, Poor),
            (Strong, Poor, Poor),
            (Plausible, Poor, Poor),
            (Strong, Plausible, Plausible),
            (Strong, Insufficient, Plausible),
            (Strong, Strong, Strong),
            (Plausible, Strong, Strong),
            (Plausible, Plausible, Plausible),
            (Insufficient, Strong, Plausible),
            (Insufficient, Insufficient, Insufficient),
        ] {
            let mut fit = assessment(rules);
            apply(&mut fit, &review(model));
            assert_eq!(fit.level, expected, "{rules:?} + {model:?}");
            assert!(matches!(fit.review, ReviewState::Reviewed { .. }));
            // Applying twice changes nothing.
            let again = fit.clone();
            apply(&mut fit, &review(Poor));
            assert_eq!(fit, again);
        }
        // A holding contradiction the rules found keeps a plausible fit
        // from being raised.
        let mut held = assessment(Plausible);
        held.contradictions.push(FitContradiction {
            kind: ContradictionKind::Seniority,
            severity: Severity::Holding,
            text: "A step below".into(),
            evidence: Vec::new(),
            firmness: Firmness::Soft,
        });
        apply(&mut held, &review(Strong));
        assert_eq!(held.level, Plausible);
    }

    #[test]
    fn keys_follow_what_was_sent() {
        let r = request();
        let a = review_key(&r, "model/a:x");
        assert_eq!(a, review_key(&r, "model/a:x"));
        assert_ne!(a, review_key(&r, "model/a:y"), "another model");
        let mut edited = r.clone();
        edited.job.text.push("New sentence.".into());
        assert_ne!(a, review_key(&edited, "model/a:x"), "a new posting version");
        let mut taste = r.clone();
        taste
            .candidate
            .taste
            .push("avoid · company: large_company · said".into());
        assert_ne!(a, review_key(&taste, "model/a:x"), "the taste changed");
    }

    #[test]
    fn nothing_identifying_is_rendered() {
        let text = render(&request());
        assert!(text.contains("<taste>"));
        assert!(text.contains("You’ll own our billing services"));
        assert!(!text.contains("prof_"));
    }
}
