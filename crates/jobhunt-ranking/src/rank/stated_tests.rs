//! BRU-308: requirements, preferences and what is unknown, end to end
//! through eligibility and ranking.
//!
//! The person of the scenario lives in Brazil, works remote only, won't
//! relocate, requires at least USD 140,000 a year (and is shown jobs with
//! unknown pay, marked unresolved), and would like a small team.

use jobhunt_eligibility::ProfileFacts;
use jobhunt_jobs::{JobRecord, WorkplaceType};
use jobhunt_profile::{CompensationBound, PayPeriod, Stance, WorkMode};

use super::*;
use crate::key::{Dimension, TasteKey};
use crate::person::{Matcher, PayPreference, RemoteGeography, StatedPreference, region_area};
use crate::signals::SignalGroup;
use crate::testing::{assessment_for, now, record, salary};

const SMALL_TEAM: &str = "You'll join a team of 6 engineers building backend services in Rust \
    and PostgreSQL.";
const PUBLIC_GIANT: &str = "We are a publicly traded company with thousands of employees. \
    You'll build backend services in Rust and PostgreSQL.";

fn job(location: &str, workplace: Option<WorkplaceType>, description: &str) -> JobRecord {
    let mut r = record(
        "ashby:acme",
        "Senior Backend Engineer",
        &format!("{location} {description}"),
    );
    r.posting.location = Some(location.to_owned());
    r.posting.workplace_type = workplace;
    r.posting.is_remote = None;
    r.posting.description_text = Some(description.to_owned());
    r
}

fn remote(location: &str, description: &str) -> JobRecord {
    job(location, Some(WorkplaceType::Remote), description)
}

fn usd(r: &mut JobRecord, min: f64, max: f64) {
    r.posting.compensation = Some(salary(Some("USD"), min, max, None));
}

/// What eligibility knows: Brazil, remote only, won't relocate.
fn facts(work_mode: Stance) -> ProfileFacts {
    let mut f = ProfileFacts::living_in("Brazil");
    f.work_modes = vec![(WorkMode::Remote, work_mode)];
    f.relocation = Some(false);
    f
}

fn minimum(amount: u64, currency: Option<&str>) -> PayPreference {
    PayPreference {
        bound: CompensationBound::Minimum,
        amount,
        currency: currency.map(str::to_owned),
        period: PayPeriod::Year,
        arrangement: None,
        stance: Stance::Required,
        text: format!(
            "at least {} {amount} per year",
            currency.unwrap_or("(currency unknown)")
        ),
    }
}

fn stated(dimension: Dimension, key: TasteKey, stance: Stance, text: &str) -> StatedPreference {
    StatedPreference {
        id: format!("pref_{text}"),
        dimension,
        matcher: Matcher::Keys(vec![key]),
        stance,
        text: text.to_owned(),
        uncertain: false,
    }
}

/// The scenario's person, also wanting backend roles (a personal reason a
/// job can be a strong fit).
fn person() -> Person {
    Person {
        profile_id: "prof_test".into(),
        revision: 1,
        stated: vec![
            stated(
                Dimension::Role,
                TasteKey::role("backend"),
                Stance::Wanted,
                "backend roles",
            ),
            stated(
                Dimension::CompanyTrait,
                TasteKey::new(Dimension::CompanyTrait, "small_team"),
                Stance::Wanted,
                "small teams",
            ),
        ],
        pay: vec![minimum(140_000, Some("USD"))],
        work_modes: vec![(WorkMode::Remote, Stance::Required)],
        engineer: true,
        ..Person::default()
    }
}

fn rank_for(record: &JobRecord, person: &Person, facts: &ProfileFacts) -> Ranking {
    let a = assessment_for(record, facts, Some(2));
    let records = [record.clone()];
    let state = OpportunityState::default();
    let candidate = Candidate {
        records: &records,
        assessment: &a,
        state: &state,
    };
    let taste = TasteModel::empty("rules/1");
    let ctx = Context {
        person,
        taste: &taste,
        now: now(),
    };
    rank(&candidate, &ctx).expect("a ranking")
}

fn summaries(r: &Ranking) -> Vec<String> {
    r.signals.iter().map(|s| s.summary.clone()).collect()
}

fn has(r: &Ranking, kind: SignalKind, text: &str) -> bool {
    r.signals
        .iter()
        .any(|s| s.kind == kind && s.summary.contains(text))
}

