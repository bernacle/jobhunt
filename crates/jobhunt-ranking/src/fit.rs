//! Fit: would this person genuinely want this company and role?
//!
//! Fit is the recommendation's reason to exist, and it is assessed apart
//! from practicality (can they pursue it: [`crate::practicality`]). A job
//! earns a recommendation on **affirmative evidence** that it matches
//! what the person wants, never because nothing rejected it, and never
//! because of pay, remote work, freshness or verification: those are
//! practical facts.
//!
//! What the person wants comes from the composed taste profile
//! ([`fn@jobhunt_profile::taste::compose`]): what they said, confirmed or
//! corrected, Narrow's reading of their words, their earlier settings, and
//! learned patterns, each with its provenance. How much a statement counts
//! depends on whose it is ([`Firmness`]): the person's own statements
//! count fully, readings and inferences less, learned-only patterns least,
//! and neutral statements not at all.
//!
//! The job is read by [`crate::facets::work`]: its level, the shape of the
//! work (title first), specialized depth, company, team and culture.
//!
//! Each aspect gives affirmative reasons, contradictions, or nothing (an
//! unknown is never evidence either way):
//!
//! | Aspect | For | Against |
//! | --- | --- | --- |
//! | role | the work shape they want (the title's, else the description's; related shapes count less) | a shape they avoid; not engineering at all |
//! | depth | a specialty they want or have demonstrated | a specialty they neither want nor have shown (using PostgreSQL is not building its storage engine), or deep work when they want broad work |
//! | seniority | the level they want | a level they avoid, or two steps from what they want (an early-career role for a senior) |
//! | company | the stage or size they want | one they avoid; a large or public company when they want startups |
//! | team, culture, ownership, domain, technology, way of working | what they want | what they avoid |
//!
//! Contradictions are first-class: a **material** one (from a statement
//! that is theirs or firmly read) makes the fit poor however much else
//! matches; a **holding** one keeps it from being strong; a **minor** one
//! is only said.
//!
//! Every reason and contradiction has a [`Scope`]: the **role** (what the
//! job itself says about the work: shape, level, depth, employment,
//! domain, technology, way of working) or the **company** (stage, size,
//! team, ownership, culture). The level ([`FitLevel`]) is decided in
//! stages, never from a sum the company side can reach:
//!
//! 1. **Role fit** ([`RoleFit`]), from role evidence only: strong when the
//!    work they want is evidenced by the job itself (a responsibility
//!    sentence describing it, or the title naming it plus another
//!    job-local fact), and nothing about the role holds it back.
//! 2. **Company fit** ([`CompanyFit`]), only then: a conflict holds an
//!    otherwise strong role back; a match only orders it among strong
//!    roles; unknown is not negative.
//!
//! So:
//!
//! * **strong**: role fit is strong and the company doesn't conflict;
//! * **plausible**: some of the work they want (or a strong role at a
//!   conflicting company);
//! * **insufficient**: nothing job-local points to it (company evidence
//!   alone never does);
//! * **poor**: a material contradiction, role or company.
//!
//! **Invariant:** company evidence can never make a job strong. It can't
//! promote an insufficient role, erase a role contradiction, compensate
//! for the wrong level or for specialization the person doesn't want.

use serde::{Deserialize, Serialize};

use jobhunt_profile::taste::{ComposedAssertion, TasteProfile};
use jobhunt_profile::{Polarity, TasteConfidence, TasteDimension, TasteOrigin};

use crate::facets::work::{Seniority, ShapeBasis, Specialty, SpecialtyFact, Trait, WorkReading};
use crate::facets::{JobFacets, JobFunction, Requirement};
use crate::person::Person;
use crate::signals::clip;

/// Revision of the fit rules. Part of every stored ranking's key (through
/// [`crate::RANKING_VERSION`]) and every semantic review's.
pub const FIT_RULES: &str = "fit-rules/3";

/// How well a job fits what the person wants. Deliberately coarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitLevel {
    /// A material contradiction.
    Poor,
    /// Little or nothing points to it.
    Insufficient,
    /// Something points to it; not enough to put Narrow's name behind it.
    Plausible,
    /// Unusually aligned with what they want.
    Strong,
}

impl FitLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Poor => "poor",
            Self::Insufficient => "insufficient",
            Self::Plausible => "plausible",
            Self::Strong => "strong",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "poor" => Some(Self::Poor),
            "insufficient" => Some(Self::Insufficient),
            "plausible" => Some(Self::Plausible),
            "strong" => Some(Self::Strong),
            _ => None,
        }
    }
}

/// Whose a taste statement is, and so how much it counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Firmness {
    /// A pattern learned from feedback, never confirmed.
    Learned,
    /// A low-confidence reading or inference.
    Weak,
    /// Narrow's reading of their words, an inference from their profile, or
    /// an earlier setting read with doubts: medium confidence or better.
    Soft,
    /// Theirs: written, confirmed or corrected by them, or an earlier
    /// setting they entered.
    Firm,
}

impl Firmness {
    pub fn of(a: &ComposedAssertion) -> Self {
        if a.is_persons() {
            return Self::Firm;
        }
        match (a.origin, a.confidence) {
            (TasteOrigin::Learned, _) => Self::Learned,
            (TasteOrigin::Legacy, TasteConfidence::High) => Self::Firm,
            (_, TasteConfidence::Medium | TasteConfidence::High) => Self::Soft,
            (_, TasteConfidence::Low) => Self::Weak,
        }
    }

    /// How much an affirmative match on it counts.
    pub fn weight(self) -> f64 {
        match self {
            Self::Firm => 1.0,
            Self::Soft => 0.6,
            Self::Weak | Self::Learned => 0.3,
        }
    }

    /// Whether a contradiction of it is material by itself.
    fn binds(self) -> bool {
        matches!(self, Self::Firm | Self::Soft)
    }
}

/// What part of the job an assessment line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aspect {
    Role,
    Specialization,
    Seniority,
    Company,
    Team,
    Ownership,
    Culture,
    Domain,
    Technology,
    WorkStyle,
    /// What the person said about this very job ("you liked it").
    Feedback,
}

/// Whose evidence a fact is: the job's own, or the company's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Local to the job: what the person will do, at what level, on what
    /// terms.
    Role,
    /// About the employer or the team: the same on every posting of the
    /// company that says it.
    Company,
}

impl Aspect {
    pub fn scope(self) -> Scope {
        match self {
            Self::Company | Self::Team | Self::Ownership | Self::Culture => Scope::Company,
            Self::Role
            | Self::Specialization
            | Self::Seniority
            | Self::Domain
            | Self::Technology
            | Self::WorkStyle
            | Self::Feedback => Scope::Role,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Specialization => "specialization",
            Self::Seniority => "seniority",
            Self::Company => "company",
            Self::Team => "team",
            Self::Ownership => "ownership",
            Self::Culture => "culture",
            Self::Domain => "domain",
            Self::Technology => "technology",
            Self::WorkStyle => "work_style",
            Self::Feedback => "feedback",
        }
    }
}

/// One affirmative reason: why they would actually want this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitReason {
    pub aspect: Aspect,
    /// What the person reads: specific to this job ("A 15-person startup,
    /// the kind of company you want").
    pub text: String,
    /// The posting's words, and what the person said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub firmness: Firmness,
    /// How much it counts toward the level (0: context only).
    pub weight: f64,
    /// Said as part of another line (the level in "Senior backend work").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub folded: bool,
}

/// What a contradiction is about (for debugging and the benchmark).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContradictionKind {
    Seniority,
    /// Specialized depth the person neither wants nor has shown.
    RoleDepth,
    /// A shape of work they avoid, or not engineering at all.
    WorkShape,
    CompanyShape,
    TeamShape,
    Culture,
    Domain,
    Technology,
    WorkStyle,
}

impl ContradictionKind {
    pub fn scope(self) -> Scope {
        match self {
            Self::CompanyShape | Self::TeamShape | Self::Culture => Scope::Company,
            Self::Seniority
            | Self::RoleDepth
            | Self::WorkShape
            | Self::Domain
            | Self::Technology
            | Self::WorkStyle => Scope::Role,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seniority => "seniority",
            Self::RoleDepth => "role_depth",
            Self::WorkShape => "work_shape",
            Self::CompanyShape => "company_shape",
            Self::TeamShape => "team_shape",
            Self::Culture => "culture",
            Self::Domain => "domain",
            Self::Technology => "technology",
            Self::WorkStyle => "work_style",
        }
    }
}

