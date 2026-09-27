//! BRU-308, end to end through the application: preferences said in
//! words and set with the structured controls are one set of records, and
//! Today reflects them (requirements, preferences, and what is unknown).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_app::controls::PreferenceControls;
use jobhunt_app::feed::FeedRequest;
use jobhunt_app::preferences::{
    Clarify, PeriodInput, PreferenceInput, PreferenceUpdate, PreferenceUpdateResult, StanceInput,
    WorkModeInput, WorkSetupInput,
};
use jobhunt_app::{AppConfig, LoadedConfig, LocalApp, Quiet, RefreshMode};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::verification::{
    ApplicationCheck, Authority, CompensationCheck, ListingStatus, PublishedPlace,
    VERIFICATION_REVISION, VerificationId, VerificationMethod, VerificationRecord,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, JobQuery, PayInterval,
    ScanBody, ScanWrite, WorkplaceType,
};
use jobhunt_ranking::Tier;
use jobhunt_storage::{SqliteJobStore, Store};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap()
}

async fn app() -> LocalApp {
    LocalApp::with_store(
        LoadedConfig {
            config: AppConfig::default(),
            file: None,
            default_file: None,
            database: PathBuf::from(":memory:"),
        },
        SqliteJobStore::open_in_memory().await.unwrap(),
    )
}

async fn update(app: &LocalApp, update: PreferenceUpdate) -> PreferenceUpdateResult {
    PreferenceUpdateResult::of(&app.update_preferences(&update, now()).await.unwrap())
}

async fn say(app: &LocalApp, words: &str) -> PreferenceUpdateResult {
    update(
        app,
        PreferenceUpdate {
            statement: Some(words.to_owned()),
            ..PreferenceUpdate::default()
        },
    )
    .await
}

async fn set(app: &LocalApp, inputs: Vec<PreferenceInput>) -> PreferenceUpdateResult {
    update(
        app,
        PreferenceUpdate {
            set: inputs,
            ..PreferenceUpdate::default()
        },
    )
    .await
}

async fn controls(app: &LocalApp) -> PreferenceControls {
    app.taste_view().await.unwrap().controls
}

