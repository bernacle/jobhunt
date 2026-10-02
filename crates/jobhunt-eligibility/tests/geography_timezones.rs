//! BRU-310: GeoNames places and DST-aware time zones, end to end through
//! the rules. Where someone lives, where they may work, where the job
//! allows remote work, whether they'd relocate and whether the hours fit
//! stay separate answers: each is its own reason, and none stands in for
//! another.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::job;
use jobhunt_eligibility::decision::{Eligibility, EligibilityDecision, Reason, RuleId, Verdict};
use jobhunt_eligibility::geo::Area;
use jobhunt_eligibility::job::WorkOption;
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::zones::zones_in;
use jobhunt_eligibility::{evaluate_record, requirements};
use jobhunt_jobs::{JobRecord, SourceLocation, WorkplaceType};

use Eligibility::{Eligible, Ineligible, Uncertain};

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

fn reason(d: &EligibilityDecision, rule: RuleId) -> &Reason {
    d.reasons
        .iter()
        .find(|r| r.rule == rule)
        .unwrap_or_else(|| panic!("a {rule:?} reason in {:#?}", d.reasons))
}

fn timezone(d: &EligibilityDecision) -> &Reason {
    reason(d, RuleId::Timezone)
}

/// The geography reason of a remote option: its scope, or the region or
/// country it is limited to.
fn geography(d: &EligibilityDecision) -> &Reason {
    d.reasons
        .iter()
        .find(|r| {
            matches!(
                r.rule,
                RuleId::RemoteScope | RuleId::RegionConstraint | RuleId::CountryConstraint
            )
        })
        .unwrap_or_else(|| panic!("a geography reason in {:#?}", d.reasons))
}

#[test]
fn a_gap_that_daylight_saving_time_changes_is_judged_by_season() {
    // São Paulo keeps UTC-3 all year; New York moves between UTC-5 and
    // UTC-4. One hour of tolerance fits from March to October only.
    let j = remote("Remote - Americas", "Work EST ±1 hours.");
    let d = evaluate_record(&j, &at("São Paulo, Brazil"));
    assert_eq!(d.status, Uncertain, "{:#?}", d.reasons);
    assert_eq!(geography(&d).verdict, Verdict::Pass);
    let tz = timezone(&d);
    assert_eq!(tz.verdict, Verdict::Unknown);
    assert!(
        tz.conclusion.contains("only part of the year")
            && tz.conclusion.contains("1h away Mar 8–Oct 31")
            && tz.conclusion.contains("2h away Nov 1–Mar 7"),
        "{}",
        tz.conclusion
    );
    // Two hours of tolerance fit all year.
    let j = remote("Remote - Americas", "Work EST ±2 hours.");
    let d = evaluate_record(&j, &at("São Paulo, Brazil"));
    assert_eq!(d.status, Eligible);
    assert_eq!(timezone(&d).verdict, Verdict::Pass);
}

#[test]
fn zones_that_change_on_different_dates() {
    // London and New York are five hours apart, except for the weeks when
    // only the US has moved its clocks: four.
    let j = remote("Remote - Europe", "You must work Eastern time ±4 hours.");
    let d = evaluate_record(&j, &at("London, UK"));
    let tz = timezone(&d);
    assert_eq!(tz.verdict, Verdict::Unknown, "{}", tz.conclusion);
    assert!(
        tz.conclusion.contains("5h away Mar 29–Oct 24, Nov 1–Mar 7"),
        "{}",
        tz.conclusion
    );
    assert!(
        tz.conclusion
            .contains("4h away Mar 8–Mar 28, Oct 25–Oct 31"),
        "{}",
        tz.conclusion
    );
}

#[test]
fn zones_without_daylight_saving_time_are_the_same_all_year() {
    // Bogotá and Lima: both UTC-5, neither changes.
    let j = remote(
        "Remote - LATAM",
        "You must overlap at least 6 hours with Lima time.",
    );
    let d = evaluate_record(&j, &at("Bogotá, Colombia"));
    assert_eq!(d.status, Eligible, "{:#?}", d.reasons);
    let tz = timezone(&d);
    assert_eq!(tz.verdict, Verdict::Pass);
    assert!(tz.conclusion.contains("you'd have 8h"), "{}", tz.conclusion);
    // The person's clock is their city's own IANA zone.
    let fact = &tz.profile[0];
    assert_eq!(fact.value, "America/Bogota (UTC-5)");
}

