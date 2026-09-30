//! What the work itself is, read for fit: its level on the taste model's
//! scale, the shape of the engineering work, how specialized it is, and
//! the shape of the company, team and culture around it.
//!
//! [`JobFacets`] reads keywords; this reads the role the
//! way a candidate would, and keeps the words behind every conclusion:
//!
//! * **Level**: the title's level ("Senior", "Staff+", "Early Career",
//!   "New Grad"), else the description's ("0-2 years", "our new-grad
//!   program", "6+ years of experience").
//! * **Shape**: the title's own role words first ("Backend Engineer,
//!   Payments Infrastructure" is backend work on a team called Payments
//!   Infrastructure; "Senior Software Engineer, Platform" is platform
//!   work), and the description only when the title says nothing.
//! * **Depth**: specialized work that shares vocabulary with ordinary
//!   engineering but is a different job: building a database's storage
//!   engine rather than using PostgreSQL, writing Kubernetes' control
//!   plane rather than running services on it, training models rather than
//!   calling LLM APIs, nanosecond trading systems, privacy engineering.
//!   A specialty needs its own evidence (a cue in the title, or two in the
//!   description), and a denied cue ("no machine learning background
//!   needed") is none.
//! * **Company, team, culture**: stage and size as the posting states them
//!   ("a 15-person startup", "a Series C company of about 400 people",
//!   "a Fortune 500 insurer"), the team someone joins, and working
//!   conditions a candidate weighs ("a dedicated mentor", "changes go
//!   through our change advisory board").
//!
//! Nothing is guessed: what the posting doesn't say stays unknown.

use serde::{Deserialize, Serialize};

use jobhunt_profile::words::{Vocabulary, Word, span_text, words};

use super::{JobFacets, JobFunction, Level, Sentence};

/// Seniority on the taste model's scale (`early_career`, `mid`, `senior`,
/// `staff_plus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seniority {
    EarlyCareer,
    Mid,
    Senior,
    StaffPlus,
}

impl Seniority {
    pub const ALL: [Seniority; 4] = [Self::EarlyCareer, Self::Mid, Self::Senior, Self::StaffPlus];

    /// The taste profile's token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EarlyCareer => "early_career",
            Self::Mid => "mid",
            Self::Senior => "senior",
            Self::StaffPlus => "staff_plus",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == token)
    }

    /// "early-career", "senior".
    pub fn label(self) -> &'static str {
        match self {
            Self::EarlyCareer => "early-career",
            Self::Mid => "mid-level",
            Self::Senior => "senior",
            Self::StaffPlus => "staff-plus",
        }
    }

    /// Steps on the ladder.
    pub fn rank(self) -> i8 {
        match self {
            Self::EarlyCareer => 0,
            Self::Mid => 1,
            Self::Senior => 2,
            Self::StaffPlus => 3,
        }
    }

    /// A title's level on this scale (management and founding roles sit
    /// beside the ladder).
    pub fn of(level: Level) -> Option<Self> {
        match level {
            Level::Intern | Level::Junior => Some(Self::EarlyCareer),
            Level::Mid => Some(Self::Mid),
            Level::Senior | Level::Lead => Some(Self::Senior),
            Level::Staff | Level::Principal => Some(Self::StaffPlus),
            Level::Manager | Level::Founding => None,
        }
    }
}

/// Where a level was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Title,
    Description,
}

/// The role's level, with its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelFact {
    pub seniority: Seniority,
    pub evidence: String,
    pub basis: Basis,
}

/// How firmly the posting says what the work is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeBasis {
    /// Read from the description of a generic title.
    Description,
    /// Part of what the title's product or full-stack work includes
    /// ("from the database to the UI").
    Included,
    /// The team or area named after the title's own role ("Backend
    /// Engineer, Payments **Infrastructure**").
    TitleArea,
    /// The specialized work the posting describes.
    Specialty,
    /// The title's own role words ("**Backend** Engineer", or "Senior
    /// Software Engineer, **Platform**").
    Title,
}

/// One shape of engineering work, on the taste model's vocabulary
/// (`backend`, `platform`, `database_internals`, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeFact {
    pub shape: String,
    pub basis: ShapeBasis,
    pub evidence: String,
}

/// Specialized work that shares vocabulary with ordinary engineering but
/// is a different job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Specialty {
    /// Storage engines, query engines, database kernels.
    DatabaseInternals,
    /// Kubernetes' own control plane: controllers, API machinery, etcd.
    OrchestrationInternals,
    /// Training models: pretraining, distributed training, CUDA kernels.
    ModelTraining,
    /// Nanosecond trading systems, kernel bypass, FPGA.
    LowLatency,
    /// Privacy engineering: anonymization, differential privacy, regulation.
    Privacy,
    /// Security research: vulnerabilities, exploits, reverse engineering.
    SecurityResearch,
}

