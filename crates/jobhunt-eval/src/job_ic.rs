//! A binary job classifier with three votes: is this a software
//! engineering individual-contributor job? The follow-up to
//! [`job_function`](crate::job_function), whose accuracy held on a fresh
//! snapshot but whose stability missed its bar on genuinely mixed postings
//! (`docs/job-ic-vote-experiment.md`). Not wired into ranking.
//!
//! One question, three answers: [`Answer::SoftwareEngineeringIc`],
//! [`Answer::NotSoftwareEngineeringIc`], or [`Answer::Unclear`], which a
//! mixed posting must get. Short verbatim quotes come first. No function
//! taxonomy, depth, specialty, seniority, customer contact, fit or score:
//! the schema has nowhere to put them.
//!
//! * [`render`], [`SYSTEM_PROMPT`] and [`schema`]: what is sent (the same
//!   job-only input as [`job_function`](crate::job_function)).
//! * [`parse`]: an answer validated against what was sent.
//! * [`vote`]: three answers into one.
//! * [`Annotation`] and [`evaluate`]: human labels and the pre-registered
//!   gates, scored on voted answers of independent ensembles.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use crate::job_class::{JobInput, PostingFields};
use crate::job_class::{MAX_QUOTES, MIN_QUOTE_CHARS, quote_key};
pub use crate::job_function::{Ratio, render};

/// Revision of the prompt, schema and input. Part of every cache key.
pub const CLASSIFIER_VERSION: &str = "job-ic/1";

/// Independent answers per posting version.
pub const VOTES: usize = 3;

/// Whether a job is a software engineering IC job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    SoftwareEngineeringIc,
    NotSoftwareEngineeringIc,
    /// Mixed, or the posting doesn't say enough.
    Unclear,
}

impl Answer {
    pub const ALL: &'static [Self] = &[
        Self::SoftwareEngineeringIc,
        Self::NotSoftwareEngineeringIc,
        Self::Unclear,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SoftwareEngineeringIc => "software_engineering_ic",
            Self::NotSoftwareEngineeringIc => "not_software_engineering_ic",
            Self::Unclear => "unclear",
        }
    }

    /// What a Today gate would see: an engineering IC job, or anything
    /// else (not one, or unclear: both stay out of Today).
    pub fn is_ic(self) -> bool {
        self == Self::SoftwareEngineeringIc
    }
}

/// The fixed instructions. Committed before the evaluation sample was
/// drawn; names job families, never titles from any fixture.
pub const SYSTEM_PROMPT: &str = "You read one job posting and answer one question: is it a \
software engineering individual-contributor job? You never evaluate any applicant: no fit, no \
qualifications, no recommendation.\n\n\
Answers:\n\
- software_engineering_ic: the person hired is primarily expected to design, build and ship \
software or systems hands-on, as an individual contributor, for the employer's own product, \
platform or infrastructure (for example backend, frontend, mobile, full stack, platform, \
infrastructure, reliability, security, data or machine learning engineering, testing, tooling, \
embedded software). A tech lead who still builds is an individual contributor.\n\
- not_software_engineering_ic: the primary function is something else: managing people; \
technical work for or with particular customers or prospects (solutions, pre-sales, field or \
forward-deployed engineering, implementation, integration, consulting, professional services, \
support, customer success); developer relations or community; creating or delivering training or \
courses; building or running sales, marketing or revenue systems; product or program management; \
research whose output is findings, models or papers rather than shipped software; or any job \
that isn't software (sales, marketing, design, analysis, finance, legal, people, operations, \
internal IT, hardware, mechanical or electrical engineering).\n\
- unclear: the job is mixed, with a substantial part of its core responsibilities in software \
engineering and a substantial part in another function, or the posting doesn't say enough to \
tell.\n\n\
Before the answer, give one to three short quotes copied exactly from the title, the department \
and team, or the posting text (a few words to one clause each, no ellipses) that show what the \
person will primarily do. Give no quotes only when the posting says too little.\n\n\
How to read a posting:\n\
- The title, the department and team, and the core responsibilities outrank the requirements. A \
requirement or a technology list never decides the answer.\n\
- Technology words don't make a job software engineering. A job that teaches, sells, supports, \
implements, integrates or deploys a technical product for customers is not \
software_engineering_ic, however deep the technology.\n\
- \"Engineer\", \"Architect\" or \"Developer\" in a title doesn't decide the answer, and neither \
does a team or product name such as \"Platform\".\n\
- Specialization, seniority and difficulty don't matter: a narrow, deep, junior or senior \
engineering job is still software_engineering_ic.\n\
- Occasional customer calls, user research, feedback from users, on-call duty or open-source \
community contact don't change an engineering job's answer.\n\
- When the core responsibilities are split between software engineering and another function, \
answer unclear. Don't pick a side for a mixed job.\n\
- Ignore company descriptions, mission, funding, growth, awards, founder biographies, culture, \
benefits and other employer marketing: never answer from them and never quote them.";