#[tokio::test]
async fn words_fill_the_controls_and_edits_replace_what_the_words_set() {
    let app = app().await;
    let said = say(
        &app,
        "remote from Brazil, at least USD 140k, prefer small teams",
    )
    .await;
    assert!(said.not_understood.is_empty(), "{:?}", said.not_understood);
    assert!(
        said.interpreted.iter().all(|p| p.clarify.is_none()),
        "nothing ambiguous: {:?}",
        said.interpreted
    );
    let c = controls(&app).await;
    assert_eq!(c.work.setup, "remote_only");
    assert_eq!(c.work.setup_layer, "requirement");
    assert_eq!(c.location.home.as_deref(), Some("Brazil"));
    assert_eq!(c.location.home_country.as_deref(), Some("Brazil"));
    assert!(
        c.location
            .remote_open_to_you
            .contains(&"Latin America".to_owned())
    );
    let min = &c.pay.minimum[0];
    assert_eq!(
        (min.amount, min.currency.as_deref(), min.period.as_str()),
        (140_000, Some("USD"), "year")
    );
    assert_eq!(min.record.layer, "requirement");
    assert_eq!(min.record.origin, "statement");
    let team = c
        .company
        .items
        .iter()
        .find(|i| i.value == "small_team")
        .unwrap();
    assert_eq!(team.importance, "nice_to_have");
    assert_eq!(team.layer, "preference");
    assert_eq!(
        c.company
            .items
            .iter()
            .find(|i| i.value == "small_company")
            .unwrap()
            .importance,
        "off",
        "a team is not a company"
    );

    // Direct edits, without rewriting the sentence.
    set(
        &app,
        vec![
            PreferenceInput::WorkSetup {
                setup: WorkSetupInput::PreferRemote,
            },
            PreferenceInput::Company {
                company: "small-team".into(),
                stance: StanceInput::Require,
            },
            PreferenceInput::Compensation {
                minimum: Some(150_000),
                target: None,
                currency: "USD".into(),
                period: PeriodInput::Year,
                applies_to: None,
            },
            PreferenceInput::UnknownPay { show: false },
        ],
    )
    .await;
    let c = controls(&app).await;
    assert_eq!(c.work.setup, "prefer_remote");
    assert_eq!(c.work.setup_layer, "preference");
    assert_eq!(c.pay.minimum.len(), 1, "the new minimum replaced the old");
    assert_eq!(c.pay.minimum[0].amount, 150_000);
    assert_eq!(c.pay.minimum[0].record.origin, "user_entered");
    assert_eq!(c.pay.unknown_pay, "hide");
    let team = c
        .company
        .items
        .iter()
        .find(|i| i.value == "small_team")
        .unwrap();
    assert_eq!(
        (team.importance.as_str(), team.layer.as_str()),
        ("must_have", "requirement")
    );
    // One set of records: the statement is kept word for word, and the
    // explicit list the Preferences page shows is the same as the controls.
    let taste = app.taste_view().await.unwrap();
    assert_eq!(
        taste.statements[0].text,
        "remote from Brazil, at least USD 140k, prefer small teams"
    );
    let active_work_modes: Vec<&str> = taste
        .stated
        .iter()
        .filter(|p| p.value.ends_with(" work"))
        .map(|p| p.value.as_str())
        .collect();
    assert_eq!(active_work_modes, ["remote work"]);
    assert_eq!(
        taste
            .stated
            .iter()
            .filter(|p| p.category == "compensation" && p.value.starts_with("at least"))
            .count(),
        1
    );

    // "Hybrid okay" replaces the remote preference with remote-or-hybrid;
    // "No preference" clears the work setup entirely.
    set(
        &app,
        vec![PreferenceInput::WorkSetup {
            setup: WorkSetupInput::HybridOkay,
        }],
    )
    .await;
    assert_eq!(controls(&app).await.work.setup, "hybrid_okay");
    set(
        &app,
        vec![PreferenceInput::WorkSetup {
            setup: WorkSetupInput::NoPreference,
        }],
    )
    .await;
    let c = controls(&app).await;
    assert_eq!(c.work.setup, "no_preference");
    assert!(c.work.setup_records.is_empty());

    // Relocation is its own answer.
    set(
        &app,
        vec![PreferenceInput::Relocation {
            willing: true,
            only_to: vec!["Portugal".into(), "Spain".into()],
        }],
    )
    .await;
    let c = controls(&app).await;
    assert_eq!(c.work.relocation, "only_selected");
    assert_eq!(c.work.relocation_only_to, ["Portugal", "Spain"]);
    assert_eq!(c.work.setup, "no_preference", "relocation isn't work setup");

    // A new sentence lands on the same controls again.
    say(&app, "I only want remote roles").await;
    assert_eq!(controls(&app).await.work.setup, "remote_only");
}

#[tokio::test]
async fn ambiguous_words_ask_instead_of_guessing() {
    let app = app().await;
    let said = say(&app, "remote, 140k, small teams").await;
    let clarify = |key: &str| {
        said.interpreted
            .iter()
            .find(|p| p.value.contains(key))
            .unwrap_or_else(|| panic!("{key} in {:?}", said.interpreted))
            .clarify
            .clone()
    };
    // Must have, or nice to have?
    let remote = clarify("remote");
    assert!(
        matches!(
            &remote,
            Some(Clarify::Importance {
                input: PreferenceInput::WorkMode {
                    mode: WorkModeInput::Remote,
                    ..
                },
                ..
            })
        ),
        "{remote:?}"
    );
    // Currency missing, and at least or around?
    assert!(
        matches!(
            clarify("140,000"),
            Some(Clarify::Pay { currency: None, .. })
        ),
        "{:?}",
        clarify("140,000")
    );
    // The team or the company, must or nice?
    assert!(matches!(clarify("small teams"), Some(Clarify::Size { .. })));
    // Until answered, the reading is a want and never a requirement.
    let c = controls(&app).await;
    assert_eq!(c.work.setup, "prefer_remote");
    assert_eq!(c.pay.minimum.len(), 0);
    assert_eq!(c.pay.target[0].currency, None, "never assumed");

    // Answering replaces the reading, in one update.
    let remote_id = said
        .interpreted
        .iter()
        .find(|p| p.value == "remote work")
        .unwrap()
        .id
        .clone();
    update(
        &app,
        PreferenceUpdate {
            set: vec![PreferenceInput::WorkMode {
                mode: WorkModeInput::Remote,
                stance: StanceInput::Require,
            }],
            remove: vec![remote_id],
            ..PreferenceUpdate::default()
        },
    )
    .await;
    let taste = app.taste_view().await.unwrap();
    assert_eq!(taste.controls.work.setup, "remote_only");
    assert!(
        taste
            .stated
            .iter()
            .filter(|p| p.value == "remote work")
            .all(|p| p.clarify.is_none())
    );
}

