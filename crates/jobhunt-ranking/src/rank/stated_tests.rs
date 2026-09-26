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
            r.brief.unknowns.iter().any(|u| u.starts_with("Unresolved")),
            "the card says so: {:?}",
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
    // An unrecognized place neither matches nor rules anything out.
    let mut odd = person();
    odd.remote_geography = vec![geography("Atlantis", Stance::Required)];
    let r = rank_for(
        &remote("Remote - Brazil", SMALL_TEAM),
        &odd,
        &facts(Stance::Required),
    );
    assert_eq!(r.gate, Gate::Recommended);
}
