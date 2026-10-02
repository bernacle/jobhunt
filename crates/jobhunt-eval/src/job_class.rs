//! BRU-330 experiment: a semantic job classifier, read offline against
//! the BRU-325 real-posting fixture
//! (`docs/job-function-semantic-classifier-experiment.md`). Not wired into
//! ranking.
//!
//! The classifier describes the **job**, never a candidate: what the
//! person in it does every day ([`Function`]), the kind of engineering
//! ([`Shape`]), how narrow and deep the work is ([`Depth`],
//! [`Specialty`]), the level hired for ([`Seniority`]) and how much of it
//! is spent with customers ([`CustomerFacing`]), each with short verbatim
//! quotes. What is sent ([`JobInput`]) carries no candidate, taste,
//! resume, pay preference or eligibility logic, so one answer per posting
//! version can be cached for everyone.
//!
//! * [`JobInput::build`] and [`render`]: the bounded job text sent.
//! * [`SYSTEM_PROMPT`] and [`schema`]: the fixed instructions and the
//!   strict JSON schema of an answer.
//! * [`parse`]: an answer validated against what was sent (every quote
//!   must be verbatim).
//! * [`Expected`] and [`agreement`]: a human job-description annotation and
//!   how an answer is scored against it.
//! * [`compare`]: whether two answers for the same posting agree.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Revision of the prompt, schema and input. Part of every cache key.
pub const CLASSIFIER_VERSION: &str = "job-class/1";

/// Content sentences sent at most. Higher than the fit reviewer's 60:
/// numbered company intros ("1.", "Product-led.") split into many short
/// sentences and pushed PostHog's responsibilities past that bound.
pub const MAX_SENTENCES: usize = 150;
/// Characters of content sent at most (about 2,000 tokens).
pub const MAX_JOB_CHARS: usize = 8_000;
/// Quotes kept per field.
const MAX_QUOTES: usize = 3;
/// Shortest quote that counts as evidence.
const MIN_QUOTE_CHARS: usize = 4;

macro_rules! vocabulary {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vdoc])* $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
    };
}

vocabulary!(
    /// What the person in the job does every day.
    Function {
        SoftwareEngineeringIc => "software_engineering_ic",
        EngineeringManagement => "engineering_management",
        SolutionsOrCustomerEngineering => "solutions_or_customer_engineering",
        SupportOrSuccess => "support_or_success",
        DeveloperRelations => "developer_relations",
        CurriculumOrTraining => "curriculum_or_training",
        GtmOrRevenueEngineering => "gtm_or_revenue_engineering",
        ProductOrProgramManagement => "product_or_program_management",
        Research => "research",
        OtherNonEngineering => "other_non_engineering",
        Unclear => "unclear",
    }
);

vocabulary!(
    /// The kind of software engineering (for a software engineering IC).
    Shape {
        Backend => "backend",
        Platform => "platform",
        Infrastructure => "infrastructure",
        ProductEngineering => "product_engineering",
        FullStack => "full_stack",
        Frontend => "frontend",
        Mobile => "mobile",
        DeveloperTooling => "developer_tooling",
        Sre => "sre",
        Security => "security",
        Data => "data",
        MlProduct => "ml_product",
        Other => "other",
    }
);

vocabulary!(
    /// How narrow and deep the work itself is.
    Depth {
        General => "general",
        Specialized => "specialized",
        DeepSpecialist => "deep_specialist",
        Unclear => "unclear",
    }
);

vocabulary!(
    /// What specialized work is specialized in.
    Specialty {
        KubernetesInternals => "kubernetes_internals",
        DatabaseInternals => "database_internals",
        StorageKernelIo => "storage_kernel_io",
        BareMetal => "bare_metal",
        NetworkingEdge => "networking_edge",
        RuntimeCompilers => "runtime_compilers",
        CryptographyZk => "cryptography_zk",
        MlResearch => "ml_research",
        DistributedSystems => "distributed_systems",
        Security => "security",
        DeveloperProductivity => "developer_productivity",
        Observability => "observability",
        TestAutomation => "test_automation",
        Performance => "performance",
        Other => "other",
    }
);

vocabulary!(
    /// The level the posting hires for.
    Seniority {
        EarlyCareer => "early_career",
        Mid => "mid",
        Senior => "senior",
        StaffPlus => "staff_plus",
        Management => "management",
        Unclear => "unclear",
    }
);