/// How much a contradiction weighs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Said, nothing more.
    Minor,
    /// Keeps it from being a strong fit.
    Holding,
    /// Makes the fit poor, whatever else matches.
    Material,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Holding => "holding",
            Self::Material => "material",
        }
    }
}

/// Something about the job that goes against what the person wants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitContradiction {
    pub kind: ContradictionKind,
    pub severity: Severity,
    /// "An early-career role, while you said you don't want early-career
    /// roles".
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub firmness: Firmness,
}

/// How well the job itself fits, from role-scope evidence only.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RoleFit {
    /// A material role contradiction.
    Contradictory,
    /// Nothing job-local points to it.
    #[default]
    Insufficient,
    /// Some of the work they want, not enough (or held back).
    Plausible,
    /// The work they want, evidenced by the job itself.
    Strong,
}

impl RoleFit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contradictory => "contradictory",
            Self::Insufficient => "insufficient",
            Self::Plausible => "plausible",
            Self::Strong => "strong",
        }
    }
}

/// How the employer fits, assessed after the role.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CompanyFit {
    /// A company contradiction (material or holding).
    Conflicts,
    /// The posting doesn't show the company side: not negative.
    #[default]
    Unknown,
    /// Company-side reasons fit.
    Matches,
}

impl CompanyFit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflicts => "conflicts",
            Self::Unknown => "unknown",
            Self::Matches => "matches",
        }
    }
}

/// What says the work is the kind the person wants.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RoleBasis {
    /// Nothing, or only scattered words or a requirements list ("from what
    /// the description asks for"), the team's name, a shape merely related
    /// to the one they want, or a weak or learned-only want.
    #[default]
    Inferred,
    /// The title names it, and nothing about what the person will do says
    /// so.
    Title,
    /// What the person will do describes it: a responsibility sentence, an
    /// included shape's sentence, the specialty itself, or the title's
    /// work confirmed by one.
    Described,
}

/// Whether a semantic reviewer looked at the assessment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReviewState {
    /// No reviewer is configured, or the job wasn't on the shortlist.
    #[default]
    NotReviewed,
    /// Reviewed by `by`; `changed` when it moved the level.
    Reviewed { by: String, changed: bool },
    /// A reviewer is configured but couldn't be used for this job: the
    /// rules' assessment stands.
    Unavailable { why: String },
}

/// The fit of one job for one person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FitAssessment {
    pub level: FitLevel,
    /// Affirmative reasons, strongest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<FitReason>,
    /// Material first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradictions: Vec<FitContradiction>,
    /// What bears on fit and the posting doesn't say.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertainties: Vec<String>,
    /// The aspects the person has a view on and the job was read for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evaluated: Vec<Aspect>,
    /// How well the work itself fits (0 when nothing says it does).
    pub role_fit: f64,
    /// What says the work fits (see [`RoleBasis`]).
    #[serde(default)]
    pub role_basis: RoleBasis,
    /// Narrow read the work they want (soft), rather than them saying it:
    /// the job must show more of it.
    #[serde(default)]
    pub role_read: bool,
    /// How much the other job-local aspects affirmatively fit.
    pub support: f64,
    /// How much the company side affirmatively fits: orders strong roles,
    /// never makes one.
    #[serde(default)]
    pub company_support: f64,
    /// Role fit, from role evidence only.
    #[serde(default)]
    pub role: RoleFit,
    /// Company fit, assessed after the role.
    #[serde(default)]
    pub company: CompanyFit,
    /// `fit-rules/2`.
    pub assessor: String,
    #[serde(default)]
    pub review: ReviewState,
}

impl Default for FitAssessment {
    fn default() -> Self {
        Self {
            level: FitLevel::Insufficient,
            reasons: Vec::new(),
            contradictions: Vec::new(),
            uncertainties: Vec::new(),
            evaluated: Vec::new(),
            role_fit: 0.0,
            role_basis: RoleBasis::Inferred,
            role_read: false,
            support: 0.0,
            company_support: 0.0,
            role: RoleFit::Insufficient,
            company: CompanyFit::Unknown,
            assessor: FIT_RULES.to_owned(),
            review: ReviewState::NotReviewed,
        }
    }
}

impl FitAssessment {
    /// Orders jobs of the same level: more evidence first (the company's
    /// included: it may rank one strong role above another), holding and
    /// minor contradictions last. Not shown as a number, and never what
    /// makes the level.
    pub fn score(&self) -> f64 {
        let held: f64 = self
            .contradictions
            .iter()
            .map(|c| match c.severity {
                Severity::Material => 3.0,
                Severity::Holding => 0.5,
                Severity::Minor => 0.25,
            })
            .sum();
        ((self.role_fit + self.support + self.company_support - held) * 100.0).round() / 100.0
    }

    pub fn material(&self) -> impl Iterator<Item = &FitContradiction> {
        self.contradictions
            .iter()
            .filter(|c| c.severity == Severity::Material)
    }

    /// The contradiction worth saying first, if any.
    pub fn main_contradiction(&self) -> Option<&FitContradiction> {
        self.contradictions.first()
    }

    /// The reasons as they read, strongest first (context lines after).
    pub fn reason_texts(&self) -> Vec<String> {
        self.reasons
            .iter()
            .filter(|r| !r.folded)
            .map(|r| r.text.clone())
            .collect()
    }

    /// Why this role: the job-local reasons.
    pub fn role_reasons(&self) -> impl Iterator<Item = &FitReason> {
        self.reasons
            .iter()
            .filter(|r| r.aspect.scope() == Scope::Role)
    }

    /// Why this company: the company-side reasons.
    pub fn company_reasons(&self) -> impl Iterator<Item = &FitReason> {
        self.reasons
            .iter()
            .filter(|r| r.aspect.scope() == Scope::Company)
    }

    fn any(&self, scope: Scope, severity: Severity) -> bool {
        self.contradictions
            .iter()
            .any(|c| c.kind.scope() == scope && c.severity == severity)
    }

    /// Whether the work they want is evidenced by the job itself: described
    /// by what the person will do, or named by the title with another
    /// job-local fact (the level they want, a specialty they want or have
    /// shown, their own interest in this job).
    ///
    /// A want Narrow only read (soft) needs more from the job: the work
    /// described and another job-local fact.
    fn role_evidenced(&self) -> bool {
        let another = self.reasons.iter().any(|r| {
            r.weight > 0.0
                && matches!(
                    r.aspect,
                    Aspect::Seniority | Aspect::Specialization | Aspect::Feedback
                )
        });
        match (self.role_basis, self.role_read) {
            (RoleBasis::Described, false) => true,
            (RoleBasis::Described, true) | (RoleBasis::Title, false) => another,
            (RoleBasis::Title, true) | (RoleBasis::Inferred, _) => false,
        }
    }

    /// The level the evidence supports, in stages (see the module docs):
    /// the role from role evidence alone, then the company.
    pub fn classify(&mut self) {
        use Scope::{Company, Role};
        // 1. Role fit: job-local evidence only.
        self.role = if self.any(Role, Severity::Material) {
            RoleFit::Contradictory
        } else if self.role_evidenced() && !self.any(Role, Severity::Holding) {
            RoleFit::Strong
        } else if self.role_fit > 0.0 || self.support >= 1.5 {
            RoleFit::Plausible
        } else {
            RoleFit::Insufficient
        };
        // 2. Company fit, after the role.
        self.company =
            if self.any(Company, Severity::Material) || self.any(Company, Severity::Holding) {
                CompanyFit::Conflicts
            } else if self.company_support > 0.0 {
                CompanyFit::Matches
            } else {
                CompanyFit::Unknown
            };
        // 3. The level. Company evidence can hold a strong role back or make
        // the fit poor; it never makes one.
        self.level = if self.any(Role, Severity::Material) || self.any(Company, Severity::Material)
        {
            FitLevel::Poor
        } else {
            match (self.role, self.company) {
                (RoleFit::Strong, CompanyFit::Conflicts) => FitLevel::Plausible,
                (RoleFit::Strong, _) => FitLevel::Strong,
                (RoleFit::Plausible, _) => FitLevel::Plausible,
                (RoleFit::Insufficient | RoleFit::Contradictory, _) => FitLevel::Insufficient,
            }
        };
    }
}

