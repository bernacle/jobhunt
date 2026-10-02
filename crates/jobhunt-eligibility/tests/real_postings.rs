//! Decisions on real postings: every adapter's saved responses
//! (`crates/jobhunt-sources/tests/fixtures/`), for people in different
//! places.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use jobhunt_eligibility::decision::{Eligibility, RuleId, Verdict};
use jobhunt_eligibility::job::{JobMode, RemoteScope, ScopeBasis};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{evaluate_record, requirements};

use Eligibility::{Eligible, Ineligible, Uncertain};

fn at(place: &str) -> ProfileFacts {
    ProfileFacts::living_in(place)
}

#[test]
fn every_fixture_posting_gets_an_explained_decision() {
    let people = [
        "São Paulo, Brazil",
        "Seattle, WA",
        "Berlin, Germany",
        "Bengaluru, India",
    ];
    let all = common::all();
    assert!(all.len() > 30);
    for job in &all {
        for place in people {
            let d = evaluate_record(job, &at(place));
            assert!(!d.reasons.is_empty(), "{}: no reasons", job.posting.title);
            assert!(!d.headline.is_empty());
            // Every non-trivial conclusion names what it rests on.
            for r in &d.reasons {
                if matches!(r.verdict, Verdict::Pass | Verdict::Fail) && r.rule != RuleId::WorkMode
                {
                    assert!(
                        !r.evidence.is_empty() || !r.profile.is_empty(),
                        "{}: {:?} without evidence",
                        job.posting.title,
                        r
                    );
                }
            }
        }
    }
}

#[test]
fn linear_europe_listing_with_a_broader_description() {
    // Listed "Europe"; the description says "This role is open to
    // candidates based in North America and Europe": a statement about this
    // role, so its wider scope applies.
    let job = common::find("Senior / Staff Fullstack Engineer");
    let r = requirements(&job);
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(
        evaluate_record(&job, &at("Berlin, Germany")).status,
        Eligible
    );
    assert_eq!(
        evaluate_record(&job, &at("Recife, Brazil")).status,
        Ineligible
    );
    // The description includes North America, the listing doesn't.
    let d = evaluate_record(&job, &at("Toronto"));
    assert_eq!(d.status, Eligible);
    let resolved = d
        .reasons
        .iter()
        .find(|r| r.rule == RuleId::Ambiguity)
        .unwrap();
    assert_eq!(resolved.verdict, Verdict::NotApplicable);
    assert!(
        resolved
            .conclusion
            .contains("this role's scope more widely")
    );
    assert!(resolved.evidence.iter().any(|e| e.field == "locations"));
    assert!(resolved.evidence.iter().any(|e| e.field == "description"));
}

#[test]
fn figma_remote_in_the_united_states_or_offices() {
    let job = common::find("Account Executive, Enterprise");
    let d = evaluate_record(&job, &at("Austin, TX"));
    assert_eq!(d.status, Eligible);
    assert!(d.headline.contains("the United States"));
    let d = evaluate_record(&job, &at("Recife, Brazil"));
    assert_eq!(d.status, Uncertain, "the offices depend on relocation");
    let mut stay = at("Recife, Brazil");
    stay.relocation = Some(false);
    assert_eq!(evaluate_record(&job, &stay).status, Ineligible);
}

#[test]
fn spotify_listed_city_gives_way_to_the_description() {
    // Remote, listing only "New York, NY" (a finite list: the United
    // States); the description says North America, and outranks it.
    let job = common::find("Associate – Audiobook Licensing & Author Partnerships");
    let r = requirements(&job);
    assert_eq!(r.mode, JobMode::Remote);
    let Some(RemoteScope::Areas(areas)) = r.remote_option() else {
        panic!("expected an inferred scope");
    };
    assert_eq!(areas[0].basis, ScopeBasis::Listed);
    assert_eq!(evaluate_record(&job, &at("Chicago")).status, Eligible);
    assert_eq!(evaluate_record(&job, &at("Toronto")).status, Eligible);
    assert_eq!(evaluate_record(&job, &at("Mexico City")).status, Uncertain);
    assert_eq!(evaluate_record(&job, &at("Berlin")).status, Ineligible);
}

#[test]
fn work_at_a_startup_visa_field() {
    // "US citizen/visa only", first-party.
    let job = common::find("Security Engineer");
    let mut seattle = at("Seattle, WA");
    let d = evaluate_record(&job, &seattle);
    assert_eq!(
        d.status, Uncertain,
        "authorization is not assumed from residence"
    );
    let auth = d
        .reasons
        .iter()
        .find(|r| r.rule == RuleId::Authorization)
        .unwrap();
    assert_eq!(auth.evidence[0].field, "work_authorization");
    assert_eq!(auth.evidence[0].text, "US citizen/visa only");
    seattle.needs_sponsorship = Some(false);
    assert_eq!(evaluate_record(&job, &seattle).status, Eligible);
    seattle.needs_sponsorship = Some(true);
    assert_eq!(evaluate_record(&job, &seattle).status, Ineligible);
}

#[test]
fn published_pacific_hours() {
    let job = common::find("Site Reliability Engineer (US - Pacific time)");
    let mut seattle = at("Seattle, WA");
    seattle.needs_sponsorship = Some(false);
    let d = evaluate_record(&job, &seattle);
    assert_eq!(d.status, Eligible);
    assert!(
        d.reasons
            .iter()
            .any(|r| r.rule == RuleId::Timezone && r.verdict == Verdict::Pass)
    );
    let mut boston = at("Boston");
    boston.needs_sponsorship = Some(false);
    // 3h from Pacific: within the default tolerance.
    assert_eq!(evaluate_record(&job, &boston).status, Eligible);
}

#[test]
fn anthropic_sydney_office_and_partial_sponsorship() {
    let job = common::find("Applied AI Architect");
    let mut lisbon = at("Lisbon, Portugal");
    assert_eq!(evaluate_record(&job, &lisbon).status, Uncertain);
    lisbon.relocation = Some(false);
    assert_eq!(evaluate_record(&job, &lisbon).status, Ineligible);
    lisbon.relocation = Some(true);
    lisbon.needs_sponsorship = Some(true);
    // Anthropic sponsors visas, "but not for every role".
    let d = evaluate_record(&job, &lisbon);
    assert_eq!(d.status, Uncertain);
    assert!(
        d.reasons
            .iter()
            .any(|r| r.conclusion.contains("not for every role"))
    );
}

#[test]
fn hybrid_job_with_a_remote_flag() {
    // Ashby: workplace "Hybrid" and isRemote true: the workplace type wins.
    let job = common::find("Solutions Engineer, Europe");
    let r = requirements(&job);
    assert!(r.remote_option().is_none());
    assert!(r.conflicts[0].summary.contains("remote flag"));
    assert_eq!(evaluate_record(&job, &at("London")).status, Eligible);
    assert_eq!(
        evaluate_record(&job, &at("Manchester, UK")).status,
        Uncertain
    );
}

#[test]
fn remote_within_commuting_cities() {
    // Pine Park Health: "Remote (San Francisco, CA, US; Berkeley, CA, US; …)".
    let job = common::find("Senior Software Engineer");
    let d = evaluate_record(&job, &at("Oakland, CA"));
    assert_eq!(d.status, Uncertain, "authorization is still unstated");
    let d = evaluate_record(&job, &at("Seattle"));
    assert_eq!(d.status, Uncertain);
}
