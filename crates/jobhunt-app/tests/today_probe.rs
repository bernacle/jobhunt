//! Timing probe for Today (not part of the regular suite):
//! `cargo test --release -p jobhunt-app --test today_probe -- --ignored --nocapture`
//! Discovers ~2,000 postings of varied shapes, then times warm feeds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use chrono::{Duration, TimeZone, Utc};
use jobhunt_app::feed::FeedRequest;
use jobhunt_app::preferences::{PeriodInput, PreferenceInput, PreferenceUpdate, StanceInput};
use jobhunt_app::{AppConfig, LoadedConfig, LocalApp, Quiet, RefreshMode};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::{JobPosting, ScanBody, ScanWrite, WorkplaceType};
use jobhunt_storage::SqliteJobStore;

const LOCATIONS: [&str; 6] = [
    "Remote - Brazil",
    "Remote (LATAM)",
    "Remote",
    "Remote - Worldwide",
    "New York, NY (HQ)",
    "Remote-Friendly (Travel-Required) | San Francisco, CA",
];

const PARAGRAPH: &str = "You'll build backend services in Rust, Go and PostgreSQL for a team of \
    8 engineers. We expect all staff to be in one of our offices at least 25% of the time. We \
    raised our Series B last year. Strong experience with distributed systems, Kafka and AWS. \
    You'll own features end to end and work closely with product. We offer equity, a home office \
    budget and flexible hours.";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "timing probe; run by hand"]
async fn today_probe() {
    let now = Utc.with_ymd_and_hms(2026, 9, 27, 12, 0, 0).unwrap();
    let app = LocalApp::with_store(
        LoadedConfig {
            config: AppConfig::default(),
            file: None,
            default_file: None,
            database: PathBuf::from(":memory:"),
        },
        SqliteJobStore::open_in_memory().await.unwrap(),
    );
    app.update_preferences(
        &PreferenceUpdate {
            set: vec![
                PreferenceInput::Location {
                    place: "Dourados, Brazil".into(),
                },
                PreferenceInput::Region {
                    region: "Worldwide".into(),
                    stance: StanceInput::Want,
                },
                PreferenceInput::Region {
                    region: "Latin America".into(),
                    stance: StanceInput::Want,
                },
                PreferenceInput::Compensation {
                    minimum: Some(140_000),
                    target: None,
                    currency: "USD".into(),
                    period: PeriodInput::Year,
                    applies_to: None,
                },
                PreferenceInput::Company {
                    company: "small_team".into(),
                    stance: StanceInput::Want,
                },
                PreferenceInput::Role {
                    role: "backend".into(),
                    stance: StanceInput::Want,
                },
            ],
            ..PreferenceUpdate::default()
        },
        now,
    )
    .await
    .unwrap();
    let source = SourceKey::new("greenhouse", "probe").unwrap();
    let postings: Vec<JobPosting> = (0..2_000)
        .map(|i| {
            let native = format!("job-{i}");
            JobPosting {
                provenance: Provenance {
                    source: source.clone(),
                    source_record_id: Some(native.clone()),
                    fetched_from: None,
                },
                url: CanonicalUrl::parse(&format!("https://boards.example.com/probe/{native}"))
                    .unwrap(),
                apply_url: None,
                company: format!("Company {}", i % 400),
                title: [
                    "Senior Backend Engineer",
                    "Platform Engineer",
                    "Software Engineer",
                ][i % 3]
                    .into(),
                department: None,
                team: None,
                location: Some(LOCATIONS[i % LOCATIONS.len()].into()),
                locations: Vec::new(),
                employment_type: None,
                workplace_type: (i % 2 == 0).then_some(WorkplaceType::Remote),
                is_remote: None,
                compensation: None,
                work_authorization: None,
                description_text: Some(format!("{PARAGRAPH} Posting {i}.")),
                description_html: None,
                posted_at: Some(now - Duration::days((i % 20) as i64)),
                source_updated_at: None,
            }
        })
        .collect();
    let store = app.store();
    let run = store.begin_run(now).await.unwrap();
    store
        .apply_scan(&ScanWrite {
            run,
            source: &source,
            started_at: now,
            observed_at: now,
            counts: IngestCounts::default(),
            body: ScanBody::Listing {
                postings: &postings,
                complete: true,
                close_missing: false,
                closing_withheld: None,
                retain: &[],
                validator: None,
            },
        })
        .await
        .unwrap();
    let request = FeedRequest {
        limit: 5,
        verify: false,
        refresh: RefreshMode::Never,
    };
    let started = Instant::now();
    app.feed(&request, &Quiet, now).await.unwrap();
    let cold = started.elapsed();
    let mut warm = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        app.feed(&request, &Quiet, now).await.unwrap();
        warm.push(started.elapsed());
    }
    warm.sort();
    println!(
        "today over 2,000 postings: cold {cold:?}, warm median {:?} (min {:?}, max {:?})",
        warm[2], warm[0], warm[4]
    );
}