// ---------------------------------------------------------------------------
// The person's side.

/// One taste statement as fit reads it.
#[derive(Debug, Clone)]
struct Stated<'a> {
    value: &'a str,
    polarity: Polarity,
    firmness: Firmness,
    /// Read or learned with high confidence.
    confident: bool,
    text: &'a str,
}

impl Stated<'_> {
    /// How much a job going against this statement weighs: the person's
    /// own or firmly read statements are material; weak readings and
    /// established learned patterns hold a job back; tentative learned
    /// patterns are only said.
    fn severity(&self) -> Severity {
        match self.firmness {
            Firmness::Firm | Firmness::Soft => Severity::Material,
            Firmness::Weak => Severity::Holding,
            Firmness::Learned if self.confident => Severity::Holding,
            Firmness::Learned => Severity::Minor,
        }
    }

    /// "you said you want", "you asked for", "you're looking for" (Narrow's
    /// reading), "you've favored" (learned).
    fn wants(&self) -> &'static str {
        match self.firmness {
            Firmness::Firm => "you want",
            Firmness::Soft => "you're looking for",
            Firmness::Weak => "you may want",
            Firmness::Learned => "you've favored",
        }
    }

    fn avoids(&self) -> &'static str {
        match self.firmness {
            Firmness::Firm => "you said you don't want",
            Firmness::Soft => "you'd rather avoid",
            Firmness::Weak => "you may want to avoid",
            Firmness::Learned => "you've turned down",
        }
    }
}

/// The composed taste profile, by dimension, without neutral statements
/// (which say "it doesn't matter": no signal either way).
struct Wants<'a> {
    all: Vec<(TasteDimension, Stated<'a>)>,
}

impl<'a> Wants<'a> {
    fn of(profile: &'a TasteProfile) -> Self {
        let all = profile
            .assertions
            .iter()
            .filter(|a| a.polarity != Polarity::Neutral)
            .map(|a| {
                (
                    a.dimension,
                    Stated {
                        value: a.value.as_str(),
                        polarity: a.polarity,
                        firmness: Firmness::of(a),
                        confident: a.confidence == TasteConfidence::High,
                        text: a.text.as_str(),
                    },
                )
            })
            .collect();
        Self { all }
    }

    fn about(&self, d: TasteDimension) -> impl Iterator<Item = &Stated<'a>> {
        self.all
            .iter()
            .filter(move |(x, _)| *x == d)
            .map(|(_, s)| s)
    }

    fn prefer(&self, d: TasteDimension) -> impl Iterator<Item = &Stated<'a>> {
        self.about(d).filter(|s| s.polarity == Polarity::Prefer)
    }

    fn open(&self, d: TasteDimension) -> impl Iterator<Item = &Stated<'a>> {
        self.about(d).filter(|s| s.polarity == Polarity::Open)
    }

    fn avoid(&self, d: TasteDimension) -> impl Iterator<Item = &Stated<'a>> {
        self.about(d).filter(|s| s.polarity == Polarity::Avoid)
    }

    fn find(&self, d: TasteDimension, value: &str, polarity: Polarity) -> Option<&Stated<'a>> {
        self.about(d)
            .filter(|s| s.polarity == polarity && same(s.value, value))
            .max_by_key(|s| s.firmness)
    }
}

/// Taste values compare as words ("developer_tools" is "developer tools").
fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase().replace(['_', '-'], " ");
    norm(a) == norm(b)
}

// ---------------------------------------------------------------------------
// Vocabulary.

/// How a work shape reads: "backend", "database internals".
pub fn shape_label(shape: &str) -> String {
    match shape {
        "full_stack" => "full-stack".into(),
        "product" => "product engineering".into(),
        "database_internals" => "database internals".into(),
        "ml_research" => "ML research".into(),
        "ml_product" => "ML product engineering".into(),
        "sre" => "SRE".into(),
        "data" => "data engineering".into(),
        "developer_tooling" => "developer tooling".into(),
        "distributed_systems" => "distributed systems".into(),
        "low_latency" => "low-latency systems".into(),
        "solutions" => "customer-facing engineering".into(),
        other => other.replace('_', " "),
    }
}

/// Shapes close enough that one partly satisfies wanting the other.
const RELATED: &[(&str, &str)] = &[
    ("backend", "full_stack"),
    ("backend", "product"),
    ("backend", "distributed_systems"),
    ("platform", "infrastructure"),
    ("platform", "sre"),
    ("platform", "developer_tooling"),
    ("infrastructure", "platform"),
    ("infrastructure", "sre"),
    ("full_stack", "product"),
    ("full_stack", "backend"),
    ("full_stack", "frontend"),
    ("product", "full_stack"),
    ("product", "backend"),
    // Not ("product", "frontend"): frontend is its own kind of role to
    // choose (BRU-324), so someone who chose product engineering and not
    // frontend isn't sent design-system and UI-only roles as close to it.
    // Full-stack work still is.
    ("product", "ml_product"),
    ("ml_product", "product"),
    ("database_internals", "distributed_systems"),
    ("distributed_systems", "database_internals"),
    ("distributed_systems", "backend"),
    ("research", "ml_research"),
    ("sre", "infrastructure"),
    ("sre", "platform"),
];

fn related(wanted: &str, job: &str) -> Option<f64> {
    if same(wanted, job) {
        Some(1.0)
    } else if RELATED.iter().any(|(a, b)| same(a, wanted) && same(b, job)) {
        Some(0.5)
    } else {
        None
    }
}

/// Work done *on* another shape's systems: security for a cloud or
/// platform team is work on that infrastructure, so the team's area
/// counts ("Security Engineer, Cloud"), unlike iOS work for a platform team.
const WORKS_ON: &[(&str, &str)] = &[
    ("security", "infrastructure"),
    ("security", "platform"),
    ("security", "sre"),
];

fn basis_weight(basis: ShapeBasis) -> f64 {
    match basis {
        ShapeBasis::Title | ShapeBasis::Specialty => 1.0,
        ShapeBasis::Description | ShapeBasis::Included => 0.75,
        ShapeBasis::TitleArea => 0.5,
    }
}

/// "A startup", "A growth-stage company".
fn company_phrase(value: &str) -> String {
    match value {
        "startup" => "A startup".into(),
        "early_stage" => "An early-stage company".into(),
        "growth" => "A growth-stage company".into(),
        "established" => "An established company".into(),
        "small_company" => "A small company".into(),
        "large_company" => "A large company".into(),
        "public_company" => "A public company".into(),
        "founder_led" => "A founder-led company".into(),
        "product_company" => "A product company".into(),
        "consulting" => "A consultancy".into(),
        "agency" => "An agency".into(),
        "open_source" => "An open-source company".into(),
        other => format!("A {} company", other.replace('_', " ")),
    }
}

/// The company kinds that satisfy wanting (or avoiding) `value`.
fn satisfies<'w>(value: &str, w: &'w WorkReading) -> Option<&'w Trait> {
    let any = |values: &[&str]| values.iter().find_map(|v| w.company_is(v));
    match value {
        "startup" => any(&["startup", "early_stage", "small_company"]),
        "small_company" => any(&["small_company", "early_stage"]),
        "large_company" | "established" => any(&["large_company", "public_company"]),
        other => w.company_is(other),
    }
}

const SMALL_KINDS: [&str; 4] = ["startup", "early_stage", "small_company", "founder_led"];
const LARGE_KINDS: [&str; 3] = ["large_company", "public_company", "established"];

fn quote(text: &str) -> String {
    format!("“{}”", clip(text.trim(), 140))
}

