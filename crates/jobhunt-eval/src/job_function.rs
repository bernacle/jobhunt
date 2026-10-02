//! A function-only job classifier, measured on a fresh real-posting
//! snapshot (`docs/job-function-only-fresh-snapshot-experiment.md`). The
//! follow-up to [`job_class`](crate::job_class) (BRU-330): the same job-only
//! input, asked only the two things that held up there. Not wired into
//! ranking.
//!
//! It answers what the person hired into a job is primarily expected to do
//! ([`Function`]) and whether working with customers is a defining part of
//! it ([`CustomerFacing`]), each with short verbatim quotes. No depth,
//! specialty, seniority, fit or score: the schema has nowhere to put them.
//!
//! * [`render`], [`SYSTEM_PROMPT`] and [`schema`]: what is sent.
//! * [`parse`]: an answer validated against what was sent.
//! * [`Annotation`] and [`evaluate`]: human labels and the pre-registered
//!   gates three runs are scored against.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use crate::job_class::{Function, JobInput, PostingFields};
use crate::job_class::{MAX_QUOTES, MIN_QUOTE_CHARS, quote_key};

/// Revision of the prompt, schema and input. Part of every cache key.
pub const CLASSIFIER_VERSION: &str = "job-function/1";

/// Whether working with customers is a defining part of the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerFacing {
    Yes,
    No,
    Unclear,
}

impl CustomerFacing {
    pub const ALL: &'static [Self] = &[Self::Yes, Self::No, Self::Unclear];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unclear => "unclear",
        }
    }
}

/// The text a model reads after [`SYSTEM_PROMPT`]: the title, the
/// department and team, and the description's content sentences. Unlike
/// BRU-330, the location fields aren't sent: they say nothing about the
/// function.
pub fn render(input: &JobInput) -> String {
    let mut out = String::from("<job>\n");
    out.push_str(&format!("<title>{}</title>\n", input.title));
    if !input.team.is_empty() {
        out.push_str(&format!(
            "<department_and_team>{}</department_and_team>\n",
            input.team
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

/// The fixed instructions. Written before the evaluation sample was drawn.
pub const SYSTEM_PROMPT: &str = "You read one job posting and say what kind of job it is: what \
the person hired into this role is primarily expected to do. You never evaluate any applicant: no \
fit, no qualifications, no recommendation.\n\n\
Answer two questions. Before each answer, give one to three short quotes copied exactly from the \
title, the department and team, or the posting text (a few words to one clause each, no \
ellipses). Quote what the person will do. Give no quotes only when the answer is unclear.\n\n\
1. function: the job's primary function.\n\
- software_engineering_ic: designs, builds and ships software or systems as an individual \
contributor, for the employer's own product, platform or infrastructure (backend, frontend, \
mobile, full stack, platform, infrastructure, reliability, security, data, machine learning, \
testing, tooling, embedded).\n\
- engineering_management: manages engineers as the main job: direct reports, hiring, leading \
teams.\n\
- solutions_or_customer_engineering: technical work for or with particular customers or \
prospects: pre-sales and solutions engineering, architects who advise customers, implementing, \
integrating or deploying the product for customers, consulting, professional services, field and \
forward-deployed engineering.\n\
- support_or_success: resolving customers' issues and tickets, technical support, customer \
success and account health.\n\
- developer_relations: advocacy, community and content for outside developers.\n\
- curriculum_or_training: creating or delivering courses, training or certifications.\n\
- gtm_or_revenue_engineering: building or running systems for sales, marketing or revenue \
operations (CRM, lead routing, data enrichment, outbound and other go-to-market tooling).\n\
- product_or_program_management: owns product direction, or plans and coordinates programs and \
projects, rather than building.\n\
- research: the main output is research (experiments, findings, new models or methods, papers), \
not shipped software.\n\
- other_non_engineering: any other job, including sales, marketing, design, data analysis, \
finance, legal, people, operations, internal IT support, and hardware, mechanical or electrical \
engineering.\n\
- unclear: the posting doesn't say enough, or the job is evenly split between functions.\n\
2. primary_customer_facing: is working directly with customers, prospects or outside users a \
defining part of the job?\n\
- yes: the job exists to serve, advise, implement for, support, teach or sell to customers or \
outside developers; that contact is central to the daily work.\n\
- no: the work is mainly internal. Occasional customer calls, user research, feedback from users \
or open-source community contact don't make it customer-facing.\n\
- unclear.\n\n\
How to read a posting:\n\
- The title, the department and team, and the core responsibilities outrank the requirements. A \
requirement or a technology list never defines the job on its own.\n\
- Technology words don't make a job software engineering. A job that teaches, sells, supports, \
implements or integrates a technical product for customers keeps that function however deep the \
technology (cloud, Kubernetes, databases, APIs).\n\
- \"Engineer\", \"Architect\" or \"Developer\" in a title doesn't decide the function, and neither \
does a team or product name such as \"Platform\".\n\
- Building internal systems for sales, marketing or revenue teams is gtm_or_revenue_engineering, \
even when the work is writing code against APIs.\n\
- A customer-facing architect or engineer is customer-facing whatever the technical domain.\n\
- Ignore company descriptions, mission, funding, growth, awards, founder biographies, culture, \
benefits and other employer marketing: never classify from them and never quote them.\n\
- If the primary function or the customer contact is genuinely ambiguous, answer unclear.";

fn string_array() -> Value {
    json!({"type": "array", "items": {"type": "string"}})
}

/// The JSON schema of an answer (strict: every field required, nothing
/// else allowed). Evidence comes before each label, so the model quotes
/// before it decides.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "function_evidence", "function",
            "customer_facing_evidence", "primary_customer_facing"
        ],
        "properties": {
            "function_evidence": string_array(),
            "function": {
                "type": "string",
                "enum": Function::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            },
            "customer_facing_evidence": string_array(),
            "primary_customer_facing": {
                "type": "string",
                "enum": CustomerFacing::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            },
        }
    })
}

