//! Assessments of real postings (the adapters' saved responses) for users
//! in different places, and of postings edited to state what the fixtures
//! don't (Latin America, contractors, anywhere).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::Duration;
use jobhunt_eligibility::assess::{Dimension, Fit, assess_record};
use jobhunt_eligibility::facts::{Basis, JobMode, job_facts};
use jobhunt_eligibility::user::{
    LocationBasis, MoneyPreference, UserConstraints, UserLocation, place_area,
};
use jobhunt_eligibility::verify::{Freshness, verify};
use jobhunt_jobs::{JobRecord, JobStatus, WorkplaceType};
use jobhunt_profile::{CompensationBound, PayPeriod, Stance, WorkMode};

fn living_in(place: &str) -> UserConstraints {
    let area = place_area(place);
    assert!(area.is_some(), "{place} is recognized");
    UserConstraints {
        location: Some(UserLocation {
            raw: place.into(),
            area,
            basis: LocationBasis::Preference,
        }),
        ..Default::default()
    }
}

fn minimum(amount: u64, currency: Option<&str>) -> MoneyPreference {
    MoneyPreference {
        bound: CompensationBound::Minimum,
        amount,
        currency: currency.map(str::to_owned),
        period: PayPeriod::Year,
        arrangement: None,
        label: format!("at least {amount}"),
    }
}

/// A fixture job whose location and description say something else.
fn edited(title: &str, location: &str, description: &str) -> JobRecord {
    let mut r = common::find(title);
    r.posting.location = Some(location.into());
    r.posting.locations.clear();
    r.posting.workplace_type = Some(WorkplaceType::Remote);
    r.posting.description_text = Some(description.into());
    r.posting.compensation = None;
    r
}

#[test]
fn remote_is_not_a_promise_of_anywhere() {
    // "Remote" with no place: the answer is unknown, never yes.
    let job = edited("Security Engineer", "Remote", "We build product analytics.");
    let mut job = job;
    job.posting.work_authorization = None;
    let a = assess_record(&job, &living_in("São Paulo, Brazil"));
    assert_eq!(a.fit, Fit::Unknown);
    assert_eq!(
        a.headline(),
        "Remote, but the posting doesn't say where you can work from"
    );
}

#[test]
fn stated_remote_regions() {
    let latam = edited("Security Engineer", "Remote (LATAM)", "Join our team.");
    let mut latam = latam;
    latam.posting.work_authorization = None;
    let brazil = living_in("Porto Alegre, RS");
    let a = assess_record(&latam, &brazil);
    assert_eq!(a.fit, Fit::Yes);
    assert_eq!(a.headline(), "Brazil eligible: remote in Latin America");
    assert_eq!(a.checks[0].evidence[0].field, "location");

    let americas = edited(
        "Design Engineer (Web & Brand)",
        "Remote - Americas",
        "We are a small team.",
    );
    let a = assess_record(&americas, &living_in("Lisbon"));
    assert_eq!(a.fit, Fit::No);
    assert_eq!(a.headline(), "Remote, but the Americas only");
    // The Americas span many hours, and the posting names none.
    let a = assess_record(&americas, &brazil);
    assert_eq!(a.fit, Fit::Likely);
    assert_eq!(
        a.check(Dimension::Timezone).unwrap().summary,
        "Eligibility unclear because the time zone isn't published"
    );
}

#[test]
fn us_residents_only() {
    // Figma: "Remote (United States)" in the location fields.
    // Figma: remote in the United States, or its San Francisco and New
    // York offices.
    let job = common::find("Account Executive, Enterprise");
    let mut brazil = living_in("São Paulo, Brazil");
    let a = assess_record(&job, &brazil);
    assert_eq!(a.fit, Fit::Unknown, "moving to an office is still open");
    assert!(a.headline().starts_with("Office in"), "{}", a.headline());
    assert!(
        a.checks[0]
            .notes
            .contains(&"Otherwise: Remote, but the United States only.".to_owned())
    );
    brazil.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    let a = assess_record(&job, &brazil);
    assert_eq!(a.fit, Fit::No);
    assert_eq!(a.headline(), "Remote, but the United States only");
    let a = assess_record(&job, &living_in("Austin, TX"));
    assert_eq!(a.fit, Fit::Yes);
    assert_eq!(
        a.headline(),
        "United States eligible: remote in the United States"
    );
}

