//! The binary, three-vote software-engineering-IC experiment
//! (`docs/job-ic-vote-experiment.md`): the annotations are well formed and
//! disjoint from every earlier judged set, and the recorded ensembles
//! re-score offline to the recorded verdicts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use jobhunt_eval::job_ic::{self, Annotation, Answer, Critical, Ratio};
use serde::Deserialize;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/real-postings")
}

fn read<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let text = std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[derive(Deserialize)]
struct Annotated {
    job: String,
    annotation: Annotation,
}

#[derive(Deserialize)]
struct Annotations {
    jobs: Vec<Annotated>,
}

fn annotations(name: &str) -> Vec<(String, Annotation)> {
    let a: Annotations = read(name);
    a.jobs.into_iter().map(|j| (j.job, j.annotation)).collect()
}

#[derive(Deserialize)]
struct Judged {
    job: String,
}

#[derive(Deserialize)]
struct Judgments {
    jobs: Vec<Judged>,
}

#[test]
fn annotations_are_fresh_and_consistent() {
    let a = annotations("fresh-2026-10-03-job-ic.json");
    assert_eq!(a.len(), 94);
    let ids: BTreeSet<&str> = a.iter().map(|(j, _)| j.as_str()).collect();
    assert_eq!(ids.len(), a.len(), "a posting is annotated twice");
    for earlier in [
        "bru325-2026-10-01.json",
        "fresh-2026-10-02-job-functions.json",
    ] {
        let old: Judgments = read(earlier);
        for j in &old.jobs {
            assert!(!ids.contains(j.job.as_str()), "{} was judged before", j.job);
        }
    }
    for (job, x) in &a {
        if x.critical.contains(&Critical::WrongFunction) {
            assert!(!x.ok(Answer::SoftwareEngineeringIc), "{job}");
        }
        if x.critical.contains(&Critical::OrdinaryEngineering) {
            assert_eq!(x.answer, Answer::SoftwareEngineeringIc, "{job}");
            assert!(x.also.is_empty(), "{job}");
        }
    }
}

#[derive(Deserialize)]
struct Voted {
    job: String,
    #[serde(default)]
    answers: Vec<Option<Answer>>,
    #[serde(default)]
    votes: Vec<Vote>,
    voted: Answer,
}

#[derive(Deserialize)]
struct Vote {
    answer: Option<Answer>,
}

#[derive(Deserialize)]
struct Ensemble {
    prompt_digest: String,
    model: String,
    jobs: Vec<Voted>,
}

#[derive(Deserialize)]
struct Regression {
    fresh_2026_10_02: Vec<Ensemble>,
    bru330: Vec<Ensemble>,
}

#[derive(Deserialize)]
struct Runs {
    phase1: Vec<Ensemble>,
    regression: Regression,
    phase2: Vec<Ensemble>,
}

type Answers = BTreeMap<String, Vec<Option<Answer>>>;

/// Each ensemble's answers, checked: today's prompt and schema, the
/// recorded model, three answers per posting, and a recorded voted answer
/// that the vote rule reproduces.
fn answers(ensembles: Vec<Ensemble>) -> Vec<Answers> {
    assert_eq!(ensembles.len(), 3);
    ensembles
        .into_iter()
        .map(|e| {
            assert_eq!(e.prompt_digest, job_ic::prompt_digest());
            assert_eq!(e.model, "gpt-6-luna");
            e.jobs
                .into_iter()
                .map(|j| {
                    let a: Vec<Option<Answer>> = if j.votes.is_empty() {
                        j.answers
                    } else {
                        j.votes.iter().map(|v| v.answer).collect()
                    };
                    assert_eq!(a.len(), job_ic::VOTES, "{}", j.job);
                    assert!(
                        a.iter().all(Option::is_some),
                        "{}: a vote is missing",
                        j.job
                    );
                    assert_eq!(job_ic::vote(&a), j.voted, "{}", j.job);
                    (j.job, a)
                })
                .collect()
        })
        .collect()
}

fn runs() -> Runs {
    read("fresh-2026-10-03-job-ic-runs.json")
}

fn ratio(hits: usize, of: usize) -> Ratio {
    Ratio { hits, of }
}

#[test]
fn phase1_rescores_to_the_recorded_verdict() {
    let e = job_ic::evaluate(
        &annotations("fresh-2026-10-03-job-ic.json"),
        &answers(runs().phase1),
    );
    let agreement: Vec<Ratio> = e.ensembles.iter().map(|s| s.agreement).collect();
    assert_eq!(agreement, [ratio(92, 94), ratio(92, 94), ratio(93, 94)]);
    for s in &e.ensembles {
        assert_eq!(s.wrong_function_excluded, ratio(44, 44));
        assert_eq!(s.ordinary_engineering_kept, ratio(36, 36));
        assert!(s.non_ic_voted_ic.is_empty());
        assert!(s.ic_lost.is_empty());
    }
    assert_eq!(e.decision_stable, ratio(93, 94));
    assert_eq!(e.answer_stable, ratio(89, 94));
    assert!(e.pass, "{:?}", e.gates);
}

#[test]
fn regression_sets_rescore_to_the_recorded_verdict() {
    let r = runs().regression;
    let e = job_ic::evaluate(
        &annotations("fresh-2026-10-02-job-ic-derived.json"),
        &answers(r.fresh_2026_10_02),
    );
    assert_eq!(e.decision_stable, ratio(94, 95));
    assert!(e.pass, "{:?}", e.gates);
    let e = job_ic::evaluate(
        &annotations("bru330-job-ic-derived.json"),
        &answers(r.bru330),
    );
    assert_eq!(e.decision_stable, ratio(50, 50));
    assert!(e.pass, "{:?}", e.gates);
}

#[test]
fn phase2_decisions_are_stable() {
    let ensembles = answers(runs().phase2);
    let jobs: Vec<&String> = ensembles[0].keys().collect();
    assert_eq!(jobs.len(), 353);
    let stable = jobs
        .iter()
        .filter(|j| {
            let d: Vec<bool> = ensembles
                .iter()
                .map(|e| job_ic::vote(&e[**j]).is_ic())
                .collect();
            d.windows(2).all(|w| w[0] == w[1])
        })
        .count();
    assert_eq!(stable, 349);
}
