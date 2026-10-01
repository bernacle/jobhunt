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
            // Excluded jobs never qualify; a qualifying job is a strong fit.
            if case.observed.gate.starts_with("excluded") {
                assert!(!case.observed.qualifies, "{}/{}", c.id, case.job);
            }
            if case.observed.qualifies {
                assert_eq!(case.observed.tier, jobhunt_ranking::Tier::StrongFit);
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

#[test]
fn taste_fixtures_load_into_the_built_profile() {
    use jobhunt_profile::taste::{TasteOrigin, TasteReview, compose};
    let f = fixtures();
    for c in &f.candidates {
        let taste = c
            .taste
            .as_ref()
            .unwrap_or_else(|| panic!("{}: every candidate has a taste profile", c.id));
        let data = jobhunt_eval::build::profile(c).unwrap();
        assert_eq!(
            data.taste_brief.as_ref().map(|b| b.text.as_str()),
            Some(taste.looking_for.as_str())
        );
        let profile = compose(&data, &[]);
        for s in &taste.statement {
            let key = jobhunt_profile::taste::key(s.dimension, &s.value);
            let a = profile
                .assertions
                .iter()
                .find(|a| a.key() == key)
                .unwrap_or_else(|| panic!("{}: {key} missing", c.id));
            assert_eq!(a.polarity, s.polarity, "{}: {key}", c.id);
            assert!(
                a.origin == TasteOrigin::Stated && a.review == TasteReview::Confirmed,
                "{}: {key} is the candidate's",
                c.id
            );
        }
        // Seniority and engineering work shape are expressed for every
        // archetype: what BRU-320 showed the old model couldn't.
        for d in [
            jobhunt_profile::TasteDimension::Seniority,
            jobhunt_profile::TasteDimension::WorkShape,
            jobhunt_profile::TasteDimension::Specialization,
        ] {
            assert!(profile.about(d).next().is_some(), "{}: {d}", c.id);
        }
    }
    // The same job can fit one profile and not another: the specialist
    // wants what the generalist avoids.
    let senior = f.candidate(SENIOR).unwrap().taste.as_ref().unwrap();
    let specialist = f
        .candidate("database-internals-specialist")
        .unwrap()
        .taste
        .as_ref()
        .unwrap();
    let polarity = |t: &jobhunt_eval::fixture::TasteFixture, value: &str| {
        t.statement
            .iter()
            .find(|s| {
                s.dimension == jobhunt_profile::TasteDimension::Specialization && s.value == value
            })
            .map(|s| s.polarity)
    };
    assert_eq!(
        polarity(senior, "deep"),
        Some(jobhunt_profile::Polarity::Avoid)
    );
    assert_eq!(
        polarity(specialist, "deep"),
        Some(jobhunt_profile::Polarity::Prefer)
    );
}

#[test]
fn the_built_in_reader_never_contradicts_the_confirmed_taste() {
    use jobhunt_profile::Polarity;
    use jobhunt_profile::taste::RulesInterpreter;
    use jobhunt_profile::taste::reading::{TasteRequest, Words};
    let f = fixtures();
    for c in &f.candidates {
        let taste = c.taste.as_ref().unwrap();
        let data = jobhunt_eval::build::profile(c).unwrap();
        let mut bare = data.clone();
        bare.taste.clear();
        let request = TasteRequest::build(
            &bare,
            vec![Words {
                text: taste.looking_for.clone(),
                statement: None,
            }],
            &[],
            None,
        );
        let reading = RulesInterpreter.read(&request);
        assert!(!reading.assertions.is_empty(), "{}: nothing read", c.id);
        let lean = |p: Polarity| match p {
            Polarity::Prefer | Polarity::Open => 1,
            Polarity::Avoid => -1,
            Polarity::Neutral => 0,
        };
        for s in &taste.statement {
            if let Some(read) = reading
                .assertions
                .iter()
                .find(|a| a.dimension == s.dimension && a.value == s.value)
            {
                assert_eq!(
                    lean(read.polarity),
                    lean(s.polarity),
                    "{}: the reader reads {}:{} the other way",
                    c.id,
                    s.dimension,
                    s.value
                );
            }
        }
    }
}

/// BRU-322's acceptance gate: what Today must never do again, and what it
/// must keep doing. Quality stops being only recorded here: a ranking
/// change that surfaces a No, an Impossible or a Maybe, lets pay carry a
/// job, reads another location's range as the person's, or loses a
/// practical Strong yes fails CI.
#[test]
fn the_ranker_meets_the_recommendation_gate() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let all = Metrics::of(result.candidates.iter().flat_map(|c| &c.cases));
    let failing: Vec<String> = result
        .candidates
        .iter()
        .flat_map(|c| {
            c.cases
                .iter()
                .filter(|x| x.verdict().failed())
                .map(move |x| format!("{}/{}: {}", c.id, x.job, x.verdict().as_str()))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
    assert_eq!(all.obvious_false_positives.hits, 0);
    assert_eq!(all.maybes_surfaced, 0);
    assert_eq!(all.surfaced_on_pay, 0);
    assert_eq!(all.foreign_range_read_as_met.hits, 0);
    assert_eq!(
        all.strong_yes_surfaced.hits, all.strong_yes_surfaced.of,
        "every practical Strong yes still surfaces"
    );
    assert!(
        all.strong_yes_surfaced.of >= 15,
        "not precise by being empty"
    );
    for c in &all.contradictions {
        assert_eq!(c.surfaced, 0, "{} contradictions surfaced", c.kind);
    }

    // The golden cases, on the candidate they were observed on.
    let airbnb = case(&result, SENIOR, "airbnb-early-career");
    assert!(!airbnb.surfaced());
    assert!(airbnb.observed.detected.contains(&Contradiction::Seniority));
    let stripe = case(&result, SENIOR, "stripe-payments-us-remote");
    assert!(!stripe.surfaced());
    assert!(
        stripe.observed.gate.contains("ineligible"),
        "{}",
        stripe.observed.gate
    );
    assert!(
        stripe
            .observed
            .detected
            .contains(&Contradiction::Eligibility)
    );
    let orioledb = case(&result, SENIOR, "supabase-orioledb");
    assert!(!orioledb.surfaced());
    assert!(
        orioledb
            .observed
            .detected
            .contains(&Contradiction::RoleDepth)
    );
    let anthropic = case(&result, SENIOR, "anthropic-staff-privacy");
    assert!(!anthropic.surfaced());
    assert!(!anthropic.observed.pay_carried);
    // The contrast: the same storage engine is exactly right for the
    // specialist.
    let specialist = case(
        &result,
        "database-internals-specialist",
        "supabase-orioledb",
    );
    assert!(specialist.surfaced());
    assert!(
        specialist.observed.fit.starts_with("strong"),
        "{}",
        specialist.observed.fit
    );
    // Pay unpublished doesn't hold a strong fit back.
    assert!(case(&result, SENIOR, "lanternfish-platform-no-pay").surfaced());
}

// --- Intent capture (BRU-324) ---------------------------------------------

fn variant<'a>(result: &'a jobhunt_eval::Run, id: &str) -> &'a jobhunt_eval::VariantRun {
    result
        .intent
        .iter()
        .find(|v| v.id == id)
        .unwrap_or_else(|| panic!("no variant {id}"))
}