vocabulary!(
    /// How much of the job is spent with customers.
    CustomerFacing {
        No => "no",
        Secondary => "secondary",
        Primary => "primary",
        Unclear => "unclear",
    }
);

/// A posting's fields, as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostingFields<'a> {
    pub title: &'a str,
    pub department: Option<&'a str>,
    pub team: Option<&'a str>,
    pub location: Option<&'a str>,
    pub workplace: Option<&'a str>,
    pub description: Option<&'a str>,
}

/// What the classifier reads: the job only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInput {
    pub title: String,
    /// "Go To Market · Revenue Operations".
    pub team: String,
    /// Location and workplace fields, as published (descriptive only).
    pub setup: String,
    /// The description's content sentences, company sections removed.
    pub text: Vec<String>,
    /// Content sentences left out by the bounds.
    pub omitted: usize,
}

/// Section headings about the company rather than the job: their
/// paragraphs are not sent. "About the role", "About the team" and "About
/// you" are the job.
const COMPANY_HEADINGS: [&str; 10] = [
    "about us",
    "who we are",
    "our mission",
    "our story",
    "our values",
    "our culture",
    "why join",
    "why work",
    "life at",
    "the company",
];
const JOB_ABOUT: [&str; 7] = [
    "about the role",
    "about this role",
    "about the team",
    "about this team",
    "about you",
    "about the job",
    "about the position",
];

/// A short line that isn't a sentence or a bullet: a section heading.
fn heading(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_end_matches(':').trim();
    let words = trimmed.split_whitespace().count();
    let is_heading = !trimmed.is_empty()
        && words <= 6
        && !trimmed.ends_with(['.', '!', '?'])
        && !trimmed.starts_with(['-', '*', '•', '–']);
    is_heading.then(|| trimmed.to_lowercase())
}

fn company_heading(heading: &str) -> bool {
    let heading = heading.trim_start_matches(|c: char| !c.is_alphanumeric());
    if JOB_ABOUT.iter().any(|h| heading.starts_with(h)) {
        return false;
    }
    COMPANY_HEADINGS.iter().any(|h| heading.starts_with(h))
        // "About Supabase", "About ClickHouse"
        || (heading.starts_with("about ") && heading.split_whitespace().count() <= 3)
}