/// One answer, as the schema shapes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub function_evidence: Vec<String>,
    pub function: Function,
    pub customer_facing_evidence: Vec<String>,
    pub primary_customer_facing: CustomerFacing,
}

/// An answer checked against what was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validated {
    pub classification: Classification,
    /// Quotes verbatim in what was sent (at most three per field).
    pub valid_quotes: usize,
    /// Quotes not found in what was sent: `(field, quote)`.
    pub invalid_quotes: Vec<(String, String)>,
    /// Labels other than unclear left without a valid quote.
    pub unsupported: Vec<String>,
}

/// Validates an answer: malformed JSON, unknown fields or values reject it
/// whole; quotes not in what was sent are dropped and counted, and a label
/// left without a valid quote is reported (never re-labeled).
pub fn parse(text: &str, input: &JobInput) -> Result<Validated, String> {
    let mut c: Classification =
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
    check("function", &mut c.function_evidence);
    check("customer_facing", &mut c.customer_facing_evidence);
    let mut unsupported = Vec::new();
    if c.function != Function::Unclear && c.function_evidence.is_empty() {
        unsupported.push("function".to_owned());
    }
    if c.primary_customer_facing != CustomerFacing::Unclear && c.customer_facing_evidence.is_empty()
    {
        unsupported.push("primary_customer_facing".to_owned());
    }
    Ok(Validated {
        classification: c,
        valid_quotes,
        invalid_quotes: invalid,
        unsupported,
    })
}

/// Why a job is a critical case, decided at annotation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Critical {
    /// Clearly not a software engineering IC job, though its title or
    /// technology words could pass for one. Must never be answered
    /// `software_engineering_ic`.
    WrongFunction,
    /// A clearly ordinary software engineering IC job. Must always be
    /// answered `software_engineering_ic`.
    OrdinaryEngineering,
    /// Solutions, support, implementation or success work whose main
    /// purpose is customers. Must always be answered customer-facing `yes`.
    CustomerRole,
}

/// A human label for one posting. `*_also` lists other answers accepted
/// for a genuinely mixed posting (used sparingly; strict agreement ignores
/// them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub function: Function,
    #[serde(default)]
    pub function_also: Vec<Function>,
    pub primary_customer_facing: CustomerFacing,
    #[serde(default)]
    pub customer_facing_also: Vec<CustomerFacing>,
    #[serde(default)]
    pub critical: Vec<Critical>,
}

impl Annotation {
    pub fn function_ok(&self, f: Function) -> bool {
        f == self.function || self.function_also.contains(&f)
    }

    pub fn customer_facing_ok(&self, c: CustomerFacing) -> bool {
        c == self.primary_customer_facing || self.customer_facing_also.contains(&c)
    }
}

/// The bars, fixed before any model output existed.
pub mod bars {
    /// Function agreement with the annotations, in every run.
    pub const FUNCTION_AGREEMENT: f64 = 0.95;
    /// Customer-facing agreement with the annotations, in every run.
    pub const CUSTOMER_FACING_AGREEMENT: f64 = 0.90;
    /// Jobs answered identically in all three runs: function.
    pub const FUNCTION_STABILITY: f64 = 0.98;
    /// Jobs answered identically in all three runs: customer-facing.
    pub const CUSTOMER_FACING_STABILITY: f64 = 0.95;
}