fn excluded(r: &Ranking) -> Option<&Exclusion> {
    match &r.gate {
        Gate::Excluded { exclusion } => Some(exclusion),
        _ => None,
    }
}

/// On Today: not excluded, and at least worth reviewing.
fn on_today(r: &Ranking) -> bool {
    !r.gate.is_excluded() && r.tier >= Tier::WorthReviewing
}

#[test]
fn brazil_remote_and_global_remote_with_unknown_pay_appear_unresolved() {
    let person = person();
    for location in ["Remote - Brazil", "Remote - Worldwide", "Remote (LATAM)"] {
        let r = rank_for(
            &remote(location, SMALL_TEAM),
            &person,
            &facts(Stance::Required),
        );
        assert_eq!(r.gate, Gate::Recommended, "{location}: {:?}", r.gate);
        assert!(on_today(&r), "{location}: {:?} {:?}", r.tier, summaries(&r));
        assert_eq!(
            r.tier,
            Tier::WorthReviewing,
            "{location}: unknown pay against a required minimum is never a strong fit"
        );
        assert!(
            has(
                &r,
                SignalKind::Unknown,
                "Unresolved: you require at least USD 140000"
            ),
            "{location}: {:?}",
            summaries(&r)
        );
        assert!(
            r.brief.unknowns[0].starts_with("Unresolved: you require at least"),
            "the card shows the unresolved requirement first: {:?}",
            r.brief.unknowns
        );
        assert!(
            !has(&r, SignalKind::Plus, "Meets your minimum"),
            "unknown pay never meets a minimum"
        );
    }
}

#[test]
fn verified_pay_decides_the_minimum() {
    let person = person();
    // At or above the minimum across the range: it meets it, and nothing
    // stands between the job and a strong fit.
    let mut above = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut above, 150_000.0, 180_000.0);
    let r = rank_for(&above, &person, &facts(Stance::Required));
    assert_eq!(r.gate, Gate::Recommended);
    assert!(has(
        &r,
        SignalKind::Plus,
        "Meets your minimum of USD 140,000 per year"
    ));
    assert_eq!(r.tier, Tier::StrongFit, "{:?}", summaries(&r));

    // Verified below: a conflict, out of Today.
    let mut below = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut below, 100_000.0, 130_000.0);
    let r = rank_for(&below, &person, &facts(Stance::Required));
    assert!(
        matches!(excluded(&r), Some(Exclusion::BelowMinimum { why }) if why.contains("below your minimum")),
        "{:?}",
        r.gate
    );
    assert!(!on_today(&r));
}

#[test]
fn currency_is_never_assumed() {
    // Pay in another currency isn't converted, and doesn't meet the floor.
    let person = person();
    let mut eur = remote("Remote - Worldwide", SMALL_TEAM);
    eur.posting.compensation = Some(salary(Some("EUR"), 150_000.0, 180_000.0, None));
    let r = rank_for(&eur, &person, &facts(Stance::Required));
    assert!(has(&r, SignalKind::Unknown, "currencies are not converted"));
    assert!(has(&r, SignalKind::Unknown, "Unresolved"));
    assert!(r.tier <= Tier::WorthReviewing);

    // A minimum read without a currency is never compared with anything.
    let mut no_currency = person.clone();
    no_currency.pay = vec![minimum(140_000, None)];
    let mut paid = remote("Remote - Worldwide", SMALL_TEAM);
    usd(&mut paid, 100_000.0, 120_000.0);
    let r = rank_for(&paid, &no_currency, &facts(Stance::Required));
    assert!(
        !r.gate.is_excluded(),
        "not ruled out on a guessed currency: {:?}",
        r.gate
    );
    assert!(has(&r, SignalKind::Unknown, "needs a currency"));
    assert!(r.tier <= Tier::WorthReviewing);
}

#[test]
fn unknown_pay_policy_show_or_hide() {
    let mut hide = person();
    hide.hides_unknown_pay = true;
    let unknown = remote("Remote - Brazil", SMALL_TEAM);
    let r = rank_for(&unknown, &hide, &facts(Stance::Required));
    assert!(
        matches!(excluded(&r), Some(Exclusion::PayUnknown { .. })),
        "{:?}",
        r.gate
    );
    // Published, but not comparable (another currency): hidden too.
    let mut eur = remote("Remote - Brazil", SMALL_TEAM);
    eur.posting.compensation = Some(salary(Some("EUR"), 150_000.0, 180_000.0, None));
    let r = rank_for(&eur, &hide, &facts(Stance::Required));
    assert!(matches!(excluded(&r), Some(Exclusion::PayUnknown { .. })));
    // Published and comparable: shown.
    let mut paid = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut paid, 150_000.0, 180_000.0);
    let r = rank_for(&paid, &hide, &facts(Stance::Required));
    assert_eq!(r.gate, Gate::Recommended);

    // Show (the default): the unknown-pay job stays, unresolved.
    let r = rank_for(&unknown, &person(), &facts(Stance::Required));
    assert_eq!(r.gate, Gate::Recommended);
    assert!(has(&r, SignalKind::Unknown, "Unresolved"));
}