/// The description without its company sections.
fn without_company_sections(description: &str) -> String {
    let mut out = String::with_capacity(description.len());
    let mut skipping = false;
    for line in description.lines() {
        if let Some(h) = heading(line) {
            skipping = company_heading(&h);
            if skipping {
                continue;
            }
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

impl JobInput {
    /// The title, department and team, the location fields, and the
    /// description's content sentences (benefits, pay and policy
    /// boilerplate and "About <company>" sections removed), at most
    /// [`MAX_SENTENCES`] and [`MAX_JOB_CHARS`] characters. Company name,
    /// pay, ids and URLs are not sent.
    pub fn build(p: &PostingFields<'_>) -> Self {
        let present = |v: Option<&str>| {
            v.map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let team: Vec<String> = [present(p.department), present(p.team)]
            .into_iter()
            .flatten()
            .fold(Vec::new(), |mut acc, v| {
                if !acc.contains(&v) {
                    acc.push(v);
                }
                acc
            });
        let setup: Vec<String> = [present(p.location), present(p.workplace)]
            .into_iter()
            .flatten()
            .collect();
        let description = without_company_sections(p.description.unwrap_or_default());
        let sentences = jobhunt_ranking::facets::content_sentences(&description);
        let total = sentences.len();
        let mut text = Vec::new();
        let mut chars = 0;
        for s in sentences.into_iter().take(MAX_SENTENCES) {
            chars += s.chars().count();
            if chars > MAX_JOB_CHARS {
                break;
            }
            text.push(s);
        }
        Self {
            title: p.title.trim().to_owned(),
            team: team.join(" · "),
            setup: setup.join(" · "),
            omitted: total - text.len(),
            text,
        }
    }

    /// Everything a quote may come from, normalized.
    fn quotable(&self) -> String {
        normalize(&format!(
            "{}\n{}\n{}",
            self.title,
            self.team,
            self.text.join("\n")
        ))
    }
}

/// The text a model reads after [`SYSTEM_PROMPT`].
pub fn render(input: &JobInput) -> String {
    let mut out = String::from("<job>\n");
    out.push_str(&format!("<title>{}</title>\n", input.title));
    if !input.team.is_empty() {
        out.push_str(&format!(
            "<department_and_team>{}</department_and_team>\n",
            input.team
        ));
    }
    if !input.setup.is_empty() {
        out.push_str(&format!(
            "<location_fields>{}</location_fields>\n",
            input.setup
        ));
    }
    out.push_str("<posting>\n");
    for s in &input.text {
        out.push_str(s);
        out.push('\n');
    }
    out.push_str("</posting>\n</job>\n");
    out
}

/// The fixed instructions.
pub const SYSTEM_PROMPT: &str = "You classify one job posting: what the job IS. You are not told \
who will read your answer, and you never evaluate any applicant: no fit, no qualifications, no \
gaps, no recommendation.\n\n\
Answer these, in order:\n\
1. daily_work: one plain sentence, in your own words, on what the person in this job spends \
their days doing.\n\
2. function: the job's actual daily function.\n\
- software_engineering_ic: designs, builds and ships software or systems as an individual \
contributor (product, backend, infrastructure, tooling, data, and so on).\n\
- engineering_management: manages engineers (reports, hiring, team leadership as the main job).\n\
- solutions_or_customer_engineering: technical work for or with customers: pre- or post-sales, \
implementations, integrations, onboarding, consulting, professional services, field or \
solutions engineering, architects who advise customers.\n\
- support_or_success: resolving customer issues and cases, customer success.\n\
- developer_relations: advocacy, community and content for developers.\n\
- curriculum_or_training: creating or teaching courses, training or certifications.\n\
- gtm_or_revenue_engineering: building systems for sales, marketing or revenue operations \
(CRM, enrichment, routing, outbound tooling).\n\
- product_or_program_management, research (the main output is research, not shipped \
software), other_non_engineering, unclear.\n\
3. engineering_shapes: only for software_engineering_ic, otherwise []. The kind(s) of \
engineering the work is: backend, platform (internal platforms other engineers build on), \
infrastructure (cloud, compute, networks, provisioning), product_engineering (user-facing \
product features end to end), full_stack, frontend, mobile, developer_tooling, sre \
(reliability, on-call, incident response), security, data (pipelines, warehouses), ml_product, \
other. Give more than one only when the work genuinely spans them.\n\
4. specialization_depth: how narrow and deep the work itself is.\n\
- general: broad engineering in its area. Technologies are used, not built: running services on \
Kubernetes, using PostgreSQL, building APIs, operating cloud infrastructure.\n\
- specialized: centered on one sub-area that most engineers of that kind don't work in daily, \
without building a system's internals (an observability stack, release engineering, auth, \
benchmarking, cloud cost, a networking edge).\n\
- deep_specialist: building the internals of systems most engineers only use, or work that needs \
years of narrow expertise: Kubernetes operators, controllers or storage drivers; database or \
storage engines; kernels and I/O paths; compilers and runtimes; cryptography and zero-knowledge \
proofs; bare-metal provisioning and firmware; network data planes.\n\
- unclear.\n\
5. specialties: what specialized work is specialized in; [] for general work. Never tag a \
technology the job merely uses.\n\
6. seniority: the level this posting hires for. The title decides first (Junior, Associate, I, \
II, III, IC1-IC5, Senior, Staff, Principal, Lead, Manager, Director, Head of), then the scope of \
responsibility. A years-of-experience line alone never sets the level. \"II\" and \"IC3\" are \
mid-level unless the posting says otherwise; Staff and Principal are staff_plus; people managers \
are management. A title with no level and no clear scope is unclear.\n\
7. customer_facing: how much of the job is spent with customers, prospects or external users: \
no; secondary (some contact, not the core of the job); primary (the job mainly serves customers); \
unclear.\n\
8. evidence: for every answer that isn't unclear, including \"no\" and every shape and \
specialty, one to three short quotes copied exactly from the title, the department and team, or \
the posting text (a few words to one clause each, no ellipses). Quote what the person will do.\n\n\
How to read a posting:\n\
- Classify the work done every day. The title and the responsibilities outrank the requirements \
list; a requirement or a technology list never defines the job on its own.\n\
- \"Engineer\" in a title doesn't make a job software engineering. Revenue or marketing operations \
engineers, solutions, sales, support, consulting or implementation engineers, architects who \
serve customers, and instructors are not software engineering ICs.\n\
- A technology that is the subject of teaching, selling, supporting or integrating for customers \
is not the job's engineering shape, and doesn't make the work specialized.\n\
- Depth is about the work, not about how hard the company says it is: \"high scale\", \"complex\" \
or \"distributed\" alone don't make a job specialized.\n\
- Company descriptions, mission, funding, growth, awards, founders, culture and benefits are \
irrelevant: never classify from them and never quote them. \"Platform\" may be a product or team \
name; classify by the work.\n\
- Department and team fields are hints, not answers. Location fields are descriptive only.";

fn string_array() -> Value {
    json!({"type": "array", "items": {"type": "string"}})
}

fn enum_of(values: Vec<&'static str>) -> Value {
    json!({"type": "string", "enum": values})
}

/// The JSON schema of an answer (strict: every field required, nothing
/// else allowed).
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "daily_work", "function", "engineering_shapes", "specialization_depth",
            "specialties", "seniority", "customer_facing", "evidence"
        ],
        "properties": {
            "daily_work": {"type": "string"},
            "function": enum_of(Function::ALL.iter().map(|v| v.as_str()).collect()),
            "engineering_shapes": {
                "type": "array",
                "items": enum_of(Shape::ALL.iter().map(|v| v.as_str()).collect()),
            },
            "specialization_depth": enum_of(Depth::ALL.iter().map(|v| v.as_str()).collect()),
            "specialties": {
                "type": "array",
                "items": enum_of(Specialty::ALL.iter().map(|v| v.as_str()).collect()),
            },
            "seniority": enum_of(Seniority::ALL.iter().map(|v| v.as_str()).collect()),
            "customer_facing": enum_of(CustomerFacing::ALL.iter().map(|v| v.as_str()).collect()),
            "evidence": {
                "type": "object",
                "additionalProperties": false,
                "required": ["function", "shapes", "specialization", "seniority", "customer_facing"],
                "properties": {
                    "function": string_array(),
                    "shapes": string_array(),
                    "specialization": string_array(),
                    "seniority": string_array(),
                    "customer_facing": string_array(),
                }
            }
        }
    })
}

