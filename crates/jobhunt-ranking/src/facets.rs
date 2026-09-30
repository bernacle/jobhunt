//! What a job is, read deterministically from its posting: the shape of
//! the role, its level, the technologies it requires or merely mentions,
//! its domains, the kind of company and team, how the work is done, where,
//! and on what terms.
//!
//! Every fact keeps the words it was read from ([`Fact::evidence`]), and
//! where they were (title, structured field, description). Nothing is
//! guessed: a posting that doesn't say how big the company is has no
//! company-size fact, and that stays unknown rather than "large".
//!
//! Title words decide the role; the description only fills in when the
//! title says nothing more than "Software Engineer". Technologies are
//! weighed by where they appear: in the title or a requirements list they
//! are required, in a "nice to have" list preferred, elsewhere only
//! mentioned.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, LazyLock, Mutex};

use chrono::{DateTime, Utc};
use jobhunt_jobs::{EmploymentType, JobId, JobRecord, WorkplaceType};
use jobhunt_profile::WorkMode;
use jobhunt_profile::infer::{
    domains_in, domains_in_words, role_signals_in, technologies_in, technologies_in_words,
};
use jobhunt_profile::words::{Pattern, Vocabulary, Word, span_text, words};
use serde::{Deserialize, Serialize};

use crate::key::{Dimension, TasteKey};

pub mod work;

pub use work::{Seniority, ShapeBasis, Specialty, WorkReading};

/// Where a fact was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactSource {
    Title,
    /// A structured field: department, team, employment or workplace type,
    /// the source itself.
    Structured,
    Description,
}

impl FactSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Structured => "posting",
            Self::Description => "description",
        }
    }
}

/// One thing the posting says, with its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub key: TasteKey,
    /// The words it was read from, verbatim ("Site Reliability", or a
    /// description sentence).
    pub evidence: String,
    pub source: FactSource,
}

impl Fact {
    /// “Site Reliability” (title)
    pub fn cite(&self) -> String {
        format!("“{}” ({})", self.evidence, self.source.as_str())
    }
}

/// What kind of work the job is, broadly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobFunction {
    Engineering,
    /// Customer-facing engineering: solutions, forward-deployed, sales
    /// engineering.
    CustomerEngineering,
    Sales,
    ProductManagement,
    Design,
    /// Anything else, or a title that doesn't say.
    Other,
}

impl JobFunction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Engineering => "engineering",
            Self::CustomerEngineering => "customer-facing engineering",
            Self::Sales => "sales",
            Self::ProductManagement => "product management",
            Self::Design => "design",
            Self::Other => "a non-engineering role",
        }
    }

    pub fn is_engineering(self) -> bool {
        matches!(self, Self::Engineering | Self::CustomerEngineering)
    }
}

/// Seniority, as a title states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Intern,
    Junior,
    Mid,
    Senior,
    Lead,
    Staff,
    Principal,
    /// People management (engineering manager, head of, director, …).
    Manager,
    Founding,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intern => "intern",
            Self::Junior => "junior",
            Self::Mid => "mid",
            Self::Senior => "senior",
            Self::Lead => "lead",
            Self::Staff => "staff",
            Self::Principal => "principal",
            Self::Manager => "manager",
            Self::Founding => "founding",
        }
    }

    /// Rank on the individual-contributor ladder (management and founding
    /// roles sit beside it, not on it).
    pub fn ic_rank(self) -> Option<u8> {
        match self {
            Self::Intern => Some(0),
            Self::Junior => Some(1),
            Self::Mid => Some(2),
            Self::Senior => Some(3),
            Self::Lead | Self::Staff => Some(4),
            Self::Principal => Some(5),
            Self::Manager | Self::Founding => None,
        }
    }
}

/// Levels titles state, first match wins (so "Senior / Staff" is senior).
const LEVEL_TERMS: [(&str, Level); 24] = [
    ("co founder", Level::Founding),
    ("cofounder", Level::Founding),
    ("founding", Level::Founding),
    ("intern*", Level::Intern),
    ("early career", Level::Junior),
    ("new graduate*", Level::Junior),
    ("new grad*", Level::Junior),
    ("graduate", Level::Junior),
    ("junior", Level::Junior),
    ("=Jr", Level::Junior),
    ("entry level", Level::Junior),
    ("senior", Level::Senior),
    ("=Sr", Level::Senior),
    ("staff", Level::Staff),
    ("principal", Level::Principal),
    ("distinguished", Level::Principal),
    ("lead", Level::Lead),
    ("engineering manager*", Level::Manager),
    ("manager", Level::Manager),
    ("head of", Level::Manager),
    ("director", Level::Manager),
    ("=VP", Level::Manager),
    ("vice president", Level::Manager),
    ("=CTO", Level::Manager),
];

/// Title words that are not a level even though they contain one.
const NOT_A_LEVEL: [&str; 3] = ["technical staff", "account manag*", "product manag*"];

/// Role shapes by title words. `value` is the canonical role key.
const ROLE_TITLE_TERMS: &[(&str, &str)] = &[
    ("site reliability", "sre"),
    ("reliability engineer*", "sre"),
    ("=SRE", "sre"),
    ("sre", "sre"),
    ("devops", "sre"),
    ("dev ops", "sre"),
    ("solutions engineer*", "solutions"),
    ("solutions architect*", "solutions"),
    ("solution architect*", "solutions"),
    ("customer solution*", "solutions"),
    ("support engineer*", "solutions"),
    ("forward deployed", "solutions"),
    ("sales engineer*", "solutions"),
    ("customer engineer*", "solutions"),
    ("field engineer*", "solutions"),
    ("full stack", "full stack"),
    ("fullstack", "full stack"),
    ("backend", "backend"),
    ("back end", "backend"),
    ("server side", "backend"),
    ("distributed systems", "backend"),
    ("=API", "backend"),
    ("frontend", "frontend"),
    ("front end", "frontend"),
    ("design engineer*", "frontend"),
    ("=UI", "frontend"),
    ("web", "frontend"),
    ("platform", "platform"),
    ("infrastructure", "infrastructure"),
    ("infra", "infrastructure"),
    ("cloud", "infrastructure"),
    ("data engineer*", "data"),
    ("analytics engineer*", "data"),
    ("data", "data"),
    ("mobile", "mobile"),
    ("=iOS", "mobile"),
    ("ios", "mobile"),
    ("android", "mobile"),
    ("machine learning", "machine learning"),
    ("=ML", "machine learning"),
    ("=AI", "machine learning"),
    ("research engineer*", "machine learning"),
    ("security", "security"),
    ("appsec", "security"),
    ("embedded", "embedded"),
    ("firmware", "embedded"),
    ("account executive*", "sales"),
    ("account manag*", "sales"),
    ("sales", "sales"),
    ("business development", "sales"),
    ("partnership*", "sales"),
    ("product manag*", "product management"),
    ("product owner*", "product management"),
    ("product lead", "product management"),
    ("platform manager*", "product management"),
    ("program manag*", "product management"),
    ("programs manag*", "product management"),
    ("product designer*", "design"),
    ("designer*", "design"),
    ("=UX", "design"),
];

/// Leadership words that, without an engineering word, make a title about
/// managing something rather than engineering it.
static NON_ENGINEERING_LEAD: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&["manager*", "lead", "director*", "head of"]));

