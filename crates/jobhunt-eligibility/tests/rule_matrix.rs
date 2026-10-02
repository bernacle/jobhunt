//! The eligibility rule matrix on synthetic postings: remote scope,
//! countries and regions, offices, time zones, work authorization,
//! contractors and EOR, conflicts, and unknown profile facts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::job;
use jobhunt_eligibility::decision::{Eligibility, EligibilityDecision, RuleId, Verdict};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::zones::zones_in;
use jobhunt_eligibility::{evaluate_record, requirements};
use jobhunt_jobs::{EmploymentType, JobRecord, WorkplaceType};
use jobhunt_profile::{Engagement, Stance, WorkMode};

use Eligibility::{Conditional, Eligible, Ineligible, Uncertain};

fn remote(location: &str, description: &str) -> JobRecord {
    job(
        "ashby:example",
        location,
        Some(WorkplaceType::Remote),
        description,
    )
}

fn at(place: &str) -> ProfileFacts {
    let p = ProfileFacts::living_in(place);
    assert!(p.country().is_some(), "{place} is recognized");
    p
}

fn decide(job: &JobRecord, profile: &ProfileFacts) -> EligibilityDecision {
    evaluate_record(job, profile)
}

fn status(job: &JobRecord, profile: &ProfileFacts) -> Eligibility {
    decide(job, profile).status
}

fn has_reason(d: &EligibilityDecision, rule: RuleId, verdict: Verdict, words: &str) -> bool {
    d.reasons
        .iter()
        .any(|r| r.rule == rule && r.verdict == verdict && r.conclusion.contains(words))
}

#[test]
fn remote_scopes_for_a_brazil_profile() {
    let brazil = at("São Paulo, Brazil");
    for (location, expected) in [
        ("Remote - Brazil", Eligible),
        ("Remote (LATAM)", Eligible),
        ("Remote - Americas", Eligible),
        ("Remote - Worldwide", Eligible),
        ("Remote - South America", Eligible),
        ("Remote (US)", Ineligible),
        ("US Remote", Ineligible),
        ("Remote - Canada", Ineligible),
        ("Remote - North America", Ineligible),
        ("Remote - Europe", Ineligible),
        ("Remote - EMEA", Ineligible),
        ("Remote - APAC", Ineligible),
        // How Stripe's jobs site (and others) write a remote scope.
        ("Remote in United States", Ineligible),
        ("Remote in North America", Ineligible),
        ("Remote within Canada", Ineligible),
        ("Remote in Brazil", Eligible),
        ("Remote in Latin America", Eligible),
        ("Remote", Uncertain),
    ] {
        let j = remote(location, "We build developer tools.");
        assert_eq!(status(&j, &brazil), expected, "{location}");
    }
}

#[test]
fn remote_alone_is_never_global() {
    let j = remote("Remote", "We build developer tools.");
    let d = decide(&j, &at("Lisbon, Portugal"));
    assert_eq!(d.status, Uncertain);
    assert!(has_reason(
        &d,
        RuleId::RemoteScope,
        Verdict::Unknown,
        "no geographic scope"
    ));
    assert!(has_reason(
        &d,
        RuleId::Engagement,
        Verdict::Unknown,
        "isn't stated"
    ));
    assert_eq!(d.reasons[0].evidence[0].field, "workplace_type");
}

#[test]
fn eligible_decisions_explain_themselves() {
    let j = remote(
        "Remote - Americas",
        "We hire contractors worldwide through Deel.",
    );
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Eligible);
    assert!(has_reason(
        &d,
        RuleId::RegionConstraint,
        Verdict::Pass,
        "Brazil is within the listed Americas region"
    ));
    assert!(has_reason(
        &d,
        RuleId::Engagement,
        Verdict::NotApplicable,
        "explicitly supported"
    ));
    let region = d
        .reasons
        .iter()
        .find(|r| r.rule == RuleId::RegionConstraint)
        .unwrap();
    assert_eq!(region.evidence[0].text, "Remote - Americas");
    assert_eq!(region.profile[0].what, "location");
    assert_eq!(region.profile[0].value, "Recife, Brazil");
}

#[test]
fn other_countries_too() {
    let cases = [
        ("Remote (US)", "Austin, TX", Eligible),
        ("Remote (US)", "Toronto", Ineligible),
        ("Remote - Canada", "Toronto", Eligible),
        ("Remote - Europe", "Berlin, Germany", Eligible),
        ("Remote - EU", "Berlin, Germany", Eligible),
        ("Remote - EMEA", "Nairobi, Kenya", Eligible),
        ("Remote - APAC", "Singapore", Eligible),
        ("Remote - APAC", "Sydney", Eligible),
        ("Remote - North America", "Chicago", Eligible),
        ("Remote - LATAM", "Mexico City", Eligible),
        ("Remote - LATAM", "Madrid", Ineligible),
        ("Remote: Brazil, Argentina, Chile", "Buenos Aires", Eligible),
        ("Remote: Brazil, Argentina, Chile", "Bogotá", Ineligible),
    ];
    for (location, place, expected) in cases {
        assert_eq!(
            status(&remote(location, ""), &at(place)),
            expected,
            "{location} / {place}"
        );
    }
}