/// The prompt and schema, digested: runs with the same digest sent the
/// same instructions.
pub fn prompt_digest() -> String {
    jobhunt_core::StableId::derive(
        "narrow.eval.job_ic.prompt",
        &[CLASSIFIER_VERSION, SYSTEM_PROMPT, &schema().to_string()],
    )
    .to_string()
}

/// The JSON schema of an answer (strict: every field required, nothing
/// else allowed). Evidence comes first, so the model quotes before it
/// decides.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["evidence", "answer"],
        "properties": {
            "evidence": {"type": "array", "items": {"type": "string"}},
            "answer": {
                "type": "string",
                "enum": Answer::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            },
        }
    })
}

/// One answer, as the schema shapes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub evidence: Vec<String>,
    pub answer: Answer,
}

/// An answer checked against what was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validated {
    pub classification: Classification,
    /// Quotes verbatim in what was sent (at most three).
    pub valid_quotes: usize,
    /// Quotes not found in what was sent.
    pub invalid_quotes: Vec<String>,
    /// A definite answer left without a valid quote (never re-labeled).
    pub unsupported: bool,
}

/// Validates an answer: malformed JSON, unknown fields or values reject it
/// whole; quotes not in what was sent are dropped and counted.
pub fn parse(text: &str, input: &JobInput) -> Result<Validated, String> {
    let mut c: Classification =
        serde_json::from_str(text.trim()).map_err(|e| format!("not the expected JSON: {e}"))?;
    let quotable = input.quotable();
    let mut kept = Vec::new();
    let mut invalid = Vec::new();
    for q in c.evidence.drain(..) {
        let key = quote_key(&q);
        if key.chars().count() >= MIN_QUOTE_CHARS && quotable.contains(&key) {
            if kept.len() < MAX_QUOTES {
                kept.push(q.trim().to_owned());
            }
        } else {
            invalid.push(q);
        }
    }
    c.evidence = kept;
    Ok(Validated {
        valid_quotes: c.evidence.len(),
        unsupported: c.answer != Answer::Unclear && c.evidence.is_empty(),
        classification: c,
        invalid_quotes: invalid,
    })
}

/// Three answers into one: any `unclear` makes it unclear (the posting
/// reads as mixed to at least one reading); otherwise the majority. A
/// missing answer counts as unclear.
pub fn vote(answers: &[Option<Answer>]) -> Answer {
    if answers.len() != VOTES
        || answers
            .iter()
            .any(|a| matches!(a, None | Some(Answer::Unclear)))
    {
        return Answer::Unclear;
    }
    let ic = answers
        .iter()
        .filter(|a| **a == Some(Answer::SoftwareEngineeringIc))
        .count();
    if ic * 2 > VOTES {
        Answer::SoftwareEngineeringIc
    } else {
        Answer::NotSoftwareEngineeringIc
    }
}

/// Why a job is a critical case, decided at annotation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Critical {
    /// Clearly not a software engineering IC job, though its title or
    /// technology words could pass for one. Its voted answer must never be
    /// `software_engineering_ic`.
    WrongFunction,
    /// A clearly ordinary software engineering IC job. Its voted answer
    /// must always be `software_engineering_ic`.
    OrdinaryEngineering,
}

/// A human label for one posting. `also` lists other answers accepted for
/// a borderline posting (strict agreement ignores them). A posting the
/// annotator reads as split between engineering and another function is
/// labeled `unclear` (mixed), with the side or sides it could fall on in
/// `also`; how many mixed postings are voted `unclear` is reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub answer: Answer,
    #[serde(default)]
    pub also: Vec<Answer>,
    #[serde(default)]
    pub critical: Vec<Critical>,
}

impl Annotation {
    pub fn ok(&self, a: Answer) -> bool {
        a == self.answer || self.also.contains(&a)
    }

    /// Annotated as mixed: expected unclear.
    pub fn mixed(&self) -> bool {
        self.answer == Answer::Unclear
    }
}

/// The bars, fixed before any model output existed.
pub mod bars {
    /// Voted-answer agreement with the annotations, in every ensemble.
    pub const AGREEMENT: f64 = 0.95;
    /// Postings whose voted IC-or-not decision is identical in every
    /// ensemble.
    pub const STABILITY: f64 = 0.98;
}

fn ratio(hits: usize, of: usize) -> Ratio {
    Ratio { hits, of }
}

