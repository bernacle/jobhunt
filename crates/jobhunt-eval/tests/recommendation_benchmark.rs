//! The recommendation benchmark's integrity, on the fixtures shipped with
//! the crate, and its recorded baseline.
//!
//! Integrity must always pass: the fixtures parse, every judgment is
//! consistent with its reasons, the golden, positive, contrastive and
//! compensation cases the benchmark promises are there, and the evaluator
//! runs deterministically on them.
//!
//! The current ranker's *quality* is recorded, not asserted: cases failing
//! their judgment don't fail this suite. `baseline/recommendation.md` is
//! the report of the ranker as it is; `baseline_is_current` fails when the
//! ranker's behavior on the benchmark changes without the baseline being
//! regenerated, so every ranking change shows its effect in review:
//!
//! ```text
//! JOBHUNT_UPDATE_BENCHMARK=1 cargo test -p jobhunt-eval --test recommendation_benchmark
//! ```

use std::collections::BTreeSet;

use jobhunt_eval::fixture::Group;
use jobhunt_eval::report::{Options, render};
use jobhunt_eval::run::{FEED_LIMIT, Verdict};
use jobhunt_eval::{
    Case, Contradiction, Fit, Fixtures, Label, Metrics, Practicality, Reason, TodayExpectation,
    baseline_path, default_dir, run,
};

fn fixtures() -> Fixtures {
    Fixtures::load(&default_dir()).unwrap_or_else(|e| panic!("{e}"))
}

fn judgment<'a>(
    f: &'a Fixtures,
    candidate: &str,
    job: &str,
) -> &'a jobhunt_eval::fixture::Judgment {
    f.candidate(candidate)
        .unwrap_or_else(|| panic!("no candidate {candidate}"))
        .judgment
        .iter()
        .find(|j| j.job == job)
        .unwrap_or_else(|| panic!("{candidate} doesn't judge {job}"))
}

const SENIOR: &str = "senior-startup-generalist";

#[test]
fn fixtures_parse_and_are_consistent() {
    let f = fixtures();
    assert!(f.problems().is_empty(), "{:?}", f.problems());
    assert!(
        f.candidates.len() >= 4,
        "several archetypes, not one person"
    );
    for c in &f.candidates {
        assert!(
            c.judgment.len() >= 5,
            "{} judges too few jobs to measure",
            c.id
        );
    }
}

#[test]
fn every_candidate_is_built_through_the_production_profile_path() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    for c in &result.candidates {
        let r = &c.reading;
        assert!(r.location.is_some(), "{}: location read", c.id);
        assert!(r.level.is_some(), "{}: latest title read", c.id);
        assert!(!r.technologies.is_empty(), "{}: resume technologies", c.id);
        assert!(!r.stated.is_empty(), "{}: stated preferences", c.id);
    }
    let senior = result.candidates.iter().find(|c| c.id == SENIOR).unwrap();
    assert!(
        senior
            .reading
            .technologies
            .iter()
            .any(|t| t.starts_with("postgresql")),
        "the senior's PostgreSQL experience is what the Supabase case tests"
    );
}

