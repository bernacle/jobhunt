//! Hiring scope (BRU-325's eligibility errors): explicit US scopes, finite
//! location lists, hiring language in the description, region-coded
//! fields a role statement widens, and pay or company sentences that are
//! not hiring scope. Each posting is a synthetic copy of a real one's
//! location fields and the few sentences that decide it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::job;
use jobhunt_eligibility::decision::{Eligibility, EligibilityDecision, RuleId, Verdict};
use jobhunt_eligibility::job::{RemoteScope, ScopeBasis};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{evaluate_record, requirements};
use jobhunt_jobs::{JobRecord, SourceLocation, WorkplaceType};
use jobhunt_profile::{Stance, WorkMode};

use Eligibility::{Eligible, Ineligible, Uncertain};

/// BRU-325's candidate: São Paulo, remote only, no relocation.
fn brazil() -> ProfileFacts {
    living("São Paulo, Brazil")
}

fn living(place: &str) -> ProfileFacts {
    let mut p = ProfileFacts::living_in(place);
    assert!(p.country().is_some(), "{place} is recognized");
    p.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    p.relocation = Some(false);
    p
}

fn place(name: &str, country: Option<&str>) -> SourceLocation {
    SourceLocation {
        name: Some(name.into()),
        locality: None,
        region: None,
        country: country.map(Into::into),
    }
}

/// A posting with a primary location, structured locations and a
/// description.
fn posting(
    location: &str,
    locations: &[SourceLocation],
    workplace: Option<WorkplaceType>,
    description: &str,
) -> JobRecord {
    let mut j = job("ashby:example", location, workplace, description);
    j.posting.locations = locations.to_vec();
    j
}

fn decide(j: &JobRecord, p: &ProfileFacts) -> EligibilityDecision {
    evaluate_record(j, p)
}

fn has_reason(d: &EligibilityDecision, rule: RuleId, verdict: Verdict, words: &str) -> bool {
    d.reasons
        .iter()
        .any(|r| r.rule == rule && r.verdict == verdict && r.conclusion.contains(words))
}

// ---------------------------------------------------------------------------
// 1. Explicit US scope

#[test]
fn explicit_us_scopes_are_us_only() {
    for location in [
        "US-Remote",
        "Remote - US",
        "Remote US",
        "Remote (United States)",
        "Remote-US",
        "Remote – United States",
    ] {
        let j = posting(location, &[], Some(WorkplaceType::Remote), "");
        assert_eq!(decide(&j, &brazil()).status, Ineligible, "{location}");
        assert_eq!(
            decide(&j, &living("Austin, TX")).status,
            Eligible,
            "{location}"
        );
    }
}

#[test]
fn stripe_us_remote_among_office_cities() {
    // Stripe, Backend Engineer, Core Technology.
    let j = posting(
        "US-Remote, Chicago, Seattle, San Francisco",
        &[place("US", None)],
        None,
        "",
    );
    let r = requirements(&j);
    assert_eq!(r.options[0].label(), "Remote (United States)");
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Ineligible, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::CountryConstraint,
        Verdict::Fail,
        "limits remote work to the United States"
    ));
    assert_eq!(decide(&j, &living("Denver, CO")).status, Eligible);
}

#[test]
fn a_bare_remote_beside_a_country_is_scoped_to_it() {
    // Stripe, Integration Engineer (Metronome): "Remote", location "US".
    for country in ["US", "United States"] {
        let j = posting("Remote", &[place(country, None)], None, "");
        assert_eq!(
            requirements(&j).options[0].label(),
            "Remote (United States)",
            "{country}"
        );
        assert_eq!(decide(&j, &brazil()).status, Ineligible, "{country}");
        assert_eq!(decide(&j, &living("Chicago")).status, Eligible, "{country}");
    }
    // A structured country attribute on "Remote" is the source's default
    // (PostHog lists "Remote"/"USA" on global roles), not a scope.
    let j = posting(
        "Remote",
        &[place("Remote", Some("USA"))],
        Some(WorkplaceType::Remote),
        "",
    );
    assert_eq!(decide(&j, &brazil()).status, Uncertain);
}

// ---------------------------------------------------------------------------
// 2. Finite location lists