/// One ensemble's voted answers scored against the annotations.
#[derive(Debug, Clone, Serialize)]
pub struct EnsembleScore {
    pub agreement: Ratio,
    pub agreement_strict: Ratio,
    pub wrong_function_excluded: Ratio,
    pub ordinary_engineering_kept: Ratio,
    /// Mixed postings voted unclear.
    pub mixed_unclear: Ratio,
    /// Postings whose annotation rules out IC but were voted IC.
    pub non_ic_voted_ic: Vec<String>,
    /// Postings annotated IC (and not accepting anything else) voted
    /// otherwise.
    pub ic_lost: Vec<String>,
}

/// Every gate, scored.
#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub ensembles: Vec<EnsembleScore>,
    /// Postings whose voted IC-or-not decision is identical in every
    /// ensemble (the gate).
    pub decision_stable: Ratio,
    /// Postings whose voted three-way answer is identical in every
    /// ensemble (reported).
    pub answer_stable: Ratio,
    /// Posting ensembles whose three votes were identical (reported: how
    /// often a lone reading disagrees).
    pub unanimous_ensembles: Ratio,
    pub unstable: Vec<(String, Vec<String>)>,
    pub gates: Vec<(&'static str, bool)>,
    pub pass: bool,
}

/// Scores ensembles (each a map from job id to its [`VOTES`] answers)
/// against the annotations and checks every pre-registered gate. A job
/// missing from an ensemble is voted unclear.
pub fn evaluate(
    annotations: &[(String, Annotation)],
    ensembles: &[BTreeMap<String, Vec<Option<Answer>>>],
) -> Evaluation {
    let n = annotations.len();
    let voted = |e: &BTreeMap<String, Vec<Option<Answer>>>, job: &str| {
        e.get(job).map_or(Answer::Unclear, |v| vote(v))
    };
    let mut scores = Vec::new();
    let (mut unanimous, mut ensembles_seen) = (0, 0);
    for e in ensembles {
        let (mut agree, mut strict, mut wf, mut wf_n, mut oe, mut oe_n, mut mu, mut mu_n) =
            (0, 0, 0, 0, 0, 0, 0, 0);
        let mut non_ic = Vec::new();
        let mut lost = Vec::new();
        for (job, a) in annotations {
            let v = voted(e, job);
            if let Some(votes) = e.get(job) {
                ensembles_seen += 1;
                unanimous += usize::from(votes.windows(2).all(|w| w[0] == w[1]));
            }
            agree += usize::from(a.ok(v));
            strict += usize::from(v == a.answer);
            if a.critical.contains(&Critical::WrongFunction) {
                wf_n += 1;
                wf += usize::from(!v.is_ic());
            }
            if a.critical.contains(&Critical::OrdinaryEngineering) {
                oe_n += 1;
                oe += usize::from(v.is_ic());
            }
            if a.mixed() {
                mu_n += 1;
                mu += usize::from(v == Answer::Unclear);
            }
            if v.is_ic() && !a.ok(Answer::SoftwareEngineeringIc) {
                non_ic.push(job.clone());
            }
            if a.answer.is_ic() && a.also.is_empty() && !v.is_ic() {
                lost.push(job.clone());
            }
        }
        scores.push(EnsembleScore {
            agreement: ratio(agree, n),
            agreement_strict: ratio(strict, n),
            wrong_function_excluded: ratio(wf, wf_n),
            ordinary_engineering_kept: ratio(oe, oe_n),
            mixed_unclear: ratio(mu, mu_n),
            non_ic_voted_ic: non_ic,
            ic_lost: lost,
        });
    }
    let (mut decision, mut answer) = (0, 0);
    let mut unstable = Vec::new();
    for (job, _) in annotations {
        let vs: Vec<Answer> = ensembles.iter().map(|e| voted(e, job)).collect();
        let same_decision = vs.windows(2).all(|w| w[0].is_ic() == w[1].is_ic());
        let same_answer = vs.windows(2).all(|w| w[0] == w[1]);
        decision += usize::from(same_decision);
        answer += usize::from(same_answer);
        if !same_answer {
            unstable.push((
                job.clone(),
                vs.iter().map(|v| v.as_str().to_owned()).collect(),
            ));
        }
    }
    let decision_stable = ratio(decision, n);
    let every = |p: &dyn Fn(&EnsembleScore) -> bool| !scores.is_empty() && scores.iter().all(p);
    let gates = vec![
        (
            "voted agreement >= 95% in every ensemble",
            every(&|s| s.agreement.rate() >= bars::AGREEMENT),
        ),
        (
            "no posting whose annotation rules out IC is voted IC, in any ensemble",
            every(&|s| s.non_ic_voted_ic.is_empty()),
        ),
        (
            "every wrong-function job voted out of IC in every ensemble",
            every(&|s| s.wrong_function_excluded.all()),
        ),
        (
            "every ordinary engineering job voted IC in every ensemble",
            every(&|s| s.ordinary_engineering_kept.all()),
        ),
        (
            "voted IC-or-not decision identical across all three ensembles for >= 98% of jobs",
            ensembles.len() >= 3 && decision_stable.rate() >= bars::STABILITY,
        ),
    ];
    let pass = gates.iter().all(|(_, ok)| *ok);
    Evaluation {
        ensembles: scores,
        decision_stable,
        answer_stable: ratio(answer, n),
        unanimous_ensembles: ratio(unanimous, ensembles_seen),
        unstable,
        gates,
        pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Answer::{NotSoftwareEngineeringIc as Not, SoftwareEngineeringIc as Ic, Unclear};

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
            "customer",
            "function",
        ] {
            assert!(!s.contains(word), "{word}");
        }
        for word in ["candidate", "résumé", "salary", "authorization"] {
            assert!(!SYSTEM_PROMPT.contains(word), "{word}");
        }
        assert_eq!(schema()["required"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn quotes_must_come_from_what_was_sent() {
        let i = input();
        let answer = |a: &str, q: &str| json!({"evidence": [q], "answer": a}).to_string();
        let ok = parse(
            &answer(
                "not_software_engineering_ic",
                "Partner with customers to design their Kubernetes deployments",
            ),
            &i,
        )
        .unwrap();
        assert_eq!(ok.valid_quotes, 1);
        assert!(!ok.unsupported);
        let company = parse(&answer("software_engineering_ic", "raised $500M"), &i).unwrap();
        assert_eq!(company.invalid_quotes.len(), 1);
        assert!(company.unsupported);
        assert!(parse("not json", &i).is_err());
        assert!(parse(&answer("strong_fit", "x"), &i).is_err());
        let mut extra: Value =
            serde_json::from_str(&answer("unclear", "Lead technical evaluations")).unwrap();
        extra["seniority"] = json!("senior");
        assert!(parse(&extra.to_string(), &i).is_err());
    }

    #[test]
    fn any_unclear_vote_makes_the_answer_unclear() {
        assert_eq!(vote(&[Some(Ic), Some(Ic), Some(Ic)]), Ic);
        assert_eq!(vote(&[Some(Ic), Some(Not), Some(Ic)]), Ic);
        assert_eq!(vote(&[Some(Not), Some(Not), Some(Ic)]), Not);
        assert_eq!(vote(&[Some(Ic), Some(Ic), Some(Unclear)]), Unclear);
        assert_eq!(vote(&[Some(Ic), Some(Ic), None]), Unclear);
        assert_eq!(vote(&[Some(Ic), Some(Ic)]), Unclear);
    }

    #[test]
    fn gates_need_every_ensemble_and_every_critical_case() {
        let a = |answer, critical| Annotation {
            answer,
            also: vec![],
            critical,
        };
        let annotations = vec![
            ("a".to_owned(), a(Ic, vec![Critical::OrdinaryEngineering])),
            ("b".to_owned(), a(Not, vec![Critical::WrongFunction])),
            ("m".to_owned(), a(Unclear, vec![])),
        ];
        let e = |a: [Answer; 3], b: [Answer; 3], m: [Answer; 3]| -> BTreeMap<_, _> {
            [
                ("a".to_owned(), a.map(Some).to_vec()),
                ("b".to_owned(), b.map(Some).to_vec()),
                ("m".to_owned(), m.map(Some).to_vec()),
            ]
            .into()
        };
        let good = e([Ic; 3], [Not; 3], [Ic, Unclear, Ic]);
        let r = evaluate(&annotations, &[good.clone(), good.clone(), good.clone()]);
        assert!(r.pass, "{:?}", r.gates);
        assert_eq!(r.ensembles[0].mixed_unclear, ratio(1, 1));
        // One lone wrong vote is outvoted.
        let outvoted = e([Ic, Not, Ic], [Not, Ic, Not], [Unclear; 3]);
        assert!(evaluate(&annotations, &[good.clone(), outvoted, good.clone()]).pass);
        // A wrong-function job voted IC once fails.
        let bad = e([Ic; 3], [Ic, Ic, Not], [Unclear; 3]);
        let r = evaluate(&annotations, &[good.clone(), bad, good.clone()]);
        assert!(!r.pass);
        assert_eq!(r.ensembles[1].non_ic_voted_ic, vec!["b".to_owned()]);
        // An ordinary engineering job voted unclear is lost.
        let lost = e([Ic, Unclear, Ic], [Not; 3], [Unclear; 3]);
        let r = evaluate(&annotations, &[good.clone(), lost, good.clone()]);
        assert!(!r.pass);
        assert_eq!(r.ensembles[1].ic_lost, vec!["a".to_owned()]);
        // Two ensembles can't pass the stability bar.
        assert!(!evaluate(&annotations, &[good.clone(), good]).pass);
    }
}