#[test]
fn a_target_is_a_preference_and_a_minimum_a_requirement() {
    let mut person = person();
    person.pay = vec![PayPreference {
        bound: CompensationBound::Target,
        amount: 180_000,
        currency: Some("USD".into()),
        period: PayPeriod::Year,
        arrangement: None,
        stance: Stance::Wanted,
        text: "target USD 180000 per year".into(),
    }];
    // Unknown pay against a target alone: nothing unresolved.
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &person,
        &facts(Stance::Required),
    );
    assert!(!has(&r, SignalKind::Unknown, "Unresolved"));
    assert_eq!(r.tier, Tier::StrongFit, "{:?}", summaries(&r));
    // Below the target: a minus, never an exclusion.
    let mut below = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut below, 120_000.0, 150_000.0);
    let r = rank_for(&below, &person, &facts(Stance::Required));
    assert_eq!(r.gate, Gate::Recommended);
    assert!(has(&r, SignalKind::Minus, "Below your target"));
}

#[test]
fn remote_with_no_scope_follows_the_unclear_eligibility_policy() {
    let posting = remote("Remote", SMALL_TEAM);
    let r = rank_for(&posting, &person(), &facts(Stance::Required));
    assert!(
        matches!(&r.gate, Gate::EligibilityUnclear { why } if why.contains("no geographic scope")),
        "Remote is never taken as global: {:?}",
        r.gate
    );
    let mut strict = person();
    strict.hides_unclear_eligibility = true;
    let r = rank_for(&posting, &strict, &facts(Stance::Required));
    assert!(
        matches!(excluded(&r), Some(Exclusion::EligibilityUnconfirmed { .. })),
        "{:?}",
        r.gate
    );
    // Known eligibility is unaffected by the policy.
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &strict,
        &facts(Stance::Required),
    );
    assert_eq!(r.gate, Gate::Recommended);
}

#[test]
fn explicit_hybrid_in_california_or_new_york_is_a_stated_conflict() {
    let postings = [
        job(
            "San Francisco, CA; New York, NY",
            Some(WorkplaceType::Hybrid),
            SMALL_TEAM,
        ),
        job(
            "",
            None,
            "This is a hybrid role: you'll work from our San Francisco or New York office three \
             days a week. You'll join a team of 6 engineers.",
        ),
    ];
    for posting in &postings {
        let r = rank_for(posting, &person(), &facts(Stance::Required));
        assert!(
            matches!(excluded(&r), Some(Exclusion::UnmetRequirement { why })
                if why.contains("you require remote work") && why.contains("not willing to relocate")),
            "{:?}",
            r.gate
        );
        assert!(!on_today(&r));
        assert!(
            !matches!(r.gate, Gate::EligibilityUnclear { .. }),
            "never merely “eligibility unclear”"
        );
        let blocker = r
            .signals
            .iter()
            .find(|s| s.kind == SignalKind::Blocker)
            .expect("a blocker");
        assert_eq!(blocker.group, SignalGroup::WorkMode);
        assert_eq!(blocker.basis, crate::signals::Basis::Stated);
    }
}

#[test]
fn prefer_remote_is_ranking_only() {
    let mut person = person();
    person.work_modes = vec![(WorkMode::Remote, Stance::Wanted)];
    let mut f = facts(Stance::Wanted);
    // Relocation not said: an office abroad is unclear, not ruled out.
    f.relocation = None;
    let hybrid = job("San Francisco, CA", Some(WorkplaceType::Hybrid), SMALL_TEAM);
    let r = rank_for(&hybrid, &person, &f);
    assert!(
        matches!(r.gate, Gate::EligibilityUnclear { .. }),
        "{:?}",
        r.gate
    );
    assert!(!has(&r, SignalKind::Plus, "as you prefer"));
    let r = rank_for(&remote("Remote - Brazil", SMALL_TEAM), &person, &f);
    assert!(has(
        &r,
        SignalKind::Plus,
        "Can be done remote, as you prefer"
    ));
}