// ---------------------------------------------------------------------------
// Today.

const SMALL_TEAM: &str = "You'll join a team of 6 engineers building backend services in Rust \
    and PostgreSQL.";
const PUBLIC_GIANT: &str = "We are a publicly traded company with thousands of employees. \
    You'll build backend services in Rust and PostgreSQL.";

fn posting(
    company: &str,
    location: &str,
    workplace: Option<WorkplaceType>,
    description: &str,
    pay: Option<(f64, f64)>,
) -> JobPosting {
    let source = SourceKey::new("greenhouse", "acme").unwrap();
    let native = company.to_lowercase().replace(' ', "-");
    JobPosting {
        provenance: Provenance {
            source: source.clone(),
            source_record_id: Some(native.clone()),
            fetched_from: None,
        },
        url: CanonicalUrl::parse(&format!("https://boards.example.com/acme/{native}")).unwrap(),
        apply_url: None,
        company: company.into(),
        title: "Senior Backend Engineer".into(),
        department: None,
        team: None,
        location: Some(location.into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: workplace,
        is_remote: None,
        compensation: pay.map(|(min, max)| Compensation {
            summary: None,
            components: vec![CompensationComponent {
                kind: CompensationKind::Salary,
                label: None,
                currency: Some("USD".into()),
                min: Some(min),
                max: Some(max),
                interval: Some(PayInterval::Year),
            }],
        }),
        work_authorization: None,
        description_text: Some(description.into()),
        description_html: None,
        posted_at: Some(now() - Duration::days(1)),
        source_updated_at: None,
    }
}

async fn discover(store: &dyn Store, postings: &[JobPosting]) {
    let source = SourceKey::new("greenhouse", "acme").unwrap();
    let when = now() - Duration::hours(6);
    let run = store.begin_run(when).await.unwrap();
    store
        .apply_scan(&ScanWrite {
            run,
            source: &source,
            started_at: when,
            observed_at: when,
            counts: IngestCounts::default(),
            body: ScanBody::Listing {
                postings,
                complete: true,
                close_missing: false,
                closing_withheld: None,
                retain: &[],
                validator: None,
            },
        })
        .await
        .unwrap();
    // Every job verified active at the employer's board an hour ago, with
    // the pay it publishes.
    for record in store.search(&JobQuery::default()).await.unwrap() {
        let at = now() - Duration::hours(1);
        store
            .save_verification(&VerificationRecord {
                id: VerificationId::derive(record.id, at),
                job_id: record.id,
                opportunity_id: record.opportunity_id,
                source: record.posting.provenance.source.clone(),
                source_record_id: record.posting.provenance.source_record_id.clone(),
                attempted_at: at,
                method: VerificationMethod::GreenhouseJobApi,
                listing: ListingStatus::Active,
                application: ApplicationCheck::unknown("not checked in tests"),
                authority: Authority::EmployerConfiguredAts,
                authority_chain: Vec::new(),
                checked_url: None,
                listing_url: None,
                content_fingerprint: None,
                changed_fields: Vec::new(),
                changed_since_last_verification: None,
                lifecycle: None,
                compensation: CompensationCheck::observe(
                    record.posting.compensation.as_ref(),
                    None,
                ),
                published: PublishedPlace::default(),
                unknowns: Vec::new(),
                failure: None,
                revision: VERIFICATION_REVISION.to_owned(),
            })
            .await
            .unwrap();
    }
}

async fn today(app: &LocalApp) -> jobhunt_app::feed::Feed {
    app.feed(
        &FeedRequest {
            limit: 10,
            verify: false,
            refresh: RefreshMode::Never,
        },
        &Quiet,
        now(),
    )
    .await
    .unwrap()
}

fn companies(feed: &jobhunt_app::feed::Feed) -> Vec<String> {
    feed.entries
        .iter()
        .map(|e| e.entry.ranking.company.clone())
        .collect()
}

fn entry<'a>(feed: &'a jobhunt_app::feed::Feed, company: &str) -> &'a jobhunt_ranking::Ranking {
    &feed
        .entries
        .iter()
        .find(|e| e.entry.ranking.company == company)
        .unwrap_or_else(|| panic!("{company} not on Today: {:?}", companies(feed)))
        .entry
        .ranking
}