/// Title words that make a job engineering even without a role shape.
const ENGINEERING_TITLE: [&str; 9] = [
    "engineer*",
    "developer*",
    "programmer*",
    "technical staff",
    "architect*",
    "scientist*",
    "=SWE",
    "=MTS",
    "software",
];

/// Section headings that say what follows is required, or only preferred.
const REQUIRED_HEADINGS: [&str; 16] = [
    "requirement*",
    "qualification*",
    "what you ll need",
    "what you need",
    "what you bring",
    "you have",
    "you ll have",
    "you bring",
    "what we re looking for",
    "who you are",
    "about you",
    "must have*",
    "you should have",
    "we re looking for",
    "skills",
    "experience",
];
const PREFERRED_HEADINGS: [&str; 8] = [
    "nice to have*",
    "bonus*",
    "preferred",
    "plus*",
    "a plus",
    "extra credit",
    "nice to haves",
    "it s a plus",
];
const OTHER_HEADINGS: [&str; 10] = [
    "about us",
    "about the role",
    "about the team",
    "responsibilit*",
    "what you ll do",
    "what you will do",
    "benefits",
    "perks",
    "the role",
    "compensation",
];
const PREFERRED_CUES: [&str; 6] = [
    "nice to have",
    "a plus",
    "bonus",
    "preferred",
    "familiarity",
    "exposure to",
];
const REQUIRED_CUES: [&str; 9] = [
    "required",
    "must",
    "proficien*",
    "expert*",
    "experience with",
    "experience in",
    "years of",
    "strong",
    "deep",
];

/// Company and team kinds a description states, with the words.
///
/// A company's stage is read only from wording about the company itself
/// ("we're an early-stage startup", "our seed round"). The stage of a
/// project, a product, a new team or a role ("early-stage work", "a
/// brand-new team", "founding engineer", "first engineer on the product")
/// says nothing about the company: a public company hires founding
/// engineers for new internal bets.
const COMPANY_TERMS: &[(&str, &str)] = &[
    ("we are early stage", "early_stage"),
    ("we re early stage", "early_stage"),
    ("we are an early stage", "early_stage"),
    ("we re an early stage", "early_stage"),
    ("an early stage company", "early_stage"),
    ("an early stage startup", "early_stage"),
    ("our early stage", "early_stage"),
    ("pre seed", "early_stage"),
    ("seed stage", "early_stage"),
    ("seed round", "early_stage"),
    ("series a", "early_stage"),
    ("series b", "scaleup"),
    ("series c", "scaleup"),
    ("series d", "scaleup"),
    // Later rounds are a stage, not a size: they say nothing about teams.
    ("series e", "scaleup"),
    ("series f", "scaleup"),
    ("series g", "scaleup"),
    ("series h", "scaleup"),
    ("pre ipo", "scaleup"),
    ("hypergrowth", "scaleup"),
    ("startup", "startup"),
    ("start up", "startup"),
    ("y combinator", "startup"),
    ("founder*", "founder_led"),
    // Team sizes are read by `team_size`, clause by clause: whose team a
    // size is decides what it can mean.
    // The company itself ("we're a small company"), not its customers
    // ("tools for small companies").
    ("a small company", "small_company"),
    ("a small startup", "small_company"),
    ("fortune 500", "large_company"),
    ("thousands of employees", "large_company"),
    ("publicly traded", "public_company"),
    ("=NYSE", "public_company"),
    ("=NASDAQ", "public_company"),
    ("consultancy", "consulting"),
    ("consulting firm", "consulting"),
    ("consulting company", "consulting"),
    ("client project*", "consulting"),
    ("client engagement*", "consulting"),
    ("staff augmentation", "consulting"),
    ("for our clients", "consulting"),
    ("digital agency", "agency"),
    ("creative agency", "agency"),
    ("our agency", "agency"),
    ("open source", "open_source"),
    ("remote first", "remote_first"),
    ("fully remote", "remote_first"),
    ("fully distributed", "remote_first"),
];

/// How the work is done, as a description states it.
const WORK_STYLE_TERMS: &[(&str, &str)] = &[
    ("ownership", "ownership"),
    ("autonomy", "ownership"),
    ("autonomous*", "ownership"),
    ("end to end", "ownership"),
    ("high agency", "ownership"),
    ("you ll own", "ownership"),
    ("you will own", "ownership"),
    ("on call", "on_call"),
    ("pager*", "on_call"),
    ("greenfield", "greenfield"),
    ("from scratch", "greenfield"),
    ("zero to one", "greenfield"),
    ("0 to 1", "greenfield"),
    ("legacy", "maintenance"),
    ("direct reports", "management"),
    ("manage a team", "management"),
    ("managing a team", "management"),
    ("people management", "management"),
    ("line manag*", "management"),
    ("hire and grow", "management"),
    ("async*", "async_communication"),
    ("asynchronous*", "async_communication"),
    ("written communication", "async_communication"),
    ("product minded", "product_closeness"),
    ("product engineer*", "product_closeness"),
    ("close to customers", "product_closeness"),
    ("close to users", "product_closeness"),
    ("close to the product", "product_closeness"),
    ("talk to users", "product_closeness"),
    ("talk to customers", "product_closeness"),
    ("individual contributor", "individual_contributor"),
    ("hands on", "individual_contributor"),
];

/// Headings of sections about the employer's benefits and policies, which
/// say nothing about the job ("medical, dental & vision insurance" is not
/// the healthcare domain).
const BOILERPLATE_HEADINGS: [&str; 9] = [
    "benefit*",
    "perks",
    "what we offer",
    "compensation",
    "equal opportunit*",
    "equal employment",
    "our commitment",
    "accommodation*",
    "pay transparency",
];

/// Sentences about benefits, pay and hiring policy.
const BOILERPLATE_CUES: [&str; 20] = [
    "dental",
    "401k",
    "401 k",
    "health insurance",
    "life insurance",
    "medical insurance",
    "insurance coverage",
    "parental leave",
    "paid time off",
    "=PTO",
    "equal opportunity",
    "equal employment",
    "reasonable accommodation*",
    "stipend*",
    "wellness",
    "vacation",
    "benefits",
    "perks",
    "base salary",
    "pay range",
];

// The vocabularies above, compiled once (a posting is read against every
// one of them, sentence by sentence).
fn compile_terms(terms: &[(&str, &'static str)]) -> Terms {
    let patterns: Vec<&str> = terms.iter().map(|(p, _)| *p).collect();
    (
        Vocabulary::compile(&patterns),
        terms.iter().map(|(_, v)| *v).collect(),
    )
}
/// A vocabulary of phrases and the value each one stands for.
type Terms = (Vocabulary, Vec<&'static str>);
static REQUIRED_HEADING_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&REQUIRED_HEADINGS));
static PREFERRED_HEADING_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&PREFERRED_HEADINGS));
static OTHER_HEADING_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&OTHER_HEADINGS));
static PREFERRED_CUE_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&PREFERRED_CUES));
static REQUIRED_CUE_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&REQUIRED_CUES));
static BOILERPLATE_HEADING_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&BOILERPLATE_HEADINGS));
static BOILERPLATE_CUE_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&BOILERPLATE_CUES));
static ENGINEERING_TITLE_PATTERNS: LazyLock<Vocabulary> =
    LazyLock::new(|| Vocabulary::compile(&ENGINEERING_TITLE));