#[test]
fn a_large_company_never_implies_a_large_team() {
    let r = rank_for(
        &remote("Remote - Brazil", PUBLIC_GIANT),
        &person(),
        &facts(Stance::Required),
    );
    assert_eq!(r.gate, Gate::Recommended, "a want never rules out");
    assert!(
        !r.signals
            .iter()
            .any(|s| s.group == SignalGroup::Company && s.weight < 0.0),
        "a company's size says nothing about the team: {:?}",
        summaries(&r)
    );
    assert!(has(
        &r,
        SignalKind::Unknown,
        "doesn't say whether it's small teams"
    ));
}

fn geography(text: &str, stance: Stance) -> RemoteGeography {
    RemoteGeography {
        id: format!("pref_{text}"),
        text: text.to_owned(),
        area: region_area(text),
        stance,
    }
}

#[test]
fn remote_geography_must_have_and_nice_to_have() {
    assert_eq!(
        region_area("Worldwide"),
        Some(jobhunt_eligibility::geo::Area::Worldwide)
    );
    assert!(region_area("Latin America").is_some());
    assert!(region_area("Americas").is_some());

    // Must have Latin America: Brazil, LATAM, the Americas and global
    // scopes all include it.
    let mut must = person();
    must.remote_geography = vec![geography("Latin America", Stance::Required)];
    for location in [
        "Remote - Brazil",
        "Remote (LATAM)",
        "Remote - Americas",
        "Remote - Worldwide",
    ] {
        let r = rank_for(
            &remote(location, SMALL_TEAM),
            &must,
            &facts(Stance::Required),
        );
        assert_eq!(r.gate, Gate::Recommended, "{location}");
    }
    // A posting with no scope: unresolved, never a strong fit.
    let r = rank_for(
        &remote("Remote", SMALL_TEAM),
        &must,
        &facts(Stance::Required),
    );
    assert!(has(
        &r,
        SignalKind::Unknown,
        "Unresolved: you require remote roles open to"
    ));
    assert!(r.tier <= Tier::WorthReviewing);

    // Must have Europe (someone in Brazil with an eye on European teams):
    // a Brazil-only scope is a stated conflict, not "can't take it".
    let mut europe = person();
    europe.remote_geography = vec![geography("Europe", Stance::Required)];
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &europe,
        &facts(Stance::Required),
    );
    assert!(
        matches!(excluded(&r), Some(Exclusion::UnmetRequirement { why }) if why.contains("Europe")),
        "{:?}",
        r.gate
    );
    // Nice to have: ordering only.
    let mut nice = person();
    nice.remote_geography = vec![geography("Europe", Stance::Wanted)];
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &nice,
        &facts(Stance::Required),
    );
    assert_eq!(r.gate, Gate::Recommended);
    assert!(has(&r, SignalKind::Minus, "not Europe as you prefer"));
}

/// Codex review #5: an alternative Narrow can't recognize is unresolved,
/// never dropped: it neither passes silently nor lets a known alternative
/// alone decide a conflict.
#[test]
fn unrecognized_required_geography_stays_unresolved() {
    let mut paid = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut paid, 150_000.0, 180_000.0);
    // Narnia alone: shown, unresolved, never a strong fit.
    let mut odd = person();
    odd.remote_geography = vec![geography("Narnia", Stance::Required)];
    let r = rank_for(&paid, &odd, &facts(Stance::Required));
    assert_eq!(r.gate, Gate::Recommended, "{:?}", r.gate);
    assert!(has(
        &r,
        SignalKind::Unknown,
        "Unresolved: you require remote roles open to Narnia"
    ));
    assert_eq!(r.tier, Tier::WorthReviewing, "{:?}", summaries(&r));
    // The same job with a recognized, met requirement can be a strong fit.
    let mut latam = person();
    latam.remote_geography = vec![geography("Latin America", Stance::Required)];
    assert_eq!(
        rank_for(&paid, &latam, &facts(Stance::Required)).tier,
        Tier::StrongFit
    );
    // Europe or Narnia against a Brazil-only scope: Europe conflicts,
    // Narnia can't be checked, so not a conflict: unresolved.
    let mut either = person();
    either.remote_geography = vec![
        geography("Europe", Stance::Required),
        geography("Narnia", Stance::Required),
    ];
    let r = rank_for(&paid, &either, &facts(Stance::Required));
    assert!(!r.gate.is_excluded(), "{:?}", r.gate);
    assert!(has(
        &r,
        SignalKind::Unknown,
        "Unresolved: you require remote roles open to Europe or Narnia"
    ));
    assert!(r.tier <= Tier::WorthReviewing);
    // Europe or Latin America: the certain match decides.
    either.remote_geography = vec![
        geography("Europe", Stance::Required),
        geography("Latin America", Stance::Required),
    ];
    let r = rank_for(&paid, &either, &facts(Stance::Required));
    assert_eq!(r.tier, Tier::StrongFit, "{:?}", summaries(&r));
    // Every alternative certainly conflicting: the stated conflict.
    either.remote_geography = vec![
        geography("Europe", Stance::Required),
        geography("Asia", Stance::Required),
    ];
    let r = rank_for(&paid, &either, &facts(Stance::Required));
    assert!(
        matches!(excluded(&r), Some(Exclusion::UnmetRequirement { .. })),
        "{:?}",
        r.gate
    );
}