/// The scenario: lives in Brazil, remote only, won't relocate, at least
/// USD 140k a year, unknown pay shown, small teams nice to have.
#[tokio::test]
async fn today_for_a_remote_only_person_in_brazil() {
    let app = app().await;
    set(
        &app,
        vec![
            PreferenceInput::Location {
                place: "Brazil".into(),
            },
            PreferenceInput::WorkSetup {
                setup: WorkSetupInput::RemoteOnly,
            },
            PreferenceInput::Relocation {
                willing: false,
                only_to: Vec::new(),
            },
            PreferenceInput::Compensation {
                minimum: Some(140_000),
                target: None,
                currency: "USD".into(),
                period: PeriodInput::Year,
                applies_to: None,
            },
            PreferenceInput::UnknownPay { show: true },
            PreferenceInput::Company {
                company: "small_team".into(),
                stance: StanceInput::Want,
            },
            PreferenceInput::Role {
                role: "backend".into(),
                stance: StanceInput::Want,
            },
        ],
    )
    .await;
    discover(
        app.store(),
        &[
            posting(
                "Brazil Remote",
                "Remote - Brazil",
                Some(WorkplaceType::Remote),
                SMALL_TEAM,
                None,
            ),
            posting(
                "Global Remote",
                "Remote - Worldwide",
                Some(WorkplaceType::Remote),
                SMALL_TEAM,
                None,
            ),
            posting(
                "No Scope",
                "Remote",
                Some(WorkplaceType::Remote),
                SMALL_TEAM,
                Some((150_000.0, 180_000.0)),
            ),
            posting(
                "Hybrid West East",
                "San Francisco, CA; New York, NY",
                Some(WorkplaceType::Hybrid),
                SMALL_TEAM,
                Some((200_000.0, 250_000.0)),
            ),
            posting(
                "Pays Well",
                "Remote - Brazil",
                Some(WorkplaceType::Remote),
                SMALL_TEAM,
                Some((150_000.0, 180_000.0)),
            ),
            posting(
                "Pays Less",
                "Remote - Brazil",
                Some(WorkplaceType::Remote),
                SMALL_TEAM,
                Some((100_000.0, 120_000.0)),
            ),
            posting(
                "Big Public",
                "Remote - Worldwide",
                Some(WorkplaceType::Remote),
                PUBLIC_GIANT,
                Some((150_000.0, 180_000.0)),
            ),
        ],
    )
    .await;

    let feed = today(&app).await;
    let on = companies(&feed);

    // Brazil-remote and global remote with unknown pay: on Today, unresolved.
    for company in ["Brazil Remote", "Global Remote"] {
        let r = entry(&feed, company);
        assert_eq!(
            r.tier,
            Tier::WorthReviewing,
            "{company}: never a strong fit"
        );
        assert!(
            r.brief.unknowns[0].starts_with("Unresolved: you require at least USD 140,000"),
            "{company}: {:?}",
            r.brief.unknowns
        );
        // Today's card shows one unknown: it is that requirement, not only
        // "pay isn't published".
        let card = feed
            .entries
            .iter()
            .find(|e| e.entry.ranking.company == company)
            .map(|e| jobhunt_app::shortlist::ShortlistItem::of(&e.entry))
            .unwrap();
        assert!(
            card.unknowns[0].starts_with("Unresolved: you require at least USD 140,000"),
            "{company}: {:?}",
            card.unknowns
        );
    }
    // Remote with no geographic scope: shown (the default policy), with
    // eligibility unclear, never taken as global.
    let r = entry(&feed, "No Scope");
    assert!(
        matches!(&r.gate, jobhunt_ranking::Gate::EligibilityUnclear { why } if why.contains("no geographic scope")),
        "{:?}",
        r.gate
    );
    // Explicit hybrid in California and New York: not on Today, as a
    // stated conflict (not "eligibility unclear", not "can't take it").
    assert!(!on.contains(&"Hybrid West East".to_owned()), "{on:?}");
    // Verified pay above the minimum meets it; verified below conflicts.
    let r = entry(&feed, "Pays Well");
    assert_eq!(r.tier, Tier::StrongFit, "{:?}", r.brief);
    assert!(
        r.brief
            .worth
            .iter()
            .any(|w| w.contains("Meets your minimum"))
    );
    assert!(!on.contains(&"Pays Less".to_owned()), "{on:?}");
    // A large company doesn't make a large team: nothing counts against a
    // small-team wish, which stays unknown.
    let r = entry(&feed, "Big Public");
    assert!(
        r.brief
            .unknowns
            .iter()
            .any(|u| u.contains("doesn't say whether it's small teams")),
        "{:?}",
        r.brief
    );
    assert!(!r.brief.caveats.iter().any(|c| c.contains("small")));
    let excluded = &feed.report.excluded;
    assert_eq!(excluded.unmet_requirement, 1, "{excluded:?}");
    assert_eq!(excluded.below_minimum, 1, "{excluded:?}");
    assert_eq!(excluded.ineligible, 0, "{excluded:?}");

    // Hiding what isn't confirmed: the scope-less job and the unknown-pay
    // jobs leave Today; the rest stay.
    set(
        &app,
        vec![
            PreferenceInput::UnclearEligibility { show: false },
            PreferenceInput::UnknownPay { show: false },
        ],
    )
    .await;
    let feed = today(&app).await;
    let on = companies(&feed);
    for gone in ["No Scope", "Brazil Remote", "Global Remote"] {
        assert!(!on.contains(&gone.to_owned()), "{gone}: {on:?}");
    }
    for kept in ["Pays Well", "Big Public"] {
        assert!(on.contains(&kept.to_owned()), "{kept}: {on:?}");
    }
    assert_eq!(feed.report.excluded.pay_unknown, 2);
    assert_eq!(feed.report.excluded.eligibility_unconfirmed, 1);
}