fn variant_case<'a>(v: &'a jobhunt_eval::VariantRun, job: &str) -> &'a Case {
    v.run
        .cases
        .iter()
        .find(|c| c.job == job)
        .unwrap_or_else(|| panic!("no case {}/{job}", v.id))
}

fn surfaced(v: &jobhunt_eval::VariantRun) -> Vec<&str> {
    v.run
        .cases
        .iter()
        .filter(|c| c.surfaced())
        .map(|c| c.job.as_str())
        .collect()
}

/// The kinds of work the variant's composed profile wants.
fn wanted_shapes(f: &Fixtures, id: &str) -> Vec<String> {
    let v = f
        .variants
        .iter()
        .find(|v| v.id == id)
        .unwrap_or_else(|| panic!("no variant {id}"));
    let (_, data) =
        jobhunt_eval::run::variant_profile(f, v).unwrap_or_else(|e| panic!("{id}: {e}"));
    jobhunt_profile::taste::compose(&data, &[])
        .wanted(jobhunt_profile::TasteDimension::WorkShape)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn intent_capture_variants_meet_their_gates() {
    let f = fixtures();
    assert!(f.variants.len() >= 7, "the intent cases are there");
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(result.intent.len(), f.variants.len());
    for v in &result.intent {
        let failing: Vec<String> = v
            .failing()
            .iter()
            .map(|c| format!("{} {}: {}", c.verdict().as_str(), c.job, c.observed.fit))
            .collect();
        assert!(failing.is_empty(), "{}: {failing:#?}", v.id);
        // Nothing wrong surfaces, whatever was said about the work.
        let m = Metrics::of(&v.run.cases);
        assert_eq!(m.obvious_false_positives.hits, 0, "{}", v.id);
        assert_eq!(m.maybes_surfaced, 0, "{}", v.id);
        assert_eq!(m.surfaced_on_pay, 0, "{}: pay never creates fit", v.id);
    }
}

#[test]
fn without_a_stated_kind_of_work_today_stays_empty() {
    // The dogfood condition: the words name only the team and company.
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let none = variant(&result, "no-role");
    assert!(surfaced(none).is_empty(), "{:?}", surfaced(none));
    assert!(
        none.run.feed.is_empty(),
        "Today may be empty; nothing pads it"
    );
}

#[test]
fn an_explicit_choice_is_stronger_than_career_inference() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let inferred = variant(&result, "career-inferred");
    let chosen = variant(&result, "chosen-backend-platform");
    // Candidate B surfaces every practical Strong yes of the pool.
    let b = Metrics::of(&chosen.run.cases);
    assert_eq!(b.strong_yes_surfaced.hits, b.strong_yes_surfaced.of);
    assert!(b.strong_yes_surfaced.of >= 10);
    // Candidate A: what the career shows stays soft, and Today conservative.
    let a = Metrics::of(&inferred.run.cases);
    assert!(a.surfaced < b.surfaced, "{} vs {}", a.surfaced, b.surfaced);
    for job in surfaced(inferred) {
        assert!(
            surfaced(chosen).contains(&job),
            "{job}: A never surfaces more"
        );
    }
    let quarry = "quarrybird-senior-backend";
    assert!(
        variant_case(inferred, quarry)
            .observed
            .fit
            .contains("role 0.60"),
        "inferred work counts softly: {}",
        variant_case(inferred, quarry).observed.fit
    );
    assert!(
        variant_case(chosen, quarry)
            .observed
            .fit
            .contains("role 1.00"),
        "{}",
        variant_case(chosen, quarry).observed.fit
    );
    assert!(variant_case(chosen, quarry).surfaced());
}