impl Specialty {
    pub const ALL: [Specialty; 6] = [
        Self::DatabaseInternals,
        Self::OrchestrationInternals,
        Self::ModelTraining,
        Self::LowLatency,
        Self::Privacy,
        Self::SecurityResearch,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DatabaseInternals => "database_internals",
            Self::OrchestrationInternals => "orchestration_internals",
            Self::ModelTraining => "model_training",
            Self::LowLatency => "low_latency",
            Self::Privacy => "privacy",
            Self::SecurityResearch => "security_research",
        }
    }

    /// How it reads: "database-engine internals".
    pub fn label(self) -> &'static str {
        match self {
            Self::DatabaseInternals => "database-engine internals",
            Self::OrchestrationInternals => "Kubernetes control-plane internals",
            Self::ModelTraining => "ML model training",
            Self::LowLatency => "low-latency trading systems",
            Self::Privacy => "privacy engineering",
            Self::SecurityResearch => "security research",
        }
    }

    /// The ordinary work it is easily confused with.
    pub fn not_to_confuse_with(self) -> &'static str {
        match self {
            Self::DatabaseInternals => "using a database in applications",
            Self::OrchestrationInternals => "running services on Kubernetes",
            Self::ModelTraining => "building products on LLM APIs",
            Self::LowLatency => "ordinary backend performance work",
            Self::Privacy => "general application security",
            Self::SecurityResearch => "general application security",
        }
    }

    /// The taste model's work shape for it.
    pub fn shape(self) -> &'static str {
        match self {
            Self::DatabaseInternals => "database_internals",
            Self::OrchestrationInternals => "infrastructure",
            Self::ModelTraining => "ml_research",
            Self::LowLatency => "low_latency",
            Self::Privacy | Self::SecurityResearch => "security",
        }
    }

    /// A technology the taste profile may name for it ("Kubernetes
    /// control-plane work").
    pub fn technology(self) -> Option<&'static str> {
        match self {
            Self::OrchestrationInternals => Some("kubernetes"),
            Self::DatabaseInternals => Some("postgresql"),
            _ => None,
        }
    }

    /// Words that mean it. A title cue settles it; in a description it
    /// takes two different cues.
    fn cues(self) -> &'static [&'static str] {
        match self {
            Self::DatabaseInternals => &[
                "storage engine*",
                "database engine",
                "database engines",
                "database internals",
                "internals of postgresql",
                "internals of postgres",
                "postgresql internals",
                "postgres internals",
                "internals of the database",
                "database kernel",
                "query engine*",
                "query executor",
                "query optimizer",
                "write ahead log*",
                "=WAL",
                "=MVCC",
                "buffer manager",
                "buffer pool",
                "b tree*",
                "crash recovery",
                "compaction",
                "page layout",
                "table access method",
                "concurrency control",
                "undo log*",
                "=OrioleDB",
            ],
            Self::OrchestrationInternals => &[
                "control plane",
                "kubernetes internals",
                "api machinery",
                "custom controllers",
                "controllers and operators",
                "operators and controllers",
                "kubernetes operators",
                "api server",
                "=etcd",
                "scheduler behavior",
                "kubernetes sig*",
                "upstream to kubernetes",
                "contributions to kubernetes",
                "contribute upstream",
            ],
            Self::ModelTraining => &[
                "pretraining",
                "pre training",
                "train frontier models",
                "training frontier models",
                "frontier models",
                "model training",
                "distributed training",
                "cuda kernel*",
                "=FSDP",
                "tensor parallelism",
                "pipeline parallelism",
                "ablations",
                "machine learning research",
                "research lab",
                "training large language models",
            ],
            Self::LowLatency => &[
                "low latency",
                "nanosecond*",
                "kernel bypass",
                "=FPGA",
                "market microstructure",
                "exchange protocols",
                "order execution",
                "high frequency trading",
                "trading systems",
                "market data",
            ],
            Self::Privacy => &[
                "privacy engineering",
                "privacy engineer*",
                "differential privacy",
                "anonymization",
                "anonymisation",
                "data minimization",
                "privacy preserving",
                "technical privacy",
                "=GDPR",
                "=CCPA",
            ],
            Self::SecurityResearch => &[
                "security research",
                "vulnerability research",
                "exploit development",
                "reverse engineering",
                "fuzzing",
                "malware analysis",
            ],
        }
    }
}

/// A specialty the posting describes, with its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialtyFact {
    pub specialty: Specialty,
    /// The cues found, as written ("storage engine", "MVCC").
    pub cues: Vec<String>,
    /// The first sentence (or the title) they were found in.
    pub evidence: String,
}

/// Something about the company, team or culture, on the taste model's
/// vocabulary, with its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trait {
    pub value: String,
    pub evidence: String,
}