/// Codex review #6: an uncertain membership (Mexico in "North America")
/// is unresolved: never rendered as met, never enough for a strong fit.
#[test]
fn an_uncertain_membership_is_not_a_match() {
    let mut mexico = ProfileFacts::living_in("Mexico City, Mexico");
    mexico.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    let mut job = remote("Remote - Mexico", SMALL_TEAM);
    usd(&mut job, 150_000.0, 180_000.0);
    let mut na = person();
    na.remote_geography = vec![geography("North America", Stance::Required)];
    let r = rank_for(&job, &na, &mexico);
    assert_eq!(r.gate, Gate::Recommended, "{:?}", r.gate);
    assert!(
        !has(&r, SignalKind::Context, "within what you require"),
        "{:?}",
        summaries(&r)
    );
    assert!(has(
        &r,
        SignalKind::Unknown,
        "Unresolved: you require remote roles open to North America"
    ));
    assert_eq!(r.tier, Tier::WorthReviewing);
    // A wanted one that can't be checked weighs nothing either way.
    let mut nice = person();
    nice.remote_geography = vec![geography("North America", Stance::Wanted)];
    let r = rank_for(&job, &nice, &mexico);
    assert!(
        !r.signals.iter().any(|s| s.group == SignalGroup::WorkMode
            && s.weight != 0.0
            && s.summary.contains("North America")),
        "{:?}",
        summaries(&r)
    );
    // Where the table is certain, it decides.
    let mut latam = person();
    latam.remote_geography = vec![geography("Latin America", Stance::Required)];
    let r = rank_for(&job, &latam, &mexico);
    assert!(has(&r, SignalKind::Context, "within what you require"));
    assert_eq!(r.tier, Tier::StrongFit, "{:?}", summaries(&r));
}

/// Codex review #8: a posting's remote reach is read once per version of
/// the posting, not on every ranking.
#[test]
fn remote_reach_is_read_once_per_posting_version() {
    use crate::signals::REACH_READS;
    let reads = || REACH_READS.with(std::cell::Cell::get);
    let mut job = remote(
        "Remote (LATAM)",
        "A unique posting for the reach cache test: Rust and PostgreSQL.",
    );
    usd(&mut job, 150_000.0, 180_000.0);
    let mut must = person();
    must.remote_geography = vec![geography("Latin America", Stance::Required)];
    let f = facts(Stance::Required);
    let before = reads();
    let first = rank_for(&job, &must, &f);
    for _ in 0..5 {
        assert_eq!(rank_for(&job, &must, &f).tier, first.tier);
    }
    assert_eq!(reads() - before, 1, "read once, then remembered");
    // Without a stated geography nothing is read at all.
    let before = reads();
    rank_for(&job, &person(), &f);
    assert_eq!(reads(), before);
    // A new version of the posting is read again, and its new scope used.
    let mut edited = job.clone();
    edited.posting.location = Some("Remote - Europe".into());
    let before = reads();
    let r = rank_for(&edited, &must, &f);
    assert_eq!(reads() - before, 1);
    assert!(r.gate.is_excluded(), "{:?}", r.gate);
}

