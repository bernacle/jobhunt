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

use chrono::{DateTime, Utc};
use jobhunt_jobs::{EmploymentType, JobRecord, WorkplaceType};
use jobhunt_profile::WorkMode;
use jobhunt_profile::infer::{domains_in, role_signals, technologies_in};
use jobhunt_profile::words::{Pattern, Word, span_text, words};
use serde::{Deserialize, Serialize};

use crate::key::{Dimension, TasteKey};

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
const LEVEL_TERMS: [(&str, Level); 22] = [
    ("co founder", Level::Founding),
    ("cofounder", Level::Founding),
    ("founding", Level::Founding),
    ("intern*", Level::Intern),
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
    ("product designer*", "design"),
    ("designer*", "design"),
    ("=UX", "design"),
];

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
const COMPANY_TERMS: &[(&str, &str)] = &[
    ("early stage", "early_stage"),
    ("pre seed", "early_stage"),
    ("seed stage", "early_stage"),
    ("seed round", "early_stage"),
    ("series a", "early_stage"),
    ("first engineer*", "early_stage"),
    ("founding engineer*", "early_stage"),
    ("series b", "scaleup"),
    ("series c", "scaleup"),
    ("series d", "scaleup"),
    ("hypergrowth", "scaleup"),
    ("startup", "startup"),
    ("start up", "startup"),
    ("y combinator", "startup"),
    ("founder*", "founder_led"),
    ("founding team", "founder_led"),
    ("small team", "small_team"),
    ("tiny team", "small_team"),
    ("lean team", "small_team"),
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

/// The sentences of a description that describe the job and the company,
/// without benefits and policy boilerplate.
pub fn content_sentences(description: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in description.lines() {
        let trimmed = line.trim().trim_end_matches(':');
        let ws = words(trimmed);
        let short = !trimmed.is_empty() && ws.len() <= 6 && !trimmed.ends_with('.');
        if short && !trimmed.starts_with(['-', '*', '•', '–']) {
            if BOILERPLATE_HEADINGS
                .iter()
                .any(|p| Pattern::new(p).find(&ws).is_some_and(|r| r.start <= 2))
            {
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
            if !has(&words(&sentence), &BOILERPLATE_CUES) {
                out.push(sentence);
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

fn find_terms<'a>(
    text: &str,
    ws: &[Word],
    terms: &'a [(&'a str, &'a str)],
) -> Vec<(&'a str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for (pattern, value) in terms {
        if out.iter().any(|(v, _)| v == value) {
            continue;
        }
        if let Some(range) = Pattern::new(pattern).find(ws) {
            out.push((value, span_text(text, ws, &range).to_owned()));
        }
    }
    out
}

fn has(ws: &[Word], patterns: &[&str]) -> bool {
    patterns.iter().any(|p| Pattern::new(p).find(ws).is_some())
}

/// The level a title states.
pub fn title_level(title: &str) -> Option<(Level, String)> {
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
    let mut found = find_terms(title, &ws, ROLE_TITLE_TERMS);
    // "Design Engineer" is frontend engineering, not design.
    if found.iter().any(|(v, _)| *v == "frontend") {
        found.retain(|(v, _)| *v != "design");
    }
    // "Product Manager - Data Platform": the data platform is the product.
    if found
        .iter()
        .any(|(v, _)| matches!(*v, "sales" | "product management"))
        && !has(&ws, &ENGINEERING_TITLE)
    {
        found.retain(|(v, _)| matches!(*v, "sales" | "product management" | "design"));
    }
    found
}

fn function_of(title: &str, roles: &[(&str, String)]) -> JobFunction {
    let ws = words(title);
    let is = |v: &str| roles.iter().any(|(r, _)| *r == v);
    if is("solutions") {
        JobFunction::CustomerEngineering
    } else if has(&ws, &ENGINEERING_TITLE) || is("sre") {
        JobFunction::Engineering
    } else if is("sales") {
        JobFunction::Sales
    } else if is("product management") {
        JobFunction::ProductManagement
    } else if is("design") {
        JobFunction::Design
    } else if !roles.is_empty() {
        // "Backend (Rust)" without the word "engineer".
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
        || !technologies_in(trimmed, false).is_empty()
    {
        return None;
    }
    let ws = words(trimmed);
    if ws.len() > 6 {
        return None;
    }
    let starts = |patterns: &[&str]| {
        patterns
            .iter()
            .any(|p| Pattern::new(p).find(&ws).is_some_and(|r| r.start <= 2))
    };
    if starts(&PREFERRED_HEADINGS) {
        Some(Requirement::Preferred)
    } else if starts(&REQUIRED_HEADINGS) {
        Some(Requirement::Required)
    } else if starts(&OTHER_HEADINGS) {
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
            let mentions = technologies_in(&sentence, false);
            if mentions.is_empty() {
                continue;
            }
            let ws = words(&sentence);
            let level = if has(&ws, &PREFERRED_CUES) {
                Requirement::Preferred
            } else if section == Requirement::Mentioned && has(&ws, &REQUIRED_CUES) {
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

fn domains(title: &str, structured: &[&str], description: &str) -> Vec<Fact> {
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
    for sentence in content_sentences(description) {
        for hit in domains_in(&sentence) {
            match tally.iter_mut().find(|(d, ..)| *d == hit.domain) {
                Some(entry) => {
                    if hit.strong {
                        entry.1 += 1;
                        if entry.1 == 1 {
                            entry.3.clone_from(&sentence);
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

fn statements(description: &str, terms: &[(&str, &str)], dimension: Dimension) -> Vec<Fact> {
    let mut out: Vec<Fact> = Vec::new();
    for sentence in content_sentences(description) {
        let ws = words(&sentence);
        for (value, _) in find_terms(&sentence, &ws, terms) {
            if !out.iter().any(|f| f.key.value == value) {
                out.push(Fact {
                    key: TasteKey::new(dimension, value),
                    evidence: sentence.clone(),
                    source: FactSource::Description,
                });
            }
        }
    }
    out
}

/// "a team of 6 engineers": a small team when the number is small.
fn team_size(description: &str) -> Option<Fact> {
    for sentence in content_sentences(description) {
        let ws = words(&sentence);
        for range in Pattern::new("team of _").find_all(&ws) {
            let n: Option<u32> = ws.get(range.end - 1).and_then(|w| w.lower.parse().ok());
            if let Some(n) = n
                && (2..=25).contains(&n)
            {
                return Some(Fact {
                    key: TasteKey::new(Dimension::CompanyTrait, "small_team"),
                    evidence: sentence.clone(),
                    source: FactSource::Description,
                });
            }
        }
    }
    None
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
    // A generic engineering title: the description says what kind.
    if function == JobFunction::Engineering && roles.is_empty() {
        let techs: Vec<&str> = technologies
            .iter()
            .filter(|t| t.requirement >= Requirement::Preferred)
            .map(|t| t.name.as_str())
            .collect();
        let sentences = content_sentences(description);
        let texts: Vec<&str> = sentences.iter().map(String::as_str).collect();
        for signal in role_signals(None, &techs, &texts) {
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
    let mut company_traits = statements(description, COMPANY_TERMS, Dimension::CompanyTrait);
    if let Some(fact) = team_size(description)
        && !company_traits.iter().any(|f| f.key == fact.key)
    {
        company_traits.push(fact);
    }
    if record.posting.provenance.source.kind() == "yc"
        && !company_traits.iter().any(|f| f.key.value == "startup")
    {
        company_traits.push(Fact {
            key: TasteKey::new(Dimension::CompanyTrait, "startup"),
            evidence: "listed on Y Combinator's Work at a Startup".into(),
            source: FactSource::Structured,
        });
    }
    let mut work_style = statements(description, WORK_STYLE_TERMS, Dimension::WorkStyle);
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
    JobFacets {
        company: job.company.trim().to_owned(),
        function,
        roles,
        level,
        domains: domains(&title, &structured, description),
        technologies,
        company_traits,
        work_style,
        work_modes,
        contract,
        listed_at: job.posted_at.unwrap_or(record.first_seen_at),
        posted: job.posted_at.is_some(),
        title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::record;

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
    fn yc_companies_are_startups_and_managers_manage() {
        let f = facets(&record("yc:acme", "Engineering Manager", ""));
        assert!(f.company_traits.iter().any(|t| t.key.value == "startup"));
        assert!(f.manages_people());
    }
}