/// Quotes behind each classification.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub function: Vec<String>,
    pub shapes: Vec<String>,
    pub specialization: Vec<String>,
    pub seniority: Vec<String>,
    pub customer_facing: Vec<String>,
}

/// One answer, as the schema shapes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub daily_work: String,
    pub function: Function,
    pub engineering_shapes: Vec<Shape>,
    pub specialization_depth: Depth,
    pub specialties: Vec<Specialty>,
    pub seniority: Seniority,
    pub customer_facing: CustomerFacing,
    pub evidence: Evidence,
}

/// A classification field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Function,
    Shapes,
    Depth,
    Specialties,
    Seniority,
    CustomerFacing,
}

impl Field {
    pub const ALL: [Self; 6] = [
        Self::Function,
        Self::Shapes,
        Self::Depth,
        Self::Specialties,
        Self::Seniority,
        Self::CustomerFacing,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Shapes => "engineering_shapes",
            Self::Depth => "specialization_depth",
            Self::Specialties => "specialties",
            Self::Seniority => "seniority",
            Self::CustomerFacing => "customer_facing",
        }
    }
}

/// An answer checked against what was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validated {
    pub classification: Classification,
    /// Quotes, verbatim in the posting, per field (at most three each).
    pub valid_quotes: usize,
    /// Quotes not found in what was sent: `(evidence field, quote)`.
    pub invalid_quotes: Vec<(String, String)>,
    /// Classifications that aren't unclear but have no valid quote.
    pub unsupported: Vec<Field>,
}

/// Lowercased, with typographic quotes and dashes made plain and
/// whitespace collapsed.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        let c = match c {
            '\u{2019}' | '\u{2018}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
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

/// A quote as it should appear in the posting: normalized, without
/// surrounding quotation marks or a trailing ellipsis.
fn quote_key(quote: &str) -> String {
    let n = normalize(quote);
    let n = n.trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace());
    let n = n.trim_end_matches("...").trim_end_matches('\u{2026}');
    n.trim_end_matches(['.', ',', ';', ':']).trim().to_owned()
}