/// The work, read for fit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkReading {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LevelFact>,
    /// A founding role ("Founding Engineer", "first engineering hire").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub founding: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<ShapeFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specialties: Vec<SpecialtyFact>,
    /// The company: `startup`, `early_stage`, `growth`, `small_company`,
    /// `large_company`, `public_company`, `founder_led`, `consulting`, ….
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub company: Vec<Trait>,
    /// How many people the company has, as the posting says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headcount: Option<(u32, String)>,
    /// The team someone joins: `small_team` or `large_team`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<Trait>,
    /// `mentorship`, `process_heavy`, `remote_first`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub culture: Vec<Trait>,
    /// The posting's words about owning the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership: Option<String>,
}

impl WorkReading {
    pub fn has_shape(&self, shape: &str) -> bool {
        self.shapes.iter().any(|s| s.shape == shape)
    }

    pub fn company_is(&self, value: &str) -> Option<&Trait> {
        self.company.iter().find(|t| t.value == value)
    }

    pub fn culture_is(&self, value: &str) -> Option<&Trait> {
        self.culture.iter().find(|t| t.value == value)
    }

    pub fn specialty(&self, specialty: Specialty) -> Option<&SpecialtyFact> {
        self.specialties.iter().find(|s| s.specialty == specialty)
    }
}

/// Role words of a title on the taste model's work shapes. `None`: not a
/// shape of engineering work (sales, design, …).
fn shape_of_role(role: &str) -> Option<&'static str> {
    Some(match role {
        "backend" => "backend",
        "frontend" => "frontend",
        "full stack" => "full_stack",
        "platform" => "platform",
        "infrastructure" => "infrastructure",
        "sre" => "sre",
        "data" => "data",
        "mobile" => "mobile",
        "machine learning" => "ml_product",
        "security" => "security",
        "embedded" => "embedded",
        "solutions" => "solutions",
        "product" => "product",
        _ => return None,
    })
}

/// Title words for shapes the title vocabulary of [`JobFacets`] doesn't
/// carry, because they only matter for fit.
const EXTRA_TITLE_SHAPES: &[(&str, &str)] = &[
    ("product engineer*", "product"),
    ("privacy", "security"),
    ("developer tool*", "developer_tooling"),
    ("devtools", "developer_tooling"),
    ("developer experience", "developer_tooling"),
    ("distributed systems", "distributed_systems"),
];

/// The title's own role words, and the team or area named after them:
/// "Backend Engineer, Payments Infrastructure" → ("Backend Engineer",
/// "Payments Infrastructure"). Parentheses and dashes between spaces
/// separate too ("Senior Backend Engineer (Payments)", "Engineer - Data").
pub fn split_title(title: &str) -> (&str, &str) {
    let cut = [", ", " (", " - ", " – ", " — ", " | ", ": "]
        .iter()
        .filter_map(|sep| title.find(sep))
        .min();
    match cut {
        Some(at) => {
            let rest = title[at..]
                .trim_start_matches([',', ' ', '(', '-', '–', '—', '|', ':'])
                .trim_end_matches(')');
            (title[..at].trim(), rest.trim())
        }
        None => (title.trim(), ""),
    }
}

fn title_shapes(part: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let ws = words(part);
    for (pattern, shape) in EXTRA_TITLE_SHAPES {
        if let Some(r) = jobhunt_profile::words::Pattern::new(pattern).find(&ws)
            && !out.iter().any(|(s, _)| s == shape)
        {
            out.push((shape, span_text(part, &ws, &r).to_owned()));
        }
    }
    for (role, evidence) in super::title_roles(part) {
        // "Product Engineer, AI": AI is the product's feature here, and the
        // title's product engineering already says what the work is.
        if let Some(shape) = shape_of_role(role)
            && !out.iter().any(|(s, _)| *s == shape)
        {
            out.push((shape, evidence));
        }
    }
    // "Research Engineer" is machine-learning research, not product work.
    if jobhunt_profile::words::Pattern::new("research engineer*")
        .find(&ws)
        .is_some()
    {
        out.retain(|(s, _)| *s != "ml_product");
        if !out.iter().any(|(s, _)| *s == "ml_research") {
            out.push(("ml_research", part.to_owned()));
        }
    }
    out
}

/// A sentence about the company rather than the role ("Supabase is the
/// Postgres development platform", "We provide a complete backend
/// solution"): what the company does, which says nothing about the work
/// of a role with a generic title.
pub(super) fn about_company(sentence: &Sentence, company: &str) -> bool {
    let key = jobhunt_core::text::search_key(company);
    let first = key.split(' ').next().unwrap_or_default();
    let ws = &sentence.words;
    let names =
        !first.is_empty() && first.len() >= 3 && ws.iter().take(4).any(|w| w.lower == first);
    let we = matches!(
        (
            ws.first().map(|w| w.lower.as_str()),
            ws.get(1).map(|w| w.lower.as_str())
        ),
        (
            Some("we"),
            Some(
                "provide"
                    | "are"
                    | "re"
                    | "build"
                    | "make"
                    | "started"
                    | "help"
                    | "power"
                    | "believe"
                    | "raised"
            )
        ) | (
            Some("our"),
            Some("mission" | "customers" | "platform" | "product" | "company" | "vision")
        )
    );
    let you = ws
        .iter()
        .any(|w| matches!(w.lower.as_str(), "you" | "your" | "you'll"));
    (names || we) && !you
}