/// A count out of a total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ratio {
    pub hits: usize,
    pub of: usize,
}

impl Ratio {
    pub fn rate(self) -> f64 {
        if self.of == 0 {
            1.0
        } else {
            self.hits as f64 / self.of as f64
        }
    }

    pub fn all(self) -> bool {
        self.hits == self.of
    }
}

/// One run scored against the annotations.
#[derive(Debug, Clone, Serialize)]
pub struct RunScore {
    pub answered: usize,
    pub function: Ratio,
    pub function_strict: Ratio,
    pub customer_facing: Ratio,
    pub customer_facing_strict: Ratio,
    pub wrong_function_excluded: Ratio,
    pub ordinary_engineering_kept: Ratio,
    pub customer_roles_yes: Ratio,
    /// Jobs whose annotation rules out software engineering IC but were
    /// answered it, by annotated function.
    pub non_ic_read_as_ic: BTreeMap<String, Vec<String>>,
}

/// One job's answers across runs, when they differ on a field.
#[derive(Debug, Clone, Serialize)]
pub struct Unstable {
    pub job: String,
    pub field: &'static str,
    pub answers: Vec<String>,
}

/// Every gate, scored.
#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub runs: Vec<RunScore>,
    /// Jobs answered identically in every run.
    pub function_stable: Ratio,
    pub customer_facing_stable: Ratio,
    /// Pairwise exact agreement, per run pair `(i, j)`.
    pub function_pairs: Vec<(usize, usize, Ratio)>,
    pub customer_facing_pairs: Vec<(usize, usize, Ratio)>,
    pub unstable: Vec<Unstable>,
    /// Non-IC classes answered IC for two or more distinct jobs, or a job
    /// answered IC in two or more runs.
    pub systematic_non_ic_as_ic: Vec<String>,
    pub gates: Vec<(&'static str, bool)>,
    pub pass: bool,
}

fn ratio(hits: usize, of: usize) -> Ratio {
    Ratio { hits, of }
}