#[test]
fn description_limits_and_their_evidence() {
    // Linear: listed "Europe"; the description is open to North America
    // and Europe (other postings cover North America).
    let job = common::find("Senior / Staff Fullstack Engineer");
    let a = assess_record(&job, &living_in("Berlin, Germany"));
    assert_eq!(a.fit, Fit::Yes, "{:?}", a.checks);
    assert!(
        a.check(Dimension::Timezone).is_none(),
        "Europe implies the hours"
    );
    let a = assess_record(&job, &living_in("Toronto"));
    assert_eq!(a.fit, Fit::No);
    assert!(
        a.checks[0]
            .notes
            .iter()
            .any(|n| n.contains("North America"))
    );
    assert!(
        a.checks[0]
            .evidence
            .iter()
            .any(|e| e.field == "description" && e.text.contains("open to candidates based in"))
    );
}

#[test]
fn remote_job_with_only_an_office_city() {
    // Spotify lists only "New York, NY" on a remote job: probably the US.
    let job = common::find("Associate – Audiobook Licensing & Author Partnerships");
    let facts = job_facts(&job);
    assert_eq!(facts.mode, JobMode::Remote);
    assert_eq!(facts.remote_areas[0].basis, Basis::OfficeCountry);
    let a = assess_record(&job, &living_in("Chicago"));
    assert_eq!(a.fit, Fit::Likely);
    assert!(
        a.headline()
            .starts_with("Remote, probably within the United States")
    );
}

#[test]
fn first_party_work_authorization() {
    // Work at a Startup's visa field: "US citizen/visa only".
    let job = common::find("Security Engineer");
    let mut user = living_in("Seattle, WA");
    let a = assess_record(&job, &user);
    assert_eq!(a.check(Dimension::Authorization).unwrap().fit, Fit::Likely);
    assert_eq!(a.fit, Fit::Likely);
    user.needs_sponsorship = Some(true);
    let a = assess_record(&job, &user);
    assert_eq!(a.fit, Fit::No);
    assert_eq!(
        a.headline(),
        "Requires authorization to work in the United States, with no sponsorship; you need sponsorship"
    );
    let auth = a.check(Dimension::Authorization).unwrap();
    assert_eq!(auth.evidence[0].field, "work_authorization");
    assert_eq!(auth.evidence[0].text, "US citizen/visa only");
}

#[test]
fn published_time_zones() {
    let job = common::find("Site Reliability Engineer (US - Pacific time)");
    let mut user = living_in("Seattle, WA");
    user.needs_sponsorship = Some(false);
    let a = assess_record(&job, &user);
    assert_eq!(a.check(Dimension::Timezone).unwrap().fit, Fit::Yes);
    let mut user = living_in("New York");
    user.zones = jobhunt_eligibility::zones::zones_in("UTC-3");
    let a = assess_record(&job, &user);
    let tz = a.check(Dimension::Timezone).unwrap();
    assert_eq!(tz.fit, Fit::Unlikely);
    assert_eq!(
        tz.summary,
        "Expects Pacific timezone hours, 5h from your time zone"
    );
}

#[test]
fn contractors_and_anywhere() {
    let job = edited(
        "Design Engineer (Web & Brand)",
        "Remote (US)",
        "Outside the US, we hire contractors in Brazil, Argentina and Mexico through Deel.",
    );
    let a = assess_record(&job, &living_in("Recife, Brazil"));
    assert_eq!(a.fit, Fit::Likely);
    assert_eq!(a.headline(), "Contractors from Brazil appear supported");
    assert!(
        a.checks[0]
            .notes
            .iter()
            .any(|n| n.contains("Remote, but the United States only"))
    );

    let job = edited(
        "Design Engineer (Web & Brand)",
        "Remote",
        "You can work from anywhere in the world.",
    );
    let a = assess_record(&job, &living_in("Nairobi, Kenya"));
    assert_eq!(a.fit, Fit::Likely);
    assert_eq!(
        a.checks[0].summary,
        "Remote from anywhere, per the description"
    );
    assert_eq!(a.check(Dimension::Timezone).unwrap().fit, Fit::Unknown);
}