/// The part of a long sentence around the first of `cues` (case
/// insensitive), quoted: "“…and own backend services end to end: API
/// design, data model…”".
fn excerpt(text: &str, cues: &[&str]) -> String {
    let text = text.trim();
    const MAX: usize = 140;
    if text.chars().count() <= MAX {
        return quote(text);
    }
    let lower = text.to_lowercase();
    let Some(at) = cues.iter().filter_map(|c| lower.find(c)).min() else {
        return quote(text);
    };
    // Start a little before the cue, at a word boundary.
    let mut start = at.saturating_sub(40);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    if start > 0 {
        start = text[start..].find(' ').map_or(start, |i| start + i + 1);
    }
    let rest = &text[start..];
    let body = clip(rest, MAX);
    let lead = if start > 0 { "…" } else { "" };
    format!("“{lead}{body}”")
}

const OWNERSHIP_CUES: [&str; 7] = [
    "own",
    "ownership",
    "end to end",
    "end-to-end",
    "autonomy",
    "agency",
    "autonomous",
];
const TEAM_CUES: [&str; 5] = ["team of", "one of", "small team", "large team", "engineers"];
const COMPANY_CUES: [&str; 6] = [
    "startup", "person", "people", "series", "company", "founder",
];

// ---------------------------------------------------------------------------
// The assessment.

struct Builder {
    fit: FitAssessment,
}

impl Builder {
    fn reason(
        &mut self,
        aspect: Aspect,
        text: String,
        evidence: Vec<String>,
        firmness: Firmness,
        weight: f64,
    ) {
        self.fit.reasons.push(FitReason {
            aspect,
            text,
            evidence,
            firmness,
            weight,
            folded: false,
        });
    }

    fn against(
        &mut self,
        kind: ContradictionKind,
        severity: Severity,
        text: String,
        evidence: Vec<String>,
        firmness: Firmness,
    ) {
        if self.fit.contradictions.iter().any(|c| c.text == text) {
            return;
        }
        self.fit.contradictions.push(FitContradiction {
            kind,
            severity,
            text,
            evidence,
            firmness,
        });
    }

    fn evaluated(&mut self, aspect: Aspect) {
        if !self.fit.evaluated.contains(&aspect) {
            self.fit.evaluated.push(aspect);
        }
    }
}

/// What a company says about its own size, read once for the company: a
/// headcount one posting states ("with 1200+ colleagues in 75+
/// countries") holds for the company's other postings too, and the same
/// statement on 200 postings is one company fact.
///
/// Only the company's stated headcount is shared. Stage and kind words
/// read from a single posting ("a Fortune 500 CISO" about a customer, an
/// award name) stay that posting's: shared, one misreading would hold back
/// every role at the company.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompanyFacts {
    pub headcount: Option<(u32, String)>,
    /// Digest of the facts, for cache keys.
    pub digest: String,
}

impl CompanyFacts {
    /// The size the headcount means: `small_company` (≤ 100 people),
    /// `large_company` (≥ 1,000), or nothing in between.
    fn size(&self) -> Option<Trait> {
        let (n, evidence) = self.headcount.as_ref()?;
        let value = match n {
            0..=100 => "small_company",
            1000.. => "large_company",
            _ => return None,
        };
        Some(Trait {
            value: value.to_owned(),
            evidence: evidence.clone(),
        })
    }
}

/// [`CompanyFacts`] by company (normalized name).
#[derive(Debug, Clone, Default)]
pub struct CompanyBook {
    by_company: std::collections::HashMap<String, CompanyFacts>,
}

impl CompanyBook {
    /// The company facts of every company among `facets`.
    pub fn of<'f>(facets: impl IntoIterator<Item = &'f JobFacets>) -> Self {
        let mut by_company: std::collections::HashMap<String, CompanyFacts> =
            std::collections::HashMap::new();
        for f in facets {
            let entry = by_company
                .entry(jobhunt_core::text::search_key(&f.company))
                .or_default();
            if entry.headcount.is_none() {
                entry.headcount.clone_from(&f.work.headcount);
            }
        }
        for facts in by_company.values_mut() {
            let n = facts
                .headcount
                .as_ref()
                .map(|(n, _)| n.to_string())
                .unwrap_or_default();
            facts.digest =
                jobhunt_core::StableId::derive("narrow.ranking.company_facts", &[&n]).to_string();
        }
        Self { by_company }
    }

    pub fn get(&self, company: &str) -> Option<&CompanyFacts> {
        self.by_company
            .get(&jobhunt_core::text::search_key(company))
    }
}

/// Assesses how well a job fits what the person wants, from the posting's
/// own company facts.
pub fn assess(facets: &JobFacets, person: &Person, profile: &TasteProfile) -> FitAssessment {
    assess_with(facets, person, profile, None)
}

/// [`assess`], with the company's facts read across its postings when
/// they are known ([`CompanyBook`]).
pub fn assess_with(
    facets: &JobFacets,
    person: &Person,
    profile: &TasteProfile,
    company_facts: Option<&CompanyFacts>,
) -> FitAssessment {
    let wants = Wants::of(profile);
    let work = &facets.work;
    // The company's own headcount, when another of its postings states it
    // and this one doesn't.
    let merged;
    let company_work = match company_facts.and_then(|c| c.size().map(|t| (c, t))) {
        Some((c, size)) if work.headcount.is_none() => {
            let mut company = work.company.clone();
            if !company.iter().any(|t| t.value == size.value) {
                company.push(size.clone());
            }
            if size.value == "large_company" {
                company.retain(|t| {
                    !matches!(
                        t.value.as_str(),
                        "startup" | "early_stage" | "small_company"
                    )
                });
            }
            merged = WorkReading {
                company,
                headcount: c.headcount.clone(),
                ..work.clone()
            };
            &merged
        }
        _ => work,
    };
    let mut b = Builder {
        fit: FitAssessment::default(),
    };
    function(&mut b, facets, person, &wants);
    role(&mut b, facets, &wants);
    depth(&mut b, work, person, &wants);
    seniority(&mut b, work, person, &wants);
    company(&mut b, company_work, &wants);
    team_and_culture(&mut b, work, &wants);
    domain(&mut b, facets, &wants);
    technology(&mut b, facets, &wants);
    work_style(&mut b, facets, &wants);
    context(&mut b, facets, person, &wants);

    employment(&mut b, facets);

    // Role fit is the best the work itself offers. Every other aspect counts
    // once, by its best reason, on its own side: job-local support, or the
    // company's (which orders strong roles and never makes one).
    let mut fit = b.fit;
    fit.role_fit = fit
        .reasons
        .iter()
        .filter(|r| r.aspect == Aspect::Role)
        .map(|r| r.weight)
        .fold(0.0, f64::max);
    let (mut support, mut company_support) = (0.0, 0.0);
    let mut aspects: Vec<Aspect> = fit.reasons.iter().map(|r| r.aspect).collect();
    aspects.sort();
    aspects.dedup();
    for aspect in aspects.into_iter().filter(|a| *a != Aspect::Role) {
        let best = fit
            .reasons
            .iter()
            .filter(|r| r.aspect == aspect)
            .map(|r| r.weight)
            .fold(0.0, f64::max);
        match aspect.scope() {
            Scope::Role => support += best,
            Scope::Company => company_support += best,
        }
    }
    fit.support = (support * 100.0).round() / 100.0;
    fit.company_support = (company_support * 100.0).round() / 100.0;
    fit.role_fit = (fit.role_fit * 100.0).round() / 100.0;
    // A firm view on the company side that the posting doesn't show stays
    // unknown: said, not held against the job.
    let company_view = [
        TasteDimension::Company,
        TasteDimension::Team,
        TasteDimension::Ownership,
        TasteDimension::Culture,
    ]
    .iter()
    .any(|d| {
        wants
            .prefer(*d)
            .any(|p| matches!(p.firmness, Firmness::Firm | Firmness::Soft))
    });
    if company_view
        && fit.company_support == 0.0
        && !fit
            .uncertainties
            .iter()
            .any(|u| u.contains("company's stage") || u.contains("team is"))
    {
        fit.uncertainties.push(
            "The posting doesn't show the kind of company, team or ownership you want".into(),
        );
    }
    // Strongest first: the work, then by weight; context lines last.
    fit.reasons.sort_by(|a, b| {
        (b.weight > 0.0)
            .cmp(&(a.weight > 0.0))
            .then((b.aspect == Aspect::Role).cmp(&(a.aspect == Aspect::Role)))
            .then(b.weight.total_cmp(&a.weight))
    });
    // Keep one reason per aspect (the best), then the context lines.
    let mut seen: Vec<Aspect> = Vec::new();
    fit.reasons.retain(|r| {
        if r.weight <= 0.0 {
            return true;
        }
        if seen.contains(&r.aspect) {
            return false;
        }
        seen.push(r.aspect);
        true
    });
    fold_level(&mut fit, work);
    fit.contradictions.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.firmness.cmp(&a.firmness))
    });
    fit.classify();
    fit
}