#[test]
fn a_chosen_role_beats_conflicting_history_avoid_and_neutral() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let chosen = variant(&result, "chosen-backend-platform");
    let history = variant(&result, "chosen-over-history");
    // Earlier React Native and web work doesn't make mobile or frontend
    // wanted: the choice comes first, and Today is the same.
    assert_eq!(
        wanted_shapes(&f, "chosen-over-history"),
        ["backend", "platform"]
    );
    assert_eq!(surfaced(history), surfaced(chosen));
    for job in ["tidewater-senior-mobile", "fernlight-senior-frontend"] {
        assert!(!variant_case(history, job).surfaced(), "{job}");
    }
    // "I don't want full-stack work" holds against the profile's inference.
    let avoid = variant(&result, "avoid-wins");
    let parcel = variant_case(avoid, "parcelwise-fullstack-unscoped-remote");
    assert!(!parcel.surfaced());
    assert!(
        parcel
            .observed
            .against
            .iter()
            .any(|a| a.to_lowercase().contains("full-stack")),
        "{:?}",
        parcel.observed.against
    );
    assert!(variant_case(chosen, "parcelwise-fullstack-unscoped-remote").surfaced());
    // "Mobile doesn't matter" is never inferred back into a preference.
    assert!(!wanted_shapes(&f, "neutral-wins").contains(&"mobile".to_owned()));
    let neutral = variant(&result, "neutral-wins");
    assert!(!variant_case(neutral, "tidewater-senior-mobile").surfaced());
}

#[test]
fn several_roles_and_a_custom_title() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let chosen = variant(&result, "chosen-backend-platform");
    assert_eq!(
        wanted_shapes(&f, "multiple-roles"),
        ["backend", "platform", "product"]
    );
    let several = variant(&result, "multiple-roles");
    for job in surfaced(chosen) {
        assert!(surfaced(several).contains(&job), "{job}");
    }
    // The title is context: it adds no kind of work and changes no ranking.
    assert_eq!(wanted_shapes(&f, "custom-title"), ["backend", "platform"]);
    let titled = variant(&result, "custom-title");
    assert_eq!(surfaced(titled), surfaced(chosen));
}

#[test]
fn ranking_invariants_hold_with_an_explicit_choice() {
    let f = fixtures();
    let result = run(&f).unwrap_or_else(|e| panic!("{e}"));
    let chosen = variant(&result, "chosen-backend-platform");
    // An explicit role never bypasses seniority: Early Career stays out.
    let airbnb = variant_case(chosen, "airbnb-early-career");
    assert!(!airbnb.surfaced());
    assert!(airbnb.observed.detected.contains(&Contradiction::Seniority));
    // US-only stays out.
    let stripe = variant_case(chosen, "stripe-payments-us-remote");
    assert!(
        stripe.observed.gate.starts_with("excluded"),
        "{}",
        stripe.observed.gate
    );
    // Backend isn't database internals, Platform isn't Kubernetes internals
    // or a team merely named "Platform".
    for job in [
        "supabase-orioledb",
        "stratadb-storage-engine",
        "helmsman-k8s-control-plane",
        "kitebrook-ios-user-platform",
    ] {
        let c = variant_case(chosen, job);
        assert!(!c.surfaced(), "{job}: {}", c.observed.fit);
    }
    assert!(
        variant_case(chosen, "supabase-orioledb")
            .observed
            .detected
            .contains(&Contradiction::RoleDepth)
    );
    // OrioleDB is still right for the database-internals specialist.
    assert!(
        case(
            &result,
            "database-internals-specialist",
            "supabase-orioledb"
        )
        .surfaced()
    );
}