#[test]
fn the_golden_failures_are_encoded_on_the_senior_candidate() {
    let f = fixtures();
    let golden: BTreeSet<&str> = f
        .jobs
        .values()
        .filter(|j| j.group == Group::Golden)
        .map(|j| j.id.as_str())
        .collect();
    assert_eq!(
        golden,
        BTreeSet::from([
            "airbnb-early-career",
            "anthropic-staff-privacy",
            "stripe-payments-us-remote",
            "supabase-orioledb",
        ])
    );
    for id in &golden {
        assert_eq!(f.jobs[*id].observed_for.as_deref(), Some(SENIOR));
    }
    // A: early career is a seniority No, whatever the backend overlap.
    let airbnb = judgment(&f, SENIOR, "airbnb-early-career");
    assert_eq!(airbnb.fit, Fit::No);
    assert!(airbnb.reasons.contains(&Reason::SeniorityMismatch));
    assert_eq!(airbnb.today(), TodayExpectation::Never);
    // B: the role may fit; geography makes it impossible.
    let stripe = judgment(&f, SENIOR, "stripe-payments-us-remote");
    assert_ne!(stripe.fit, Fit::No, "role fit is plausible");
    assert_eq!(stripe.practicality, Practicality::Impossible);
    assert_eq!(stripe.label(), Label::Impossible);
    // C: PostgreSQL use is not storage-engine depth.
    let supabase = judgment(&f, SENIOR, "supabase-orioledb");
    assert!(matches!(supabase.fit, Fit::No | Fit::Maybe));
    assert!(supabase.reasons.contains(&Reason::SpecializationTooDeep));
    // D: pay alone doesn't make a strong yes.
    let anthropic = judgment(&f, SENIOR, "anthropic-staff-privacy");
    assert_ne!(anthropic.fit, Fit::StrongYes);
    assert!(anthropic.reasons.contains(&Reason::CompensationGood));
    assert_ne!(anthropic.today(), TodayExpectation::Surface);
}

#[test]
fn fit_is_the_candidates_not_the_jobs() {
    let f = fixtures();
    // The same posting, opposite judgments for different archetypes.
    for (job, yes, no) in [
        ("airbnb-early-career", "early-career-generalist", SENIOR),
        ("supabase-orioledb", "database-internals-specialist", SENIOR),
        (
            "stratadb-storage-engine",
            "database-internals-specialist",
            SENIOR,
        ),
        ("helmsman-k8s-control-plane", "us-platform-engineer", SENIOR),
        ("orbitly-senior-engineer", SENIOR, "early-career-generalist"),
    ] {
        assert_eq!(
            judgment(&f, yes, job).fit,
            Fit::StrongYes,
            "{job} for {yes}"
        );
        assert_eq!(judgment(&f, no, job).fit, Fit::No, "{job} for {no}");
    }
    // Geography is the candidate's too: US-only is impossible from Brazil,
    // practical from Denver.
    assert_eq!(
        judgment(&f, SENIOR, "crestline-remote-us-canada").practicality,
        Practicality::Impossible
    );
    assert_eq!(
        judgment(&f, "us-platform-engineer", "crestline-remote-us-canada").practicality,
        Practicality::Valid
    );
}

#[test]
fn contrastive_pairs_have_both_sides_judged_apart() {
    let f = fixtures();
    let pairs: BTreeSet<&str> = f.jobs.values().filter_map(|j| j.pair.as_deref()).collect();
    assert_eq!(
        pairs,
        BTreeSet::from(["ai", "company-shape", "kubernetes", "postgres", "seniority"])
    );
    for pair in pairs {
        let sides: Vec<&str> = f
            .jobs
            .values()
            .filter(|j| j.pair.as_deref() == Some(pair))
            .map(|j| j.id.as_str())
            .collect();
        assert_eq!(sides.len(), 2, "{pair}");
        // The senior candidate judges both sides of every pair, one a
        // strong yes and the other a no.
        let labels: BTreeSet<Label> = sides
            .iter()
            .map(|job| judgment(&f, SENIOR, job).label())
            .collect();
        assert_eq!(
            labels,
            BTreeSet::from([Label::StrongYes, Label::No]),
            "{pair}"
        );
    }
    // Role-depth sides carry a role-depth contradiction; the seniority side
    // a seniority one; the enterprise side a company-shape one.
    let contradiction = |job: &str| -> Vec<Contradiction> {
        judgment(&f, SENIOR, job)
            .reasons
            .iter()
            .filter_map(|r| r.contradiction())
            .collect()
    };
    for job in [
        "stratadb-storage-engine",
        "helmsman-k8s-control-plane",
        "northstar-research-pretraining",
    ] {
        assert!(
            contradiction(job).contains(&Contradiction::RoleDepth),
            "{job}"
        );
    }
    assert!(contradiction("orbitly-new-grad").contains(&Contradiction::Seniority));
    assert!(
        contradiction("consolidated-enterprise-backend").contains(&Contradiction::CompanyShape)
    );
}