#[test]
fn disputed_memberships_are_uncertain() {
    // Mexico in North America, the UK in "EU", Pakistan in APAC.
    for (location, place) in [
        ("Remote - North America", "Mexico City"),
        ("Remote - EU", "London"),
        ("Remote - APAC", "Pakistan"),
    ] {
        let d = decide(&remote(location, ""), &at(place));
        assert_eq!(d.status, Uncertain, "{location} / {place}");
        assert!(d.headline.contains("Usage disagrees"), "{}", d.headline);
    }
    // EEA is a formal area: the UK is outside it.
    assert_eq!(
        status(&remote("Remote - EEA", ""), &at("London")),
        Ineligible
    );
    assert_eq!(status(&remote("Remote - EEA", ""), &at("Oslo")), Eligible);
}

#[test]
fn description_restrictions() {
    let brazil = at("Porto Alegre, RS");
    for (description, expected) in [
        ("You must be based in Canada.", Ineligible),
        ("This role is open to candidates in Brazil.", Eligible),
        ("We're hiring in LATAM for this role.", Eligible),
        ("This position is US only.", Ineligible),
        ("Candidates must be based in the EU.", Ineligible),
        ("You can work from anywhere in the world.", Eligible),
        // "Anywhere in X" is a limit, not "anywhere".
        (
            "You can work from anywhere in the US or Europe.",
            Ineligible,
        ),
        ("Remote in the US.", Ineligible),
        // "US-based" names where candidates must be.
        ("This role is open to US-based candidates.", Ineligible),
        ("We are only considering US-based applicants.", Ineligible),
        ("This role is open to Brazil-based candidates.", Eligible),
    ] {
        let j = remote("Remote", description);
        assert_eq!(status(&j, &brazil), expected, "{description}");
    }
}

#[test]
fn anywhere_within_places_is_not_global() {
    let j = remote("Remote", "You can work from anywhere in the US or Europe.");
    let r = requirements(&j);
    assert!(r.worldwide.is_none());
    assert_eq!(r.allow.len(), 2);
    assert!(r.conflicts.is_empty());
    assert_eq!(status(&j, &at("Berlin")), Eligible);
}

#[test]
fn preferences_are_not_rules() {
    let j = remote("Remote", "Candidates based in Europe are preferred.");
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Uncertain);
    assert!(d.headline.contains("doesn't say others are excluded"));
    assert_eq!(status(&j, &at("Berlin")), Eligible);
    // Marketing language is not a scope.
    let j = remote("Remote", "We are a globally distributed team.");
    assert_eq!(status(&j, &at("Berlin")), Uncertain);
}

#[test]
fn exclusions() {
    let j = remote(
        "Remote - Worldwide",
        "We are unable to hire in Cuba, Iran, North Korea or Syria.",
    );
    let d = decide(&j, &at("Iran"));
    assert_eq!(d.status, Ineligible);
    assert!(has_reason(
        &d,
        RuleId::CountryConstraint,
        Verdict::Fail,
        "rules out people in Iran"
    ));
    assert_eq!(status(&j, &at("Brazil")), Eligible);
}

#[test]
fn onsite_and_hybrid() {
    let ny = job(
        "greenhouse:example",
        "New York, NY",
        Some(WorkplaceType::OnSite),
        "",
    );
    let mut brazil = at("São Paulo, Brazil");
    // Relocation unknown: uncertain, never assumed.
    let d = decide(&ny, &brazil);
    assert_eq!(d.status, Uncertain);
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Unknown,
        "whether you'd relocate"
    ));
    brazil.relocation = Some(false);
    let d = decide(&ny, &brazil);
    assert_eq!(d.status, Ineligible);
    assert!(d.headline.contains("not willing to relocate"));
    // Willing to relocate: still needs US work authorization.
    brazil.relocation = Some(true);
    let d = decide(&ny, &brazil);
    assert_eq!(d.status, Uncertain);
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Conditional,
        "relocating to New York"
    ));
    assert!(has_reason(
        &d,
        RuleId::Authorization,
        Verdict::Unknown,
        "authorization to work in the United States"
    ));
    brazil.authorized_in = vec![(
        jobhunt_eligibility::profile::place_area("United States"),
        "United States".into(),
    )];
    assert_eq!(status(&ny, &brazil), Conditional);
    brazil.authorized_in.clear();
    brazil.needs_sponsorship = Some(true);
    let sponsor = job(
        "greenhouse:example",
        "New York, NY",
        Some(WorkplaceType::OnSite),
        "We sponsor visas for this role and offer relocation assistance.",
    );
    assert_eq!(status(&sponsor, &brazil), Conditional);
    let no_sponsor = job(
        "greenhouse:example",
        "New York, NY",
        Some(WorkplaceType::OnSite),
        "We are unable to sponsor visas.",
    );
    assert_eq!(status(&no_sponsor, &brazil), Ineligible);

    // Local presence.
    let london = job(
        "lever:example",
        "London, United Kingdom",
        Some(WorkplaceType::Hybrid),
        "",
    );
    assert_eq!(status(&london, &at("London")), Eligible);
    let d = decide(&london, &at("Manchester, UK"));
    assert_eq!(d.status, Uncertain, "commuting distance unknown");
    let d = decide(&london, &at("United Kingdom"));
    assert!(d.headline.contains("only your country"));
    // A remote requirement rules out a hybrid job; remote is never assumed.
    let mut remote_only = at("London");
    remote_only.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    let d = decide(&london, &remote_only);
    assert_eq!(d.status, Ineligible);
    assert!(has_reason(
        &d,
        RuleId::WorkMode,
        Verdict::Fail,
        "you require remote"
    ));
}

