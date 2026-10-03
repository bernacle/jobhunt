//! Ranking on synthetic jobs: gates, tiers, pay, learned taste against
//! stated preferences, and the brief.

use jobhunt_eligibility::Eligibility;
use jobhunt_jobs::JobRecord;
use jobhunt_profile::{CompensationBound, PayPeriod, Stance, WorkMode};

use super::*;
use crate::facets::facets;
use crate::feedback::FeedbackAction;
use crate::key::{Dimension, TasteKey};
use crate::person::{Matcher, PayPreference, StatedPreference, role_matcher};
use crate::reason::RuleReader;
use crate::taste::{FeedbackOpportunity, derive};
use crate::testing::{assessment, event_on, now, record, salary};

/// Feedback on one job, in order.
type Actions<'a> = &'a [(FeedbackAction, Option<&'a str>)];

/// A job anyone can take from anywhere.
fn job(title: &str, description: &str) -> JobRecord {
    let mut r = record("ashby:acme", title, description);
    r.posting.location = Some("Remote - Worldwide".into());
    r
}

fn stated(dimension: Dimension, matcher: Matcher, stance: Stance, text: &str) -> StatedPreference {
    StatedPreference {
        id: format!("pref_{text}"),
        dimension,
        matcher,
        stance,
        text: text.to_owned(),
        uncertain: false,
    }
}

fn role(text: &str, stance: Stance) -> StatedPreference {
    stated(
        Dimension::Role,
        role_matcher(text),
        stance,
        &format!("{text} roles"),
    )
}

fn trait_pref(value: &str, stance: Stance) -> StatedPreference {
    stated(
        Dimension::CompanyTrait,
        Matcher::Keys(vec![TasteKey::new(Dimension::CompanyTrait, value)]),
        stance,
        value,
    )
}

fn pay(
    bound: CompensationBound,
    amount: u64,
    currency: Option<&str>,
    stance: Stance,
) -> PayPreference {
    PayPreference {
        bound,
        amount,
        currency: currency.map(str::to_owned),
        period: PayPeriod::Year,
        arrangement: None,
        stance,
        text: format!("{} {amount}", bound.as_str()),
    }
}

/// A backend engineer who has used Rust and PostgreSQL.
fn engineer() -> Person {
    Person {
        profile_id: "prof_test".into(),
        revision: 3,
        roles: vec![crate::person::Experience {
            topic: "backend".into(),
            places: vec!["Ledgerly".into()],
        }],
        technologies: vec![
            (
                "Rust".into(),
                jobhunt_profile::EvidenceStrength::Demonstrated,
            ),
            (
                "PostgreSQL".into(),
                jobhunt_profile::EvidenceStrength::Demonstrated,
            ),
        ],
        level: Some((
            crate::facets::Level::Senior,
            "Senior Software Engineer".into(),
        )),
        engineer: true,
        ..Person::default()
    }
}

/// Ranks `record` for `person` (living in Berlin), verified `hours` ago.
fn ranked(record: &JobRecord, person: &Person, taste: &TasteModel, hours: Option<i64>) -> Ranking {
    ranked_with(record, person, taste, hours, &OpportunityState::default())
}

fn ranked_with(
    record: &JobRecord,
    person: &Person,
    taste: &TasteModel,
    hours: Option<i64>,
    state: &OpportunityState,
) -> Ranking {
    let a = assessment(record, "Berlin, Germany", hours);
    let records = [record.clone()];
    let candidate = Candidate {
        records: &records,
        assessment: &a,
        state,
    };
    let profile = crate::testing::taste_with(person, taste);
    let ctx = Context {
        person,
        taste,
        taste_profile: &profile,
        now: now(),
        companies: None,
    };
    rank(&candidate, &ctx).expect("a ranking")
}

fn no_taste() -> TasteModel {
    TasteModel::empty("rules/1")
}

fn summaries(r: &Ranking) -> Vec<String> {
    r.signals.iter().map(|s| s.summary.clone()).collect()
}

const RUST_BACKEND: &str = "About the role\nYou'll own our payments API end to end.\n\
    Requirements\n- Strong experience with Rust\n- PostgreSQL in production\n";

fn well_paid(person: &mut Person) -> JobRecord {
    let mut r = job("Senior Backend Engineer", RUST_BACKEND);
    r.posting.compensation = Some(salary(Some("USD"), 170_000.0, 210_000.0, None));
    person.pay.push(pay(
        CompensationBound::Minimum,
        150_000,
        Some("USD"),
        Stance::Required,
    ));
    person.pay.push(pay(
        CompensationBound::Target,
        180_000,
        Some("USD"),
        Stance::Wanted,
    ));
    r
}

#[test]
fn a_wanted_role_alone_is_not_a_strong_fit_however_well_paid() {
    let mut person = engineer();
    person.stated.push(role("backend", Stance::Wanted));
    let r = well_paid(&mut person);
    let ranking = ranked(&r, &person, &no_taste(), Some(2));
    assert_eq!(ranking.gate, Gate::Recommended);
    // The work fits, and nothing else is known to: worth reviewing, not
    // Today. Pay reaching the target adds nothing to fit.
    assert_eq!(ranking.tier, Tier::WorthReviewing, "{:#?}", ranking.fit);
    assert!(
        !ranking
            .brief
            .worth
            .iter()
            .any(|w| w.contains("minimum") || w.contains("target")),
        "{:?}",
        ranking.brief.worth
    );
    assert_eq!(
        ranking.practicality.pay,
        crate::practicality::PayStanding::Meets
    );
    assert!(
        ranking
            .practicality
            .facts
            .iter()
            .any(|f| f.contains("Reaches your target of USD 180,000 per year at the top")),
        "{:?}",
        ranking.practicality
    );
    // Without any pay preference, the same fit.
    let mut unpaid = person.clone();
    unpaid.pay.clear();
    assert_eq!(ranked(&r, &unpaid, &no_taste(), Some(2)).tier, ranking.tier);
}

#[test]
fn a_wanted_role_and_way_of_working_is_a_strong_fit() {
    let mut person = engineer();
    person.stated.push(role("backend", Stance::Wanted));
    person.stated.push(stated(
        Dimension::WorkStyle,
        Matcher::Keys(vec![TasteKey::new(Dimension::WorkStyle, "ownership")]),
        Stance::Wanted,
        "ownership and autonomy",
    ));
    let r = well_paid(&mut person);
    let ranking = ranked(&r, &person, &no_taste(), Some(2));
    assert_eq!(ranking.gate, Gate::Recommended);
    assert_eq!(ranking.tier, Tier::StrongFit, "{:#?}", ranking.fit);
    let brief = &ranking.brief;
    assert!(
        brief.verdict.starts_with("Looks unusually aligned"),
        "{}",
        brief.verdict
    );
    assert_eq!(
        brief.worth[0],
        "Senior backend work, the kind of engineering you want"
    );
    assert!(
        brief.worth[1].starts_with("Real ownership, as you want: “You'll own our payments API"),
        "{:?}",
        brief.worth
    );
    assert!(
        brief
            .summary
            .starts_with("backend · senior · Rust, PostgreSQL"),
        "{}",
        brief.summary
    );
    assert!(
        brief.summary.contains("USD 170,000 – 210,000 per year"),
        "{}",
        brief.summary
    );
    // Every signal is inspectable on its own.
    let stack = ranking
        .signals
        .iter()
        .find(|s| s.group == crate::signals::SignalGroup::Stack)
        .unwrap();
    assert_eq!(
        stack.summary,
        "Asks for Rust and PostgreSQL, which you've used"
    );
    assert!(stack.evidence[0].contains("Strong experience with Rust"));
}

#[test]
fn eligibility_and_verification_gate_the_ranking() {
    let person = engineer();
    let r = job("Backend Engineer", RUST_BACKEND);
    // Never verified: worth checking, but not recommended yet.
    let ranking = ranked(&r, &person, &no_taste(), None);
    assert!(
        matches!(ranking.gate, Gate::VerifyFirst { .. }),
        "{:?}",
        ranking.gate
    );
    assert!(
        ranking.brief.verdict.contains("verify it first"),
        "{}",
        ranking.brief.verdict
    );
    assert!(
        ranking
            .brief
            .unknowns
            .iter()
            .any(|u| u.starts_with("Not verified yet"))
    );
    // Stale is the same.
    let ranking = ranked(&r, &person, &no_taste(), Some(24 * 5));
    assert!(matches!(ranking.gate, Gate::VerifyFirst { .. }));
    // "Remote" with no scope: nothing confirms the person can take it.
    let mut unclear = r.clone();
    unclear.posting.location = Some("Remote".into());
    let ranking = ranked(&unclear, &person, &no_taste(), Some(2));
    assert!(
        matches!(ranking.gate, Gate::EligibilityUnclear { .. }),
        "{:?}",
        ranking.gate
    );
    // Excluded by the posting's own restriction.
    let mut us_only = r.clone();
    us_only.posting.location = Some("Remote (US)".into());
    let ranking = ranked(&us_only, &person, &no_taste(), Some(2));
    assert!(
        matches!(
            &ranking.gate,
            Gate::Excluded {
                exclusion: Exclusion::Ineligible { .. }
            }
        ),
        "{:?}",
        ranking.gate
    );
    assert!(
        ranking
            .brief
            .verdict
            .starts_with("Not recommended: you can't take it")
    );
}

#[test]
fn conditional_eligibility_stays_rankable_with_its_condition_visible() {
    let mut r = job("Backend Engineer", RUST_BACKEND);
    r.posting.location = Some("London".into());
    r.posting.workplace_type = Some(jobhunt_jobs::WorkplaceType::OnSite);
    r.posting.is_remote = Some(false);
    let a = assessment(&r, "Berlin, Germany", Some(2));
    assert_eq!(
        a.decision.status,
        Eligibility::Uncertain,
        "no relocation stated"
    );
    // With relocation stated the job is conditional.
    let mut facts = jobhunt_eligibility::ProfileFacts::living_in("Berlin, Germany");
    facts.relocation = Some(true);
    facts.authorized_in.push((
        jobhunt_eligibility::profile::place_area("United Kingdom"),
        "United Kingdom".into(),
    ));
    let v = crate::testing::verified(&r, 2);
    let a = jobhunt_eligibility::assess(
        &[jobhunt_jobs::verification::RecordVerification {
            record: r.clone(),
            latest: Some(v.clone()),
            last_success: Some(v),
            reused: true,
        }],
        &facts,
        &jobhunt_jobs::verification::FreshnessPolicy::default(),
        now(),
    );
    assert_eq!(a.decision.status, Eligibility::Conditional);
    let records = [r];
    let state = OpportunityState::default();
    let person = engineer();
    let taste = no_taste();
    let ranking = rank(
        &Candidate {
            records: &records,
            assessment: &a,
            state: &state,
        },
        &Context {
            person: &person,
            taste: &taste,
            taste_profile: &crate::testing::taste_of(&person),
            now: now(),
            companies: None,
        },
    )
    .unwrap();
    assert_eq!(ranking.gate, Gate::Recommended);
    // The condition is a thing to check before applying.
    assert!(
        ranking
            .brief
            .unknowns
            .iter()
            .any(|c| c.starts_with("Conditional:")),
        "{:?}",
        ranking.brief.unknowns
    );
}

#[test]
fn feedback_on_the_job_itself() {
    let person = engineer();
    let r = job("Backend Engineer", RUST_BACKEND);
    let state = |actions: Actions<'_>| {
        OpportunityState::of(
            actions
                .iter()
                .enumerate()
                .map(|(i, (a, why))| event_on(&r, *a, *why, i as i64)),
        )
    };
    let rejected = ranked_with(
        &r,
        &person,
        &no_taste(),
        Some(2),
        &state(&[(FeedbackAction::Reject, Some("too corporate"))]),
    );
    assert_eq!(
        rejected.gate,
        Gate::Excluded {
            exclusion: Exclusion::Rejected
        }
    );
    assert!(
        rejected.brief.history[0].contains("“too corporate”"),
        "{:?}",
        rejected.brief.history
    );
    let applied = ranked_with(
        &r,
        &person,
        &no_taste(),
        Some(2),
        &state(&[(FeedbackAction::Applied, None)]),
    );
    assert_eq!(
        applied.gate,
        Gate::Excluded {
            exclusion: Exclusion::InPipeline {
                stage: Stage::Applied
            }
        }
    );
    let base = ranked(&r, &person, &no_taste(), Some(2));
    let liked = ranked_with(
        &r,
        &person,
        &no_taste(),
        Some(2),
        &state(&[(FeedbackAction::Like, None)]),
    );
    let disliked = ranked_with(
        &r,
        &person,
        &no_taste(),
        Some(2),
        &state(&[(FeedbackAction::Dislike, None)]),
    );
    assert!(liked.score > base.score && base.score > disliked.score);
    assert!(disliked.tier <= Tier::Maybe);
}

#[test]
fn unwanted_roles_and_other_kinds_of_work_rank_low() {
    let mut person = engineer();
    person.stated.push(role("SRE", Stance::Unwanted));
    let sre = ranked(
        &job("Site Reliability Engineer", ""),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(sre.tier <= Tier::Maybe, "{:?}", sre.tier);
    assert!(
        sre.brief.caveats[0].contains("SRE work, which you said you don't want"),
        "{:?}",
        sre.brief.caveats
    );
    let sales = ranked(
        &job("Account Executive, Enterprise", ""),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(sales.tier <= Tier::Maybe);
    assert!(
        sales.brief.caveats[0].contains("sales, while your experience is in engineering"),
        "{:?}",
        sales.brief.caveats
    );
    let backend = ranked(
        &job("Backend Engineer", RUST_BACKEND),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(backend.score > sre.score && backend.score > sales.score);
}

#[test]
fn without_stated_preferences_nothing_is_a_strong_fit() {
    let ranking = ranked(
        &job("Backend Engineer", RUST_BACKEND),
        &engineer(),
        &no_taste(),
        Some(2),
    );
    assert!(ranking.tier <= Tier::WorthReviewing);
    assert!(
        ranking
            .brief
            .verdict
            .contains("you haven't said what you want yet")
    );
}

#[test]
fn pay_below_a_required_minimum() {
    let mut r = job("Backend Engineer", RUST_BACKEND);
    r.posting.compensation = Some(salary(Some("USD"), 90_000.0, 110_000.0, None));
    let mut person = engineer();
    person.pay.push(pay(
        CompensationBound::Minimum,
        150_000,
        Some("USD"),
        Stance::Required,
    ));
    // Verified below the floor: ruled out.
    let verified = ranked(&r, &person, &no_taste(), Some(2));
    assert_eq!(
        verified.gate,
        Gate::Excluded {
            exclusion: Exclusion::BelowMinimum {
                why:
                    "Pays at most USD 110,000 per year, below your minimum of USD 150,000 per year"
                        .into()
            }
        }
    );
    assert_eq!(
        verified.practicality.status,
        crate::practicality::PracticalStatus::Concern
    );
    // Only what discovery stored: a practical concern, not hidden, and
    // never a reason to doubt the fit.
    let unverified = ranked(&r, &person, &no_taste(), None);
    assert!(matches!(unverified.gate, Gate::VerifyFirst { .. }));
    let signal = unverified
        .signals
        .iter()
        .find(|s| s.summary.starts_with("Pays at most"))
        .unwrap();
    assert_eq!(signal.kind, SignalKind::Minus);
    assert_eq!(signal.weight, 0.0, "pay never moves fit");
    assert!(signal.evidence.iter().any(|e| e.contains("not verified")));
    assert!(
        unverified
            .practicality
            .concerns
            .iter()
            .any(|c| c.starts_with("Pays at most"))
    );
    // A preferred (not required) minimum: shown as a concern, the fit is
    // what it would be without it.
    person.pay[0].stance = Stance::Wanted;
    let preferred = ranked(&r, &person, &no_taste(), Some(2));
    assert_eq!(preferred.gate, Gate::Recommended);
    let mut unpaid = person.clone();
    unpaid.pay.clear();
    assert_eq!(
        preferred.tier,
        ranked(&r, &unpaid, &no_taste(), Some(2)).tier
    );
    assert_eq!(
        preferred.practicality.status,
        crate::practicality::PracticalStatus::Concern
    );
}

#[test]
fn unknown_or_ambiguous_pay_is_never_read_as_low() {
    let mut person = engineer();
    person.pay.push(pay(
        CompensationBound::Minimum,
        150_000,
        Some("USD"),
        Stance::Required,
    ));
    let unpublished = ranked(
        &job("Backend Engineer", RUST_BACKEND),
        &person,
        &no_taste(),
        Some(2),
    );
    let s = unpublished
        .signals
        .iter()
        .find(|s| s.group == crate::signals::SignalGroup::Compensation)
        .unwrap();
    assert_eq!(s.summary, "Pay isn't published: unknown, not low");
    assert_eq!((s.kind, s.weight), (SignalKind::Unknown, 0.0));
    assert!(unpublished.brief.unknowns.contains(&s.summary));

    let mut dollars = job("Backend Engineer", RUST_BACKEND);
    dollars.posting.compensation = Some(salary(None, 90_000.0, 100_000.0, Some("$90K - $100K")));
    let ranking = ranked(&dollars, &person, &no_taste(), Some(2));
    assert_eq!(ranking.gate, Gate::Recommended, "a bare $ is not USD");
    assert!(
        ranking
            .brief
            .unknowns
            .iter()
            .any(|u| u.starts_with("Pay is in “$”, which several currencies use")),
        "{:?}",
        ranking.brief.unknowns
    );

    let mut euros = job("Backend Engineer", RUST_BACKEND);
    euros.posting.compensation = Some(salary(Some("EUR"), 60_000.0, 80_000.0, None));
    let ranking = ranked(&euros, &person, &no_taste(), Some(2));
    assert_eq!(
        ranking.gate,
        Gate::Recommended,
        "currencies are not converted"
    );
    assert!(
        ranking
            .brief
            .unknowns
            .iter()
            .any(|u| u.contains("Pay is in EUR; your minimum is in USD"))
    );

    person.pay[0].currency = None;
    let mut usd = job("Backend Engineer", RUST_BACKEND);
    usd.posting.compensation = Some(salary(Some("USD"), 90_000.0, 100_000.0, None));
    let ranking = ranked(&usd, &person, &no_taste(), Some(2));
    assert_eq!(ranking.gate, Gate::Recommended);
    assert!(
        ranking
            .brief
            .unknowns
            .iter()
            .any(|u| u.contains("has no currency"))
    );
}

fn taste_from(opps: &[(&JobRecord, Actions<'_>)], person: &Person) -> TasteModel {
    let opps: Vec<FeedbackOpportunity> = opps
        .iter()
        .enumerate()
        .map(|(n, (r, actions))| FeedbackOpportunity {
            opportunity: r.opportunity_id,
            state: OpportunityState::of(
                actions
                    .iter()
                    .enumerate()
                    .map(|(i, (a, why))| event_on(r, *a, *why, (n * 10 + i) as i64)),
            ),
            facets: Some(facets(r)),
            fingerprint: String::new(),
        })
        .collect();
    derive(&opps, &person.explicit_keys(), &RuleReader)
}

fn distinct(title: &str, n: u32) -> JobRecord {
    let mut r = record(&format!("lever:c{n}"), title, "");
    r.posting.company = format!("Company {n}");
    r.posting.location = Some("Remote - Worldwide".into());
    r
}

#[test]
fn learned_taste_moves_rankings_and_shows_its_evidence() {
    let person = engineer();
    let rejected: Vec<JobRecord> = (0..3)
        .map(|n| distinct("Site Reliability Engineer", n))
        .collect();
    let reject: Actions<'_> = &[(FeedbackAction::Reject, Some("pure SRE"))];
    let taste = taste_from(
        &rejected.iter().map(|r| (r, reject)).collect::<Vec<_>>(),
        &person,
    );
    let sre = job("Senior Site Reliability Engineer", "");
    let before = ranked(&sre, &person, &no_taste(), Some(2));
    let after = ranked(&sre, &person, &taste, Some(2));
    assert!(after.score < before.score);
    let learned = after
        .signals
        .iter()
        .find(|s| s.basis == crate::signals::Basis::Learned)
        .unwrap();
    assert_eq!(
        learned.summary,
        "Role: SRE / DevOps: you've turned down jobs like this (3 reasons in your words; strong)"
    );
    assert!(
        learned.evidence[1]
            .starts_with("“pure SRE” — rejected Site Reliability Engineer at Company"),
        "{:?}",
        learned.evidence
    );
    assert_eq!(after.taste_digest, taste.digest);
}

#[test]
fn stated_preferences_outrank_learned_taste() {
    let mut person = engineer();
    person
        .stated
        .push(trait_pref("large_company", Stance::Wanted));
    let rejected: Vec<JobRecord> = (0..3).map(|n| distinct("Backend Engineer", n)).collect();
    let reject: Actions<'_> = &[(FeedbackAction::Reject, Some("too corporate"))];
    let taste = taste_from(
        &rejected.iter().map(|r| (r, reject)).collect::<Vec<_>>(),
        &person,
    );
    let corporate = job(
        "Backend Engineer",
        "We are a Fortune 500 company with thousands of employees.",
    );
    let ranking = ranked(&corporate, &person, &taste, Some(2));
    let company: Vec<&Signal> = ranking
        .signals
        .iter()
        .filter(|s| s.group == crate::signals::SignalGroup::Company)
        .collect();
    assert_eq!(company.len(), 1, "{company:#?}");
    assert_eq!(company[0].basis, crate::signals::Basis::Stated);
    assert!(company[0].weight > 0.0);
}

#[test]
fn one_rejection_does_not_blacklist_a_domain() {
    let person = engineer();
    let first = distinct("Backend Engineer, Fintech", 0);
    let infra_a = distinct("Infrastructure Engineer, Fintech", 1);
    let infra_b = distinct("Senior Infrastructure Engineer (Fintech)", 2);
    let reject: Actions<'_> = &[(FeedbackAction::Reject, Some("fintech"))];
    let apply: Actions<'_> = &[
        (FeedbackAction::Save, None),
        (FeedbackAction::Applied, None),
    ];
    let only_reject = taste_from(&[(&first, reject)], &person);
    let mixed = taste_from(
        &[(&first, reject), (&infra_a, apply), (&infra_b, apply)],
        &person,
    );
    let fintech_job = job("Platform Engineer, Fintech", "");
    let after_reject = ranked(&fintech_job, &person, &only_reject, Some(2));
    assert!(
        after_reject
            .signals
            .iter()
            .any(|s| s.basis == crate::signals::Basis::Learned && s.weight < 0.0),
        "one explicit reason is a tentative pattern"
    );
    let after_applying = ranked(&fintech_job, &person, &mixed, Some(2));
    assert!(
        !after_applying
            .signals
            .iter()
            .any(|s| s.basis == crate::signals::Basis::Learned
                && s.group == crate::signals::SignalGroup::Domain),
        "{:#?}",
        summaries(&after_applying)
    );
    assert!(after_applying.score > after_reject.score);
}

#[test]
fn work_mode_and_work_style_preferences() {
    let mut person = engineer();
    person.work_modes.push((WorkMode::Remote, Stance::Wanted));
    person.stated.push(stated(
        Dimension::WorkStyle,
        Matcher::Keys(vec![TasteKey::new(
            Dimension::WorkStyle,
            "individual_contributor",
        )]),
        Stance::Wanted,
        "individual-contributor work",
    ));
    let manager = ranked(
        &job("Engineering Manager, Platform", ""),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(
        manager
            .brief
            .caveats
            .contains(&"People management, while you want individual-contributor work".to_owned()),
        "{:?}",
        manager.brief.caveats
    );
    let ic = ranked(&job("Platform Engineer", ""), &person, &no_taste(), Some(2));
    // Remote work is practical: a fact about the job, not a reason for fit.
    assert!(
        ic.practicality
            .facts
            .contains(&"Can be done remote, as you prefer".to_owned()),
        "{:?}",
        ic.practicality
    );
    assert!(!ic.brief.worth.iter().any(|w| w.contains("remote")));
    assert!(ic.score > manager.score);
}

#[test]
fn rankings_order_and_round_trip() {
    let person = engineer();
    let mut rankings = vec![
        ranked(
            &job("Backend Engineer", RUST_BACKEND),
            &person,
            &no_taste(),
            None,
        ),
        ranked(
            &job("Backend Engineer II", RUST_BACKEND),
            &person,
            &no_taste(),
            Some(2),
        ),
    ];
    order(&mut rankings);
    assert_eq!(rankings[0].gate, Gate::Recommended, "trusted first");
    let json = serde_json::to_string(&rankings[0]).unwrap();
    let back: Ranking = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rankings[0]);
}

// ---------------------------------------------------------------------------
// Company size and team size are different facts.

const PUBLIC_GIANT: &str = "We are a publicly traded company with thousands of employees. \
    You will build backend services in Rust and PostgreSQL.";
const BIG_TEAM: &str = "Join a team of 200 engineers building backend services in Rust \
    and PostgreSQL.";
const SMALL_TEAM: &str = "You join a team of 6 engineers building backend services in Rust \
    and PostgreSQL.";
const SMALL_COMPANY: &str = "We're a small company building backend services in Rust and \
    PostgreSQL.";
const SAYS_NOTHING: &str = "You will build backend services in Rust and PostgreSQL.";
const LATE_STAGE: &str = "Over $1B raised, including our $500M Series F. You will build \
    backend services in Rust and PostgreSQL.";

fn wanting(value: &str, stance: Stance) -> Person {
    let mut person = engineer();
    person.stated.push(trait_pref(value, stance));
    person
}

fn company_weight(r: &Ranking) -> f64 {
    r.signals
        .iter()
        .filter(|s| s.group == crate::signals::SignalGroup::Company)
        .map(|s| s.weight)
        .sum()
}

fn has_unknown(r: &Ranking, text: &str) -> bool {
    r.signals
        .iter()
        .any(|s| s.kind == SignalKind::Unknown && s.summary.contains(text))
}

#[test]
fn a_big_company_says_nothing_about_the_size_of_the_team() {
    let person = wanting("small_team", Stance::Wanted);
    let r = ranked(
        &job("Backend Engineer", PUBLIC_GIANT),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), 0.0, "{:?}", summaries(&r));
    assert!(
        has_unknown(&r, "doesn't say whether it's small_team"),
        "{:?}",
        summaries(&r)
    );
    // A later funding round is a stage, not a team size either.
    let r = ranked(
        &job("Backend Engineer", LATE_STAGE),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), 0.0, "{:?}", summaries(&r));
    assert!(
        facets(&job("x", LATE_STAGE))
            .company_traits
            .iter()
            .any(|f| f.key.value == "scaleup")
    );
}

#[test]
fn a_wanted_small_team_counts_against_a_stated_large_team() {
    let person = wanting("small_team", Stance::Wanted);
    let r = ranked(
        &job("Backend Engineer", BIG_TEAM),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), -1.5, "{:?}", summaries(&r));
    assert!(
        summaries(&r)
            .iter()
            .any(|s| s == "The posting says it's a large team, while you want small_team"),
        "{:?}",
        summaries(&r)
    );
    assert!(
        matches!(r.gate, Gate::Recommended),
        "a want never rules out"
    );
    let r = ranked(
        &job("Backend Engineer", SMALL_TEAM),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), 1.5);
}

#[test]
fn a_wanted_small_company_counts_against_a_public_one() {
    let person = wanting("small_company", Stance::Wanted);
    let r = ranked(
        &job("Backend Engineer", PUBLIC_GIANT),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), -1.5, "{:?}", summaries(&r));
    assert!(matches!(r.gate, Gate::Recommended));
    let r = ranked(
        &job("Backend Engineer", SAYS_NOTHING),
        &person,
        &no_taste(),
        Some(2),
    );
    assert_eq!(company_weight(&r), 0.0, "unknown stays unknown");
}