/// "Backend work, the kind of engineering you want" and "Senior level, as
/// you want" read better as one line: "Senior backend work, …". The level
/// still counts on its own.
fn fold_level(fit: &mut FitAssessment, work: &WorkReading) {
    let Some(level) = work.level.as_ref().map(|l| l.seniority) else {
        return;
    };
    let has_level = fit
        .reasons
        .iter()
        .any(|r| r.aspect == Aspect::Seniority && r.weight > 0.0);
    let Some(role) = fit
        .reasons
        .iter_mut()
        .find(|r| r.aspect == Aspect::Role && r.weight > 0.0)
    else {
        return;
    };
    if !has_level
        || !(role.text.contains(" work, the kind of engineering")
            || role
                .text
                .contains(" work (from what the description asks for)"))
    {
        return;
    }
    role.text = format!("{} {}", capitalize(level.label()), lower_first(&role.text));
    for r in fit
        .reasons
        .iter_mut()
        .filter(|r| r.aspect == Aspect::Seniority)
    {
        r.folded = true;
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase() => text.to_owned(),
        (Some(a), _) => a.to_lowercase().chain(text.chars().skip(1)).collect(),
        _ => String::new(),
    }
}

/// Not engineering at all, for an engineer.
fn function(b: &mut Builder, facets: &JobFacets, person: &Person, wants: &Wants<'_>) {
    let title = format!("“{}” (title)", facets.title);
    if person.engineer && !facets.function.is_engineering() {
        b.against(
            ContradictionKind::WorkShape,
            Severity::Material,
            format!(
                "{}: {}, while your experience is in engineering",
                facets.title,
                facets.function.label()
            ),
            vec![title],
            Firmness::Firm,
        );
    } else if person.engineer
        && facets.manages_people()
        && !matches!(person.level, Some((crate::facets::Level::Manager, _)))
        && wants
            .find(TasteDimension::WorkStyle, "management", Polarity::Prefer)
            .or_else(|| wants.find(TasteDimension::WorkStyle, "management", Polarity::Open))
            .or_else(|| wants.find(TasteDimension::WorkStyle, "management", Polarity::Avoid))
            .or_else(|| {
                wants.find(
                    TasteDimension::WorkStyle,
                    "individual_contributor",
                    Polarity::Prefer,
                )
            })
            .is_none()
    {
        // Moving into management is a different job: an engineer who hasn't
        // said they want it (and whose latest title isn't a manager's) is
        // not recommended people-management roles. What they said about
        // management, either way, is read by `work_style`.
        b.against(
            ContradictionKind::WorkStyle,
            Severity::Material,
            format!(
                "People management ({title}), while your work so far is hands-on engineering and you haven't said you want to manage"
            ),
            vec![title.clone()],
            Firmness::Soft,
        );
    } else if person.engineer
        && facets.function == JobFunction::CustomerEngineering
        && wants
            .find(TasteDimension::WorkShape, "solutions", Polarity::Prefer)
            .is_none()
    {
        b.against(
            ContradictionKind::WorkShape,
            Severity::Holding,
            "Customer-facing engineering (solutions, forward-deployed), not product or platform engineering".into(),
            vec![title],
            Firmness::Soft,
        );
    }
}

/// The shape of the work against the shapes they want and avoid.
fn role(b: &mut Builder, facets: &JobFacets, wants: &Wants<'_>) {
    let work = &facets.work;
    let wanted: Vec<&Stated<'_>> = wants
        .prefer(TasteDimension::WorkShape)
        .chain(wants.open(TasteDimension::WorkShape))
        .collect();
    if !wanted.is_empty() || wants.avoid(TasteDimension::WorkShape).next().is_some() {
        b.evaluated(Aspect::Role);
    }
    // The best match between what they want and what the work is.
    let mut best: Option<(f64, &Stated<'_>, &crate::facets::work::ShapeFact, f64)> = None;
    for w in wanted.iter().filter(|w| w.polarity == Polarity::Prefer) {
        for shape in &work.shapes {
            let Some(relation) = related(w.value, &shape.shape) else {
                continue;
            };
            let value = relation * basis_weight(shape.basis) * w.firmness.weight();
            if best.is_none_or(|(v, ..)| value > v) {
                best = Some((value, w, shape, relation));
            }
        }
    }
    // A team's name ("… - User Platform") doesn't stand in for the title's
    // own work when that work is something they didn't ask for (iOS, ML).
    let own: Vec<&crate::facets::work::ShapeFact> = work
        .shapes
        .iter()
        .filter(|s| s.basis == ShapeBasis::Title)
        .collect();
    let own_unrelated = !own.is_empty()
        && own.iter().all(|o| {
            !wanted
                .iter()
                .any(|w| w.polarity == Polarity::Prefer && related(w.value, &o.shape).is_some())
        });
    let works_on = |area: &str| {
        own.iter().any(|o| {
            WORKS_ON
                .iter()
                .any(|(a, b)| same(a, &o.shape) && same(b, area))
        })
    };
    if let Some((_, w, shape, _)) = best
        && shape.basis == ShapeBasis::TitleArea
        && own_unrelated
        && !works_on(&shape.shape)
    {
        let own_label: Vec<String> = own.iter().map(|o| shape_label(&o.shape)).collect();
        b.against(
            ContradictionKind::WorkShape,
            Severity::Holding,
            format!(
                "{} work: “{}” names the team, not the {} work {}",
                capitalize(&own_label.join(" and ")),
                shape.evidence,
                shape_label(w.value),
                w.wants()
            ),
            vec![shape.evidence.clone()],
            w.firmness,
        );
        best = None;
    }
    if let Some((value, w, shape, relation)) = best {
        let what = shape_label(&shape.shape);
        let wanted_label = shape_label(w.value);
        let included_in = work
            .shapes
            .iter()
            .find(|s| matches!(s.shape.as_str(), "product" | "full_stack"))
            .map(|s| shape_label(&s.shape));
        let text = match (shape.basis, relation >= 1.0) {
            (ShapeBasis::Title, true) => {
                format!(
                    "{} work, the kind of engineering {}",
                    capitalize(&what),
                    w.wants()
                )
            }
            (ShapeBasis::Specialty, true) => {
                let core = work
                    .specialties
                    .iter()
                    .find(|s| s.specialty.shape() == shape.shape)
                    .map_or(what.clone(), |s| s.specialty.label().to_owned());
                format!(
                    "Building {core} is the core of the role, the work {}",
                    w.wants()
                )
            }
            (ShapeBasis::Description, true) if shape.evidence.is_empty() => format!(
                "{} work (from what the description asks for), the kind of engineering {}",
                capitalize(&what),
                w.wants()
            ),
            (ShapeBasis::Description, true) => format!(
                "{} work, the kind of engineering {}: {}",
                capitalize(&what),
                w.wants(),
                excerpt(&shape.evidence, &[&what, "services", "api"])
            ),
            (ShapeBasis::Included, true) => {
                let kind = included_in.unwrap_or_else(|| "product engineering".into());
                let kind = if kind.ends_with("engineering") {
                    kind
                } else {
                    format!("{kind} work")
                };
                format!(
                    "{} that includes the {what}, close to the work {}: {}",
                    capitalize(&kind),
                    w.wants(),
                    excerpt(
                        &shape.evidence,
                        &["database", "backend", "back end", "services", "api"]
                    )
                )
            }
            (ShapeBasis::TitleArea, _) => format!(
                "Touches {what} (the team: {}), close to the {wanted_label} work {}",
                quote(&shape.evidence),
                w.wants()
            ),
            (_, false) => format!(
                "{} work, close to the {wanted_label} work {}",
                capitalize(&what),
                w.wants()
            ),
        };
        b.reason(
            Aspect::Role,
            text,
            vec![
                format!("{} ({:?})", shape.evidence, shape.basis).to_lowercase(),
                w.text.to_owned(),
            ],
            w.firmness,
            value,
        );
        // The work they want, not only something related to it, wanted
        // firmly or as Narrow read it (a weak or learned-only want never
        // carries a strong fit).
        let full = relation >= 1.0 && matches!(w.firmness, Firmness::Firm | Firmness::Soft);
        b.fit.role_read = w.firmness == Firmness::Soft;
        b.fit.role_basis = match shape.basis {
            _ if !full => RoleBasis::Inferred,
            ShapeBasis::Specialty => RoleBasis::Described,
            ShapeBasis::Description | ShapeBasis::Included if !shape.evidence.is_empty() => {
                RoleBasis::Described
            }
            ShapeBasis::Title if shape.described.is_some() => RoleBasis::Described,
            ShapeBasis::Title => RoleBasis::Title,
            ShapeBasis::Description | ShapeBasis::Included | ShapeBasis::TitleArea => {
                RoleBasis::Inferred
            }
        };
    }
    // Shapes they avoid. A title's shape is what the job is; a shape read
    // from a description counts only when the title names none.
    let title_named = work
        .shapes
        .iter()
        .any(|s| matches!(s.basis, ShapeBasis::Title | ShapeBasis::Specialty));
    for avoid in wants.avoid(TasteDimension::WorkShape) {
        let Some(shape) = work.shapes.iter().find(|s| {
            same(avoid.value, &s.shape)
                && (matches!(s.basis, ShapeBasis::Title | ShapeBasis::Specialty) || !title_named)
        }) else {
            continue;
        };
        // Also wanted (a product role that is also backend work, for someone
        // avoiding product work but wanting backend): the job is mixed.
        let severity = avoid.severity();
        b.against(
            ContradictionKind::WorkShape,
            severity,
            format!(
                "{} work, which {} ({})",
                capitalize(&shape_label(&shape.shape)),
                avoid.avoids(),
                avoid.text
            ),
            vec![shape.evidence.clone()],
            avoid.firmness,
        );
    }
}

/// Whether the person wants a specialty itself (not only deep work in
/// general).
fn wants_specialty<'w>(s: Specialty, wants: &'w Wants<'_>) -> Option<&'w Stated<'w>> {
    let specific: &[&str] = match s {
        Specialty::DatabaseInternals => &["database_internals", "storage_engines", "databases"],
        Specialty::ModelTraining => &["ml_research", "research", "model_training"],
        Specialty::LowLatency => &["low_latency", "trading"],
        Specialty::Privacy => &["privacy", "security"],
        Specialty::SecurityResearch => &["security", "security_research", "research"],
        Specialty::OrchestrationInternals => &["kubernetes", "orchestration"],
    };
    let by_shape = wants
        .prefer(TasteDimension::WorkShape)
        .filter(|w| specific.iter().any(|v| same(v, w.value)))
        .max_by_key(|w| w.firmness);
    let by_technology = s.technology().and_then(|t| {
        wants
            .prefer(TasteDimension::Technology)
            .filter(|w| same(w.value, t))
            // "Kubernetes" alone is a technology; wanting its internals
            // takes wanting deep work too.
            .find(|_| {
                wants
                    .find(TasteDimension::Specialization, "deep", Polarity::Prefer)
                    .is_some()
            })
    });
    by_shape.or(by_technology)
}