static ROLE_TITLE_PATTERNS: LazyLock<Terms> = LazyLock::new(|| compile_terms(ROLE_TITLE_TERMS));
static COMPANY_PATTERNS: LazyLock<Terms> = LazyLock::new(|| compile_terms(COMPANY_TERMS));
static WORK_STYLE_PATTERNS: LazyLock<Terms> = LazyLock::new(|| compile_terms(WORK_STYLE_TERMS));
static TEAM_OF: LazyLock<Pattern> = LazyLock::new(|| Pattern::new("team of _"));

/// A content sentence of a description, split into words once.
struct Sentence {
    text: String,
    words: Vec<Word>,
    /// Its clauses, as word ranges (see [`clauses`]).
    clauses: Vec<Range<usize>>,
}

/// The sentences of a description that describe the job and the company,
/// without benefits and policy boilerplate.
pub fn content_sentences(description: &str) -> Vec<String> {
    content(description).into_iter().map(|s| s.text).collect()
}

/// [`content_sentences`], with their words: read once per posting and
/// shared by every reader of the description.
fn content(description: &str) -> Vec<Sentence> {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in description.lines() {
        let trimmed = line.trim().trim_end_matches(':');
        let ws = words(trimmed);
        let short = !trimmed.is_empty() && ws.len() <= 6 && !trimmed.ends_with('.');
        if short && !trimmed.starts_with(['-', '*', '•', '–']) {
            if starts_with_any(&ws, &BOILERPLATE_HEADING_PATTERNS) {
                skipping = true;
                continue;
            }
            if heading_kind(line).is_some() {
                skipping = false;
            }
        }
        if skipping {
            continue;
        }
        for sentence in jobhunt_eligibility::job::sentences(line) {
            let ws = words(&sentence);
            if !has(&ws, &BOILERPLATE_CUE_PATTERNS) {
                let clauses = clauses(&sentence, &ws);
                out.push(Sentence {
                    text: sentence,
                    words: ws,
                    clauses,
                });
            }
        }
    }
    out
}

/// How strongly a job asks for a technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    Mentioned,
    Preferred,
    Required,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mentioned => "mentioned",
            Self::Preferred => "preferred",
            Self::Required => "required",
        }
    }
}

/// A technology a job names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechFact {
    /// Canonical name (`PostgreSQL`).
    pub name: String,
    pub requirement: Requirement,
    pub in_title: bool,
    /// The sentence it appears in.
    pub evidence: String,
}

/// Everything read from one job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobFacets {
    pub title: String,
    pub company: String,
    pub function: JobFunction,
    /// Role shapes: `role:*` keys.
    pub roles: Vec<Fact>,
    pub level: Option<(Level, String)>,
    pub technologies: Vec<TechFact>,
    /// `domain:*` keys.
    pub domains: Vec<Fact>,
    /// `company_trait:*` keys.
    pub company_traits: Vec<Fact>,
    /// `work_style:*` keys.
    pub work_style: Vec<Fact>,
    /// Ways of working the posting offers.
    pub work_modes: Vec<WorkMode>,
    /// `Some(true)` for contract work, `Some(false)` for employment,
    /// `None` when the posting doesn't say.
    pub contract: Option<bool>,
    /// When it was posted, or first seen when the source doesn't say.
    pub listed_at: DateTime<Utc>,
    /// Whether `listed_at` is the source's own publish date.
    pub posted: bool,
    /// The work itself, read for fit: level, shape, depth, company shape.
    #[serde(default)]
    pub work: WorkReading,
}

impl JobFacets {
    pub fn has_role(&self, value: &str) -> bool {
        self.roles.iter().any(|f| f.key.value == value)
    }

    /// Technologies at or above a requirement level.
    pub fn technologies_at(&self, at_least: Requirement) -> Vec<&TechFact> {
        self.technologies
            .iter()
            .filter(|t| t.requirement >= at_least || t.in_title)
            .collect()
    }

    /// Whether people management is part of the job.
    pub fn manages_people(&self) -> bool {
        matches!(self.level, Some((Level::Manager, _)))
            || self.work_style.iter().any(|f| f.key.value == "management")
    }

    /// Every fact as a key with its words, for matching preferences and
    /// learning from feedback. Only technologies the job asks for (or
    /// names in its title) count; a technology merely mentioned says
    /// little about the job.
    pub fn keys(&self) -> Vec<(TasteKey, String)> {
        let mut out: Vec<(TasteKey, String)> = Vec::new();
        let mut push = |key: TasteKey, evidence: String| {
            if !out.iter().any(|(k, _)| *k == key) {
                out.push((key, evidence));
            }
        };
        for f in self
            .roles
            .iter()
            .chain(&self.domains)
            .chain(&self.company_traits)
            .chain(&self.work_style)
        {
            push(f.key.clone(), f.cite());
        }
        if let Some((level, words)) = &self.level {
            push(
                TasteKey::new(Dimension::Seniority, level.as_str()),
                format!("“{words}” (title)"),
            );
        }
        for t in self
            .technologies_at(Requirement::Required)
            .into_iter()
            .take(6)
        {
            push(
                TasteKey::new(Dimension::Technology, t.name.clone()),
                format!("{} ({})", t.name, t.requirement.as_str()),
            );
        }
        push(company_key(&self.company), format!("at {}", self.company));
        out
    }

    /// The fact for a key, if the job has it.
    pub fn evidence_for(&self, key: &TasteKey) -> Option<String> {
        self.keys()
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, e)| e)
    }
}

/// The key for one employer.
pub fn company_key(company: &str) -> TasteKey {
    TasteKey::new(Dimension::Company, jobhunt_core::text::search_key(company))
}

fn find_terms(
    text: &str,
    ws: &[Word],
    (vocabulary, values): &Terms,
) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for (id, range) in vocabulary.find_first(ws) {
        let value = values[id];
        if !out.iter().any(|(v, _)| *v == value) {
            out.push((value, span_text(text, ws, &range).to_owned()));
        }
    }
    out
}

fn has(ws: &[Word], vocabulary: &Vocabulary) -> bool {
    vocabulary.any(ws)
}

/// A phrase of `vocabulary` within the first three words (a heading's
/// subject).
fn starts_with_any(ws: &[Word], vocabulary: &Vocabulary) -> bool {
    vocabulary.find_first(ws).iter().any(|(_, r)| r.start <= 2)
}

/// The level a title states.
pub fn title_level(title: &str) -> Option<(Level, String)> {
    // "Staff+ Engineer" is staff ("+" otherwise stays part of a word).
    let title = &title.replace('+', " ");
    let ws = words(title);
    let masked: Vec<std::ops::Range<usize>> = NOT_A_LEVEL
        .iter()
        .filter_map(|p| Pattern::new(p).find(&ws))
        .collect();
    LEVEL_TERMS.iter().find_map(|(pattern, level)| {
        Pattern::new(pattern)
            .find_all(&ws)
            .into_iter()
            .find(|r| !masked.iter().any(|m| m.start <= r.start && r.end <= m.end))
            .map(|r| (*level, span_text(title, &ws, &r).to_owned()))
    })
}