#[test]
fn the_city_zone_is_used_not_the_capital() {
    // Dourados (Mato Grosso do Sul) is UTC-4, an hour from São Paulo.
    let j = remote("Remote - Brazil", "You must be located within UTC-3.");
    let d = evaluate_record(&j, &at("Dourados, Brazil"));
    let tz = timezone(&d);
    assert_eq!(tz.verdict, Verdict::Fail, "{}", tz.conclusion);
    assert!(tz.conclusion.contains("1h outside it"), "{}", tz.conclusion);
    let d = evaluate_record(&j, &at("São Paulo, Brazil"));
    assert_eq!(timezone(&d).verdict, Verdict::Pass);
}

#[test]
fn geography_can_fit_while_the_hours_do_not() {
    // Brazil is in the Americas; Pacific time is 4 to 5 hours away.
    let j = remote(
        "Remote - Americas",
        "You must overlap at least 6 hours with Pacific time.",
    );
    let d = evaluate_record(&j, &at("São Paulo, Brazil"));
    assert_eq!(d.status, Ineligible);
    assert_eq!(geography(&d).verdict, Verdict::Pass);
    assert_eq!(timezone(&d).verdict, Verdict::Fail);
}

#[test]
fn the_hours_can_fit_while_authorization_is_unknown() {
    let j = remote(
        "Remote - Americas",
        "Candidates must be authorized to work in the United States. You must overlap at least 4 hours with EST.",
    );
    let d = evaluate_record(&j, &at("São Paulo, Brazil"));
    assert_eq!(timezone(&d).verdict, Verdict::Pass);
    // Living in Brazil says nothing about US work authorization.
    assert_eq!(reason(&d, RuleId::Authorization).verdict, Verdict::Unknown);
    assert_ne!(d.status, Eligible);
}

#[test]
fn relocation_stays_its_own_answer() {
    // An office abroad: time zones don't apply to it, relocation does.
    let lisbon = job(
        "lever:example",
        "Lisbon, Portugal",
        Some(WorkplaceType::Hybrid),
        "",
    );
    let mut sp = at("São Paulo, Brazil");
    sp.zones = zones_in("Europe/Lisbon");
    let d = evaluate_record(&lisbon, &sp);
    assert!(d.reasons.iter().all(|r| r.rule != RuleId::Timezone));
    assert_eq!(reason(&d, RuleId::Presence).verdict, Verdict::Unknown);
    sp.relocation = Some(true);
    let d = evaluate_record(&lisbon, &sp);
    assert_eq!(reason(&d, RuleId::Presence).verdict, Verdict::Conditional);
    // A remote job: not relocating changes nothing, the hours decide.
    let remote_job = remote(
        "Remote - Europe",
        "You must be located between UTC-1 and UTC+2.",
    );
    let mut stays = at("Lisbon, Portugal");
    stays.relocation = Some(false);
    let d = evaluate_record(&remote_job, &stays);
    assert_eq!(d.status, Eligible, "{:#?}", d.reasons);
    assert_eq!(timezone(&d).verdict, Verdict::Pass);
}

#[test]
fn an_explicit_remote_scope_beats_international_offices() {
    let offices = remote(
        "London, UK; São Paulo, Brazil; Remote (US)",
        "We build developer tools.",
    );
    let r = requirements(&offices);
    assert_eq!(r.options.len(), 1, "{:?}", r.options);
    assert_eq!(r.options[0].label(), "Remote (United States)");
    let d = evaluate_record(&offices, &at("São Paulo, Brazil"));
    assert_eq!(d.status, Ineligible, "{:#?}", d.reasons);
    // "Remote, Global" is global.
    let global = remote("Remote, Global", "We build developer tools.");
    let r = requirements(&global);
    assert_eq!(r.options[0].label(), "Remote (anywhere (global))");
    assert_eq!(
        evaluate_record(&global, &at("São Paulo, Brazil")).status,
        Eligible
    );
    // "Remote — Americas" is the Americas, not global.
    let americas = remote("Remote — Americas", "We build developer tools.");
    assert_eq!(
        requirements(&americas).options[0].label(),
        "Remote (the Americas)"
    );
    assert_eq!(
        evaluate_record(&americas, &at("Lisbon, Portugal")).status,
        Ineligible
    );
}