#[test]
fn a_remote_job_listing_only_us_cities_is_us_only() {
    // Temporal, Senior Software Engineer, Cloud Platform Foundations.
    let j = posting(
        "Seattle, Washington",
        &[
            place("Seattle, Washington", Some("United States")),
            place("Austin, Texas", Some("United States")),
            place("San Francisco, California", Some("United States")),
        ],
        Some(WorkplaceType::Remote),
        "",
    );
    let Some(RemoteScope::Areas(areas)) = requirements(&j).remote_option().cloned() else {
        panic!("expected a scope");
    };
    assert!(areas.iter().all(|a| a.basis == ScopeBasis::Listed));
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Ineligible, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::CountryConstraint,
        Verdict::Fail,
        "lists only places in the United States"
    ));
    // Elsewhere in the country is fine: the list scopes remote work, it
    // doesn't require those cities.
    assert_eq!(decide(&j, &living("Denver, CO")).status, Eligible);
}

#[test]
fn a_remote_job_listing_only_uk_cities_is_uk_only() {
    // Temporal, Senior Platform Architect.
    let uk = [
        place("London, England", Some("United Kingdom")),
        place("Manchester, England", Some("United Kingdom")),
    ];
    let j = posting("London, England", &uk, Some(WorkplaceType::Remote), "");
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
    assert_eq!(
        decide(&j, &living("Leeds, United Kingdom")).status,
        Eligible
    );
    // Unless the description widens it.
    let j = posting(
        "London, England",
        &uk,
        Some(WorkplaceType::Remote),
        "This role is open to candidates based in Europe or Latin America.",
    );
    assert_eq!(decide(&j, &brazil()).status, Eligible);
}

#[test]
fn a_finite_set_of_countries_is_the_scope() {
    let j = posting(
        "London, UK",
        &[place("London, UK", None), place("Toronto, Canada", None)],
        Some(WorkplaceType::Remote),
        "",
    );
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
    assert_eq!(decide(&j, &living("Vancouver, Canada")).status, Eligible);
}

#[test]
fn a_city_beside_an_unscoped_remote_stays_unresolved() {
    // "Remote" and an office ("Remote, San Francisco, CA"): the city may be
    // the office of a role open elsewhere. Not a finite list.
    let j = posting(
        "Remote, San Francisco, CA",
        &[],
        Some(WorkplaceType::Remote),
        "",
    );
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Uncertain, "{:?}", d.reasons);
    assert!(
        d.headline
            .contains("doesn't say where remote work is allowed")
    );
}

// ---------------------------------------------------------------------------
// 3. Hiring language in the description

#[test]
fn based_within_a_region_is_a_hiring_scope() {
    let j = posting(
        "Remote",
        &[],
        Some(WorkplaceType::Remote),
        "To create the best experience for you and to meet our business needs, this role requires you to be based within EMEA.",
    );
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
    assert_eq!(decide(&j, &living("Lisbon, Portugal")).status, Eligible);
    for sentence in [
        "Candidates must be located in Brazil or Argentina.",
        "This role is available to applicants in Latin America.",
        "We can hire in Brazil, Mexico and Colombia.",
        "Eligible countries include Brazil, Chile and Peru.",
    ] {
        let j = posting("Remote", &[], Some(WorkplaceType::Remote), sentence);
        assert_eq!(decide(&j, &brazil()).status, Eligible, "{sentence}");
        assert_eq!(
            decide(&j, &living("Berlin, Germany")).status,
            Ineligible,
            "{sentence}"
        );
    }
}