/// Words of a clause that deny what follows them.
fn denied(sentence: &Sentence, start: usize) -> bool {
    sentence.denied(start)
}

/// A vocabulary compiled once per process (every posting is read against
/// the same few; compiling them per posting dominated ranking time).
fn compile(patterns: &'static [&'static str]) -> &'static Vocabulary {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex, PoisonError};
    type Compiled = HashMap<(usize, usize), &'static Vocabulary>;
    static COMPILED: LazyLock<Mutex<Compiled>> = LazyLock::new(Mutex::default);
    let key = (patterns.as_ptr() as usize, patterns.len());
    let mut compiled = COMPILED.lock().unwrap_or_else(PoisonError::into_inner);
    compiled
        .entry(key)
        .or_insert_with(|| Box::leak(Box::new(Vocabulary::compile(patterns))))
}

/// The specialties in a title and sentences (a job's content, or a
/// person's career evidence).
pub fn specialties_in(title: &str, sentences: &[(String, Vec<Word>)]) -> Vec<SpecialtyFact> {
    let mut out = Vec::new();
    let title_words = words(title);
    for specialty in Specialty::ALL {
        let vocabulary = compile(specialty.cues());
        let mut cues: Vec<String> = Vec::new();
        let mut evidence: Option<String> = None;
        let mut in_title = false;
        for (_, range) in vocabulary.find_first(&title_words) {
            in_title = true;
            let cue = span_text(title, &title_words, &range).to_owned();
            if !cues.iter().any(|c| c.eq_ignore_ascii_case(&cue)) {
                cues.push(cue);
            }
            evidence.get_or_insert_with(|| title.to_owned());
        }
        for (text, ws) in sentences {
            let clauses = super::clauses(text, ws);
            let sentence = Sentence {
                text: text.clone(),
                words: ws.clone(),
                clauses,
            };
            for (_, ranges) in vocabulary.find_all(ws) {
                for range in ranges {
                    if denied(&sentence, range.start) {
                        continue;
                    }
                    let cue = span_text(text, ws, &range).to_owned();
                    if !cues.iter().any(|c| c.eq_ignore_ascii_case(&cue)) {
                        cues.push(cue);
                    }
                    evidence.get_or_insert_with(|| text.clone());
                }
            }
        }
        if in_title || cues.len() >= 2 {
            out.push(SpecialtyFact {
                specialty,
                cues,
                evidence: evidence.unwrap_or_default(),
            });
        }
    }
    out
}

/// Title level terms the title vocabulary of [`JobFacets`] doesn't read
/// as levels (it predates them).
const EARLY_CAREER_TITLE: [&str; 6] = [
    "early career",
    "new grad*",
    "new graduate*",
    "university grad*",
    "recent grad*",
    "apprentice*",
];

/// Description wording that places a role early in a career.
const EARLY_CAREER_CUES: [&str; 14] = [
    "early career",
    "new grad*",
    "new graduate*",
    "recent graduate*",
    "university graduate*",
    "start of their careers",
    "start of your career",
    "entry level",
    "0 2 years",
    "0 1 years",
    "0 to 2 years",
    "0 3 years",
    "graduate program*",
    "junior",
];

/// The level a description states: early-career wording, or years of
/// experience asked for ("6+ years of experience").
fn description_level(sentences: &[Sentence]) -> Option<LevelFact> {
    let early = compile(&EARLY_CAREER_CUES);
    for s in sentences {
        if let Some((_, r)) = early.find_first(&s.words).into_iter().next()
            && !denied(s, r.start)
        {
            return Some(LevelFact {
                seniority: Seniority::EarlyCareer,
                evidence: s.text.clone(),
                basis: Basis::Description,
            });
        }
    }
    // "6+ years of experience", "10+ years of software engineering".
    let mut most: Option<(u32, &Sentence)> = None;
    for s in sentences {
        for (i, w) in s.words.iter().enumerate() {
            let Ok(n) = w.lower.trim_end_matches('+').parse::<u32>() else {
                continue;
            };
            let next = s.words.get(i + 1).map(|w| w.lower.as_str());
            let plus = w.lower.ends_with('+') || next == Some("plus");
            let years = s.words[i + 1..]
                .iter()
                .take(2)
                .any(|w| w.lower == "years" || w.lower == "yrs");
            if years
                && (plus
                    || s.words[i + 1..]
                        .iter()
                        .take(4)
                        .any(|w| w.lower == "experience"))
                && (1..=25).contains(&n)
                && most.is_none_or(|(m, _)| n > m)
            {
                most = Some((n, s));
            }
        }
    }
    let (n, s) = most?;
    let seniority = match n {
        0..=2 => Seniority::EarlyCareer,
        3..=4 => Seniority::Mid,
        _ => Seniority::Senior,
    };
    Some(LevelFact {
        seniority,
        evidence: s.text.clone(),
        basis: Basis::Description,
    })
}