/// Performance probe (not part of the regular suite): 1,000 warmed
/// rankings with and without a remote-geography preference.
/// `cargo test --release -p jobhunt-ranking geography_probe -- --ignored --nocapture`
#[test]
#[ignore = "timing probe; run by hand"]
fn geography_probe() {
    let description = "About the role. You'll own our payments API end to end, working with a team of 6 \
        engineers. Requirements: strong experience with Rust and PostgreSQL in production; \
        comfortable with distributed systems. We are remote-first and hire across Latin America. \
        You'll overlap at least 4 hours with UTC-3. We offer equity, a home-office budget and \
        flexible hours. We don't sponsor visas. Contractors are welcome through Deel.";
    let jobs: Vec<JobRecord> = (0..100)
        .map(|i| {
            let mut r = remote(
                if i % 2 == 0 {
                    "Remote - Brazil"
                } else {
                    "Remote (LATAM)"
                },
                &format!("{description} Job {i}."),
            );
            usd(&mut r, 150_000.0, 180_000.0);
            r
        })
        .collect();
    let f = facts(Stance::Required);
    let assessed: Vec<_> = jobs
        .iter()
        .map(|j| crate::testing::assessment_for(j, &f, Some(2)))
        .collect();
    let state = OpportunityState::default();
    let taste = TasteModel::empty("rules/1");
    let run = |person: &Person| {
        let ctx = Context {
            person,
            taste: &taste,
            now: now(),
        };
        let started = std::time::Instant::now();
        for _ in 0..10 {
            for (job, a) in jobs.iter().zip(&assessed) {
                let records = [job.clone()];
                let candidate = Candidate {
                    records: &records,
                    assessment: a,
                    state: &state,
                };
                std::hint::black_box(rank(&candidate, &ctx));
            }
        }
        started.elapsed()
    };
    let plain = person();
    let mut geography = person();
    geography.remote_geography = vec![geography_pref("Latin America", Stance::Required)];
    run(&plain);
    run(&geography);
    let without = run(&plain);
    let with = run(&geography);
    println!("1,000 warmed rankings: without geography {without:?}, with geography {with:?}");
}

fn geography_pref(text: &str, stance: Stance) -> RemoteGeography {
    geography(text, stance)
}

/// Codex review #7: "hide jobs with unknown pay" hides every job whose pay
/// Narrow can't compare with the person's, not only jobs that publish
/// none, and the job's own pay fact is left as published.
#[test]
fn hiding_unknown_pay_covers_pay_that_cant_be_compared() {
    let hide = |pay: PayPreference| {
        let mut p = person();
        p.pay = vec![pay];
        p.hides_unknown_pay = true;
        p
    };
    let f = facts(Stance::Required);
    let mut usd_job = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut usd_job, 150_000.0, 180_000.0);

    // The person's minimum has no currency: published USD pay can't be
    // compared, so it is hidden, and still reads as published.
    let r = rank_for(&usd_job, &hide(minimum(140_000, None)), &f);
    assert!(
        matches!(excluded(&r), Some(Exclusion::PayUnknown { why }) if why.contains("no currency")),
        "{:?}",
        r.gate
    );
    assert!(
        r.brief.summary.contains("USD 150,000"),
        "{}",
        r.brief.summary
    );
    // Another currency, no conversion.
    let mut eur = remote("Remote - Brazil", SMALL_TEAM);
    eur.posting.compensation = Some(salary(Some("EUR"), 150_000.0, 180_000.0, None));
    let r = rank_for(&eur, &hide(minimum(140_000, Some("USD"))), &f);
    assert!(
        matches!(excluded(&r), Some(Exclusion::PayUnknown { .. })),
        "{:?}",
        r.gate
    );
    // Another period, no conversion.
    let mut monthly = remote("Remote - Brazil", SMALL_TEAM);
    let mut pay = salary(Some("USD"), 12_000.0, 15_000.0, None);
    pay.components[0].interval = Some(jobhunt_jobs::PayInterval::Month);
    monthly.posting.compensation = Some(pay);
    let r = rank_for(&monthly, &hide(minimum(140_000, Some("USD"))), &f);
    assert!(
        matches!(excluded(&r), Some(Exclusion::PayUnknown { why }) if why.contains("another period")),
        "{:?}",
        r.gate
    );
    // A range without amounts is not "below the minimum": it is unknown.
    let mut empty = remote("Remote - Brazil", SMALL_TEAM);
    let mut pay = salary(Some("USD"), 0.0, 0.0, Some("Competitive"));
    pay.components[0].min = None;
    pay.components[0].max = None;
    empty.posting.compensation = Some(pay);
    let r = rank_for(&empty, &hide(minimum(140_000, Some("USD"))), &f);
    assert!(
        matches!(excluded(&r), Some(Exclusion::PayUnknown { .. })),
        "{:?}",
        r.gate
    );
    let mut shown = hide(minimum(140_000, Some("USD")));
    shown.hides_unknown_pay = false;
    let r = rank_for(&empty, &shown, &f);
    assert!(!r.gate.is_excluded(), "unknown is never low: {:?}", r.gate);
    assert!(has(&r, SignalKind::Unknown, "Unresolved"));
    // Comparable pay stays.
    let r = rank_for(&usd_job, &hide(minimum(140_000, Some("USD"))), &f);
    assert_eq!(r.gate, Gate::Recommended);
}