/// The active remote/hybrid/on-site records, as (value, stance).
async fn work_modes(app: &LocalApp) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = app
        .taste_view()
        .await
        .unwrap()
        .stated
        .iter()
        .filter(|p| p.value.ends_with(" work"))
        .map(|p| (p.value.clone(), p.stance.clone()))
        .collect();
    out.sort();
    out
}

fn modes(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = pairs
        .iter()
        .map(|(v, s)| ((*v).to_owned(), (*s).to_owned()))
        .collect();
    out.sort();
    out
}

async fn setup(app: &LocalApp, answer: WorkSetupInput) {
    set(app, vec![PreferenceInput::WorkSetup { setup: answer }]).await;
}

/// Codex review #2: words that state a work setup replace it exactly as a
/// structured answer does: one work setup, no stale records, both ways.
#[tokio::test]
async fn words_and_answers_share_one_work_setup() {
    // Hybrid okay → "remote only".
    let app = app().await;
    setup(&app, WorkSetupInput::HybridOkay).await;
    say(&app, "remote only").await;
    assert_eq!(controls(&app).await.work.setup, "remote_only");
    assert_eq!(
        work_modes(&app).await,
        modes(&[("remote work", "required")])
    );

    // Remote only → "hybrid is okay": what was said now, nothing stale
    // (a nice-to-have is never upgraded to a requirement).
    let app = app_with(WorkSetupInput::RemoteOnly).await;
    say(&app, "hybrid is okay").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "acceptable")])
    );
    let c = controls(&app).await;
    assert_eq!(c.work.setup, "custom");
    assert_eq!(c.work.setup_layer, "preference");
    // … and "hybrid is okay but I prefer remote" keeps both, as said.
    say(&app, "hybrid is okay but I prefer remote").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "acceptable"), ("remote work", "wanted")])
    );

    // Prefer remote → "remote only".
    let app = app_with(WorkSetupInput::PreferRemote).await;
    say(&app, "remote only").await;
    assert_eq!(controls(&app).await.work.setup, "remote_only");
    assert_eq!(
        work_modes(&app).await,
        modes(&[("remote work", "required")])
    );

    // Remote only → "prefer remote".
    let app = app_with(WorkSetupInput::RemoteOnly).await;
    say(&app, "I prefer remote").await;
    assert_eq!(controls(&app).await.work.setup, "prefer_remote");
    assert_eq!(work_modes(&app).await, modes(&[("remote work", "wanted")]));

    // And back through a structured answer: it replaces what words set.
    setup(&app, WorkSetupInput::HybridOkay).await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "required"), ("remote work", "required")])
    );
    // Words that only rule a mode out add to the setup.
    say(&app, "no on-site").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[
            ("hybrid work", "required"),
            ("onsite work", "unwanted"),
            ("remote work", "required")
        ])
    );
}