/// Company-size wording: "a 15-person startup", "a company of about 400
/// people", "we are a team of 9", "Supabase has 300 employees".
fn headcount(sentences: &[Sentence]) -> Option<(u32, String)> {
    const COMPANY: [&str; 6] = [
        "startup",
        "company",
        "companies",
        "firm",
        "business",
        "organization",
    ];
    const PEOPLE: [&str; 4] = ["people", "employees", "person", "staff"];
    for s in sentences {
        let ws = &s.words;
        let has = |set: &[&str]| ws.iter().any(|w| set.contains(&w.lower.as_str()));
        let we = ws.windows(2).any(|p| {
            matches!(
                (p[0].lower.as_str(), p[1].lower.as_str()),
                ("we", "are") | ("we", "re") | ("are", "a")
            )
        }) || ws.first().is_some_and(|w| w.lower == "we");
        for (i, w) in ws.iter().enumerate() {
            let Ok(n) = w.lower.trim_end_matches('+').parse::<u32>() else {
                continue;
            };
            if !(2..=500_000).contains(&n) {
                continue;
            }
            let next = ws.get(i + 1).map(|w| w.lower.as_str());
            // "15-person startup", "a 5-person Y Combinator startup".
            let sized = next == Some("person")
                && ws[i + 2..]
                    .iter()
                    .take(3)
                    .any(|a| COMPANY.contains(&a.lower.as_str()) || a.lower == "team");
            // "400 people", "300 employees", with the company as subject.
            let counted =
                next.is_some_and(|x| PEOPLE.contains(&x) && x != "person") && (has(&COMPANY) || we);
            // "we are a team of 9", "are a team of 9 building …".
            let team_of = i >= 2
                && ws[i - 1].lower == "of"
                && ws[i - 2].lower == "team"
                && we
                && !ws
                    .iter()
                    .any(|w| matches!(w.lower.as_str(), "join" | "joining" | "you"));
            // A team size isn't the company's ("you'll join a team of 6").
            let about_team = ws[..i].iter().rev().take(4).any(|w| w.lower == "team") && !team_of;
            if (sized || counted || team_of) && !about_team && !denied(s, i) {
                return Some((n, s.text.clone()));
            }
        }
    }
    None
}

/// The sentence that best says a description's work is of a shape
/// ("You'll own backend services end to end"), for the reason's quote.
fn shape_sentence(shape: &str, sentences: &[Sentence]) -> Option<String> {
    let cues: &'static [&'static str] = match shape {
        "backend" => &[
            "backend",
            "back end",
            "server side",
            "services",
            "=API",
            "apis",
        ],
        "frontend" => &["frontend", "front end", "=UI", "user interface*"],
        "full_stack" => &["full stack", "fullstack", "the database to the ui"],
        "platform" => &["platform"],
        "infrastructure" => &["infrastructure", "infra"],
        "sre" => &["reliability", "on call", "incident*"],
        "data" => &["data pipeline*", "data engineering", "=ETL", "warehouse"],
        "mobile" => &["mobile", "=iOS", "android"],
        "security" => &["security"],
        _ => &[],
    };
    if cues.is_empty() {
        return None;
    }
    let v = compile(cues);
    // A sentence about the role ("You'll own backend services") before one
    // about the company or its customers ("deploy previews for backend
    // teams").
    let about_you = |s: &&Sentence| {
        s.words
            .iter()
            .any(|w| matches!(w.lower.as_str(), "you" | "your" | "role"))
    };
    // No sentence about the role says it: no quote (a sentence about the
    // product would mislead).
    sentences
        .iter()
        .filter(about_you)
        .find(|s| v.any(&s.words))
        .map(|s| s.text.clone())
}

const PROCESS_HEAVY: [&str; 9] = [
    "change advisory board",
    "=ITIL",
    "change management process*",
    "change management",
    "quarterly releases",
    "architecture decisions are made by",
    "enterprise architecture",
    "approval committee*",
    "architecture review board",
];

const MENTORSHIP: [&str; 9] = [
    "dedicated mentor",
    "a mentor",
    "with a mentor",
    "mentorship program*",
    "structured onboarding",
    "learn from senior",
    "alongside senior engineers",
    "onboarding curriculum",
    "ramp up with",
];