// ---------------------------------------------------------------------------
// Production smoke test of BRU-308 (the shapes seen on a real account).

fn early_stage_wanted() -> Person {
    let mut p = person();
    p.stated.push(stated(
        Dimension::CompanyTrait,
        TasteKey::new(Dimension::CompanyTrait, "early_stage"),
        Stance::Wanted,
        "early-stage companies",
    ));
    p.stated.push(stated(
        Dimension::CompanyTrait,
        TasteKey::new(Dimension::CompanyTrait, "startup"),
        Stance::Wanted,
        "startups",
    ));
    p
}

/// "Anywhere" restricts nothing and prefers nothing: with it or without it,
/// every job ranks the same.
#[test]
fn anywhere_is_neutral() {
    let jobs = [
        remote("Remote - Brazil", SMALL_TEAM),
        remote("Remote (LATAM)", SMALL_TEAM),
        remote("Remote - Worldwide", SMALL_TEAM),
        remote("Remote", SMALL_TEAM),
    ];
    let f = facts(Stance::Required);
    for stance in [Stance::Wanted, Stance::Required, Stance::Acceptable] {
        let mut anywhere = person();
        anywhere.remote_geography = vec![geography("Worldwide", stance)];
        for j in &jobs {
            let with = rank_for(j, &anywhere, &f);
            let without = rank_for(j, &person(), &f);
            assert_eq!(
                (with.gate.clone(), with.tier, with.score),
                (without.gate.clone(), without.tier, without.score),
                "{stance:?} {:?}",
                j.posting.location
            );
            assert!(
                !summaries(&with)
                    .iter()
                    .any(|s| s.contains("as you prefer (Worldwide)")),
                "{:?}",
                summaries(&with)
            );
        }
    }
    // A required list that includes anywhere restricts nothing.
    let mut either = person();
    either.remote_geography = vec![
        geography("Anywhere", Stance::Required),
        geography("Europe", Stance::Required),
    ];
    let r = rank_for(&jobs[0], &either, &f);
    assert_eq!(r.gate, Gate::Recommended, "{:?}", r.gate);
    // Other wishes next to anywhere still count, as said.
    let mut latam = person();
    latam.remote_geography = vec![
        geography("Worldwide", Stance::Wanted),
        geography("Latin America", Stance::Wanted),
    ];
    assert!(has(
        &rank_for(&jobs[0], &latam, &f),
        SignalKind::Plus,
        "as you prefer (Latin America)"
    ));
}

/// Ramp: New York (HQ), San Francisco, Remote (US) is remote in the US
/// only: never on Today for someone in Brazil.
#[test]
fn a_us_only_remote_job_listing_offices_first_is_excluded() {
    let mut ramp = job("New York, NY (HQ)", Some(WorkplaceType::Remote), SMALL_TEAM);
    ramp.posting.is_remote = Some(true);
    let loc = |name: &str| jobhunt_jobs::SourceLocation {
        name: Some(name.into()),
        locality: None,
        region: None,
        country: Some("US".into()),
    };
    ramp.posting.locations = vec![
        loc("New York, NY (HQ)"),
        loc("San Francisco, CA"),
        loc("Remote (US)"),
    ];
    usd(&mut ramp, 189_000.0, 330_000.0);
    let r = rank_for(&ramp, &person(), &facts(Stance::Required));
    assert!(
        matches!(excluded(&r), Some(Exclusion::Ineligible { why }) if why.contains("United States")),
        "{:?}",
        r.gate
    );
    assert!(!on_today(&r));
}