/// Role shapes a title names, with the words.
pub fn title_roles(title: &str) -> Vec<(&'static str, String)> {
    let ws = words(title);
    let mut found = find_terms(title, &ws, &ROLE_TITLE_PATTERNS);
    // "Design Engineer" is frontend engineering, not design.
    if found.iter().any(|(v, _)| *v == "frontend") {
        found.retain(|(v, _)| *v != "design");
    }
    // "Product Manager - Data Platform": the data platform is the product.
    if found
        .iter()
        .any(|(v, _)| matches!(*v, "sales" | "product management"))
        && !has(&ws, &ENGINEERING_TITLE_PATTERNS)
    {
        found.retain(|(v, _)| matches!(*v, "sales" | "product management" | "design"));
    }
    found
}

/// Title words of work that isn't engineering, whatever else the title
/// says ("Event Programs Manager, Developer Community").
static NOT_ENGINEERING: LazyLock<Vocabulary> = LazyLock::new(|| {
    Vocabulary::compile(&[
        "developer community",
        "developer relations",
        "developer advocate*",
        "devrel",
        "developer marketing",
        "developer education",
        "community manag*",
        "event*",
        "recruit*",
        "marketing",
        "evangelist*",
    ])
});

fn function_of(title: &str, roles: &[(&str, String)]) -> JobFunction {
    let ws = words(title);
    let is = |v: &str| roles.iter().any(|(r, _)| *r == v);
    if has(&ws, &NOT_ENGINEERING) {
        // "Developer Relations Engineer": customer-facing engineering.
        if has(&ws, &ENGINEERING_TITLE_PATTERNS) {
            JobFunction::CustomerEngineering
        } else {
            JobFunction::Other
        }
    } else if is("solutions") {
        JobFunction::CustomerEngineering
    } else if has(&ws, &ENGINEERING_TITLE_PATTERNS) || is("sre") {
        JobFunction::Engineering
    } else if is("sales") {
        JobFunction::Sales
    } else if is("product management") {
        JobFunction::ProductManagement
    } else if (is("platform") || is("infrastructure") || is("data"))
        && has(&ws, &NON_ENGINEERING_LEAD)
    {
        // "Staff Platform Manager, Payments": a manager of a platform, not
        // an engineer on one.
        JobFunction::ProductManagement
    } else if is("design") {
        JobFunction::Design
    } else if !title_roles(work::split_title(title).0).is_empty() {
        // "Backend (Rust)" without the word "engineer" (the title's own
        // words: "Product Quality Analyst - AI Voice" isn't engineering).
        JobFunction::Engineering
    } else {
        JobFunction::Other
    }
}

/// What a description section heading says about the lines under it.
fn heading_kind(line: &str) -> Option<Requirement> {
    let trimmed = line.trim().trim_end_matches(':');
    // Bullets, sentences and lines naming a technology are content.
    if trimmed.is_empty()
        || trimmed.len() > 80
        || trimmed.ends_with('.')
        || trimmed.starts_with(['-', '*', '•', '–'])
    {
        return None;
    }
    let ws = words(trimmed);
    if !technologies_in_words(trimmed, &ws, false).is_empty() || ws.len() > 6 {
        return None;
    }
    if starts_with_any(&ws, &PREFERRED_HEADING_PATTERNS) {
        Some(Requirement::Preferred)
    } else if starts_with_any(&ws, &REQUIRED_HEADING_PATTERNS) {
        Some(Requirement::Required)
    } else if starts_with_any(&ws, &OTHER_HEADING_PATTERNS) {
        Some(Requirement::Mentioned)
    } else {
        None
    }
}

fn technologies(title: &str, description: &str) -> Vec<TechFact> {
    let mut out: Vec<TechFact> = Vec::new();
    for m in technologies_in(title, false) {
        out.push(TechFact {
            name: m.technology.to_owned(),
            requirement: Requirement::Required,
            in_title: true,
            evidence: title.to_owned(),
        });
    }
    let mut section = Requirement::Mentioned;
    for line in description.lines() {
        if let Some(kind) = heading_kind(line) {
            section = kind;
            continue;
        }
        for sentence in jobhunt_eligibility::job::sentences(line) {
            let ws = words(&sentence);
            let mentions = technologies_in_words(&sentence, &ws, false);
            if mentions.is_empty() {
                continue;
            }
            let level = if has(&ws, &PREFERRED_CUE_PATTERNS) {
                Requirement::Preferred
            } else if section == Requirement::Mentioned && has(&ws, &REQUIRED_CUE_PATTERNS) {
                Requirement::Required
            } else {
                section
            };
            for m in mentions {
                match out.iter_mut().find(|t| t.name == m.technology) {
                    Some(existing) if level > existing.requirement => {
                        existing.requirement = level;
                        existing.evidence.clone_from(&sentence);
                    }
                    Some(_) => {}
                    None => out.push(TechFact {
                        name: m.technology.to_owned(),
                        requirement: level,
                        in_title: false,
                        evidence: sentence.clone(),
                    }),
                }
            }
        }
    }
    out
}

fn domains(title: &str, structured: &[&str], sentences: &[Sentence]) -> Vec<Fact> {
    let mut out: Vec<Fact> = Vec::new();
    let mut push = |domain: &str, evidence: String, source: FactSource| {
        if !out.iter().any(|f| f.key.value == domain) {
            out.push(Fact {
                key: TasteKey::new(Dimension::Domain, domain),
                evidence,
                source,
            });
        }
    };
    for hit in domains_in(title).into_iter().filter(|h| h.strong) {
        push(hit.domain, hit.phrase, FactSource::Title);
    }
    for field in structured {
        for hit in domains_in(field).into_iter().filter(|h| h.strong) {
            push(hit.domain, hit.phrase, FactSource::Structured);
        }
    }
    // The description counts when it names the domain more than once, or
    // once alongside typical vocabulary: one stray "AI" is not a domain.
    let mut tally: Vec<(&str, usize, usize, String)> = Vec::new();
    for Sentence {
        text: sentence,
        words: ws,
        ..
    } in sentences
    {
        for hit in domains_in_words(sentence, ws) {
            match tally.iter_mut().find(|(d, ..)| *d == hit.domain) {
                Some(entry) => {
                    if hit.strong {
                        entry.1 += 1;
                        if entry.1 == 1 {
                            entry.3.clone_from(sentence);
                        }
                    } else {
                        entry.2 += 1;
                    }
                }
                None => tally.push((
                    hit.domain,
                    usize::from(hit.strong),
                    usize::from(!hit.strong),
                    sentence.clone(),
                )),
            }
        }
    }
    for (domain, strong, weak, sentence) in tally {
        if strong >= 2 || (strong >= 1 && weak >= 2) {
            push(domain, sentence, FactSource::Description);
        }
    }
    out
}