#[test]
fn oyster_emea_role_at_an_employer_of_record() {
    // Oyster's own postings name Oyster, an employer of record, and say
    // "work from anywhere" about the company. This role is EMEA only.
    let mut j = posting(
        "Spain",
        &[
            place("Spain", Some("Spain")),
            place("Romania", Some("Romania")),
            place("Portugal", Some("Portugal")),
            place("Slovakia", Some("Slovakia")),
            place("Poland", Some("Poland")),
        ],
        Some(WorkplaceType::Remote),
        "Oyster set out to close that gap - building a global employment platform that lets companies hire, pay, and care for brilliant people anywhere.\n\
         To create the best experience for you and to meet our business needs, this role requires you to be based within EMEA.\n\
         - Work from anywhere: Oyster has no borders or HQ.\n\
         As long as work is timely, your team is supported, and you're authorized to work where you live, you can work from anywhere.",
    );
    j.posting.company = "Oyster".into();
    let r = requirements(&j);
    assert!(r.mechanisms.is_empty(), "{:?}", r.mechanisms);
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Ineligible, "{:?}", d.reasons);
    assert_eq!(decide(&j, &living("Lisbon, Portugal")).status, Eligible);
    // Another company's posting naming Oyster as its EOR is still a path.
    j.posting.company = "ExampleCo".into();
    j.posting.description_text =
        Some("We hire through Oyster, our employer of record, anywhere in the world.".into());
    j.posting.locations.clear();
    j.posting.location = Some("Remote".into());
    assert_eq!(decide(&j, &brazil()).status, Eligible);
}

#[test]
fn location_labels_are_read_clause_by_clause() {
    // Temporal: the preference is the office's; remote is US only.
    let j = posting(
        "San Francisco, California",
        &[place("San Francisco, California", Some("United States"))],
        Some(WorkplaceType::Hybrid),
        "Location: San Francisco Bay Area (strongly preferred); remote (US) considered with heavier travel",
    );
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
    // Canonical: "any American region" is the Americas.
    let j = posting(
        "Home based - Worldwide",
        &[],
        None,
        "Location: This role can be filled in European, Middle East, African or any American region / time zone.",
    );
    assert_eq!(decide(&j, &brazil()).status, Eligible);
    assert_eq!(decide(&j, &living("Tokyo, Japan")).status, Ineligible);
}

#[test]
fn a_location_label_can_say_the_job_is_remote() {
    // Oyster: no workplace type, places listed, and "Location: Fully
    // remote." in the description.
    let mut j = posting(
        "EMEA",
        &[
            place("EMEA", None),
            SourceLocation {
                name: Some("Brazil".into()),
                locality: Some("São Paulo".into()),
                region: None,
                country: Some("Brazil".into()),
            },
        ],
        None,
        "Location: Fully remote.\n\
         To create the best experience for you and to meet our business needs, this role requires you to be based within the EMEA or AMER regions.",
    );
    j.posting.company = "Oyster".into();
    assert_eq!(decide(&j, &brazil()).status, Eligible);
    assert_eq!(decide(&j, &living("Tokyo, Japan")).status, Ineligible);
    // A label that says it isn't remote adds nothing.
    let j = posting(
        "London, UK",
        &[],
        None,
        "Location: London office, not remote.",
    );
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
}

// ---------------------------------------------------------------------------
// 4. Region-coded fields a role statement widens

#[test]
fn zapier_americas_widens_namer() {
    let locations = [
        SourceLocation {
            name: Some("NAMER".into()),
            locality: Some("San Francisco".into()),
            region: Some("California".into()),
            country: Some("United States".into()),
        },
        place("APAC", None),
        place("EMEA", None),
    ];
    // The fields alone: North America, APAC or EMEA.
    let fields = posting("NAMER", &locations, Some(WorkplaceType::Remote), "");
    assert_eq!(decide(&fields, &brazil()).status, Ineligible);
    assert_eq!(decide(&fields, &living("Toronto")).status, Eligible);
    // The description's "Location:" line includes South America.
    let j = posting(
        "NAMER",
        &locations,
        Some(WorkplaceType::Remote),
        "Location: Americas - North, Central and South America, EMEA, APAC\n\
         Even though we're an all-remote company, we still need to be thoughtful about where we have Zapiens working.",
    );
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Eligible, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::Ambiguity,
        Verdict::NotApplicable,
        "this role's scope more widely"
    ));
    assert_eq!(decide(&j, &living("Berlin, Germany")).status, Eligible);
}

// ---------------------------------------------------------------------------
// 5. Pay and terms for some hires are not hiring scope