#[test]
fn offices_named_in_the_description() {
    let j = job(
        "ashby:example",
        "",
        None,
        "This is a hybrid role in London, three days a week.",
    );
    let r = requirements(&j);
    assert_eq!(r.options.len(), 1);
    assert_eq!(r.options[0].label(), "Hybrid in London, United Kingdom");
    let j = job(
        "ashby:example",
        "",
        None,
        "This role is onsite in New York.",
    );
    assert_eq!(
        requirements(&j).options[0].label(),
        "On-site in New York, United States"
    );
    assert_eq!(status(&j, &at("New York")), Eligible);
}

#[test]
fn mixed_options_take_the_best_and_list_the_rest() {
    let j = job(
        "yc:example",
        "San Francisco, CA, US / Remote (US)",
        None,
        "",
    );
    let d = decide(&j, &at("Seattle"));
    assert_eq!(d.status, Eligible);
    assert_eq!(d.option.as_deref(), Some("Remote (United States)"));
    assert_eq!(d.other_options.len(), 1);
    assert_eq!(d.other_options[0].status, Uncertain);
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Uncertain, "the office path depends on relocation");
}

#[test]
fn time_zones() {
    let tz = |description: &str| remote("Remote - Americas", description);
    let mut sao_paulo = at("São Paulo, Brazil");
    // No time-zone language: no requirement, explicitly said so.
    let d = decide(&tz("We build developer tools."), &sao_paulo);
    assert_eq!(d.status, Eligible);
    assert!(has_reason(
        &d,
        RuleId::Timezone,
        Verdict::NotApplicable,
        "No time-zone requirement"
    ));
    // Compatible overlap.
    assert_eq!(
        status(
            &tz("You must overlap at least 4 hours with EST."),
            &sao_paulo
        ),
        Eligible
    );
    // An explicit UTC range.
    assert_eq!(
        status(
            &tz("You must be located between UTC-5 and UTC+1."),
            &sao_paulo
        ),
        Eligible
    );
    assert_eq!(
        status(
            &tz("You must be located between UTC-8 and UTC-5."),
            &sao_paulo
        ),
        Ineligible
    );
    // A named zone with an explicit tolerance.
    assert_eq!(status(&tz("Work EST ±3 hours."), &sao_paulo), Eligible);
    assert_eq!(status(&tz("Work PST ±1 hours."), &sao_paulo), Ineligible);
    // A named zone without tolerance, 5h away: can't tell.
    let d = decide(
        &tz("This role requires working Pacific time hours."),
        &sao_paulo,
    );
    assert_eq!(d.status, Uncertain);
    assert!(d.headline.contains("how much shift"));
    // Vague wording stays unknown.
    let d = decide(
        &tz("Some overlap with the team's time zone is needed."),
        &sao_paulo,
    );
    assert!(has_reason(
        &d,
        RuleId::Timezone,
        Verdict::Unknown,
        "without saying which zones"
    ));
    // Flexible hours.
    let d = decide(&tz("We are fully async with flexible hours."), &sao_paulo);
    assert!(has_reason(
        &d,
        RuleId::Timezone,
        Verdict::NotApplicable,
        "flexible"
    ));
    // The person's stated zone wins over their country's.
    sao_paulo.zones = zones_in("UTC-8");
    assert_eq!(
        status(
            &tz("This role requires working Pacific time hours."),
            &sao_paulo
        ),
        Eligible
    );
    // Unknown profile zone.
    let d = decide(
        &tz("You must overlap at least 4 hours with EST."),
        &ProfileFacts::default(),
    );
    assert!(has_reason(
        &d,
        RuleId::Timezone,
        Verdict::Unknown,
        "your time zone isn't known"
    ));
    // A country spanning zones: depends where.
    let d = decide(
        &remote("Remote (US)", "You must work Pacific time ±1 hours."),
        &at("United States"),
    );
    assert_eq!(d.status, Uncertain);
    assert_eq!(
        status(
            &remote("Remote (US)", "You must work Pacific time ±1 hours."),
            &at("Seattle")
        ),
        Eligible
    );
}

