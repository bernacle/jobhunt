//! The BRU-330 job-description annotations: they cover exactly the 50
//! judged BRU-325 postings, parse into [`Expected`], and keep their critical
//! cases consistent with the BRU-325 Fit judgments they sit beside (which
//! they never change).

use std::path::PathBuf;

use jobhunt_eval::job_class::{Expected, Function};
use serde::Deserialize;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/real-postings")
}

#[derive(Deserialize)]
struct Annotated {
    job: String,
    critical: Option<String>,
    bru325_fit: String,
    expected: Expected,
}

#[derive(Deserialize)]
struct Annotations {
    jobs: Vec<Annotated>,
}

#[derive(Deserialize)]
struct Judgment {
    fit: String,
}

#[derive(Deserialize)]
struct Judged {
    job: String,
    judgment: Judgment,
}

#[derive(Deserialize)]
struct Bru325 {
    jobs: Vec<Judged>,
}

fn read<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let text = std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn annotations_cover_the_judged_postings() {
    let a: Annotations = read("bru330-job-classes.json");
    let judged: Bru325 = read("bru325-2026-10-01.json");
    let ids: Vec<&str> = a.jobs.iter().map(|j| j.job.as_str()).collect();
    let judged_ids: Vec<&str> = judged.jobs.iter().map(|j| j.job.as_str()).collect();
    assert_eq!(ids, judged_ids);
    for (job, j) in a.jobs.iter().zip(&judged.jobs) {
        assert_eq!(job.bru325_fit, j.judgment.fit, "{}", job.job);
        let e = &job.expected;
        assert!(!e.function.is_empty(), "{}", job.job);
        assert!(!e.depth.is_empty(), "{}", job.job);
        assert!(!e.seniority.is_empty(), "{}", job.job);
        assert!(!e.customer_facing.is_empty(), "{}", job.job);
        let ic = e.function.contains(&Function::SoftwareEngineeringIc);
        assert_eq!(
            ic,
            !e.shapes.is_empty(),
            "{}: shapes only for IC jobs",
            job.job
        );
    }
}

#[test]
fn critical_cases_are_consistent() {
    let a: Annotations = read("bru330-job-classes.json");
    let count = |c: &str| {
        a.jobs
            .iter()
            .filter(|j| j.critical.as_deref() == Some(c))
            .count()
    };
    assert_eq!(count("strong_yes"), 10);
    assert_eq!(count("bad_function"), 8);
    assert!(count("deep_specialist") >= 6);
    for j in &a.jobs {
        let e = &j.expected;
        match j.critical.as_deref() {
            Some("strong_yes") => {
                assert_eq!(j.bru325_fit, "strong_yes");
                assert_eq!(
                    e.function,
                    vec![Function::SoftwareEngineeringIc],
                    "{}",
                    j.job
                );
            }
            Some("bad_function") => {
                assert!(
                    !e.function.contains(&Function::SoftwareEngineeringIc),
                    "{}",
                    j.job
                );
                assert_ne!(j.bru325_fit, "strong_yes");
            }
            Some("deep_specialist") => {
                assert!(
                    !e.depth.iter().any(|d| d.as_str() == "general"),
                    "{}",
                    j.job
                );
                assert_ne!(j.bru325_fit, "strong_yes");
            }
            _ => {}
        }
    }
}

#[derive(Deserialize)]
struct RunJob {
    job: String,
    result: jobhunt_eval::job_class::Validated,
}

#[derive(Deserialize)]
struct Run {
    jobs: Vec<RunJob>,
}

#[derive(Deserialize)]
struct Runs {
    runs: Vec<Run>,
}

/// The recorded BRU-330 runs, re-scored offline: the figures the experiment
/// report quotes (function agreement, specialization-depth agreement, and
/// the gate pair's stability) follow from the committed answers and
/// annotations.
#[test]
fn recorded_runs_score_as_reported() {
    use jobhunt_eval::job_class::{Field, Stability, agreement, compare};
    let a: Annotations = read("bru330-job-classes.json");
    let runs: Runs = read("bru330-classifier-runs.json");
    assert_eq!(runs.runs.len(), 3);
    let mut function = Vec::new();
    let mut depth = Vec::new();
    for run in &runs.runs {
        let ids: Vec<&str> = run.jobs.iter().map(|j| j.job.as_str()).collect();
        let expected: Vec<&str> = a.jobs.iter().map(|j| j.job.as_str()).collect();
        assert_eq!(ids, expected);
        let scores: Vec<_> = a
            .jobs
            .iter()
            .zip(&run.jobs)
            .map(|(e, r)| agreement(&e.expected, &r.result.classification))
            .collect();
        function.push(scores.iter().filter(|s| s.function).count());
        depth.push(scores.iter().filter(|s| s.depth).count());
    }
    assert_eq!(function, vec![50, 50, 50]);
    assert_eq!(depth, vec![45, 45, 46]);
    let (r1, r2) = (&runs.runs[0], &runs.runs[1]);
    let mut material = 0;
    let mut material_function = 0;
    for (x, y) in r1.jobs.iter().zip(&r2.jobs) {
        for (field, s) in compare(&x.result.classification, &y.result.classification) {
            if s == Stability::Material {
                material += 1;
                material_function += usize::from(field == Field::Function);
            }
        }
    }
    // 275 of 300 field comparisons without a material disagreement
    // (91.7%, below the 95% bar); function flips once.
    assert_eq!(material, 25);
    assert_eq!(material_function, 1);
}