#[test]
fn wikimedia_pay_range_is_not_a_hiring_scope() {
    let pay = "The anticipated annual pay range of this position for applicants based within the United States is US$143,478 to US$217,585 with multiple individualized factors, including cost of living in the location, being the determinants of the offered pay.\n\
               For applicants located outside of the US, the pay range will be adjusted to the country of hire.\n\
               For US-based applicants: this position is part of the Communication Workers of America bargaining unit.";
    let countries = "*Please note that we are currently able to hire in the following:\n\
                     Countries: Brazil, Canada, Colombia, France, Germany, Ghana, India, Indonesia, Italy, Kenya*, Mexico, Morocco, Netherlands, Poland, Singapore*, South Africa, Spain, Switzerland and the United Kingdom.";
    let j = posting(
        "Remote",
        &[place("Remote", None)],
        Some(WorkplaceType::Remote),
        &format!("{pay}\n{countries}"),
    );
    let r = requirements(&j);
    assert!(
        r.allow
            .iter()
            .all(|c| c.area.to_string() != "United States"),
        "{:?}",
        r.allow
    );
    assert_eq!(decide(&j, &brazil()).status, Eligible);
    assert_eq!(decide(&j, &living("Berlin, Germany")).status, Eligible);
    assert_eq!(decide(&j, &living("Tokyo, Japan")).status, Ineligible);
    // The pay sentences alone don't scope the job.
    let j = posting("Remote", &[], Some(WorkplaceType::Remote), pay);
    assert_eq!(decide(&j, &brazil()).status, Uncertain);
    // Nor does a salary sentence.
    let j = posting(
        "Remote",
        &[],
        Some(WorkplaceType::Remote),
        "The base salary range for candidates based in the United States is $150,000 - $190,000.",
    );
    assert_eq!(decide(&j, &brazil()).status, Uncertain);
}

// ---------------------------------------------------------------------------
// 6. Global roles stay global

#[test]
fn global_roles_stay_global() {
    for (location, description) in [
        ("Remote, Global", ""),
        ("Worldwide", ""),
        ("Anywhere", ""),
        ("Remote - Worldwide", ""),
        (
            "Remote",
            "We are a globally distributed team and we hire from anywhere in the world.",
        ),
        ("Remote", "This role is fully remote: work from anywhere."),
    ] {
        let j = posting(location, &[], Some(WorkplaceType::Remote), description);
        assert_eq!(decide(&j, &brazil()).status, Eligible, "{location}");
    }
    // A stated "anywhere" isn't narrowed by an office the posting lists.
    let j = posting(
        "Remote - Worldwide",
        &[place("San Francisco, CA", Some("US"))],
        Some(WorkplaceType::Remote),
        "",
    );
    assert_eq!(decide(&j, &brazil()).status, Eligible);
    // A worldwide role whose listed city is only its HQ, said so.
    let j = posting(
        "San Francisco, CA",
        &[],
        Some(WorkplaceType::Remote),
        "This role is open to candidates anywhere in the world.",
    );
    assert_eq!(decide(&j, &brazil()).status, Eligible);
}

// ---------------------------------------------------------------------------
// 7. Conflicts

#[test]
fn a_role_statement_settles_a_conflict_a_company_statement_does_not() {
    // Railway: "Remote (United States)"; "This is a remote position
    // available anywhere in the world! Linkedin makes us show a country".
    let j = posting(
        "Remote (United States)",
        &[place("Remote (United States)", Some("United States"))],
        Some(WorkplaceType::Remote),
        "This is a remote position available anywhere in the world! Linkedin makes us show a country, but we hire the best people wherever they are.",
    );
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Eligible, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::Ambiguity,
        Verdict::NotApplicable,
        "the description's statement applies"
    ));
    // About hiring in general, it can't be settled.
    let j = posting(
        "Remote (United States)",
        &[],
        Some(WorkplaceType::Remote),
        "We hire the best people wherever they are, from anywhere in the world.",
    );
    let d = decide(&j, &brazil());
    assert_eq!(d.status, Uncertain, "{:?}", d.reasons);
    assert!(has_reason(
        &d,
        RuleId::Ambiguity,
        Verdict::Unknown,
        "can't tell which applies"
    ));
    // A narrower statement applies whoever it is about.
    let j = posting(
        "Remote - Worldwide",
        &[],
        Some(WorkplaceType::Remote),
        "We only hire people based in the US.",
    );
    assert_eq!(decide(&j, &brazil()).status, Ineligible);
}