#[test]
fn compensation_is_a_practicality_not_fit() {
    let f = fixtures();
    // Unknown pay, strong fit: still expected in Today.
    let unknown = judgment(&f, SENIOR, "lanternfish-platform-no-pay");
    assert_eq!(unknown.fit, Fit::StrongYes);
    assert!(unknown.reasons.contains(&Reason::CompensationUnknown));
    assert_eq!(unknown.today(), TodayExpectation::Surface);
    // High pay, weak fit: not a strong yes.
    for job in ["halcyon-low-latency-cpp", "northstar-research-pretraining"] {
        let j = judgment(&f, SENIOR, job);
        assert!(j.reasons.contains(&Reason::CompensationGood), "{job}");
        assert_eq!(j.fit, Fit::No, "{job}");
    }
    // Below the floor: fit stays a strong yes; practicality carries it.
    let low = judgment(&f, SENIOR, "kestrel-staff-low-pay");
    assert_eq!(low.fit, Fit::StrongYes);
    assert_eq!(low.practicality, Practicality::Concern);
    assert_eq!(low.today(), TodayExpectation::Either);
    // A range for another location is unknown pay, not a match.
    let foreign = judgment(&f, SENIOR, "atlasfield-us-pay-range");
    assert!(
        foreign
            .reasons
            .contains(&Reason::CompensationRangeNotApplicable)
    );
    assert_eq!(foreign.practicality, Practicality::Unknown);
}

#[test]
fn the_benchmark_rewards_good_jobs_not_only_suppression() {
    let f = fixtures();
    for c in &f.candidates {
        let surface = c
            .judgment
            .iter()
            .filter(|j| j.today() == TodayExpectation::Surface)
            .count();
        assert!(
            surface >= 2,
            "{}: at least two jobs Today should find",
            c.id
        );
    }
    let positive = f
        .jobs
        .values()
        .filter(|j| j.group == Group::Positive)
        .count();
    assert!(positive >= 5);
}

/// Runs the benchmark, and the case of one candidate and one job.
fn case<'a>(result: &'a jobhunt_eval::Run, candidate: &str, job: &str) -> &'a Case {
    result
        .candidates
        .iter()
        .find(|c| c.id == candidate)
        .and_then(|c| c.cases.iter().find(|x| x.job == job))
        .unwrap_or_else(|| panic!("no case {candidate}/{job}"))
}

#[test]
fn today_is_captured_as_the_feed_selects_it() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    for c in &result.candidates {
        assert!(c.feed.len() <= FEED_LIMIT, "{}", c.id);
        let mut companies = BTreeSet::new();
        for job in &c.feed {
            let case = c.cases.iter().find(|x| &x.job == job).unwrap();
            assert!(
                case.observed.qualifies,
                "{}: the feed only shows qualifying jobs",
                c.id
            );
            assert!(case.observed.in_feed);
            assert!(
                companies.insert(case.company.to_lowercase()),
                "{}: one per company",
                c.id
            );
        }
        for case in &c.cases {
            // Excluded jobs never qualify; a qualifying job is at least
            // worth reviewing.
            if case.observed.gate.starts_with("excluded") {
                assert!(!case.observed.qualifies, "{}/{}", c.id, case.job);
            }
            if case.observed.qualifies {
                assert!(case.observed.tier >= jobhunt_ranking::Tier::WorthReviewing);
            }
        }
    }
    // Ineligible for the candidate: excluded, and so never on Today.
    let crest = case(&result, SENIOR, "crestline-remote-us-canada");
    assert!(
        crest.observed.gate.starts_with("excluded"),
        "{:?}",
        crest.observed.gate
    );
    assert!(!crest.surfaced());
    assert!(
        crest
            .observed
            .detected
            .contains(&Contradiction::Eligibility)
    );
}