#[test]
fn visa_and_work_authorization() {
    let us_auth = |extra: &str| {
        remote(
            "Remote",
            &format!("Candidates must be authorized to work in the United States. {extra}"),
        )
    };
    let mut austin = at("Austin, TX");
    let d = decide(&us_auth(""), &austin);
    assert!(has_reason(
        &d,
        RuleId::Authorization,
        Verdict::Unknown,
        "doesn't say whether you hold it"
    ));
    austin.needs_sponsorship = Some(false);
    assert!(has_reason(
        &decide(&us_auth(""), &austin),
        RuleId::Authorization,
        Verdict::Pass,
        "don't need sponsorship"
    ));

    let mut brazil = at("São Paulo, Brazil");
    brazil.needs_sponsorship = Some(true);
    assert_eq!(
        status(&us_auth("We do not sponsor visas."), &brazil),
        Ineligible
    );
    let d = decide(&us_auth("We sponsor visas."), &brazil);
    assert!(has_reason(
        &d,
        RuleId::Authorization,
        Verdict::Conditional,
        "offers visa sponsorship"
    ));
    let d = decide(
        &us_auth("We do sponsor visas, but we aren't able to sponsor for every role."),
        &brazil,
    );
    assert!(has_reason(
        &d,
        RuleId::Authorization,
        Verdict::Unknown,
        "not for every role"
    ));
    // An explicit authorization the person holds.
    let mut portugal = at("Lisbon");
    portugal.authorized_in = vec![(
        jobhunt_eligibility::profile::place_area("the US"),
        "the US".into(),
    )];
    assert!(has_reason(
        &decide(&us_auth(""), &portugal),
        RuleId::Authorization,
        Verdict::Pass,
        "authorized to work in the US"
    ));
}

#[test]
fn no_sponsorship_is_not_no_international_applicants() {
    let j = remote("Remote - Americas", "We are unable to sponsor visas.");
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Eligible);
    assert!(has_reason(
        &d,
        RuleId::Authorization,
        Verdict::NotApplicable,
        "doesn't restrict remote work from Brazil"
    ));
}

#[test]
fn contractors_and_eor() {
    // "US only, no sponsorship" doesn't answer whether a contractor path
    // exists; without one it's the US-only answer.
    let j = remote("Remote (US)", "We do not sponsor visas.");
    assert_eq!(status(&j, &at("Recife, Brazil")), Ineligible);
    // An explicit contractor path for named countries.
    let j = remote(
        "Remote (US)",
        "Outside the US, we hire contractors in Brazil, Argentina and Mexico through Deel.",
    );
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Eligible);
    assert!(
        d.option
            .as_deref()
            .unwrap()
            .starts_with("Employer of record in")
    );
    assert_eq!(d.other_options[0].status, Ineligible);
    // A person who rules contractor work out.
    let j = remote("Remote (US)", "We hire B2B contractors in Brazil.");
    let mut no_contracts = at("Recife, Brazil");
    no_contracts.engagements = vec![(Engagement::Contractor, Stance::Unwanted)];
    assert_eq!(status(&j, &no_contracts), Ineligible);
    // "Remote worldwide, contractor": strong evidence.
    let mut j = remote("Remote - Worldwide", "");
    j.posting.employment_type = Some(EmploymentType::Contract);
    let d = decide(&j, &at("Nairobi, Kenya"));
    assert_eq!(d.status, Eligible);
    assert!(has_reason(
        &d,
        RuleId::Engagement,
        Verdict::NotApplicable,
        "contractor engagement is explicitly supported"
    ));
    // A person who requires contractor engagement, on a job that doesn't say.
    let mut contractor_only = at("Recife, Brazil");
    contractor_only.engagements = vec![(Engagement::Contractor, Stance::Required)];
    let d = decide(&remote("Remote - Americas", ""), &contractor_only);
    assert_eq!(d.status, Uncertain);
    assert!(has_reason(
        &d,
        RuleId::Engagement,
        Verdict::Unknown,
        "doesn't say whether contractors"
    ));
    let d = decide(
        &remote("Remote - Americas", "We don't work with contractors."),
        &contractor_only,
    );
    assert_eq!(d.status, Ineligible);
    // EOR supported, by name.
    let j = remote(
        "Remote",
        "We employ people in LATAM through an employer of record.",
    );
    assert_eq!(status(&j, &at("Bogotá")), Eligible);
}

#[test]
fn relocation_supported() {
    let j = job(
        "greenhouse:example",
        "Berlin, Germany",
        Some(WorkplaceType::OnSite),
        "We offer a relocation package and visa sponsorship.",
    );
    let mut sp = at("São Paulo, Brazil");
    sp.relocation = Some(true);
    sp.needs_sponsorship = Some(true);
    let d = decide(&j, &sp);
    assert_eq!(d.status, Conditional);
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Conditional,
        "offers relocation help"
    ));
}