#[test]
fn unknown_and_ambiguous_places_stay_unknown() {
    // A remote region Narrow can't read: scope unknown, never global.
    let d = evaluate_record(
        &remote("Remote - Region TBD", "We build developer tools."),
        &at("Lisbon, Portugal"),
    );
    assert_eq!(d.status, Uncertain);
    assert_eq!(geography(&d).verdict, Verdict::Unknown);

    // An office named only "Cambridge": which one isn't guessed.
    let mut cambridge = job(
        "lever:example",
        "Cambridge",
        Some(WorkplaceType::OnSite),
        "",
    );
    let r = requirements(&cambridge);
    assert!(matches!(
        r.options.as_slice(),
        [WorkOption::Office { area: None, .. }]
    ));
    // A source's country field chooses among them.
    cambridge.posting.locations = vec![SourceLocation {
        name: Some("Cambridge".into()),
        locality: None,
        region: None,
        country: Some("US".into()),
    }];
    let r = requirements(&cambridge);
    match r.options.as_slice() {
        [
            WorkOption::Office {
                area: Some(area @ Area::City { .. }),
                ..
            },
        ] => {
            assert_eq!(area.to_string(), "Cambridge, United States");
        }
        other => panic!("{other:?}"),
    }

    // Someone who says only "Cambridge" is told why it can't be used.
    let d = evaluate_record(
        &remote("Remote (US)", "We build developer tools."),
        &ProfileFacts::living_in("Cambridge"),
    );
    assert_eq!(d.status, Uncertain);
    assert!(
        geography(&d)
            .conclusion
            .contains("could be Cambridge, United Kingdom"),
        "{}",
        geography(&d).conclusion
    );
    // "Georgia": the country or the US state.
    let d = evaluate_record(
        &remote("Remote (US)", "We build developer tools."),
        &ProfileFacts::living_in("Georgia"),
    );
    assert_eq!(d.status, Uncertain);
    // With its country it is certain.
    let d = evaluate_record(
        &remote("Remote (US)", "We build developer tools."),
        &at("Atlanta, Georgia"),
    );
    assert_eq!(geography(&d).verdict, Verdict::Pass);
}

#[test]
fn a_town_known_only_by_its_country_field_is_listed_in_that_country() {
    // Two Portlands in the US (Oregon, Maine), or a town Narrow doesn't
    // list: the country field says where the place is (BRU-308). A remote
    // job listing only it is remote in that country (BRU-325's finite
    // lists).
    for town in ["Portland", "Smallville Junction"] {
        let mut j = remote(town, "We build developer tools.");
        j.posting.location = None;
        j.posting.locations = vec![SourceLocation {
            name: Some(town.into()),
            locality: None,
            region: None,
            country: Some("US".into()),
        }];
        let r = requirements(&j);
        assert_eq!(
            r.options[0].label(),
            format!("Remote (United States (from {town}))"),
            "{town}"
        );
        // A stated scope still wins over it.
        j.posting.locations.push(SourceLocation {
            name: Some("Remote (Canada)".into()),
            locality: None,
            region: None,
            country: None,
        });
        assert_eq!(requirements(&j).options[0].label(), "Remote (Canada)");
    }
}

#[test]
fn a_state_code_limits_remote_work_to_that_state() {
    // "GA" is Georgia, not Gabon; "WA" Washington, not Wa, Ghana: a
    // state-limited remote job stays in the US and is uncertain for
    // someone elsewhere in it, never a foreign scope that rules them out.
    for (location, state) in [
        ("Remote - GA", "Georgia"),
        ("Remote (WA)", "Washington State"),
        ("Remote - NC", "North Carolina"),
        ("Remote, AZ", "Arizona"),
        ("Remote - TN", "Tennessee"),
    ] {
        let j = remote(location, "We build developer tools.");
        assert_eq!(
            requirements(&j).options[0].label(),
            format!("Remote ({state}, United States)"),
            "{location}"
        );
        let d = evaluate_record(&j, &at("Denver, CO"));
        assert_eq!(d.status, Uncertain, "{location}: {:#?}", d.reasons);
        let g = geography(&d);
        assert_eq!(g.verdict, Verdict::Unknown, "{location}: {}", g.conclusion);
        assert!(g.conclusion.contains(state), "{location}: {}", g.conclusion);
    }
    // In the state itself it fits.
    let d = evaluate_record(
        &remote("Remote - GA", "We build developer tools."),
        &at("Atlanta, GA"),
    );
    assert_eq!(geography(&d).verdict, Verdict::Pass, "{:#?}", d.reasons);
    let d = evaluate_record(
        &remote("Remote - TN", "We build developer tools."),
        &at("Nashville, TN"),
    );
    assert_ne!(d.status, Ineligible, "{:#?}", d.reasons);
    assert_eq!(geography(&d).verdict, Verdict::Pass, "{:#?}", d.reasons);
}