/// Validates an answer: malformed JSON, unknown fields or values reject it
/// whole; quotes not in the posting are counted, never trusted, and a
/// classification left with no valid quote is reported as unsupported.
pub fn parse(text: &str, input: &JobInput) -> Result<Validated, String> {
    let mut classification: Classification =
        serde_json::from_str(text.trim()).map_err(|e| format!("not the expected JSON: {e}"))?;
    let quotable = input.quotable();
    let mut invalid = Vec::new();
    let mut valid_quotes = 0;
    let mut check = |name: &str, quotes: &mut Vec<String>| {
        let mut kept = Vec::new();
        for q in quotes.drain(..) {
            let key = quote_key(&q);
            if key.chars().count() >= MIN_QUOTE_CHARS && quotable.contains(&key) {
                if kept.len() < MAX_QUOTES {
                    kept.push(q.trim().to_owned());
                    valid_quotes += 1;
                }
            } else {
                invalid.push((name.to_owned(), q));
            }
        }
        *quotes = kept;
    };
    let e = &mut classification.evidence;
    check("function", &mut e.function);
    check("shapes", &mut e.shapes);
    check("specialization", &mut e.specialization);
    check("seniority", &mut e.seniority);
    check("customer_facing", &mut e.customer_facing);
    let c = &classification;
    let mut unsupported = Vec::new();
    if c.function != Function::Unclear && c.evidence.function.is_empty() {
        unsupported.push(Field::Function);
    }
    if !c.engineering_shapes.is_empty() && c.evidence.shapes.is_empty() {
        unsupported.push(Field::Shapes);
    }
    if !matches!(c.specialization_depth, Depth::Unclear | Depth::General)
        && c.evidence.specialization.is_empty()
    {
        unsupported.push(Field::Depth);
    }
    if !c.specialties.is_empty() && c.evidence.specialization.is_empty() {
        unsupported.push(Field::Specialties);
    }
    if c.seniority != Seniority::Unclear && c.evidence.seniority.is_empty() {
        unsupported.push(Field::Seniority);
    }
    if c.customer_facing != CustomerFacing::Unclear && c.evidence.customer_facing.is_empty() {
        unsupported.push(Field::CustomerFacing);
    }
    Ok(Validated {
        classification,
        valid_quotes,
        invalid_quotes: invalid,
        unsupported,
    })
}

/// A human job-description annotation: every acceptable answer per field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub function: Vec<Function>,
    /// Acceptable shapes when the job is a software engineering IC (an
    /// answer's shapes must be a non-empty subset).
    #[serde(default)]
    pub shapes: Vec<Shape>,
    pub depth: Vec<Depth>,
    /// An answer must tag at least one of these (none: no requirement).
    #[serde(default)]
    pub specialties_required: Vec<Specialty>,
    /// Tags an answer may also carry without being wrong.
    #[serde(default)]
    pub specialties_allowed: Vec<Specialty>,
    pub seniority: Vec<Seniority>,
    pub customer_facing: Vec<CustomerFacing>,
}

/// Whether an answer agrees with an annotation, per field (`None`: not
/// applicable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agreement {
    pub function: bool,
    pub shapes: Option<bool>,
    pub depth: bool,
    pub specialties: bool,
    pub seniority: bool,
    pub customer_facing: bool,
}

impl Agreement {
    pub fn get(&self, field: Field) -> Option<bool> {
        match field {
            Field::Function => Some(self.function),
            Field::Shapes => self.shapes,
            Field::Depth => Some(self.depth),
            Field::Specialties => Some(self.specialties),
            Field::Seniority => Some(self.seniority),
            Field::CustomerFacing => Some(self.customer_facing),
        }
    }
}

/// Scores an answer against an annotation.
pub fn agreement(expected: &Expected, c: &Classification) -> Agreement {
    let ic_expected = expected.function.contains(&Function::SoftwareEngineeringIc);
    let shapes = (ic_expected && c.function == Function::SoftwareEngineeringIc).then(|| {
        !c.engineering_shapes.is_empty()
            && c.engineering_shapes
                .iter()
                .all(|s| expected.shapes.contains(s))
    });
    let allowed = |s: &Specialty| {
        expected.specialties_required.contains(s) || expected.specialties_allowed.contains(s)
    };
    let specialties = c.specialties.iter().all(allowed)
        && (expected.specialties_required.is_empty()
            || c.specialties
                .iter()
                .any(|s| expected.specialties_required.contains(s)));
    Agreement {
        function: expected.function.contains(&c.function),
        shapes,
        depth: expected.depth.contains(&c.specialization_depth),
        specialties,
        seniority: expected.seniority.contains(&c.seniority),
        customer_facing: expected.customer_facing.contains(&c.customer_facing),
    }
}