/// Anthropic: remote-friendly, but also 25% in an office and travel
/// required. Unresolved, said first, never a strong fit.
#[test]
fn a_remote_job_with_an_office_policy_is_unresolved_and_says_so() {
    let mut anthropic = remote(
        "Remote - Brazil",
        "You'll join a team of 6 engineers building backend services in Rust and PostgreSQL. \
         Location-based hybrid policy: Currently, we expect all staff to be in one of our offices \
         at least 25% of the time.",
    );
    usd(&mut anthropic, 405_000.0, 485_000.0);
    let r = rank_for(&anthropic, &person(), &facts(Stance::Required));
    assert!(
        matches!(r.gate, Gate::EligibilityUnclear { .. }),
        "{:?}",
        r.gate
    );
    assert!(on_today(&r), "unresolved is shown, not hidden");
    assert!(
        r.brief.unknowns[0].starts_with(
            "Unresolved: the listing says remote, but it also expects office presence"
        ),
        "{:?}",
        r.brief.unknowns
    );
    assert!(r.brief.unknowns[0].contains("25% of the time"));
    assert_eq!(
        r.tier,
        Tier::WorthReviewing,
        "never a strong fit: {:?}",
        summaries(&r)
    );
    // The same job without the policy is fine, and can be a strong fit.
    let mut plain = remote("Remote - Brazil", SMALL_TEAM);
    usd(&mut plain, 405_000.0, 485_000.0);
    assert_eq!(
        rank_for(&plain, &person(), &facts(Stance::Required)).tier,
        Tier::StrongFit
    );
}

/// DoorDash and Supabase: founding engineers for a new bet, early-stage
/// work on a brand-new team at a Series F company, a Y Combinator listing.
/// None is an early-stage company, and none a startup.
#[test]
fn team_and_project_stage_never_match_a_company_stage_wish() {
    let doordash = {
        let mut r = crate::testing::record(
            "yc:doordash",
            "Software Engineer, Distributed Databases",
            "We are bootstrapping some long term bets in all of these areas and looking for \
             founding engineers.",
        );
        r.posting.location = Some("Remote".into());
        r
    };
    let supabase = remote(
        "Remote, Global",
        "Small team, working product. Are comfortable owning ambiguous, early-stage work. This \
         is a founding role on a brand-new team. Over $1B raised (including our $500M Series F).",
    );
    for j in [&doordash, &supabase] {
        let r = rank_for(j, &early_stage_wanted(), &facts(Stance::Required));
        assert!(
            !r.signals
                .iter()
                .any(|s| s.group == crate::signals::SignalGroup::Company
                    && s.weight > 0.0
                    && (s.summary.contains("early-stage") || s.summary.contains("startup"))),
            "{}: {:?}",
            j.posting.company,
            summaries(&r)
        );
    }
    // The small team is still read, as a team.
    let r = rank_for(&supabase, &early_stage_wanted(), &facts(Stance::Required));
    assert!(
        !r.signals
            .iter()
            .any(|s| s.summary.contains("Small teams: a kind") && s.weight < 0.0)
    );
}

/// BRU-310: offices resolved through GeoNames don't widen a stated remote
/// scope, and a stated geography that names several places ("Georgia":
/// the country or the US state) is unresolved, never a match.
#[test]
fn stated_remote_scope_and_ambiguous_geography() {
    use crate::signals::RemoteReach;
    use jobhunt_eligibility::geo::Area;

    // "Remote (US)" at a company with offices in London and São Paulo is
    // remote from the US only.
    let offices = remote("London, UK; São Paulo, Brazil; Remote (US)", SMALL_TEAM);
    let reach = RemoteReach::of(&jobhunt_eligibility::requirements(&offices));
    match &reach {
        RemoteReach::Areas(areas) => {
            assert_eq!(areas.len(), 1, "{areas:?}");
            assert_eq!(areas[0].code(), "country:US");
        }
        other => panic!("{other:?}"),
    }
    let mut latam = person();
    latam.remote_geography = vec![geography("Latin America", Stance::Required)];
    let r = rank_for(&offices, &latam, &facts(Stance::Required));
    assert!(r.gate.is_excluded(), "{:?}", r.gate);
    // "Remote, Global" is global.
    let global = remote("Remote, Global", SMALL_TEAM);
    assert_eq!(
        RemoteReach::of(&jobhunt_eligibility::requirements(&global)),
        RemoteReach::Areas(vec![Area::Worldwide])
    );

    // "Georgia" alone could be the country or the US state.
    assert_eq!(region_area("Georgia"), None);
    let mut georgia = person();
    georgia.remote_geography = vec![geography("Georgia", Stance::Required)];
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &georgia,
        &facts(Stance::Required),
    );
    assert!(!r.gate.is_excluded(), "{:?}", r.gate);
    assert!(has(
        &r,
        SignalKind::Unknown,
        "Unresolved: you require remote roles open to Georgia"
    ));
}