#[test]
fn conflicting_evidence_is_kept_and_surfaced() {
    // Metadata global, description US only: the narrower description applies.
    let j = remote(
        "Remote - Worldwide",
        "This role is open to candidates based in the US only.",
    );
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Ineligible);
    assert!(!d.conflicts.is_empty());
    assert!(d.conflicts[0].summary.contains("remote anywhere"));
    assert!(has_reason(
        &d,
        RuleId::Ambiguity,
        Verdict::NotApplicable,
        "narrower restriction applies"
    ));
    assert_eq!(status(&j, &at("Austin")), Eligible);

    // Metadata US, description Americas: can't be settled for Brazil.
    let j = remote(
        "Remote (US)",
        "We are open to candidates across the Americas.",
    );
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Uncertain);
    assert!(has_reason(
        &d,
        RuleId::Ambiguity,
        Verdict::Unknown,
        "can't tell which applies"
    ));
    let ambiguity = d
        .reasons
        .iter()
        .find(|r| r.rule == RuleId::Ambiguity)
        .unwrap();
    assert_eq!(ambiguity.evidence.len(), 2, "both statements kept");
    assert_eq!(status(&j, &at("Austin")), Eligible);

    // "Remote", but the description requires living in NYC.
    let j = remote("Remote", "Applicants must live in New York City.");
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Ineligible);
    assert!(
        d.conflicts[0]
            .summary
            .contains("requires living in New York")
    );
    assert_eq!(status(&j, &at("New York")), Eligible);
}

#[test]
fn missing_profile_information_is_uncertainty() {
    let none = ProfileFacts::default();
    let d = decide(&remote("Remote - Brazil", ""), &none);
    assert_eq!(d.status, Uncertain);
    assert!(d.headline.contains("Your location isn't known"));
    assert_eq!(d.reasons[0].profile[0].basis, "missing");
    // "Anywhere" doesn't need to know.
    assert_eq!(status(&remote("Remote - Worldwide", ""), &none), Eligible);
    // An unrecognized location is not a guess.
    let d = decide(
        &remote("Remote - Brazil", ""),
        &ProfileFacts::living_in("Narnia"),
    );
    assert_eq!(d.status, Uncertain);
    assert!(d.headline.contains("doesn't recognize"));
    // A posting with no location at all.
    let d = decide(
        &job("ashby:example", "", None, "We build tools."),
        &at("Berlin"),
    );
    assert_eq!(d.status, Uncertain);
    assert!(d.headline.contains("doesn't say where"));
}

#[test]
fn remote_with_only_listed_cities_is_scoped_to_their_country() {
    // A finite list is the scope (BRU-325).
    let j = remote("New York, NY", "");
    assert_eq!(status(&j, &at("Chicago")), Eligible);
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Ineligible);
    assert!(
        d.headline
            .contains("lists only places in the United States")
    );
    // Beside an unscoped "Remote", a city only suggests it.
    let j = remote("New York, NY | Remote", "");
    let d = decide(&j, &at("Chicago"));
    assert_eq!(d.status, Uncertain);
    assert!(
        d.headline.contains("probably within the United States"),
        "{}",
        d.headline
    );
    let d = decide(&j, &at("Recife, Brazil"));
    assert_eq!(d.status, Uncertain);
    assert!(!d.headline.contains("probably"));
}