/// Scores runs (each a map from job id to answer) against the annotations
/// and checks every pre-registered gate. A job missing from a run counts
/// against every gate it would have been scored on.
pub fn evaluate(
    annotations: &[(String, Annotation)],
    runs: &[BTreeMap<String, Classification>],
) -> Evaluation {
    let n = annotations.len();
    let ic = Function::SoftwareEngineeringIc;
    let mut scores = Vec::new();
    let mut ic_runs: BTreeMap<&str, usize> = BTreeMap::new();
    for run in runs {
        let (mut f, mut fs, mut c, mut cs) = (0, 0, 0, 0);
        let (mut wf, mut wf_n, mut oe, mut oe_n, mut cr, mut cr_n) = (0, 0, 0, 0, 0, 0);
        let mut non_ic: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (job, a) in annotations {
            let got = run.get(job);
            let function = got.map(|g| g.function);
            let cf = got.map(|g| g.primary_customer_facing);
            f += usize::from(function.is_some_and(|x| a.function_ok(x)));
            fs += usize::from(function == Some(a.function));
            c += usize::from(cf.is_some_and(|x| a.customer_facing_ok(x)));
            cs += usize::from(cf == Some(a.primary_customer_facing));
            if a.critical.contains(&Critical::WrongFunction) {
                wf_n += 1;
                wf += usize::from(function.is_some_and(|x| x != ic));
            }
            if a.critical.contains(&Critical::OrdinaryEngineering) {
                oe_n += 1;
                oe += usize::from(function == Some(ic));
            }
            if a.critical.contains(&Critical::CustomerRole) {
                cr_n += 1;
                cr += usize::from(cf == Some(CustomerFacing::Yes));
            }
            if !a.function_ok(ic) && function == Some(ic) {
                non_ic
                    .entry(a.function.as_str().to_owned())
                    .or_default()
                    .push(job.clone());
                *ic_runs.entry(job.as_str()).or_default() += 1;
            }
        }
        scores.push(RunScore {
            answered: annotations
                .iter()
                .filter(|(j, _)| run.contains_key(j))
                .count(),
            function: ratio(f, n),
            function_strict: ratio(fs, n),
            customer_facing: ratio(c, n),
            customer_facing_strict: ratio(cs, n),
            wrong_function_excluded: ratio(wf, wf_n),
            ordinary_engineering_kept: ratio(oe, oe_n),
            customer_roles_yes: ratio(cr, cr_n),
            non_ic_read_as_ic: non_ic,
        });
    }

    let mut unstable = Vec::new();
    let (mut f_stable, mut c_stable) = (0, 0);
    for (job, _) in annotations {
        let answers: Vec<Option<&Classification>> = runs.iter().map(|r| r.get(job)).collect();
        let complete = answers.iter().all(Option::is_some);
        let fs: Vec<String> = answers
            .iter()
            .map(|a| a.map_or("missing", |a| a.function.as_str()).to_owned())
            .collect();
        let cs: Vec<String> = answers
            .iter()
            .map(|a| {
                a.map_or("missing", |a| a.primary_customer_facing.as_str())
                    .to_owned()
            })
            .collect();
        if complete && fs.windows(2).all(|w| w[0] == w[1]) {
            f_stable += 1;
        } else {
            unstable.push(Unstable {
                job: job.clone(),
                field: "function",
                answers: fs,
            });
        }
        if complete && cs.windows(2).all(|w| w[0] == w[1]) {
            c_stable += 1;
        } else {
            unstable.push(Unstable {
                job: job.clone(),
                field: "primary_customer_facing",
                answers: cs,
            });
        }
    }
    let pairs = |same: &dyn Fn(&Classification, &Classification) -> bool| {
        let mut out = Vec::new();
        for i in 0..runs.len() {
            for j in i + 1..runs.len() {
                let hits = annotations
                    .iter()
                    .filter(|(job, _)| match (runs[i].get(job), runs[j].get(job)) {
                        (Some(a), Some(b)) => same(a, b),
                        _ => false,
                    })
                    .count();
                out.push((i + 1, j + 1, ratio(hits, n)));
            }
        }
        out
    };
    let function_pairs = pairs(&|a, b| a.function == b.function);
    let customer_facing_pairs =
        pairs(&|a, b| a.primary_customer_facing == b.primary_customer_facing);

    let mut systematic = Vec::new();
    let mut by_class: BTreeMap<&str, std::collections::BTreeSet<&str>> = BTreeMap::new();
    for s in &scores {
        for (class, jobs) in &s.non_ic_read_as_ic {
            by_class
                .entry(class.as_str())
                .or_default()
                .extend(jobs.iter().map(String::as_str));
        }
    }
    for (class, jobs) in &by_class {
        if jobs.len() >= 2 {
            systematic.push(format!("{class}: {} jobs read as IC", jobs.len()));
        }
    }
    for (job, count) in &ic_runs {
        if *count >= 2 {
            systematic.push(format!("{job}: read as IC in {count} runs"));
        }
    }

    let function_stable = ratio(f_stable, n);
    let customer_facing_stable = ratio(c_stable, n);
    let every = |p: &dyn Fn(&RunScore) -> bool| !scores.is_empty() && scores.iter().all(p);
    let gates = vec![
        (
            "function agreement >= 95% in every run",
            every(&|s| s.function.rate() >= bars::FUNCTION_AGREEMENT),
        ),
        (
            "every wrong-function job kept out of software_engineering_ic in every run",
            every(&|s| s.wrong_function_excluded.all()),
        ),
        (
            "no non-engineering or customer class systematically read as software_engineering_ic",
            systematic.is_empty(),
        ),
        (
            "every ordinary engineering job read as software_engineering_ic in every run",
            every(&|s| s.ordinary_engineering_kept.all()),
        ),
        (
            "customer-facing agreement >= 90% in every run",
            every(&|s| s.customer_facing.rate() >= bars::CUSTOMER_FACING_AGREEMENT),
        ),
        (
            "every customer role read as customer-facing yes in every run",
            every(&|s| s.customer_roles_yes.all()),
        ),
        (
            "function identical across all three runs for >= 98% of jobs",
            runs.len() >= 3 && function_stable.rate() >= bars::FUNCTION_STABILITY,
        ),
        (
            "customer-facing identical across all three runs for >= 95% of jobs",
            runs.len() >= 3 && customer_facing_stable.rate() >= bars::CUSTOMER_FACING_STABILITY,
        ),
    ];
    let pass = gates.iter().all(|(_, ok)| *ok);
    Evaluation {
        runs: scores,
        function_stable,
        customer_facing_stable,
        function_pairs,
        customer_facing_pairs,
        unstable,
        systematic_non_ic_as_ic: systematic,
        gates,
        pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> JobInput {
        JobInput::build(&PostingFields {
            title: "Senior Solutions Architect",
            department: Some("Sales"),
            team: Some("Solutions"),
            location: Some("Remote - US"),
            workplace: Some("remote"),
            description: Some(
                "ABOUT ACME\nAcme raised $500M and won Best Startup.\n\nABOUT THE ROLE\n\
                 Partner with customers to design their Kubernetes deployments.\n\
                 Lead technical evaluations with prospects.\n\n\
                 BENEFITS\nDental and vision insurance.\n",
            ),
        })
    }

    fn answer(function: &str, quote: &str, cf: &str) -> String {
        json!({
            "function_evidence": [quote],
            "function": function,
            "customer_facing_evidence": ["Lead technical evaluations with prospects"],
            "primary_customer_facing": cf,
        })
        .to_string()
    }

    #[test]
    fn only_the_job_is_sent() {
        let text = render(&input());
        assert!(text.contains("Partner with customers"));
        assert!(text.contains("Sales · Solutions"));
        for absent in ["$500M", "Best Startup", "Dental", "Remote - US"] {
            assert!(!text.contains(absent), "{absent}");
        }
    }

    #[test]
    fn the_schema_and_prompt_ask_for_nothing_else() {
        let s = schema().to_string();
        for word in [
            "fit",
            "depth",
            "special",
            "seniority",
            "score",
            "confidence",
            "candidate",
        ] {
            assert!(!s.contains(word), "{word}");
        }
        for word in ["candidate", "résumé", "salary", "authorization"] {
            assert!(!SYSTEM_PROMPT.contains(word), "{word}");
        }
        let required = schema()["required"].as_array().unwrap().len();
        assert_eq!(required, 4);
    }

    #[test]
    fn quotes_must_come_from_what_was_sent() {
        let i = input();
        let ok = parse(
            &answer(
                "solutions_or_customer_engineering",
                "Partner with customers to design their Kubernetes deployments",
                "yes",
            ),
            &i,
        )
        .unwrap();
        assert_eq!(ok.valid_quotes, 2);
        assert!(ok.unsupported.is_empty());
        let company = parse(
            &answer("software_engineering_ic", "raised $500M", "yes"),
            &i,
        )
        .unwrap();
        assert_eq!(company.invalid_quotes.len(), 1);
        assert_eq!(company.unsupported, vec!["function".to_owned()]);
        assert!(parse("not json", &i).is_err());
        assert!(parse(&answer("strong_fit", "x", "yes"), &i).is_err());
        let mut extra: Value =
            serde_json::from_str(&answer("research", "Lead technical evaluations", "no")).unwrap();
        extra["seniority"] = json!("senior");
        assert!(parse(&extra.to_string(), &i).is_err());
    }

    fn c(function: Function, cf: CustomerFacing) -> Classification {
        Classification {
            function_evidence: vec![],
            function,
            customer_facing_evidence: vec![],
            primary_customer_facing: cf,
        }
    }

    #[test]
    fn gates_need_every_run_and_every_critical_case() {
        use CustomerFacing::{No, Yes};
        use Function::{SoftwareEngineeringIc as Ic, SolutionsOrCustomerEngineering as Sol};
        let annotations = vec![
            (
                "a".to_owned(),
                Annotation {
                    function: Ic,
                    function_also: vec![],
                    primary_customer_facing: No,
                    customer_facing_also: vec![],
                    critical: vec![Critical::OrdinaryEngineering],
                },
            ),
            (
                "b".to_owned(),
                Annotation {
                    function: Sol,
                    function_also: vec![],
                    primary_customer_facing: Yes,
                    customer_facing_also: vec![],
                    critical: vec![Critical::WrongFunction, Critical::CustomerRole],
                },
            ),
        ];
        let good: BTreeMap<String, Classification> =
            [("a".to_owned(), c(Ic, No)), ("b".to_owned(), c(Sol, Yes))].into();
        let e = evaluate(&annotations, &[good.clone(), good.clone(), good.clone()]);
        assert!(e.pass, "{:?}", e.gates);
        // One run reading the solutions job as IC fails accuracy, the
        // wrong-function gate and stability, but isn't "systematic".
        let mut bad = good.clone();
        bad.insert("b".to_owned(), c(Ic, Yes));
        let e = evaluate(&annotations, &[good.clone(), bad.clone(), good.clone()]);
        assert!(!e.pass);
        assert_eq!(e.runs[1].wrong_function_excluded, ratio(0, 1));
        assert_eq!(e.function_stable, ratio(1, 2));
        assert!(e.systematic_non_ic_as_ic.is_empty());
        // Twice is systematic.
        let e = evaluate(&annotations, &[good.clone(), bad.clone(), bad]);
        assert_eq!(e.systematic_non_ic_as_ic.len(), 1);
        // Two runs can't pass the three-run stability bar.
        assert!(!evaluate(&annotations, &[good.clone(), good]).pass);
    }
}