/// Specialized depth: wanted, demonstrated, or neither.
fn depth(b: &mut Builder, work: &WorkReading, person: &Person, wants: &Wants<'_>) {
    let avoid_deep = wants.find(TasteDimension::Specialization, "deep", Polarity::Avoid);
    let prefer_broad = wants.find(TasteDimension::Specialization, "broad", Polarity::Prefer);
    let prefer_deep = wants.find(TasteDimension::Specialization, "deep", Polarity::Prefer);
    if avoid_deep.is_some() || prefer_broad.is_some() || prefer_deep.is_some() {
        b.evaluated(Aspect::Specialization);
    }
    for fact in &work.specialties {
        b.evaluated(Aspect::Specialization);
        let s = fact.specialty;
        let cues = cues_text(fact);
        let evidence = vec![quote(&fact.evidence)];
        let demonstrated: Option<&SpecialtyFact> =
            person.specialties.iter().find(|d| d.specialty == s);
        let wanted = wants_specialty(s, wants);
        let avoided = wants
            .avoid(TasteDimension::WorkShape)
            .find(|w| same(w.value, s.shape()) && s.shape() != "infrastructure");
        if let Some(a) = avoided {
            b.against(
                ContradictionKind::RoleDepth,
                a.severity(),
                format!("{} ({cues}), which {}", capitalize(s.label()), a.avoids()),
                evidence.clone(),
                a.firmness,
            );
            continue;
        }
        let narrow = avoid_deep.or(prefer_broad);
        if let Some(n) = narrow
            && wanted.is_none()
        {
            let severity = n.severity();
            b.against(
                ContradictionKind::RoleDepth,
                severity,
                format!(
                    "Deep specialist work in {} ({cues}), while {} broad work rather than specialist roles",
                    s.label(),
                    n.wants()
                ),
                evidence.clone(),
                n.firmness,
            );
            continue;
        }
        match (wanted, demonstrated) {
            (Some(w), _) => {
                let text = match demonstrated {
                    Some(_) => format!(
                        "The deep specialization {}, and your experience shows it ({cues})",
                        w.wants()
                    ),
                    None => format!("The deep specialization {} ({cues})", w.wants()),
                };
                let firmness = w.firmness;
                let weight = firmness
                    .weight()
                    .max(prefer_deep.map_or(0.0, |d| d.firmness.weight()));
                b.reason(
                    Aspect::Specialization,
                    text,
                    evidence.clone(),
                    firmness,
                    weight,
                );
            }
            (None, Some(d)) => {
                let text = format!(
                    "{} ({cues}), which your experience shows: {}",
                    capitalize(s.label()),
                    quote(&d.evidence)
                );
                let weight = prefer_deep.map_or(Firmness::Soft.weight(), |p| p.firmness.weight());
                b.reason(
                    Aspect::Specialization,
                    text,
                    evidence.clone(),
                    Firmness::Soft,
                    weight,
                );
            }
            (None, None) => {
                b.against(
                    ContradictionKind::RoleDepth,
                    Severity::Material,
                    format!(
                        "Specialized {} work ({cues}), beyond what your experience shows: {} is a different job",
                        s.label(),
                        s.not_to_confuse_with()
                    ),
                    evidence.clone(),
                    Firmness::Soft,
                );
            }
        }
    }
}

fn cues_text(fact: &SpecialtyFact) -> String {
    let cues: Vec<&str> = fact.cues.iter().take(3).map(String::as_str).collect();
    cues.join(", ")
}