#[test]
fn decisions_serialize_for_storage() {
    let d = decide(&remote("Remote - Americas", ""), &at("Recife, Brazil"));
    let json = serde_json::to_string(&d).unwrap();
    let back: EligibilityDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

/// BRU-308: the work setup (remote only, hybrid okay, …) and relocation
/// are separate answers, and a posting that explicitly requires hybrid
/// presence in California or New York is a stated conflict for someone in
/// Brazil who requires remote work, never "unclear".
#[test]
fn work_setup_and_relocation_are_separate() {
    let hybrid_postings = [
        job(
            "greenhouse:example",
            "San Francisco, CA; New York, NY",
            Some(WorkplaceType::Hybrid),
            "",
        ),
        job(
            "ashby:example",
            "",
            None,
            "This is a hybrid role: you'll work from our San Francisco or New York office three days a week.",
        ),
        job(
            "lever:example",
            "San Francisco, California",
            Some(WorkplaceType::Hybrid),
            "Hybrid, in the office Tuesday to Thursday.",
        ),
    ];
    let brazil = || at("São Paulo, Brazil");

    for posting in &hybrid_postings {
        let what = requirements(posting)
            .options
            .iter()
            .map(|o| o.label())
            .collect::<Vec<_>>()
            .join(" / ");
        // Remote only, not willing to relocate: both are stated conflicts.
        let mut remote_only = brazil();
        remote_only.work_modes = vec![(WorkMode::Remote, Stance::Required)];
        remote_only.relocation = Some(false);
        let d = decide(posting, &remote_only);
        assert_eq!(d.status, Ineligible, "{what}");
        assert!(
            has_reason(&d, RuleId::WorkMode, Verdict::Fail, "you require remote"),
            "{what}: {:?}",
            d.reasons
        );
        assert!(
            has_reason(
                &d,
                RuleId::Presence,
                Verdict::Fail,
                "not willing to relocate"
            ),
            "{what}"
        );

        // Remote only, relocation not said: the work setup alone rules it out.
        let mut remote_only = brazil();
        remote_only.work_modes = vec![(WorkMode::Remote, Stance::Required)];
        let d = decide(posting, &remote_only);
        assert_eq!(d.status, Ineligible, "{what}");

        // Prefer remote is ranking-only: eligibility says nothing about
        // it, and relocation (not said) is what is unknown.
        let mut prefer = brazil();
        prefer.work_modes = vec![(WorkMode::Remote, Stance::Wanted)];
        let d = decide(posting, &prefer);
        assert_eq!(d.status, Uncertain, "{what}");
        assert!(!d.reasons.iter().any(|r| r.rule == RuleId::WorkMode));

        // Not willing to relocate, without any work-setup requirement:
        // relocation is its own conflict.
        let mut stays = brazil();
        stays.relocation = Some(false);
        let d = decide(posting, &stays);
        assert_eq!(d.status, Ineligible, "{what}");
        assert!(!d.reasons.iter().any(|r| r.rule == RuleId::WorkMode));
        assert!(has_reason(
            &d,
            RuleId::Presence,
            Verdict::Fail,
            "not willing to relocate"
        ));
    }

    // "Hybrid okay" (remote or hybrid) accepts the setup; relocation and
    // authorization still decide.
    let sf = &hybrid_postings[0];
    let mut hybrid_ok = brazil();
    hybrid_ok.work_modes = vec![
        (WorkMode::Remote, Stance::Required),
        (WorkMode::Hybrid, Stance::Required),
    ];
    hybrid_ok.relocation = Some(true);
    let d = decide(sf, &hybrid_ok);
    assert!(has_reason(&d, RuleId::WorkMode, Verdict::Pass, "hybrid"));
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Conditional,
        "relocating to San Francisco"
    ));
    // An on-site office is outside "hybrid okay".
    let onsite = job(
        "greenhouse:example",
        "New York, NY",
        Some(WorkplaceType::OnSite),
        "",
    );
    let d = decide(&onsite, &hybrid_ok);
    assert_eq!(d.status, Ineligible);
    assert!(has_reason(&d, RuleId::WorkMode, Verdict::Fail, "on-site"));
}

#[test]
fn relocation_only_to_selected_places() {
    use jobhunt_eligibility::geo::Membership;
    use jobhunt_eligibility::profile::place_area;
    let only = |places: &[&str]| {
        let mut p = at("São Paulo, Brazil");
        p.relocation = Some(true);
        p.relocation_only_to = places
            .iter()
            .map(|place| (place_area(place), (*place).to_owned()))
            .collect();
        p
    };
    let ny = job(
        "greenhouse:example",
        "New York, NY",
        Some(WorkplaceType::Hybrid),
        "",
    );
    let lisbon = job(
        "lever:example",
        "Lisbon, Portugal",
        Some(WorkplaceType::Hybrid),
        "",
    );
    // Every destination certainly elsewhere: a conflict.
    let d = decide(&ny, &only(&["Portugal", "Spain"]));
    assert_eq!(d.status, Ineligible);
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Fail,
        "you'd only relocate to Portugal or Spain"
    ));
    // A certain match: the move is the condition.
    let d = decide(&lisbon, &only(&["Portugal", "Spain"]));
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Conditional,
        "relocating to Lisbon"
    ));
    // Europe includes Portugal; a region counts.
    assert_eq!(
        only(&["Europe"]).would_relocate_to(place_area("Lisbon").unwrap()),
        Some(Membership::Yes)
    );
    assert_eq!(status(&ny, &only(&["Europe"])), Ineligible);

    // Codex review #4: a destination Narrow can't recognize (a typo) is
    // unknown, never a conflict.
    let typo = only(&["Portugall"]);
    assert_eq!(
        typo.relocation_only_to[0].0, None,
        "the typo isn't recognized"
    );
    for office in [&lisbon, &ny] {
        let d = decide(office, &typo);
        assert_eq!(d.status, Uncertain, "{:?}", d.reasons);
        assert!(has_reason(
            &d,
            RuleId::Presence,
            Verdict::Unknown,
            "can't tell whether it is among the places you'd relocate to (Portugall)"
        ));
    }
    // Known and unknown alternatives: a certain match still matches; a
    // known conflict next to an unknown one is unresolved.
    let mixed = only(&["Spain", "Portugall"]);
    let d = decide(&ny, &mixed);
    assert_eq!(d.status, Uncertain);
    assert!(!d.reasons.iter().any(|r| r.verdict == Verdict::Fail));
    let d = decide(&lisbon, &only(&["Portugal", "Narnia"]));
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Conditional,
        "relocating to Lisbon"
    ));
    // A state named as the destination and an office city of that country:
    // can't say; another country's city: a conflict.
    let california = only(&["California"]);
    assert_eq!(
        california.would_relocate_to(place_area("San Francisco, CA").unwrap()),
        Some(Membership::Maybe)
    );
    assert_eq!(
        california.would_relocate_to(place_area("Lisbon").unwrap()),
        Some(Membership::No)
    );
    assert_eq!(
        only(&["Lisbon"]).would_relocate_to(place_area("Porto, Portugal").unwrap()),
        Some(Membership::No)
    );
}