async fn app_with(answer: WorkSetupInput) -> LocalApp {
    let app = app().await;
    setup(&app, answer).await;
    app
}

/// Codex review #3: a pay figure keeps its period exactly: every period
/// round-trips through the controls, and setting it again changes nothing.
#[tokio::test]
async fn pay_periods_round_trip() {
    for (period, name) in [
        (PeriodInput::Hour, "hour"),
        (PeriodInput::Day, "day"),
        (PeriodInput::Month, "month"),
        (PeriodInput::Year, "year"),
    ] {
        let app = app().await;
        let input = PreferenceInput::Compensation {
            minimum: Some(100),
            target: None,
            currency: "USD".into(),
            period,
            applies_to: None,
        };
        set(&app, vec![input.clone()]).await;
        let c = controls(&app).await;
        let min = &c.pay.minimum[0];
        assert_eq!(
            (min.amount, min.currency.as_deref(), min.period.as_str()),
            (100, Some("USD"), name)
        );
        // What the editor sends back when nothing is changed.
        let again = set(&app, vec![input]).await;
        assert!(again.unchanged, "{name}: nothing changed");
        assert_eq!(controls(&app).await.pay.minimum[0].period, name);
    }
}

/// Codex review (e88868c): a statement that only rules a mode out adds to
/// the work setup; it never erases the requirement already there.
#[tokio::test]
async fn ruling_a_mode_out_keeps_the_work_setup() {
    let app = app_with(WorkSetupInput::RemoteOnly).await;
    say(&app, "Hybrid is not okay").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "unwanted"), ("remote work", "required")])
    );
    assert_eq!(controls(&app).await.work.setup, "remote_only");
    say(&app, "on-site is not fine").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[
            ("hybrid work", "unwanted"),
            ("onsite work", "unwanted"),
            ("remote work", "required")
        ])
    );
    assert_eq!(controls(&app).await.work.setup, "remote_only");

    // Hybrid okay keeps its answer when on-site is ruled out too.
    let app = app_with(WorkSetupInput::HybridOkay).await;
    say(&app, "on-site is not fine").await;
    assert_eq!(controls(&app).await.work.setup, "hybrid_okay");
    // A rule-out that changes what a preference means is shown as said.
    let app = app_with(WorkSetupInput::PreferRemote).await;
    say(&app, "hybrid is not okay").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "unwanted"), ("remote work", "wanted")])
    );
    assert_eq!(controls(&app).await.work.setup, "custom");

    // "I don't mind hybrid" is an acceptance, and a new setup: it replaces.
    let app = app_with(WorkSetupInput::RemoteOnly).await;
    say(&app, "I don't mind hybrid").await;
    assert_eq!(
        work_modes(&app).await,
        modes(&[("hybrid work", "acceptable")])
    );
}