/// How two answers for the same posting compare on one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    /// The same value (sets compared as sets).
    Exact,
    /// Different, but nothing a deterministic reader would decide
    /// differently on (see [`compare`]).
    Equivalent,
    /// A material disagreement.
    Material,
}

fn same_set<T: Ord + Copy>(a: &[T], b: &[T]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    a.dedup();
    b.sort();
    b.dedup();
    a == b
}

fn overlap<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    a.iter().any(|x| b.contains(x))
}

/// Compares two answers field by field. Materially equivalent:
///
/// * shapes: overlapping sets (`[platform]` vs `[platform,
///   infrastructure]`);
/// * depth: `specialized` vs `deep_specialist` (both "not general"; the
///   exact value is reported separately);
/// * specialties: overlapping sets, or both empty;
/// * customer-facing: `no` vs `secondary` (neither makes it a customer
///   job).
///
/// Everything else that differs (function, seniority, `unclear` vs a
/// value, general vs specialized) is material.
pub fn compare(a: &Classification, b: &Classification) -> Vec<(Field, Stability)> {
    let grade = |exact: bool, equivalent: bool| {
        if exact {
            Stability::Exact
        } else if equivalent {
            Stability::Equivalent
        } else {
            Stability::Material
        }
    };
    let non_general = |d: Depth| matches!(d, Depth::Specialized | Depth::DeepSpecialist);
    let not_customer =
        |c: CustomerFacing| matches!(c, CustomerFacing::No | CustomerFacing::Secondary);
    vec![
        (Field::Function, grade(a.function == b.function, false)),
        (
            Field::Shapes,
            grade(
                same_set(&a.engineering_shapes, &b.engineering_shapes),
                overlap(&a.engineering_shapes, &b.engineering_shapes),
            ),
        ),
        (
            Field::Depth,
            grade(
                a.specialization_depth == b.specialization_depth,
                non_general(a.specialization_depth) && non_general(b.specialization_depth),
            ),
        ),
        (
            Field::Specialties,
            grade(
                same_set(&a.specialties, &b.specialties),
                overlap(&a.specialties, &b.specialties),
            ),
        ),
        (Field::Seniority, grade(a.seniority == b.seniority, false)),
        (
            Field::CustomerFacing,
            grade(
                a.customer_facing == b.customer_facing,
                not_customer(a.customer_facing) && not_customer(b.customer_facing),
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> JobInput {
        JobInput::build(&PostingFields {
            title: "Senior Curriculum Developer & Instructor",
            department: Some("Support Services"),
            team: Some("Learning"),
            location: Some("AMER"),
            workplace: Some("remote"),
            description: Some(
                "ABOUT ACME\nAcme raised $500M and won Best Startup.\n\nABOUT THE ROLE\n\
                 Developing Acme training courses for engineers.\n\
                 Teaching courses both virtually and in person.\n\n\
                 BENEFITS\nDental and vision insurance.\n",
            ),
        })
    }

    fn answer(function: &str, quote: &str) -> String {
        json!({
            "daily_work": "Writes and teaches courses.",
            "function": function,
            "engineering_shapes": [],
            "specialization_depth": "general",
            "specialties": [],
            "seniority": "senior",
            "customer_facing": "secondary",
            "evidence": {
                "function": [quote],
                "shapes": [],
                "specialization": [],
                "seniority": ["Senior Curriculum Developer"],
                "customer_facing": ["Teaching courses both virtually and in person"]
            }
        })
        .to_string()
    }

    #[test]
    fn only_the_job_is_sent() {
        let i = input();
        let text = render(&i);
        assert!(text.contains("Developing Acme training courses"));
        assert!(text.contains("Support Services · Learning"));
        // The company section and benefits are not the job.
        assert!(!text.contains("$500M"));
        assert!(!text.contains("Best Startup"));
        assert!(!text.contains("Dental"));
    }

    #[test]
    fn about_the_role_is_kept_and_about_the_company_is_not() {
        assert!(company_heading("about supabase"));
        assert!(company_heading("who we are"));
        assert!(!company_heading("about the role"));
        assert!(!company_heading("about you"));
        assert!(!company_heading("what you'll do"));
    }

    #[test]
    fn quotes_must_come_from_the_posting() {
        let i = input();
        let ok = parse(
            &answer(
                "curriculum_or_training",
                "Developing Acme training courses...",
            ),
            &i,
        )
        .unwrap();
        assert!(ok.invalid_quotes.is_empty());
        assert!(ok.unsupported.is_empty());
        let made_up = parse(
            &answer("curriculum_or_training", "Builds Kubernetes operators"),
            &i,
        )
        .unwrap();
        assert_eq!(made_up.invalid_quotes.len(), 1);
        assert_eq!(made_up.unsupported, vec![Field::Function]);
        // Company text isn't sent, so it can't be quoted.
        let company = parse(&answer("curriculum_or_training", "raised $500M"), &i).unwrap();
        assert_eq!(company.unsupported, vec![Field::Function]);
    }

    #[test]
    fn malformed_answers_are_rejected_whole() {
        let i = input();
        assert!(parse("not json", &i).is_err());
        assert!(
            parse(
                &answer("strong_fit", "Developing Acme training courses"),
                &i
            )
            .is_err()
        );
        let mut extra: Value =
            serde_json::from_str(&answer("research", "Developing Acme training courses")).unwrap();
        extra["fit"] = json!("strong");
        assert!(parse(&extra.to_string(), &i).is_err());
    }

    #[test]
    fn the_schema_names_every_value_and_no_fit() {
        let s = schema().to_string();
        for f in Function::ALL {
            assert!(s.contains(f.as_str()));
        }
        for word in ["\"fit\"", "strong", "plausible", "candidate"] {
            assert!(!s.contains(word), "{word}");
        }
        assert!(!SYSTEM_PROMPT.contains("candidate"));
    }

    fn classification(function: Function, shapes: &[Shape], depth: Depth) -> Classification {
        Classification {
            daily_work: String::new(),
            function,
            engineering_shapes: shapes.to_vec(),
            specialization_depth: depth,
            specialties: vec![],
            seniority: Seniority::Senior,
            customer_facing: CustomerFacing::No,
            evidence: Evidence::default(),
        }
    }

    #[test]
    fn agreement_accepts_any_listed_answer() {
        let expected = Expected {
            function: vec![Function::SoftwareEngineeringIc],
            shapes: vec![Shape::Platform, Shape::Infrastructure],
            depth: vec![Depth::DeepSpecialist, Depth::Specialized],
            specialties_required: vec![Specialty::KubernetesInternals],
            specialties_allowed: vec![Specialty::DistributedSystems],
            seniority: vec![Seniority::Senior, Seniority::Unclear],
            customer_facing: vec![CustomerFacing::No],
        };
        let mut c = classification(
            Function::SoftwareEngineeringIc,
            &[Shape::Platform],
            Depth::DeepSpecialist,
        );
        c.specialties = vec![
            Specialty::KubernetesInternals,
            Specialty::DistributedSystems,
        ];
        let a = agreement(&expected, &c);
        assert!(a.function && a.depth && a.specialties && a.seniority && a.customer_facing);
        assert_eq!(a.shapes, Some(true));
        // A general reading misses the depth and the required specialty.
        let general = classification(
            Function::SoftwareEngineeringIc,
            &[Shape::Backend],
            Depth::General,
        );
        let a = agreement(&expected, &general);
        assert!(!a.depth && !a.specialties);
        assert_eq!(a.shapes, Some(false));
        // Shapes aren't scored when the function is wrong.
        let wrong = classification(Function::CurriculumOrTraining, &[], Depth::General);
        assert_eq!(agreement(&expected, &wrong).shapes, None);
    }

    #[test]
    fn stability_separates_equivalent_from_material() {
        let a = classification(
            Function::SoftwareEngineeringIc,
            &[Shape::Platform],
            Depth::Specialized,
        );
        let b = classification(
            Function::SoftwareEngineeringIc,
            &[Shape::Infrastructure, Shape::Platform],
            Depth::DeepSpecialist,
        );
        let got = compare(&a, &b);
        assert!(got.contains(&(Field::Function, Stability::Exact)));
        assert!(got.contains(&(Field::Shapes, Stability::Equivalent)));
        assert!(got.contains(&(Field::Depth, Stability::Equivalent)));
        let c = classification(
            Function::SolutionsOrCustomerEngineering,
            &[],
            Depth::General,
        );
        let got = compare(&a, &c);
        assert!(got.contains(&(Field::Function, Stability::Material)));
        assert!(got.contains(&(Field::Shapes, Stability::Material)));
        assert!(got.contains(&(Field::Depth, Stability::Material)));
    }
}