fn statements(sentences: &[Sentence], terms: &Terms, dimension: Dimension) -> Vec<Fact> {
    let mut out: Vec<Fact> = Vec::new();
    for sentence in sentences {
        let found = if dimension == Dimension::CompanyTrait {
            affirmed_terms(sentence, terms)
        } else {
            find_terms(&sentence.text, &sentence.words, terms)
                .into_iter()
                .map(|(v, _)| v)
                .collect()
        };
        for value in found {
            if !out.iter().any(|f| f.key.value == value) {
                out.push(Fact {
                    key: TasteKey::new(dimension, value),
                    evidence: sentence.text.clone(),
                    source: FactSource::Description,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Clauses: where negation and "whose team" are read.

/// Punctuation between two words that ends a clause.
const CLAUSE_MARKS: [char; 9] = [',', ';', ':', '(', ')', '—', '–', '!', '?'];
/// Words that open a new clause, turning against the one before ("we're
/// not a startup, but a public company").
const CONTRAST: [&str; 6] = ["but", "however", "although", "though", "yet", "whereas"];

/// A sentence's clauses, as word ranges: split at punctuation between
/// words and before contrast words. A fact is read within its clause:
/// negation there denies it, and what the clause is about (the team the
/// person joins, or the company) says whose size it is.
fn clauses(text: &str, ws: &[Word]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..ws.len() {
        let between = &text[ws[i - 1].span.end..ws[i].span.start];
        if between.contains(&CLAUSE_MARKS[..]) || CONTRAST.contains(&ws[i].lower.as_str()) {
            out.push(start..i);
            start = i;
        }
    }
    if !ws.is_empty() {
        out.push(start..ws.len());
    }
    out
}

/// Words that deny what follows them in the same clause ("isn't" is split
/// into "isn" and "t").
const NEGATORS: [&str; 13] = [
    "not", "no", "never", "without", "nor", "isn", "aren", "wasn", "weren", "don", "doesn", "didn",
    "won",
];

impl Sentence {
    /// The clause word `at` is in.
    fn clause(&self, at: usize) -> Range<usize> {
        self.clauses
            .iter()
            .find(|c| c.contains(&at))
            .cloned()
            .unwrap_or(0..self.words.len())
    }

    /// Whether the phrase starting at word `start` is denied: a negator
    /// before it in its own clause.
    fn denied(&self, start: usize) -> bool {
        let clause = self.clause(start);
        self.words[clause.start..start]
            .iter()
            .any(|w| NEGATORS.contains(&w.lower.as_str()))
    }
}

/// The company kinds a sentence states, not the ones it denies: a denied
/// kind is no evidence either way.
fn affirmed_terms(sentence: &Sentence, (vocabulary, values): &Terms) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (id, ranges) in vocabulary.find_all(&sentence.words) {
        let value = values[id];
        if !out.contains(&value) && ranges.iter().any(|r| !sentence.denied(r.start)) {
            out.push(value);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Team size.

/// A clause about the company, not a team ("our company is a team of 200
/// employees"): no size in it is a team's.
const COMPANY_WORDS: [&str; 8] = [
    "company",
    "companies",
    "organization",
    "organisation",
    "org",
    "employees",
    "staff",
    "headcount",
];
/// Words right before "team" that make it everyone ("our global team of
/// 200 people"). How a team works ("distributed", "remote") is not one.
const WHOLE: [&str; 6] = ["global", "entire", "whole", "worldwide", "overall", "total"];
/// What says a clause is about the team the person would join.
static JOINING: LazyLock<Vocabulary> = LazyLock::new(|| {
    Vocabulary::compile(&[
        "join*",
        "part of",
        "work in",
        "work on",
        "working in",
        "working on",
        "be on",
        "sit on",
        "sit in",
        "your team",
        "one of",
    ])
});
/// Team size said in words.
static TEAM_PHRASES: LazyLock<Terms> = LazyLock::new(|| {
    compile_terms(&[
        ("small team", "small_team"),
        ("tiny team", "small_team"),
        ("lean team", "small_team"),
        ("large team", "large_team"),
        ("big team", "large_team"),
    ])
});
/// Who a number of people counts ("has 6 engineers").
const PEOPLE: [&str; 5] = ["engineers", "people", "developers", "members", "designers"];

fn size_of(n: u32) -> Option<&'static str> {
    match n {
        2..=25 => Some("small_team"),
        50.. => Some("large_team"),
        _ => None,
    }
}

/// The team sizes a clause states, with where each starts and the
/// position of its "team": "a small team", "a team of 6", "the team has
/// 6 engineers".
fn team_mentions(ws: &[Word], clause: &Range<usize>) -> Vec<(usize, usize, &'static str)> {
    let words = &ws[clause.clone()];
    let mut out = Vec::new();
    let (phrases, values) = &*TEAM_PHRASES;
    for (id, ranges) in phrases.find_all(words) {
        for r in ranges {
            out.push((clause.start + r.start, clause.start + r.end - 1, values[id]));
        }
    }
    for r in TEAM_OF.find_all(words) {
        if let Some(size) = words[r.end - 1].lower.parse().ok().and_then(size_of) {
            out.push((clause.start + r.start, clause.start + r.start, size));
        }
    }
    if let Some(team) = words.iter().position(|w| w.lower == "team") {
        for (i, pair) in words.windows(2).enumerate().skip(team + 1) {
            let after_of = words[i - 1].lower == "of";
            if let (false, Ok(n)) = (after_of, pair[0].lower.parse::<u32>())
                && PEOPLE.contains(&pair[1].lower.as_str())
                && let Some(size) = size_of(n)
            {
                out.push((clause.start + i, clause.start + team, size));
            }
        }
    }
    // "You'll be one of 5 engineers": the team someone joins.
    for (i, w) in words.iter().enumerate() {
        if w.lower == "one"
            && words.get(i + 1).is_some_and(|x| x.lower == "of")
            && let Some(n) = words.get(i + 2).and_then(|x| x.lower.parse::<u32>().ok())
            && words
                .get(i + 3)
                .is_some_and(|x| PEOPLE.contains(&x.lower.as_str()))
            && let Some(size) = size_of(n + 1)
        {
            out.push((clause.start + i, clause.start + i, size));
        }
    }
    out.sort_by_key(|m| m.0);
    out
}

/// The size of the team someone would join, read clause by clause:
///
/// 1. said of the team the person joins ("you'll join a team of 6
///    engineers", "you'll be part of a large team"): that is the team's
///    size, small or large;
/// 2. failing that, a small team mentioned without saying whose ("a small
///    product team of 12") still counts as small;
/// 3. otherwise unknown. A large team without saying whose stays unknown,
///    since whose it is decides whether it rules anything out.
///
/// A clause about the company ("our company is a team of 200 employees")
/// or everyone ("our global team of 200 people") never gives a team's
/// size, and a denied size ("you won't join a large team") is no evidence.
fn team_size(sentences: &[Sentence]) -> Option<Fact> {
    let fact = |size: &str, evidence: &String| Fact {
        key: TasteKey::new(Dimension::CompanyTrait, size),
        evidence: evidence.clone(),
        source: FactSource::Description,
    };
    let mut unattributed_small: Option<Fact> = None;
    for sentence in sentences {
        for clause in &sentence.clauses {
            let words = &sentence.words[clause.clone()];
            // A company word before the team makes the size the company's
            // ("our company is a team of 200"); after it, it only says where
            // the team sits ("a team of 60 engineers in the Billing
            // organization").
            let company_at = words
                .iter()
                .position(|w| COMPANY_WORDS.contains(&w.lower.as_str()))
                .map(|i| clause.start + i);
            let joining = JOINING.any(words);
            for (start, team, size) in team_mentions(&sentence.words, clause) {
                if company_at.is_some_and(|at| at < team.min(start)) {
                    continue;
                }
                let everyone = sentence.words[team.saturating_sub(2).max(clause.start)..team]
                    .iter()
                    .any(|w| WHOLE.contains(&w.lower.as_str()));
                if everyone || sentence.denied(start) {
                    continue;
                }
                if joining {
                    return Some(fact(size, &sentence.text));
                }
                if size == "small_team" && unattributed_small.is_none() {
                    unattributed_small = Some(fact(size, &sentence.text));
                }
            }
        }
    }
    unattributed_small
}

/// How many postings [`facets_of`] remembers (a few megabytes); past that
/// it starts over.
const REMEMBERED: usize = 20_000;

/// Facets by record and a hash of what they were read from.
type Remembered = HashMap<(JobId, u64), Arc<JobFacets>>;

static REMEMBERED_FACETS: LazyLock<Mutex<Remembered>> = LazyLock::new(Mutex::default);

/// Everything [`facets`] reads from a record, hashed: a new version of a
/// posting is a new key.
fn facets_input(record: &JobRecord) -> u64 {
    let job = &record.posting;
    let mut h = std::hash::DefaultHasher::new();
    (
        &job.title,
        &job.description_text,
        &job.department,
        &job.team,
        &job.company,
        job.is_remote,
        job.posted_at,
        record.first_seen_at,
    )
        .hash(&mut h);
    format!(
        "{:?}{:?}{}",
        job.workplace_type, job.employment_type, job.provenance.source
    )
    .hash(&mut h);
    h.finish()
}

/// [`facets`], remembered per posting version. Reading a posting is most
/// of what ranking costs, and every ranking of every person reads the same
/// postings; the facts only change when the posting does.
pub fn facets_of(record: &JobRecord) -> Arc<JobFacets> {
    let key = (record.id, facets_input(record));
    if let Some(found) = REMEMBERED_FACETS
        .lock()
        .ok()
        .and_then(|m| m.get(&key).cloned())
    {
        return found;
    }
    let read = Arc::new(facets(record));
    if let Ok(mut remembered) = REMEMBERED_FACETS.lock() {
        if remembered.len() >= REMEMBERED {
            remembered.clear();
        }
        remembered.insert(key, Arc::clone(&read));
    }
    read
}

/// Reads one stored job.
pub fn facets(record: &JobRecord) -> JobFacets {
    let job = &record.posting;
    let title = job.title.trim().to_owned();
    let description = job.description_text.as_deref().unwrap_or_default();
    let title_found = title_roles(&title);
    let function = function_of(&title, &title_found);
    let mut roles: Vec<Fact> = title_found
        .iter()
        .map(|(value, words)| Fact {
            key: TasteKey::role(value),
            evidence: words.clone(),
            source: FactSource::Title,
        })
        .collect();
    let technologies = technologies(&title, description);
    let sentences = content(description);
    // A generic engineering title: the description says what kind.
    if function == JobFunction::Engineering && roles.is_empty() {
        let techs: Vec<&str> = technologies
            .iter()
            .filter(|t| t.requirement >= Requirement::Preferred)
            .map(|t| t.name.as_str())
            .collect();
        // What the company does ("we provide a complete backend
        // solution") says nothing about this role's work.
        let texts: Vec<(&str, &[Word])> = sentences
            .iter()
            .filter(|s| !work::about_company(s, &job.company))
            .map(|s| (s.text.as_str(), s.words.as_slice()))
            .collect();
        for signal in role_signals_in(None, &techs, &texts) {
            let value = signal.role;
            if !roles.iter().any(|f| f.key.value == value) {
                roles.push(Fact {
                    key: TasteKey::role(value),
                    evidence: signal.reasons.join("; "),
                    source: FactSource::Description,
                });
            }
        }
    }
    let structured: Vec<&str> = [job.department.as_deref(), job.team.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    let mut company_traits = statements(&sentences, &COMPANY_PATTERNS, Dimension::CompanyTrait);
    if let Some(fact) = team_size(&sentences) {
        company_traits.push(fact);
    }
    // Later-stage evidence ("our $500M Series F", "publicly traded",
    // "thousands of employees") outranks early-stage wording in the same
    // posting ("we raised our Series A in 2016"): the company is no longer
    // early-stage. A job board (Y Combinator's included, where alumni like
    // public companies post too) says nothing about the stage.
    if company_traits.iter().any(|f| {
        matches!(
            f.key.value.as_str(),
            "scaleup" | "public_company" | "large_company"
        )
    }) {
        company_traits.retain(|f| f.key.value != "early_stage");
    }
    let mut work_style = statements(&sentences, &WORK_STYLE_PATTERNS, Dimension::WorkStyle);
    let level = title_level(&title);
    if let Some((Level::Manager, words)) = &level
        && function.is_engineering()
        && !work_style.iter().any(|f| f.key.value == "management")
    {
        work_style.push(Fact {
            key: TasteKey::new(Dimension::WorkStyle, "management"),
            evidence: words.clone(),
            source: FactSource::Title,
        });
    }
    let mut work_modes = Vec::new();
    match &job.workplace_type {
        Some(WorkplaceType::Remote) => work_modes.push(WorkMode::Remote),
        Some(WorkplaceType::Hybrid) => work_modes.push(WorkMode::Hybrid),
        Some(WorkplaceType::OnSite) => work_modes.push(WorkMode::Onsite),
        Some(WorkplaceType::Other(_)) | None => {}
    }
    if job.is_remote == Some(true) && job.workplace_type.is_none() {
        work_modes.push(WorkMode::Remote);
    }
    let contract = match &job.employment_type {
        Some(EmploymentType::Contract | EmploymentType::Temporary) => Some(true),
        Some(EmploymentType::FullTime | EmploymentType::PartTime) => Some(false),
        Some(EmploymentType::Internship | EmploymentType::Other(_)) | None => None,
    };
    let mut facets = JobFacets {
        company: job.company.trim().to_owned(),
        function,
        roles,
        level,
        domains: domains(&title, &structured, &sentences),
        technologies,
        company_traits,
        work_style,
        work_modes,
        contract,
        listed_at: job.posted_at.unwrap_or(record.first_seen_at),
        posted: job.posted_at.is_some(),
        title,
        work: WorkReading::default(),
    };
    facets.work = work::read(&facets, &sentences);
    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::record;

    fn sizes(description: &str) -> Vec<String> {
        facets(&record("ashby:acme", "Backend Engineer", description))
            .company_traits
            .iter()
            .map(|f| f.key.value.clone())
            .filter(|v| v.ends_with("_team") || v.ends_with("_company"))
            .collect()
    }

    #[test]
    fn a_company_headcount_is_not_the_team_someone_joins() {
        assert_eq!(
            sizes(
                "A global team of 200 people across 30 countries. You will join a team of 6 \
                 engineers."
            ),
            ["small_team"],
            "the team you join, not the company"
        );
        assert!(sizes("A global team of 200 people across 30 countries.").is_empty());
        assert!(sizes("We are a team of 200 employees.").is_empty());
        assert!(
            sizes("Our team of 200 engineers builds payments.").is_empty(),
            "whose 200 it is isn't said: unknown, not large"
        );
        assert_eq!(
            sizes("You'll join a team of 60 engineers on payments."),
            ["large_team"]
        );
        assert_eq!(
            sizes("We are a small product team of 12 engineers."),
            ["small_team"]
        );
    }

    #[test]
    fn denied_sizes_are_not_facts() {
        assert!(sizes("You will not be part of a large team.").is_empty());
        assert!(sizes("You will not be part of a team of 200 engineers.").is_empty());
        assert!(sizes("We're not a small company.").is_empty());
        assert!(sizes("We aren't a small startup.").is_empty());
        let traits = |d: &str| -> Vec<String> {
            facets(&record("ashby:acme", "Backend Engineer", d))
                .company_traits
                .iter()
                .map(|f| f.key.value.clone())
                .collect()
        };
        assert!(
            !traits("This is not a publicly traded company.").contains(&"public_company".into())
        );
        assert!(!traits("We are not a Fortune 500 company.").contains(&"large_company".into()));
        // Stated, they are.
        assert_eq!(sizes("We're a small company."), ["small_company"]);
        assert_eq!(sizes("You will be part of a large team."), ["large_team"]);
    }

    fn traits(description: &str) -> Vec<String> {
        facets(&record("ashby:acme", "Backend Engineer", description))
            .company_traits
            .iter()
            .map(|f| f.key.value.clone())
            .collect()
    }

    #[test]
    fn the_team_someone_joins_outranks_what_the_company_says() {
        // Company wording first, the joined team after: the team's size.
        assert_eq!(
            sizes(
                "Our company is a large team of 200 employees. You will join a team of 6 engineers."
            ),
            ["small_team"]
        );
        let big_company_small_team = traits(
            "We are a publicly traded company with thousands of employees. You'll join a team \
             of 6 engineers.",
        );
        for expected in ["public_company", "large_company", "small_team"] {
            assert!(
                big_company_small_team.contains(&expected.into()),
                "{big_company_small_team:?}"
            );
        }
        assert!(!big_company_small_team.contains(&"large_team".into()));
        let small_company_big_team =
            traits("We're a small company. You'll join a team of 60 engineers.");
        assert!(small_company_big_team.contains(&"small_company".into()));
        assert!(small_company_big_team.contains(&"large_team".into()));
        // No team someone joins: unknown, unless a small team is said.
        assert!(sizes("Our company is a large team of 200 employees.").is_empty());
        assert!(
            sizes("We are a large team.").is_empty(),
            "whose large team isn't said"
        );
        assert_eq!(sizes("We are a small team."), ["small_team"]);
        assert_eq!(
            sizes("Not a large company. The team you'll join has 6 engineers."),
            ["small_team"]
        );
    }

    #[test]
    fn distributed_is_how_a_team_works_not_its_headcount() {
        assert_eq!(
            sizes("You will join a distributed team of 6 engineers."),
            ["small_team"]
        );
        assert_eq!(
            sizes("You will join a globally distributed team of 6 engineers."),
            ["small_team"]
        );
        assert!(sizes("We are a distributed company with 200 employees.").is_empty());
        assert!(
            sizes("Join our global team of 200 people.").is_empty(),
            "everyone, not a team"
        );
    }

    #[test]
    fn negation_stays_in_its_clause() {
        let t = traits("We are not a startup, but a publicly traded company.");
        assert!(!t.contains(&"startup".into()), "{t:?}");
        assert!(t.contains(&"public_company".into()), "{t:?}");
        let t = traits("We are not a publicly traded company, but we're a small startup.");
        assert!(!t.contains(&"public_company".into()), "{t:?}");
        assert!(t.contains(&"small_company".into()), "{t:?}");
        assert!(sizes("You will not be working as part of a large team.").is_empty());
        assert!(sizes("You won't join a large team.").is_empty());
        // Stated, they are.
        assert!(traits("We are a startup.").contains(&"startup".into()));
        assert!(traits("We are a publicly traded company.").contains(&"public_company".into()));
        assert_eq!(
            sizes("You will be working as part of a large team."),
            ["large_team"]
        );
        assert_eq!(sizes("You'll join a large team."), ["large_team"]);
    }

    #[test]
    fn remembered_facets_follow_the_posting_version() {
        let first = record(
            "ashby:acme",
            "Backend Engineer",
            "We are a small team of 8.",
        );
        let a = facets_of(&first);
        assert!(Arc::ptr_eq(&a, &facets_of(&first)), "read once per version");
        assert_eq!(*a, facets(&first));
        let mut edited = first.clone();
        edited.posting.description_text = Some("Join a team of 200 engineers.".into());
        let b = facets_of(&edited);
        assert!(b.company_traits.iter().any(|f| f.key.value == "large_team"));
        assert!(!b.company_traits.iter().any(|f| f.key.value == "small_team"));
    }

    #[test]
    fn titles_name_roles_and_levels() {
        let roles = |t: &str| -> Vec<&str> { title_roles(t).into_iter().map(|(v, _)| v).collect() };
        assert_eq!(roles("Site Reliability Engineer"), ["sre"]);
        assert_eq!(roles("Senior Backend Engineer (Payments)"), ["backend"]);
        assert_eq!(roles("Design Engineer (Web & Brand)"), ["frontend"]);
        assert_eq!(roles("Senior / Staff Fullstack Engineer"), ["full stack"]);
        assert_eq!(
            roles("Product Manager - Data Platform"),
            ["product management"]
        );
        assert_eq!(roles("Account Executive, Enterprise"), ["sales"]);
        assert_eq!(
            roles("Forward Deployed Engineer - ML"),
            ["solutions", "machine learning"]
        );
        assert_eq!(
            title_level("Senior / Staff Fullstack Engineer").map(|l| l.0),
            Some(Level::Senior)
        );
        assert_eq!(title_level("Member of Technical Staff - Systems"), None);
        assert_eq!(title_level("Account Manager, Commercial"), None);
        assert_eq!(
            title_level("Engineering Manager, Platform").map(|l| l.0),
            Some(Level::Manager)
        );
        assert_eq!(
            title_level("Junior Developer").map(|l| l.0),
            Some(Level::Junior)
        );
    }

    #[test]
    fn functions_separate_engineering_from_other_work() {
        let f = |t: &str| facets(&record("ashby:acme", t, "")).function;
        assert_eq!(f("Senior Backend Engineer"), JobFunction::Engineering);
        assert_eq!(
            f("Member of Technical Staff - Systems"),
            JobFunction::Engineering
        );
        assert_eq!(f("Account Executive, Enterprise"), JobFunction::Sales);
        assert_eq!(
            f("Solutions Engineer, Europe"),
            JobFunction::CustomerEngineering
        );
        assert_eq!(
            f("Associate – Audiobook Licensing & Author Partnerships"),
            JobFunction::Sales
        );
        assert_eq!(f("Head of People"), JobFunction::Other);
    }

    #[test]
    fn technologies_are_weighed_by_where_they_appear() {
        let description = "About the role\n\
            You will build our payments API in Go and talk to Kafka.\n\
            What you'll need\n\
            - 5+ years with PostgreSQL\n\
            - Experience with Kubernetes\n\
            Nice to have\n\
            - Terraform\n\
            We use React on the dashboard, and experience with Rust is a plus.";
        let f = facets(&record(
            "ashby:acme",
            "Senior Backend Engineer (Go)",
            description,
        ));
        let req = |name: &str| {
            f.technologies
                .iter()
                .find(|t| t.name == name)
                .map(|t| (t.requirement, t.in_title))
        };
        assert_eq!(req("Go"), Some((Requirement::Required, true)));
        assert_eq!(req("PostgreSQL"), Some((Requirement::Required, false)));
        assert_eq!(req("Kubernetes"), Some((Requirement::Required, false)));
        assert_eq!(req("Terraform"), Some((Requirement::Preferred, false)));
        assert_eq!(req("Rust"), Some((Requirement::Preferred, false)));
        assert_eq!(req("Kafka"), Some((Requirement::Mentioned, false)));
        let keys: Vec<String> = f.keys().into_iter().map(|(k, _)| k.to_string()).collect();
        assert!(
            keys.contains(&"technology:PostgreSQL".to_owned()),
            "{keys:?}"
        );
        assert!(!keys.contains(&"technology:Kafka".to_owned()), "{keys:?}");
    }

    #[test]
    fn company_and_work_style_statements_keep_their_sentence() {
        let description = "We're a small team of 8 backed by Y Combinator, working directly \
            with the founders. You'll have real ownership from day one. \
            There is an on-call rotation.";
        let f = facets(&record("ashby:acme", "Software Engineer", description));
        let traits: Vec<&str> = f
            .company_traits
            .iter()
            .map(|t| t.key.value.as_str())
            .collect();
        assert!(traits.contains(&"small_team"), "{traits:?}");
        assert!(traits.contains(&"startup"), "{traits:?}");
        assert!(traits.contains(&"founder_led"), "{traits:?}");
        let style: Vec<&str> = f.work_style.iter().map(|t| t.key.value.as_str()).collect();
        assert_eq!(style, ["ownership", "on_call"]);
        assert!(f.work_style[0].evidence.contains("real ownership"));
    }

    #[test]
    fn benefits_are_not_facts_about_the_job() {
        let description = "About us\nWe build insurance software for small businesses.\n\
            Our insurance platform serves 10,000 brokers.\n\
            Benefits\n- 100% medical, dental & vision insurance coverage\n\
            - Healthcare stipend\n\
            What you'll do\nYou'll have ownership of the claims API.";
        let f = facets(&record("ashby:acme", "Backend Engineer", description));
        let domains: Vec<&str> = f.domains.iter().map(|d| d.key.value.as_str()).collect();
        assert_eq!(domains, ["insurance"], "not healthcare: {domains:?}");
        assert!(f.work_style.iter().any(|w| w.key.value == "ownership"));
        let sentences = content_sentences(description);
        assert!(
            sentences.iter().all(|s| !s.contains("dental")),
            "{sentences:?}"
        );
        assert!(
            sentences.iter().any(|s| s.contains("claims API")),
            "{sentences:?}"
        );
    }

    #[test]
    fn nothing_is_guessed() {
        let f = facets(&record("greenhouse:acme", "Software Engineer", "Join us."));
        assert!(f.company_traits.is_empty());
        assert!(f.domains.is_empty());
        assert!(f.work_style.is_empty());
        assert_eq!(f.contract, None);
        // A single stray mention does not make a domain.
        let f = facets(&record(
            "greenhouse:acme",
            "Software Engineer",
            "We use AI tools internally.",
        ));
        assert!(f.domains.is_empty(), "{:?}", f.domains);
        let f = facets(&record(
            "greenhouse:acme",
            "Senior Engineer, Payments",
            "Build payment rails.",
        ));
        assert_eq!(f.domains[0].key.value, "payments");
        assert_eq!(f.domains[0].source, FactSource::Title);
    }

    #[test]
    fn generic_titles_take_their_shape_from_the_description() {
        let f = facets(&record(
            "ashby:acme",
            "Software Engineer",
            "Requirements\nStrong experience with PostgreSQL and Kafka.\nYou will design APIs and microservices.",
        ));
        assert!(f.has_role("backend"), "{:?}", f.roles);
        assert_eq!(f.roles[0].source, FactSource::Description);
    }

    #[test]
    fn a_job_board_is_not_a_company_stage_and_managers_manage() {
        // Posting on Y Combinator's job board doesn't make a company a
        // startup today (DoorDash posts there).
        let f = facets(&record("yc:acme", "Engineering Manager", ""));
        assert!(f.company_traits.is_empty(), "{:?}", f.company_traits);
        assert!(f.manages_people());
    }

    fn stages(source: &str, description: &str) -> Vec<String> {
        facets(&record(source, "Software Engineer", description))
            .company_traits
            .iter()
            .map(|t| t.key.value.clone())
            .collect()
    }

    /// Production smoke test: the stage of a team, a project or a role is
    /// never the company's stage.
    #[test]
    fn only_the_company_itself_has_a_company_stage() {
        // DoorDash (on Y Combinator's job board): a team bootstrapping new
        // bets and hiring founding engineers.
        let doordash = stages(
            "yc:doordash",
            "The mission of the Database and Messaging team is to build and operate online \
             stateful systems. We are bootstrapping some long term bets in all of these areas \
             and looking for founding engineers.",
        );
        assert!(!doordash.contains(&"early_stage".into()), "{doordash:?}");
        assert!(!doordash.contains(&"startup".into()), "{doordash:?}");
        assert!(!doordash.contains(&"founder_led".into()), "{doordash:?}");
        // Supabase: early-stage work on a brand-new team, at a Series F
        // company with a small team.
        let supabase = stages(
            "ashby:supabase",
            "This is a role with a lot of agency. Small team, working product, patterns still \
             to be set. Are comfortable owning ambiguous, early-stage work and making sound \
             calls. This is a founding role on a brand-new team. Over $1B raised (including our \
             $500M Series F).",
        );
        assert!(!supabase.contains(&"early_stage".into()), "{supabase:?}");
        assert!(supabase.contains(&"scaleup".into()), "{supabase:?}");
        assert!(
            supabase.contains(&"small_team".into()),
            "the team is still small"
        );
        for not_the_company in [
            "You'll be the first engineer on our new payments product.",
            "Join the founding team of a new internal initiative.",
            "This is greenfield work: you'll build something from zero.",
            "We're building an early-stage product for small businesses.",
            "We sell to early-stage companies.",
        ] {
            let t = stages("ashby:acme", not_the_company);
            assert!(
                !t.contains(&"early_stage".into()),
                "{not_the_company}: {t:?}"
            );
        }
        for the_company in [
            "We're an early-stage startup backed by great investors.",
            "We are early stage and move fast.",
            "We just closed our seed round.",
            "We raised a $12M Series A last year.",
        ] {
            assert!(
                stages("ashby:acme", the_company).contains(&"early_stage".into()),
                "{the_company}"
            );
        }
        // A later round in the same posting outranks an earlier one.
        let grown = stages(
            "ashby:acme",
            "We raised our Series A in 2016. We have since raised a $300M Series D.",
        );
        assert!(!grown.contains(&"early_stage".into()), "{grown:?}");
        // Unknown stays unknown.
        assert!(stages("ashby:acme", "You will build backend services in Rust.").is_empty());
    }
}