/// Codex review (e88868c): an office named only by a broad region may be in
/// the city someone would move to: unresolved, never a conflict. Missing
/// detail is never turned into "no".
#[test]
fn broad_office_locations_leave_relocation_unresolved() {
    use jobhunt_eligibility::geo::Membership::{Maybe, No, Yes};
    use jobhunt_eligibility::profile::place_area;
    let only = |places: &[&str]| {
        let mut p = at("São Paulo, Brazil");
        p.relocation = Some(true);
        p.relocation_only_to = places
            .iter()
            .map(|place| (place_area(place), (*place).to_owned()))
            .collect();
        p
    };
    let area = |place: &str| place_area(place).unwrap_or_else(|| panic!("{place} is recognized"));
    let lisbon = only(&["Lisbon"]);
    assert_eq!(lisbon.would_relocate_to(area("Lisbon")), Some(Yes));
    assert_eq!(lisbon.would_relocate_to(area("Madrid")), Some(No));
    assert_eq!(lisbon.would_relocate_to(area("Europe")), Some(Maybe));
    assert_eq!(lisbon.would_relocate_to(area("Portugal")), Some(Maybe));
    assert_eq!(
        lisbon.would_relocate_to(area("Asia")),
        Some(No),
        "Asia can't hold Lisbon"
    );
    let portugal = only(&["Portugal"]);
    assert_eq!(portugal.would_relocate_to(area("Europe")), Some(Maybe));
    assert_eq!(portugal.would_relocate_to(area("Lisbon")), Some(Yes));
    assert_eq!(portugal.would_relocate_to(area("Latin America")), Some(No));
    // Several destinations: any certain match, all certain conflicts,
    // otherwise unresolved.
    assert_eq!(
        only(&["Madrid", "Lisbon"]).would_relocate_to(area("Lisbon")),
        Some(Yes)
    );
    assert_eq!(
        only(&["Madrid", "Berlin"]).would_relocate_to(area("Lisbon")),
        Some(No)
    );
    assert_eq!(
        only(&["Madrid", "Lisbon"]).would_relocate_to(area("Europe")),
        Some(Maybe)
    );
    assert_eq!(
        only(&["Tokyo", "Lisbon"]).would_relocate_to(area("Asia")),
        Some(Maybe)
    );
    assert_eq!(
        only(&["Madrid", "Lisbon"]).would_relocate_to(area("Asia")),
        Some(No)
    );

    // Through the rules: a hybrid office "in Europe" is uncertain, not
    // ineligible, for someone who'd only move to Lisbon.
    let europe = job("lever:example", "Europe", Some(WorkplaceType::Hybrid), "");
    let d = decide(&europe, &lisbon);
    assert_eq!(d.status, Uncertain, "{:?}", d.reasons);
    assert!(
        !d.reasons.iter().any(|r| r.verdict == Verdict::Fail),
        "{:?}",
        d.reasons
    );
    assert!(has_reason(
        &d,
        RuleId::Presence,
        Verdict::Unknown,
        "can't tell whether it is among the places you'd relocate to (Lisbon)"
    ));
    let madrid = job(
        "lever:example",
        "Madrid, Spain",
        Some(WorkplaceType::Hybrid),
        "",
    );
    assert_eq!(status(&madrid, &lisbon), Ineligible);
}

fn source_location(
    name: &str,
    locality: Option<&str>,
    region: Option<&str>,
) -> jobhunt_jobs::SourceLocation {
    jobhunt_jobs::SourceLocation {
        name: Some(name.into()),
        locality: locality.map(Into::into),
        region: region.map(Into::into),
        country: Some("US".into()),
    }
}

fn remote_only_in_brazil() -> ProfileFacts {
    let mut f = at("Dourados, Brazil");
    f.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    f.relocation = Some(false);
    f
}