#[test]
fn the_evaluator_classifies_golden_outcomes() {
    // Whatever the current ranker does, the evaluator must call a surfaced
    // golden No a false positive with its contradiction, and a held-back
    // one a pass.
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let airbnb = case(&result, SENIOR, "airbnb-early-career");
    let mut surfaced = airbnb.clone();
    surfaced.observed.qualifies = true;
    assert_eq!(surfaced.verdict(), Verdict::FalsePositive);
    assert_eq!(surfaced.missed(), [Contradiction::Seniority]);
    let mut held = airbnb.clone();
    held.observed.qualifies = false;
    assert_eq!(held.verdict(), Verdict::Pass);
    assert!(held.missed().is_empty());

    let anthropic = case(&result, SENIOR, "anthropic-staff-privacy");
    let mut surfaced = anthropic.clone();
    surfaced.observed.qualifies = true;
    assert_eq!(surfaced.verdict(), Verdict::MaybeInToday);

    let stripe = case(&result, SENIOR, "stripe-payments-us-remote");
    let mut surfaced = stripe.clone();
    surfaced.observed.qualifies = true;
    assert_eq!(surfaced.verdict(), Verdict::FalsePositive);
    assert!(surfaced.missed().contains(&Contradiction::Eligibility));

    let lanternfish = case(&result, SENIOR, "lanternfish-platform-no-pay");
    let mut missed = lanternfish.clone();
    missed.observed.qualifies = false;
    assert_eq!(missed.verdict(), Verdict::Missed);
    let kestrel = case(&result, SENIOR, "kestrel-staff-low-pay");
    for qualifies in [true, false] {
        let mut k = kestrel.clone();
        k.observed.qualifies = qualifies;
        assert_eq!(
            k.verdict(),
            Verdict::Pass,
            "a strong yes below the floor may go either way"
        );
    }
}

#[test]
fn metrics_add_up_across_candidates() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let all = Metrics::of(result.candidates.iter().flat_map(|c| &c.cases));
    let each: Vec<Metrics> = result
        .candidates
        .iter()
        .map(|c| Metrics::of(&c.cases))
        .collect();
    assert_eq!(all.judged, each.iter().map(|m| m.judged).sum::<usize>());
    assert_eq!(all.surfaced, each.iter().map(|m| m.surfaced).sum::<usize>());
    assert_eq!(all.failures, each.iter().map(|m| m.failures).sum::<usize>());
    assert_eq!(
        all.strict_precision.hits,
        each.iter().map(|m| m.strict_precision.hits).sum::<usize>()
    );
    assert!(all.strict_precision.hits <= all.relaxed_precision.hits);
    assert_eq!(
        all.relaxed_precision.hits + all.obvious_false_positives.hits,
        all.surfaced,
        "every surfaced job is acceptable or an obvious mismatch"
    );
}

#[test]
fn the_report_is_deterministic() {
    let f = fixtures();
    let first = render(&f, &run(&f).unwrap(), Options::default());
    let again = render(&fixtures(), &run(&fixtures()).unwrap(), Options::default());
    assert_eq!(first, again);
    for heading in [
        "## Summary, all candidates",
        "Strict Today precision",
        "Relaxed Today precision",
        "Obvious false positives",
        "## Golden cases",
        "## Contrastive pairs",
    ] {
        assert!(first.contains(heading), "missing {heading}");
    }
}

#[test]
fn baseline_is_current() {
    let f = fixtures();
    let report = render(&f, &run(&f).unwrap(), Options::default());
    let path = baseline_path();
    if std::env::var_os("JOBHUNT_UPDATE_BENCHMARK").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &report).unwrap();
        return;
    }
    let recorded = std::fs::read_to_string(&path).unwrap_or_default();
    if recorded != report {
        let diff: Vec<String> = recorded
            .lines()
            .zip(report.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .take(5)
            .map(|(i, (a, b))| format!("line {}:\n  recorded: {a}\n  now:      {b}", i + 1))
            .collect();
        panic!(
            "The recommendation benchmark's results changed ({} recorded lines, {} now).\n\
             If the ranking change is intended, regenerate the baseline and review its diff:\n  \
             JOBHUNT_UPDATE_BENCHMARK=1 cargo test -p jobhunt-eval --test recommendation_benchmark\n\n{}",
            recorded.lines().count(),
            report.lines().count(),
            diff.join("\n")
        );
    }
}