#[test]
fn a_structured_country_code_remains_a_country() {
    for (code, expected) in [("GA", "Gabon"), ("SC", "Seychelles"), ("TN", "Tunisia")] {
        let mut j = job("lever:example", "", Some(WorkplaceType::OnSite), "");
        j.posting.location = None;
        j.posting.locations = vec![SourceLocation {
            name: Some(code.into()),
            locality: None,
            region: None,
            country: Some(code.into()),
        }];
        match requirements(&j).options.as_slice() {
            [
                WorkOption::Office {
                    area: Some(Area::Country(c)),
                    raw,
                    ..
                },
            ] => {
                assert_eq!(c.name, expected, "{code}");
                assert_eq!(raw, code);
            }
            other => panic!("{code}: {other:?}"),
        }
    }

    // The same tokens can name a subdivision inside a different structured
    // country; established city nicknames still name cities.
    for (name, code, expected) in [
        ("TN", "US", "Tennessee, United States"),
        ("SC", "BR", "Santa Catarina, Brazil"),
        ("LA", "US", "Los Angeles, United States"),
    ] {
        let mut j = job("lever:example", "", Some(WorkplaceType::OnSite), "");
        j.posting.location = None;
        j.posting.locations = vec![SourceLocation {
            name: Some(name.into()),
            locality: None,
            region: None,
            country: Some(code.into()),
        }];
        match requirements(&j).options.as_slice() {
            [
                WorkOption::Office {
                    area: Some(area), ..
                },
            ] => {
                assert_eq!(area.to_string(), expected, "{name}, {code}");
            }
            other => panic!("{name}, {code}: {other:?}"),
        }
    }
}

#[test]
fn a_source_country_constrains_a_bare_city_name() {
    // The most populous "Alexandria" is in Egypt, but the source's country
    // field says the US: the name is read there (Virginia's or
    // Louisiana's, so only the country), or the country stands in.
    for (town, code, country) in [
        ("Alexandria", "US", "United States"),
        ("St. Petersburg", "US", "United States"),
        ("Naples", "US", "United States"),
        ("León", "ES", "Spain"),
    ] {
        let mut j = remote(town, "We build developer tools.");
        j.posting.locations = vec![SourceLocation {
            name: Some(town.into()),
            locality: None,
            region: None,
            country: Some(code.into()),
        }];
        let r = requirements(&j);
        // The primary location text (the same words) adds nothing.
        assert_eq!(r.options.len(), 1, "{town}: {:?}", r.options);
        assert_eq!(
            r.options[0].label(),
            format!("Remote ({country} (from {town}))"),
            "{town}"
        );
        // As an office, it is in that country too.
        j.posting.workplace_type = Some(WorkplaceType::OnSite);
        match requirements(&j).options.as_slice() {
            [
                WorkOption::Office {
                    area: Some(area), ..
                },
            ] => {
                assert_eq!(area.country().unwrap().code, code, "{town}: {area}");
            }
            other => panic!("{town}: {other:?}"),
        }
    }
    // A place in the source's country is found, not only the country.
    let mut j = job("lever:example", "", Some(WorkplaceType::OnSite), "");
    j.posting.location = None;
    j.posting.locations = vec![SourceLocation {
        name: Some("St. Petersburg".into()),
        locality: None,
        region: None,
        country: Some("US".into()),
    }];
    match requirements(&j).options.as_slice() {
        [
            WorkOption::Office {
                area: Some(area @ Area::City { .. }),
                ..
            },
        ] => {
            assert_eq!(area.to_string(), "St. Petersburg, United States");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn hours_met_on_no_day_of_the_year_fail() {
    // Istanbul (UTC+3 all year) is 7h from New York in summer and 8h in
    // winter: a required EST schedule fits on no day, so it fails, and the
    // reason doesn't say it is met for part of the year.
    let j = remote("Remote - Worldwide", "You must work EST hours.");
    let d = evaluate_record(&j, &at("Istanbul, Turkey"));
    let tz = timezone(&d);
    assert_eq!(tz.verdict, Verdict::Fail, "{}", tz.conclusion);
    assert_eq!(d.status, Ineligible);
    assert!(
        !tz.conclusion.contains("part of the year"),
        "{}",
        tz.conclusion
    );
    assert!(
        tz.conclusion.contains("8h away Nov 1–Mar 7")
            && tz.conclusion.contains("7h away Mar 8–Oct 31"),
        "{}",
        tz.conclusion
    );
    // Met on some days and not others: still uncertain, with the dates.
    let j = remote("Remote - Americas", "Work EST ±1 hours.");
    let tz = evaluate_record(&j, &at("São Paulo, Brazil"))
        .reasons
        .into_iter()
        .find(|r| r.rule == RuleId::Timezone)
        .unwrap();
    assert_eq!(tz.verdict, Verdict::Unknown);
    assert!(
        tz.conclusion.contains("only part of the year"),
        "{}",
        tz.conclusion
    );
}
