//! The fresh-snapshot function-only experiment
//! (`docs/job-function-only-fresh-snapshot-experiment.md`): the annotations
//! are well formed and disjoint from the BRU-325 judged set, and the three
//! recorded runs re-score offline to the recorded verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use jobhunt_eval::job_function::{
    self, Annotation, Classification, Critical, CustomerFacing, Function, Ratio, Validated,
};
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

#[derive(Deserialize)]
struct Judged {
    job: String,
}

#[derive(Deserialize)]
struct Bru325 {
    jobs: Vec<Judged>,
}

fn annotations() -> Vec<(String, Annotation)> {
    let a: Annotations = read("fresh-2026-10-02-job-functions.json");
    a.jobs.into_iter().map(|j| (j.job, j.annotation)).collect()
}

#[test]
fn annotations_are_fresh_and_consistent() {
    let a = annotations();
    assert_eq!(a.len(), 95);
    let ids: BTreeSet<&str> = a.iter().map(|(j, _)| j.as_str()).collect();
    assert_eq!(ids.len(), a.len(), "a posting is annotated twice");
    let old: Bru325 = read("bru325-2026-10-01.json");
    for j in &old.jobs {
        assert!(
            !ids.contains(j.job.as_str()),
            "{} was judged in BRU-325",
            j.job
        );
    }
    let ic = Function::SoftwareEngineeringIc;
    for (job, x) in &a {
        if x.critical.contains(&Critical::WrongFunction) {
            assert!(!x.function_ok(ic), "{job}: wrong-function but IC accepted");
        }
        if x.critical.contains(&Critical::OrdinaryEngineering) {
            assert_eq!(x.function, ic, "{job}");
            assert!(x.function_also.is_empty(), "{job}");
        }
        if x.critical.contains(&Critical::CustomerRole) {
            assert_eq!(x.primary_customer_facing, CustomerFacing::Yes, "{job}");
        }
    }
}

#[derive(Deserialize)]
struct Answered {
    job: String,
    result: Validated,
}

#[derive(Deserialize)]
struct Run {
    prompt_digest: String,
    model: String,
    jobs: Vec<Answered>,
}

#[derive(Deserialize)]
struct Runs {
    runs: Vec<Run>,
}

fn runs() -> Vec<BTreeMap<String, Classification>> {
    let file: Runs = read("fresh-2026-10-02-function-runs.json");
    assert_eq!(file.runs.len(), 3);
    file.runs
        .into_iter()
        .map(|run| {
            // Every run sent today's prompt and schema.
            assert_eq!(run.prompt_digest, job_function::prompt_digest());
            assert_eq!(run.model, "gpt-6-luna");
            run.jobs
                .into_iter()
                .map(|j| (j.job, j.result.classification))
                .collect()
        })
        .collect()
}

fn ratio(hits: usize, of: usize) -> Ratio {
    Ratio { hits, of }
}

#[test]
fn recorded_runs_rescore_to_the_recorded_verdict() {
    let e = job_function::evaluate(&annotations(), &runs());
    let function: Vec<Ratio> = e.runs.iter().map(|r| r.function).collect();
    assert_eq!(function, [ratio(94, 95), ratio(95, 95), ratio(94, 95)]);
    let customer: Vec<Ratio> = e.runs.iter().map(|r| r.customer_facing).collect();
    assert_eq!(customer, [ratio(91, 95), ratio(90, 95), ratio(89, 95)]);
    for r in &e.runs {
        assert_eq!(r.wrong_function_excluded, ratio(46, 46));
        assert_eq!(r.ordinary_engineering_kept, ratio(33, 33));
        assert_eq!(r.customer_roles_yes, ratio(15, 15));
        assert!(r.non_ic_read_as_ic.is_empty());
    }
    assert_eq!(e.function_stable, ratio(91, 95));
    assert_eq!(e.customer_facing_stable, ratio(92, 95));
    // Every accuracy gate passes; function stability does not.
    let failed: Vec<&str> = e
        .gates
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(g, _)| *g)
        .collect();
    assert_eq!(
        failed,
        ["function identical across all three runs for >= 98% of jobs"]
    );
    assert!(!e.pass);
}