#[test]
fn a_required_small_company_rules_out_a_stated_large_one_and_never_assumes() {
    let person = wanting("small_company", Stance::Required);
    // Stated conflict: out, with why.
    let r = ranked(
        &job("Backend Engineer", PUBLIC_GIANT),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(
        matches!(&r.gate, Gate::Excluded { exclusion: Exclusion::UnmetRequirement { why } }
            if why.contains("small_company")),
        "{:?}",
        r.gate
    );
    // Stated match: passes.
    let r = ranked(
        &job("Backend Engineer", SMALL_COMPANY),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    assert!(company_weight(&r) > 0.0);
    // Nothing said: still offered, never a strong fit, and unresolved.
    let r = ranked(
        &job("Senior Backend Engineer", SAYS_NOTHING),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    assert!(r.tier <= Tier::WorthReviewing, "{:?}", r.tier);
    assert!(
        has_unknown(&r, "Unresolved: you require small_company"),
        "{:?}",
        summaries(&r)
    );
}

#[test]
fn a_required_small_team_is_decided_by_the_team_never_by_the_company() {
    let person = wanting("small_team", Stance::Required);
    let r = ranked(
        &job("Backend Engineer", BIG_TEAM),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(
        matches!(
            &r.gate,
            Gate::Excluded {
                exclusion: Exclusion::UnmetRequirement { .. }
            }
        ),
        "{:?}",
        r.gate
    );
    let r = ranked(
        &job("Backend Engineer", SMALL_TEAM),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    // Company headcount alone: unresolved, not a conflict.
    let r = ranked(
        &job("Senior Backend Engineer", PUBLIC_GIANT),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    assert!(r.tier <= Tier::WorthReviewing);
    assert!(
        has_unknown(&r, "Unresolved: you require small_team"),
        "{:?}",
        summaries(&r)
    );
}

#[test]
fn a_required_small_team_isnt_ruled_out_by_the_company_headcount() {
    let person = wanting("small_team", Stance::Required);
    let posting = "A global team of 200 people across 30 countries. You will join a team of 6 \
        engineers building backend services in Rust and PostgreSQL.";
    let r = ranked(
        &job("Backend Engineer", posting),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    assert!(company_weight(&r) > 0.0, "{:?}", summaries(&r));
    let posting = "You will not be part of a large team: backend services in Rust and PostgreSQL.";
    let r = ranked(
        &job("Senior Backend Engineer", posting),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(
        matches!(r.gate, Gate::Recommended),
        "a denial rules nothing out: {:?}",
        r.gate
    );
    assert!(
        has_unknown(&r, "Unresolved: you require small_team"),
        "{:?}",
        summaries(&r)
    );
}

#[test]
fn company_wording_never_overrides_the_team_someone_joins() {
    let person = wanting("small_team", Stance::Required);
    let posting = "Our company is a large team of 200 employees. You will join a team of 6 \
        engineers building backend services in Rust and PostgreSQL.";
    let r = ranked(
        &job("Backend Engineer", posting),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    assert!(company_weight(&r) > 0.0, "{:?}", summaries(&r));
    // Denied in its own clause: no evidence; affirmed in another: evidence.
    let posting = "You will not be working as part of a large team: backend services in Rust \
        and PostgreSQL.";
    let r = ranked(
        &job("Senior Backend Engineer", posting),
        &person,
        &no_taste(),
        Some(2),
    );
    assert!(matches!(r.gate, Gate::Recommended), "{:?}", r.gate);
    let small_company = wanting("small_company", Stance::Required);
    let posting = "We are not a startup, but a publicly traded company. You will build backend \
        services in Rust and PostgreSQL.";
    let r = ranked(
        &job("Backend Engineer", posting),
        &small_company,
        &no_taste(),
        Some(2),
    );
    assert!(
        matches!(
            &r.gate,
            Gate::Excluded {
                exclusion: Exclusion::UnmetRequirement { .. }
            }
        ),
        "{:?}",
        r.gate
    );
}