const FOUNDING: [&str; 5] = [
    "first engineering hire",
    "first engineer",
    "founding engineer*",
    "first backend hire",
    "founding team",
];

/// Reads the work of a job from its facets and content.
pub(super) fn read(facets: &JobFacets, sentences: &[Sentence]) -> WorkReading {
    let title = facets.title.as_str();
    let mut out = WorkReading::default();

    // Level: the title's, else the description's.
    let title_words = words(title);
    let early_title = compile(&EARLY_CAREER_TITLE);
    out.level = if let Some((_, r)) = early_title.find_first(&title_words).into_iter().next() {
        Some(LevelFact {
            seniority: Seniority::EarlyCareer,
            evidence: span_text(title, &title_words, &r).to_owned(),
            basis: Basis::Title,
        })
    } else {
        facets
            .level
            .as_ref()
            .and_then(|(level, words)| {
                Seniority::of(*level).map(|seniority| LevelFact {
                    seniority,
                    evidence: words.clone(),
                    basis: Basis::Title,
                })
            })
            .or_else(|| description_level(sentences))
    };
    if let Some((Level::Founding, words)) = &facets.level {
        out.founding = Some(words.clone());
    } else {
        let founding = compile(&FOUNDING);
        out.founding = sentences.iter().find_map(|s| {
            founding
                .find_first(&s.words)
                .into_iter()
                .next()
                .filter(|(_, r)| !denied(s, r.start))
                .map(|_| s.text.clone())
        });
    }

    // Depth.
    let texts: Vec<(String, Vec<Word>)> = sentences
        .iter()
        .map(|s| (s.text.clone(), s.words.clone()))
        .collect();
    out.specialties = specialties_in(title, &texts);

    // Shape: the title's own role words, then its area, then the
    // description (only for a title that names no role).
    let (primary, area) = split_title(title);
    let own = title_shapes(primary);
    let named = title_shapes(area);
    for (shape, evidence) in &own {
        out.shapes.push(ShapeFact {
            shape: (*shape).to_owned(),
            basis: ShapeBasis::Title,
            evidence: evidence.clone(),
        });
    }
    for (shape, evidence) in &named {
        if out.has_shape(shape) {
            continue;
        }
        out.shapes.push(ShapeFact {
            shape: (*shape).to_owned(),
            basis: if own.is_empty() {
                ShapeBasis::Title
            } else {
                ShapeBasis::TitleArea
            },
            evidence: evidence.clone(),
        });
    }
    for s in &out.specialties.clone() {
        let shape = s.specialty.shape();
        if !out.has_shape(shape) {
            out.shapes.push(ShapeFact {
                shape: shape.to_owned(),
                basis: ShapeBasis::Specialty,
                evidence: s.evidence.clone(),
            });
        }
    }
    // A specialized role is its specialty: the description's generic
    // "backend" (it mentions APIs and C) says nothing more.
    if out.shapes.is_empty() && facets.function == JobFunction::Engineering {
        for fact in facets
            .roles
            .iter()
            .filter(|f| f.source == super::FactSource::Description)
        {
            if let Some(shape) = shape_of_role(&fact.key.value)
                && !out.has_shape(shape)
            {
                out.shapes.push(ShapeFact {
                    shape: shape.to_owned(),
                    basis: ShapeBasis::Description,
                    // The sentence that says so; none when the shape rests
                    // on technologies and scattered words only.
                    evidence: shape_sentence(shape, sentences).unwrap_or_default(),
                });
            }
        }
    }
    // "Product Engineer" work that owns the backend is backend work too.
    if (out.has_shape("product") || out.has_shape("full_stack")) && !out.has_shape("backend") {
        // The most telling sentence first.
        const TELLING: [&[&str]; 2] = [
            &[
                "from the database",
                "the database to the ui",
                "backend",
                "back end",
            ],
            &["services", "=API", "apis"],
        ];
        let found = TELLING.iter().find_map(|cues| {
            let v = compile(cues);
            sentences.iter().find(|s| v.any(&s.words))
        });
        if let Some(s) = found {
            out.shapes.push(ShapeFact {
                shape: "backend".to_owned(),
                basis: ShapeBasis::Included,
                evidence: s.text.clone(),
            });
        }
    }

    // Company: stage and size.
    for fact in &facets.company_traits {
        let value = match fact.key.value.as_str() {
            "scaleup" => "growth",
            "small_team" | "large_team" => {
                out.team = Some(Trait {
                    value: fact.key.value.clone(),
                    evidence: fact.evidence.clone(),
                });
                continue;
            }
            "remote_first" => {
                out.culture.push(Trait {
                    value: "remote_first".into(),
                    evidence: fact.evidence.clone(),
                });
                continue;
            }
            other => other,
        };
        if out.company_is(value).is_none() {
            out.company.push(Trait {
                value: value.to_owned(),
                evidence: fact.evidence.clone(),
            });
        }
    }
    out.headcount = headcount(sentences);
    if let Some((n, evidence)) = &out.headcount {
        let size = match n {
            0..=100 => Some("small_company"),
            1000.. => Some("large_company"),
            _ => None,
        };
        if let Some(size) = size
            && out.company_is(size).is_none()
        {
            out.company.push(Trait {
                value: size.to_owned(),
                evidence: evidence.clone(),
            });
        }
        // "A 15-person startup": small enough to be one, whatever else.
        if *n <= 100
            && out.company_is("startup").is_none()
            && words(evidence).iter().any(|w| w.lower == "startup")
        {
            out.company.push(Trait {
                value: "startup".to_owned(),
                evidence: evidence.clone(),
            });
        }
    }
    if out.company_is("public_company").is_some() && out.company_is("large_company").is_none() {
        let evidence = out
            .company_is("public_company")
            .map(|t| t.evidence.clone())
            .unwrap_or_default();
        out.company.push(Trait {
            value: "large_company".to_owned(),
            evidence,
        });
    }
    // A large company is not a startup, whatever its history says.
    if out.company_is("large_company").is_some() {
        out.company.retain(|t| {
            !matches!(
                t.value.as_str(),
                "startup" | "early_stage" | "small_company"
            )
        });
    }

    // Culture.
    for (value, cues) in [
        ("process_heavy", &PROCESS_HEAVY[..]),
        ("mentorship", &MENTORSHIP[..]),
    ] {
        let vocabulary = compile(cues);
        if let Some(s) = sentences.iter().find(|s| {
            vocabulary
                .find_first(&s.words)
                .into_iter()
                .any(|(_, r)| !denied(s, r.start))
        }) && out.culture_is(value).is_none()
        {
            out.culture.push(Trait {
                value: value.to_owned(),
                evidence: s.text.clone(),
            });
        }
    }
    out.ownership = ownership(sentences);
    out
}

