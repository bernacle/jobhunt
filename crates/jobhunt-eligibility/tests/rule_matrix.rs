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
    let d = decide(&london, &at("Manchester"));
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
        &ProfileFacts::living_in("Atlantis"),
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
fn remote_with_only_an_office_city_is_not_a_scope() {
    let j = remote("New York, NY", "");
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