#[test]
fn offices_abroad() {
    let job = common::find("Applied AI Architect"); // Sydney
    let mut user = living_in("Lisbon, Portugal");
    assert_eq!(assess_record(&job, &user).fit, Fit::Unknown);
    user.relocation = Some(false);
    let a = assess_record(&job, &user);
    assert_eq!(a.fit, Fit::No);
    assert_eq!(
        a.headline(),
        "On-site in Sydney; you're not open to relocating"
    );
    user.relocation = Some(true);
    user.needs_sponsorship = Some(true);
    // Anthropic sponsors visas, "but not for every role".
    assert_eq!(assess_record(&job, &user).fit, Fit::Unknown);
}

#[test]
fn work_mode_requirements() {
    let job = common::find("Solutions Engineer, Europe"); // hybrid
    let mut user = living_in("London");
    user.work_modes = vec![(WorkMode::Remote, Stance::Required)];
    let a = assess_record(&job, &user);
    assert_eq!(a.fit, Fit::No);
    assert_eq!(a.headline(), "Work mode: hybrid, but you require remote");
    // Only a requirement lowers the overall fit.
    user.work_modes = vec![(WorkMode::Remote, Stance::Wanted)];
    let a = assess_record(&job, &user);
    assert_eq!(a.check(Dimension::WorkMode).unwrap().fit, Fit::Unlikely);
    assert_ne!(a.fit, Fit::No);
}

#[test]
fn compensation_against_minimums() {
    let seattle = |min: MoneyPreference| {
        let mut u = living_in("Seattle");
        u.minimums = vec![min];
        u
    };
    // Ramp: USD 211,400 – 290,600 a year.
    let job = common::find("Security Engineer, Cloud");
    let a = assess_record(&job, &seattle(minimum(180_000, Some("USD"))));
    let pay = a.check(Dimension::Compensation).unwrap();
    assert_eq!(pay.fit, Fit::Yes);
    assert_eq!(
        pay.summary,
        "Salary satisfies your minimum (USD 211,400 – 290,600 a year vs USD 180,000)"
    );
    let a = assess_record(&job, &seattle(minimum(250_000, Some("USD"))));
    assert_eq!(a.check(Dimension::Compensation).unwrap().fit, Fit::Likely);
    let a = assess_record(&job, &seattle(minimum(400_000, Some("USD"))));
    assert_eq!(a.fit, Fit::No, "a minimum is a hard constraint");
    // No currency conversion, and no guessing.
    let a = assess_record(&job, &seattle(minimum(180_000, Some("EUR"))));
    assert_eq!(
        a.check(Dimension::Compensation).unwrap().summary,
        "Pay is in USD; your minimum is in EUR"
    );
    let a = assess_record(&job, &seattle(minimum(180_000, None)));
    assert_eq!(a.check(Dimension::Compensation).unwrap().fit, Fit::Unknown);

    // Unpublished pay stays unknown and doesn't lower the fit.
    let job = common::find("Account Executive Product - Fraud & Risk");
    let a = assess_record(&job, &seattle(minimum(180_000, Some("USD"))));
    assert_eq!(
        a.check(Dimension::Compensation).unwrap().summary,
        "Compensation unknown"
    );
    assert_eq!(a.fit, Fit::Yes);

    // YC: "$190K - $215K" on a job in California. "$" is read as USD from
    // the location, so the answer is "likely", not "yes".
    let job = common::find("Senior Software Engineer");
    let a = assess_record(&job, &seattle(minimum(150_000, Some("USD"))));
    let pay = a.check(Dimension::Compensation).unwrap();
    assert_eq!(pay.fit, Fit::Likely);
    assert!(pay.notes.iter().any(|n| n.contains("“$” read as USD")));
}

#[test]
fn unknown_user_location() {
    let job = common::find("Account Executive, Enterprise");
    let a = assess_record(&job, &UserConstraints::default());
    assert_eq!(a.fit, Fit::Unknown);
    assert_eq!(a.headline(), "Your location isn't set");
}

#[test]
fn freshness_of_first_party_records() {
    let mut job = common::find("Account Executive, Enterprise");
    let v = verify([&job], common::now());
    assert!(v.first_party());
    assert_eq!(v.freshness(), Freshness::Fresh);
    assert_eq!(
        v.summary(),
        "First-party: the company's own Greenhouse job board, seen just now"
    );
    job.last_seen_at = common::now() - Duration::days(10);
    assert_eq!(
        verify([&job], common::now()).summary(),
        "First-party: the company's own Greenhouse job board; last seen 10 days ago, may be gone"
    );
    job.status = JobStatus::Closed;
    assert_eq!(verify([&job], common::now()).freshness(), Freshness::Closed);
}