/// Ownership words that say it wherever they are ("real ownership", "a
/// lot of autonomy", "high agency").
const OWNERSHIP_NOUNS: [&str; 4] = ["ownership", "autonomy", "high agency", "owner of"];
/// Owning, said of the person: needs "you" in the sentence ("you'll own
/// the ledger service", "own large areas end to end"), and never "your
/// own" (a possessive: "building your own proxies").
const OWNERSHIP_VERBS: [&str; 4] = ["own", "owning", "owns", "take charge"];

/// Where the posting says the person would own their work. Product copy
/// ("autonomous agents", "end-to-end coding agents", "we own Minions")
/// is not the role's ownership.
fn ownership(sentences: &[Sentence]) -> Option<String> {
    let nouns = compile(&OWNERSHIP_NOUNS);
    let verbs = compile(&OWNERSHIP_VERBS);
    let you = |s: &Sentence| {
        s.words
            .iter()
            .any(|w| matches!(w.lower.as_str(), "you" | "your" | "you'll" | "youll"))
    };
    let possessive = |s: &Sentence, at: usize| {
        at > 0
            && matches!(
                s.words[at - 1].lower.as_str(),
                "your" | "their" | "its" | "our" | "my" | "his" | "her"
            )
    };
    let affirmed = |s: &Sentence, v: &Vocabulary| {
        v.find_all(&s.words)
            .into_iter()
            .flat_map(|(_, ranges)| ranges)
            .any(|r| !denied(s, r.start) && !possessive(s, r.start))
    };
    sentences
        .iter()
        .find(|s| affirmed(s, nouns) || (you(s) && affirmed(s, verbs)))
        .map(|s| s.text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facets::facets;
    use crate::testing::record;

    fn read(title: &str, description: &str) -> WorkReading {
        facets(&record("ashby:acme", title, description)).work
    }

    fn shapes(w: &WorkReading) -> Vec<(&str, ShapeBasis)> {
        w.shapes
            .iter()
            .map(|s| (s.shape.as_str(), s.basis))
            .collect()
    }

    #[test]
    fn early_career_titles_and_descriptions_are_levels() {
        for title in [
            "Software Engineer, Early Career",
            "Software Engineer, New Grad (2027)",
            "New Graduate Software Engineer",
            "Junior Backend Developer",
        ] {
            assert_eq!(
                read(title, "").level.map(|l| l.seniority),
                Some(Seniority::EarlyCareer),
                "{title}"
            );
        }
        let w = read(
            "Software Engineer",
            "Requirements\n- 0-2 years of professional experience, internships included",
        );
        assert_eq!(
            w.level.map(|l| (l.seniority, l.basis)),
            Some((Seniority::EarlyCareer, Basis::Description))
        );
        let w = read(
            "Backend Engineer",
            "Minimum requirements\n- 6+ years of experience building backend systems",
        );
        assert_eq!(w.level.map(|l| l.seniority), Some(Seniority::Senior));
        assert_eq!(
            read("Staff+ Software Engineer, Privacy", "")
                .level
                .map(|l| l.seniority),
            Some(Seniority::StaffPlus)
        );
        assert!(read("Software Engineer", "Build things.").level.is_none());
    }

    #[test]
    fn the_titles_own_role_outranks_its_team_name() {
        let w = read("Backend Engineer, Payments Infrastructure", "");
        assert_eq!(
            shapes(&w),
            [
                ("backend", ShapeBasis::Title),
                ("infrastructure", ShapeBasis::TitleArea)
            ]
        );
        let w = read("Senior Software Engineer, Platform", "");
        assert_eq!(shapes(&w), [("platform", ShapeBasis::Title)]);
        let w = read(
            "Senior Product Engineer, AI",
            "You'll own features from the database to the UI.",
        );
        assert_eq!(
            shapes(&w),
            [
                ("product", ShapeBasis::Title),
                ("ml_product", ShapeBasis::TitleArea),
                ("backend", ShapeBasis::Included)
            ]
        );
        let w = read("Research Engineer, Pretraining", "");
        assert!(w.has_shape("ml_research"), "{:?}", w.shapes);
        assert!(!w.has_shape("ml_product"));
    }

    #[test]
    fn using_a_technology_is_not_building_it() {
        let app = read(
            "Senior Backend Engineer",
            "Our product lives in PostgreSQL. You'll own the schema design, query performance, \
             partitioning and logical replication for our application data.",
        );
        assert!(app.specialties.is_empty(), "{:?}", app.specialties);
        let engine = read(
            "OrioleDB Developer",
            "Develop the storage engine: page layout, B-tree indexes, the buffer manager.",
        );
        assert_eq!(
            engine.specialties[0].specialty,
            Specialty::DatabaseInternals
        );
        assert_eq!(
            shapes(&engine),
            [("database_internals", ShapeBasis::Specialty)]
        );
        let services = read(
            "Senior Backend Engineer",
            "You'll own backend services in Go that run on Kubernetes, their Helm charts and dashboards.",
        );
        assert!(services.specialties.is_empty());
        let control = read(
            "Senior Software Engineer, Kubernetes Control Plane",
            "Custom controllers and operators, API server extensions, etcd performance.",
        );
        assert_eq!(
            control.specialties[0].specialty,
            Specialty::OrchestrationInternals
        );
        let llm = read(
            "Senior Product Engineer, AI",
            "Integrate LLM APIs. Curiosity about LLMs; no machine learning background needed.",
        );
        assert!(llm.specialties.is_empty(), "{:?}", llm.specialties);
        let training = read(
            "Research Engineer, Pretraining",
            "Scale distributed training across thousands of GPUs and write custom CUDA kernels.",
        );
        assert_eq!(training.specialties[0].specialty, Specialty::ModelTraining);
        // One stray cue in a description is not a specialty.
        assert!(
            read("Backend Engineer", "We care about low latency.")
                .specialties
                .is_empty()
        );
    }

    #[test]
    fn company_size_and_stage_as_stated() {
        let values = |w: &WorkReading| -> Vec<String> {
            let mut v: Vec<String> = w.company.iter().map(|t| t.value.clone()).collect();
            v.sort();
            v
        };
        let w = read(
            "Engineer",
            "Quarrybird is a 15-person startup building freight APIs.",
        );
        assert_eq!(values(&w), ["small_company", "startup"]);
        let w = read(
            "Engineer",
            "Brightmoss is a Series C company of about 400 people.",
        );
        assert_eq!(values(&w), ["growth"]);
        assert_eq!(w.headcount.map(|h| h.0), Some(400));
        let w = read(
            "Engineer",
            "Consolidated Meridian is a Fortune 500 insurer and a publicly traded company with thousands of employees.",
        );
        assert_eq!(values(&w), ["large_company", "public_company"]);
        let w = read("Engineer", "You'll join a team of 6 engineers.");
        assert!(w.headcount.is_none(), "a team is not the company");
        let w = read(
            "Engineer",
            "We closed our seed round and are a team of 9 building analytics.",
        );
        assert_eq!(w.headcount.map(|h| h.0), Some(9));
    }

    #[test]
    fn culture_and_founding() {
        let w = read(
            "Software Engineer",
            "You will ramp up with a dedicated mentor. Changes go through our change advisory board.",
        );
        assert!(w.culture_is("mentorship").is_some());
        assert!(w.culture_is("process_heavy").is_some());
        assert!(read("Founding Engineer", "").founding.is_some());
        assert!(
            read(
                "Software Engineer",
                "As our first engineering hire, you will own the backend."
            )
            .founding
            .is_some()
        );
    }
}