/// The role's level against the levels they want and avoid (or, when they
/// haven't said, the level of their latest title).
fn seniority(b: &mut Builder, work: &WorkReading, person: &Person, wants: &Wants<'_>) {
    // What they want: their taste, else their latest title.
    let inferred;
    let mut prefer: Vec<(Seniority, &Stated<'_>)> = wants
        .prefer(TasteDimension::Seniority)
        .filter_map(|s| Seniority::from_token(s.value).map(|l| (l, s)))
        .collect();
    let open: Vec<Seniority> = wants
        .open(TasteDimension::Seniority)
        .filter_map(|s| Seniority::from_token(s.value))
        .collect();
    let avoid: Vec<(Seniority, &Stated<'_>)> = wants
        .avoid(TasteDimension::Seniority)
        .filter_map(|s| Seniority::from_token(s.value).map(|l| (l, s)))
        .collect();
    let latest = person
        .level
        .as_ref()
        .and_then(|(l, t)| Seniority::of(*l).map(|s| (s, t.clone())));
    if prefer.is_empty()
        && avoid.is_empty()
        && let Some((level, _)) = &latest
    {
        inferred = Stated {
            value: level.as_str(),
            polarity: Polarity::Prefer,
            firmness: Firmness::Soft,
            confident: false,
            text: "your latest title",
        };
        prefer.push((*level, &inferred));
    }
    if prefer.is_empty() && avoid.is_empty() && open.is_empty() {
        return;
    }
    b.evaluated(Aspect::Seniority);
    // A founding role: senior in practice, whatever the title says.
    let job = match (&work.level, &work.founding) {
        (Some(l), _) => Some((l.seniority, l.evidence.clone(), false)),
        (None, Some(words)) => Some((Seniority::Senior, words.clone(), true)),
        (None, None) => None,
    };
    let Some((level, words, founding)) = job else {
        return;
    };
    let evidence = vec![quote(&words)];
    let label = if founding {
        "A founding role".to_owned()
    } else {
        match level {
            Seniority::EarlyCareer => "Early-career level",
            Seniority::Mid => "Mid level",
            Seniority::Senior => "Senior level",
            Seniority::StaffPlus => "Staff-plus level",
        }
        .to_owned()
    };
    if let Some((_, s)) = avoid.iter().find(|(l, _)| *l == level) {
        let article = if level == Seniority::EarlyCareer {
            "An"
        } else {
            "A"
        };
        b.against(
            ContradictionKind::Seniority,
            s.severity(),
            format!(
                "{article} {} role, which {} ({})",
                level.label(),
                s.avoids(),
                s.text
            ),
            evidence,
            s.firmness,
        );
        return;
    }
    if let Some((_, s)) = prefer.iter().find(|(l, _)| *l == level) {
        let inferred = s.text == "your latest title";
        let text = match (founding, inferred) {
            (true, false) => format!("A founding role: senior scope, the level {}", s.wants()),
            (true, true) => "A founding role: senior scope, matching your latest title".into(),
            (false, false) => format!("{label}, as {}", s.wants()),
            (false, true) => format!("{label}, matching your latest title"),
        };
        b.reason(
            Aspect::Seniority,
            text,
            evidence,
            s.firmness,
            s.firmness.weight(),
        );
        return;
    }
    if open.contains(&level) || prefer.is_empty() {
        return;
    }
    // Neither wanted nor avoided: how far from what they want.
    let below = prefer.iter().all(|(l, _)| l.rank() > level.rank());
    let gap = prefer
        .iter()
        .map(|(l, _)| (l.rank() - level.rank()).abs())
        .min()
        .unwrap_or(0);
    let strongest = prefer
        .iter()
        .map(|(_, s)| s.firmness)
        .max()
        .unwrap_or(Firmness::Weak);
    let wanted: Vec<&str> = prefer.iter().map(|(l, _)| l.label()).collect();
    let wanted = wanted.join(" or ");
    // With no level stated, what they want is their latest title's level.
    let stated_level = wants.prefer(TasteDimension::Seniority).next().is_some();
    if gap >= 2 {
        b.against(
            ContradictionKind::Seniority,
            if strongest.binds() {
                Severity::Material
            } else {
                Severity::Holding
            },
            format!(
                "{label}, well {} the {wanted} level {}",
                if below { "below" } else { "above" },
                match latest.as_ref() {
                    Some((_, title)) if !stated_level => format!("of your latest title ({title})"),
                    _ => "you want".to_owned(),
                }
            ),
            evidence,
            strongest,
        );
    } else if below && !founding {
        // A step below a level they said they want holds a job back; a step
        // below their latest title is only worth noting (people move from
        // staff to senior roles, and they didn't say otherwise).
        match latest.as_ref() {
            Some((_, title)) if !stated_level => b.against(
                ContradictionKind::Seniority,
                Severity::Minor,
                format!("{label}, a step below your latest title ({title})"),
                evidence,
                strongest,
            ),
            _ => b.against(
                ContradictionKind::Seniority,
                Severity::Holding,
                format!("{label}, a step below the {wanted} level you want"),
                evidence,
                strongest,
            ),
        }
    }
}

/// The company's stage and size against the kinds they want and avoid.
fn company(b: &mut Builder, work: &WorkReading, wants: &Wants<'_>) {
    let prefer: Vec<&Stated<'_>> = wants.prefer(TasteDimension::Company).collect();
    let avoid: Vec<&Stated<'_>> = wants.avoid(TasteDimension::Company).collect();
    if prefer.is_empty() && avoid.is_empty() {
        return;
    }
    b.evaluated(Aspect::Company);
    for a in &avoid {
        if let Some(t) = satisfies(a.value, work) {
            let severity = a.severity();
            b.against(
                ContradictionKind::CompanyShape,
                severity,
                format!(
                    "{}, which {} ({})",
                    company_phrase(&t.value),
                    a.avoids(),
                    a.text
                ),
                vec![quote(&t.evidence)],
                a.firmness,
            );
        }
    }
    // Wanting small, early companies (and nothing larger) is wanting not to
    // be at a large or public one.
    let small: Vec<&&Stated<'_>> = prefer
        .iter()
        .filter(|p| SMALL_KINDS.iter().any(|k| same(k, p.value)))
        .collect();
    let open_to_large = prefer
        .iter()
        .chain(
            wants
                .open(TasteDimension::Company)
                .collect::<Vec<_>>()
                .iter(),
        )
        .any(|p| LARGE_KINDS.iter().any(|k| same(k, p.value)));
    let said = b
        .fit
        .contradictions
        .iter()
        .any(|c| c.kind == ContradictionKind::CompanyShape);
    if let Some(strongest) = small.iter().max_by_key(|p| p.firmness)
        && !open_to_large
        && !said
        && let Some(t) = LARGE_KINDS.iter().find_map(|k| work.company_is(k))
    {
        let severity = strongest.severity();
        b.against(
            ContradictionKind::CompanyShape,
            severity,
            format!(
                "{}, while {} {}",
                company_phrase(&t.value),
                strongest.wants(),
                strongest.text.to_lowercase()
            ),
            vec![quote(&t.evidence)],
            strongest.firmness,
        );
    }
    let matched = prefer
        .iter()
        .filter_map(|p| satisfies(p.value, work).map(|t| (p, t)))
        .max_by_key(|(p, _)| p.firmness);
    if let Some((p, t)) = matched {
        let what = match &work.headcount {
            Some((n, _)) if matches!(t.value.as_str(), "startup" | "small_company") => {
                format!("{} of {n} people", company_phrase(&t.value))
            }
            _ => company_phrase(&t.value),
        };
        b.reason(
            Aspect::Company,
            format!(
                "{what}, the kind of company {}: {}",
                p.wants(),
                excerpt(&t.evidence, &COMPANY_CUES)
            ),
            vec![quote(&t.evidence), p.text.to_owned()],
            p.firmness,
            p.firmness.weight(),
        );
    } else if !prefer.is_empty() && work.company.is_empty() {
        b.fit
            .uncertainties
            .push("The posting doesn't say the company's stage or size".to_owned());
    }
}

/// The team someone joins, the culture, and ownership.
fn team_and_culture(b: &mut Builder, work: &WorkReading, wants: &Wants<'_>) {
    // Team.
    let team_prefs: Vec<&Stated<'_>> = wants.about(TasteDimension::Team).collect();
    if !team_prefs.is_empty() {
        b.evaluated(Aspect::Team);
        match &work.team {
            Some(t) => {
                if let Some(p) = wants.find(TasteDimension::Team, &t.value, Polarity::Prefer) {
                    let size = if t.value == "small_team" {
                        "A small team"
                    } else {
                        "A larger team"
                    };
                    b.reason(
                        Aspect::Team,
                        format!(
                            "{size}, as {}: {}",
                            p.wants(),
                            excerpt(&t.evidence, &TEAM_CUES)
                        ),
                        vec![quote(&t.evidence), p.text.to_owned()],
                        p.firmness,
                        p.firmness.weight(),
                    );
                }
                let opposite = if t.value == "small_team" {
                    "large_team"
                } else {
                    "small_team"
                };
                let avoided = wants.find(TasteDimension::Team, &t.value, Polarity::Avoid);
                let wanted_other = wants.find(TasteDimension::Team, opposite, Polarity::Prefer);
                let open_this = wants.find(TasteDimension::Team, &t.value, Polarity::Open);
                if let Some(s) = avoided.or(wanted_other.filter(|_| open_this.is_none())) {
                    let severity = s.severity();
                    let size = if t.value == "large_team" {
                        "A large team"
                    } else {
                        "A small team"
                    };
                    b.against(
                        ContradictionKind::TeamShape,
                        severity,
                        format!(
                            "{size} ({}), while {} {}",
                            clip(&t.evidence, 90),
                            if s.polarity == Polarity::Avoid {
                                s.avoids()
                            } else {
                                s.wants()
                            },
                            s.text.to_lowercase()
                        ),
                        vec![quote(&t.evidence)],
                        s.firmness,
                    );
                }
            }
            None => b
                .fit
                .uncertainties
                .push("The posting doesn't say how big the team is".to_owned()),
        }
    }
    // Culture.
    for p in wants.about(TasteDimension::Culture) {
        b.evaluated(Aspect::Culture);
        let Some(t) = work.culture_is(p.value) else {
            continue;
        };
        match p.polarity {
            Polarity::Prefer => b.reason(
                Aspect::Culture,
                format!("{}: {}", capitalize(p.text), quote(&t.evidence)),
                vec![quote(&t.evidence)],
                p.firmness,
                p.firmness.weight(),
            ),
            Polarity::Avoid => {
                let severity = p.severity();
                b.against(
                    ContradictionKind::Culture,
                    severity,
                    format!(
                        "{}, which {} ({})",
                        quote(&t.evidence),
                        p.avoids(),
                        p.text.to_lowercase()
                    ),
                    vec![quote(&t.evidence)],
                    p.firmness,
                );
            }
            _ => {}
        }
    }
    // Ownership.
    if let Some(p) = wants
        .prefer(TasteDimension::Ownership)
        .max_by_key(|p| p.firmness)
    {
        b.evaluated(Aspect::Ownership);
        if let Some(words) = &work.ownership {
            b.reason(
                Aspect::Ownership,
                format!(
                    "Real ownership, as {}: {}",
                    p.wants(),
                    excerpt(words, &OWNERSHIP_CUES)
                ),
                vec![quote(words)],
                p.firmness,
                p.firmness.weight(),
            );
        } else if let Some(t) = work.culture_is("process_heavy") {
            b.against(
                ContradictionKind::Culture,
                Severity::Holding,
                format!(
                    "Decisions and releases are gated elsewhere ({}), while {} high ownership",
                    clip(&t.evidence, 90),
                    p.wants()
                ),
                vec![quote(&t.evidence)],
                p.firmness,
            );
        }
    }
}

fn domain(b: &mut Builder, facets: &JobFacets, wants: &Wants<'_>) {
    for p in wants.about(TasteDimension::Domain) {
        b.evaluated(Aspect::Domain);
        // "Infrastructure" read as a domain too is the work's shape, which
        // the role already weighed: one piece of evidence counts once.
        let is_shape = wants
            .about(TasteDimension::WorkShape)
            .any(|w| same(w.value, p.value))
            || facets.work.shapes.iter().any(|s| same(&s.shape, p.value));
        if is_shape {
            continue;
        }
        let Some(fact) = facets.domains.iter().find(|d| same(&d.key.value, p.value)) else {
            continue;
        };
        match p.polarity {
            Polarity::Prefer => b.reason(
                Aspect::Domain,
                format!("{}, a domain {}", capitalize(&fact.key.value), p.wants()),
                vec![fact.cite()],
                p.firmness,
                p.firmness.weight(),
            ),
            Polarity::Avoid => b.against(
                ContradictionKind::Domain,
                p.severity(),
                format!("{}, a domain {}", capitalize(&fact.key.value), p.avoids()),
                vec![fact.cite()],
                p.firmness,
            ),
            _ => {}
        }
    }
}

fn technology(b: &mut Builder, facets: &JobFacets, wants: &Wants<'_>) {
    let required = facets.technologies_at(Requirement::Required);
    for p in wants.about(TasteDimension::Technology) {
        b.evaluated(Aspect::Technology);
        let Some(t) = required.iter().find(|t| same(&t.name, p.value)) else {
            continue;
        };
        match p.polarity {
            // A technology they want to work with is taste, but overlap is
            // not role fit: it counts half.
            Polarity::Prefer => b.reason(
                Aspect::Technology,
                format!("Works in {}, as {} ({})", t.name, p.wants(), p.text),
                vec![quote(&t.evidence)],
                p.firmness,
                p.firmness.weight() * 0.5,
            ),
            Polarity::Avoid => b.against(
                ContradictionKind::Technology,
                if p.firmness == Firmness::Firm {
                    Severity::Material
                } else {
                    Severity::Minor
                },
                format!("Requires {}, which {}", t.name, p.avoids()),
                vec![quote(&t.evidence)],
                p.firmness,
            ),
            _ => {}
        }
    }
}

fn work_style(b: &mut Builder, facets: &JobFacets, wants: &Wants<'_>) {
    for p in wants.about(TasteDimension::WorkStyle) {
        b.evaluated(Aspect::WorkStyle);
        let present = facets
            .work_style
            .iter()
            .find(|f| same(&f.key.value, p.value))
            .map(|f| f.evidence.clone())
            .or_else(|| {
                (same(p.value, "management") && facets.manages_people())
                    .then(|| facets.title.clone())
            });
        // Wanting individual-contributor work is avoiding management.
        let manages = same(p.value, "individual_contributor")
            && p.polarity == Polarity::Prefer
            && facets.manages_people();
        if manages {
            b.against(
                ContradictionKind::WorkStyle,
                p.severity(),
                format!(
                    "People management, while {} individual-contributor work",
                    p.wants()
                ),
                vec![quote(&facets.title)],
                p.firmness,
            );
            continue;
        }
        let Some(words) = present else {
            continue;
        };
        match p.polarity {
            Polarity::Prefer => b.reason(
                Aspect::WorkStyle,
                format!("{}: {}", capitalize(p.text), quote(&words)),
                vec![quote(&words)],
                p.firmness,
                p.firmness.weight() * 0.5,
            ),
            Polarity::Avoid => b.against(
                ContradictionKind::WorkStyle,
                if p.firmness == Firmness::Firm {
                    Severity::Material
                } else {
                    Severity::Minor
                },
                format!("{}, which {}", capitalize(p.text), p.avoids()),
                vec![quote(&words)],
                p.firmness,
            ),
            _ => {}
        }
    }
}

/// The employment format, when the posting states it. Whether the person
/// takes contract work is their engagement preference, which eligibility
/// reads; fit says what the terms are and invents no penalty.
fn employment(b: &mut Builder, facets: &JobFacets) {
    if facets.contract == Some(true) {
        b.fit.uncertainties.push(format!(
            "A contract or fixed-term position (“{}”), not a permanent one",
            clip(&facets.title, 90)
        ));
    }
}

/// Technologies whose overlap is easily mistaken for depth.
const DEPTH_PRONE: [(&str, Specialty); 2] = [
    ("PostgreSQL", Specialty::DatabaseInternals),
    ("Kubernetes", Specialty::OrchestrationInternals),
];

/// Context for someone wary of specialist roles: a job that works with a
/// technology whose internals are a specialty, without being that
/// specialty, says so.
fn context(b: &mut Builder, facets: &JobFacets, person: &Person, wants: &Wants<'_>) {
    let broad = wants
        .find(TasteDimension::Specialization, "deep", Polarity::Avoid)
        .or_else(|| wants.find(TasteDimension::Specialization, "broad", Polarity::Prefer));
    if broad.is_none() {
        return;
    }
    for (technology, specialty) in DEPTH_PRONE {
        let uses = facets.technologies.iter().any(|t| t.name == technology);
        if uses && facets.work.specialty(specialty).is_none() {
            let yours = if person.technology(technology).is_some() {
                " the way you have"
            } else {
                ""
            };
            b.reason(
                Aspect::Specialization,
                format!(
                    "Uses {technology}{yours} ({}), not {}",
                    specialty.not_to_confuse_with(),
                    specialty.label()
                ),
                Vec::new(),
                Firmness::Soft,
                0.0,
            );
            return;
        }
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests;