/// Production smoke test (Ramp): offices listed first, then "Remote (US)".
/// The stated remote scope wins over the scope the offices only suggest,
/// so it is a US-only remote job, not "scope unclear".
#[test]
fn a_stated_remote_scope_outranks_one_inferred_from_offices() {
    let mut ramp = job(
        "ashby:ramp",
        "New York, NY (HQ)",
        Some(WorkplaceType::Remote),
        "Ramp is building the smart infrastructure for finance teams. Budget for intra-office travel. Relocation expense coverage to NYC or SF (if needed).",
    );
    ramp.posting.is_remote = Some(true);
    ramp.posting.locations = vec![
        source_location("New York, NY (HQ)", Some("New York"), Some("NY")),
        source_location("San Francisco, CA", Some("San Francisco"), Some("CA")),
        source_location("Remote (US)", None, None),
    ];
    let r = requirements(&ramp);
    assert_eq!(r.options.len(), 1);
    assert_eq!(r.options[0].label(), "Remote (United States)");
    assert!(
        r.presence_policy.is_empty(),
        "perks aren't a presence policy: {:?}",
        r.presence_policy
    );
    let d = decide(&ramp, &remote_only_in_brazil());
    assert_eq!(d.status, Ineligible, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::CountryConstraint,
        Verdict::Fail,
        "limits remote work to the United States"
    ));
    // The same order reversed, and a stated scope alone, read the same.
    ramp.posting.locations.reverse();
    assert_eq!(
        requirements(&ramp).options[0].label(),
        "Remote (United States)"
    );
    // Remote alone is never global.
    let mut bare = ramp.clone();
    bare.posting.location = Some("Remote".into());
    bare.posting.locations = Vec::new();
    assert_eq!(status(&bare, &remote_only_in_brazil()), Uncertain);
}

/// Production smoke test (Anthropic): "Remote-Friendly (Travel-Required)"
/// and a company-wide office policy. For someone who requires remote work
/// that is unresolved and says why; never "the position is remote, as you
/// require".
#[test]
fn a_presence_policy_on_a_remote_job_is_unresolved() {
    let anthropic = job(
        "greenhouse:anthropic",
        "Remote-Friendly (Travel-Required) | San Francisco, CA | Seattle, WA | New York City, NY",
        None,
        "You'll build data infrastructure. Location-based hybrid policy: Currently, we expect all staff to be in one of our offices at least 25% of the time. However, some roles may require more time in our offices.",
    );
    let r = requirements(&anthropic);
    assert!(
        r.presence_policy
            .iter()
            .any(|e| e.text.contains("at least 25% of the time")),
        "{:?}",
        r.presence_policy
    );
    assert!(
        r.presence_policy
            .iter()
            .any(|e| e.text.contains("Travel-Required"))
    );
    let d = decide(&anthropic, &remote_only_in_brazil());
    assert_eq!(d.status, Uncertain, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::WorkMode,
        Verdict::Unknown,
        "it also expects office presence or travel"
    ));
    assert!(
        !has_reason(&d, RuleId::WorkMode, Verdict::Pass, "as you require"),
        "never Fine while the posting expects presence"
    );
    // Someone fine with hybrid isn't asked: the policy is within their setup.
    let mut hybrid_ok = remote_only_in_brazil();
    hybrid_ok.work_modes = vec![
        (WorkMode::Remote, Stance::Required),
        (WorkMode::Hybrid, Stance::Required),
    ];
    assert!(has_reason(
        &decide(&anthropic, &hybrid_ok),
        RuleId::WorkMode,
        Verdict::Pass,
        "remote"
    ));

    // A hybrid cadence without a city, on a remote-classified job.
    let cadence = remote(
        "Remote - Brazil",
        "This is a hybrid role: you'll be in the office two days a week.",
    );
    assert_eq!(requirements(&cadence).presence_policy.len(), 1);
    assert_eq!(status(&cadence, &remote_only_in_brazil()), Uncertain);

    // Perks, options and software are not presence policies.
    for text in [
        "There are no Supabase offices, but we provide a WeWork membership or co-working allowance you can use anywhere in the world.",
        "Budget for intra-office travel.",
        "Experience with Microsoft Office is required.",
        "A home office stipend is provided every month.",
        "Office attendance is optional.",
    ] {
        let j = remote("Remote - Brazil", text);
        assert!(requirements(&j).presence_policy.is_empty(), "{text}");
        assert_eq!(status(&j, &remote_only_in_brazil()), Eligible, "{text}");
    }
}

/// A remote job whose remote words are only in its location fields still
/// cites them: never "not stated" next to "the position is remote".
#[test]
fn remote_evidence_comes_from_the_location_fields_too() {
    let supabase = job(
        "ashby:supabase",
        "Remote, Global",
        None,
        "We hire globally.",
    );
    let d = decide(&supabase, &remote_only_in_brazil());
    let work_mode = d
        .reasons
        .iter()
        .find(|r| r.rule == RuleId::WorkMode)
        .unwrap();
    assert_eq!(work_mode.verdict, Verdict::Pass);
    assert!(
        work_mode
            .evidence
            .iter()
            .any(|e| e.text.contains("Remote, Global")),
        "{:?}",
        work_mode.evidence
    );
}
