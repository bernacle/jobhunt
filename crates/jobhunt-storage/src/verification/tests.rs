use std::borrow::Cow;
use std::sync::Mutex;

use chrono::{Duration, TimeZone};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Eligibility, cached_assess};
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationCheck, ApplicationStatus, FreshnessPolicy, ListingStatus,
    ListingVerifier, ObserveError, ObservedListing, SourceObservation, VerificationMethod,
    VerificationService, VerifyMode,
};
use jobhunt_jobs::{
    JobEventKind, JobRecord, JobRepository, JobStatus, ScanBody, ScanWrite, WorkplaceType,
};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use super::*;

fn at(hours: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap() + Duration::hours(hours)
}

fn posting(id: &str, location: &str) -> JobPosting {
    let source = SourceKey::new("greenhouse", "acme").unwrap();
    JobPosting {
        provenance: Provenance {
            source,
            source_record_id: Some(id.into()),
            fetched_from: None,
        },
        url: CanonicalUrl::parse(&format!("https://job-boards.greenhouse.io/acme/jobs/{id}"))
            .unwrap(),
        apply_url: None,
        company: "Acme".into(),
        title: "Backend Engineer".into(),
        department: None,
        team: None,
        location: Some(location.into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: None,
        compensation: None,
        work_authorization: None,
        description_text: None,
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    }
}

async fn discovered(store: &SqliteJobStore, postings: &[JobPosting], when: DateTime<Utc>) {
    let source = SourceKey::new("greenhouse", "acme").unwrap();
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
                close_missing: true,
                closing_withheld: None,
                retain: &[],
                validator: None,
            },
        })
        .await
        .unwrap();
}

/// Answers with whatever it was last told to.
struct Scripted(Mutex<Result<SourceObservation, ObserveError>>);

impl Scripted {
    fn new() -> Self {
        Self(Mutex::new(Err(ObserveError::Timeout { url: "u".into() })))
    }

    fn found(&self, p: &JobPosting) {
        *self.0.lock().unwrap() = Ok(SourceObservation {
            method: VerificationMethod::GreenhouseJobApi,
            checked_url: "https://boards-api.greenhouse.io/v1/boards/acme/jobs/1".into(),
            listing: ObservedListing::Found(Box::new(p.clone())),
            application: ApplicationCheck {
                status: ApplicationStatus::Active,
                basis: ApplicationBasis::Probed,
                url: Some(p.url.to_string()),
                http_status: Some(200),
                detail: None,
            },
            chain: Vec::new(),
            unknowns: Vec::new(),
        });
    }

    fn fail(&self, error: ObserveError) {
        *self.0.lock().unwrap() = Err(error);
    }
}

#[async_trait]
impl ListingVerifier for Scripted {
    async fn observe(&self, _: &JobRecord) -> Result<SourceObservation, ObserveError> {
        self.0.lock().unwrap().clone()
    }
}

async fn record(store: &SqliteJobStore, p: &JobPosting) -> JobRecord {
    store.get(p.id()).await.unwrap().unwrap()
}

#[tokio::test]
async fn verification_history_keeps_the_last_success_through_failures() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let p = posting("1", "Remote - LATAM");
    discovered(&store, std::slice::from_ref(&p), at(0)).await;
    let verifier = Scripted::new();
    let service = VerificationService::new(&store, &verifier);

    verifier.found(&p);
    service
        .verify(&[record(&store, &p).await], VerifyMode::Force, at(1))
        .await
        .unwrap();
    verifier.fail(ObserveError::Unavailable {
        url: "u".into(),
        status: Some(502),
        detail: "HTTP 502".into(),
    });
    let out = service
        .verify(&[record(&store, &p).await], VerifyMode::Force, at(30))
        .await
        .unwrap();

    let latest = store.latest_verification(p.id()).await.unwrap().unwrap();
    assert_eq!(latest.attempted_at, at(30));
    assert_eq!(latest.listing, ListingStatus::Unreachable);
    assert_eq!(latest.failure.as_ref().unwrap().http_status, Some(502));
    let success = store
        .latest_successful_verification(p.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        success.attempted_at,
        at(1),
        "a failure never erases a success"
    );
    assert_eq!(out[0].last_success.as_ref(), Some(&success));
    let history = store.verification_history(p.id()).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].attempted_at, at(30), "newest first");
    // Stored and loaded exactly.
    assert_eq!(history[1], success);
    let columns: (String, i64, String) = sqlx::query_as(
        "SELECT listing_status, succeeded, authority FROM job_verifications \
         ORDER BY attempted_at LIMIT 1",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        ("active".into(), 1, "employer_configured_ats".into())
    );
}

#[tokio::test]
async fn observations_go_through_the_lifecycle_without_a_scan() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let p = posting("1", "Remote - LATAM");
    let other = posting("2", "Remote - LATAM");
    discovered(&store, &[p.clone(), other.clone()], at(0)).await;
    let scans = |store: &SqliteJobStore| {
        let pool = store.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM source_scans")
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let before = scans(&store).await;

    // UPDATED.
    let mut changed = p.clone();
    changed.location = Some("Remote (US)".into());
    let outcome = store.record_observation(&changed, at(1)).await.unwrap();
    assert_eq!(outcome, UpsertOutcome::Updated);
    let history = store.history(p.id()).await.unwrap();
    let last = history.last().unwrap();
    assert_eq!(last.kind, JobEventKind::Updated);
    assert_eq!(last.run, None);
    assert_eq!(last.changed_fields, ["location"]);
    assert_eq!(record(&store, &p).await.last_seen_at, at(1));
    // Nothing else is closed, and no scan is recorded.
    assert_eq!(record(&store, &other).await.status, JobStatus::Open);
    assert_eq!(scans(&store).await, before);

    // REOPENED keeps the history.
    discovered(&store, std::slice::from_ref(&other), at(2)).await;
    assert_eq!(record(&store, &p).await.status, JobStatus::Closed);
    let outcome = store.record_observation(&changed, at(3)).await.unwrap();
    assert_eq!(outcome, UpsertOutcome::Reopened);
    let kinds: Vec<JobEventKind> = store
        .history(p.id())
        .await
        .unwrap()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            JobEventKind::New,
            JobEventKind::Updated,
            JobEventKind::Closed,
            JobEventKind::Reopened
        ]
    );
    assert_eq!(record(&store, &p).await.first_seen_at, at(0));
}

#[tokio::test]
async fn stored_decisions_follow_their_inputs() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let p = posting("1", "Remote - LATAM");
    discovered(&store, std::slice::from_ref(&p), at(0)).await;
    let verifier = Scripted::new();
    verifier.found(&p);
    let service = VerificationService::new(&store, &verifier);
    let verified = service
        .verify(&[record(&store, &p).await], VerifyMode::Force, at(1))
        .await
        .unwrap();
    let mut profile = ProfileFacts::living_in("Recife, Brazil");
    profile.profile_id = Some("prof_1".into());
    profile.revision = 1;
    let policy = FreshnessPolicy::default();

    let (a, hit) = cached_assess(&store, &verified, &profile, &policy, at(1))
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(a.decision.status, Eligibility::Eligible);
    let (b, hit) = cached_assess(&store, &verified, &profile, &policy, at(2))
        .await
        .unwrap();
    assert!(hit);
    assert_eq!(b.decision, a.decision, "stored and loaded exactly");

    // A new profile revision misses, and both rows remain (audit trail).
    profile.revision = 2;
    profile.location = ProfileFacts::living_in("Madrid").location;
    let (c, hit) = cached_assess(&store, &verified, &profile, &policy, at(3))
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(c.decision.status, Eligibility::Ineligible);
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT profile_revision, status FROM eligibility_decisions ORDER BY profile_revision",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(rows, [(1, "eligible".into()), (2, "ineligible".into())]);

    // A key computed under other rules finds nothing.
    let mut other_rules = CacheKey::of(&verified, &profile).unwrap();
    other_rules.key = "elig_00000000000000000000000000000000".into();
    assert!(store.cached_decision(&other_rules).await.unwrap().is_none());

    // A job change (re-verified, updated content) misses.
    let mut changed = p.clone();
    changed.location = Some("Remote (US)".into());
    verifier.found(&changed);
    let reverified = service
        .verify(&[record(&store, &p).await], VerifyMode::Force, at(4))
        .await
        .unwrap();
    let (d, hit) = cached_assess(&store, &reverified, &profile, &policy, at(4))
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(d.decision.status, Eligibility::Ineligible);
}

#[tokio::test]
async fn upgrading_a_profile_database_adds_verification_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobhunt.db");
    {
        let pool = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        let before = sqlx::migrate::Migrator {
            migrations: Cow::Owned(
                crate::sqlite::MIGRATOR
                    .iter()
                    .take(crate::sqlite::MIGRATOR.iter().count() - 1)
                    .cloned()
                    .collect(),
            ),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        before.run(&pool).await.unwrap();
        pool.close().await;
    }
    let store = SqliteJobStore::open(&path).await.unwrap();
    let p = posting("1", "Remote - LATAM");
    discovered(&store, std::slice::from_ref(&p), at(0)).await;
    let verifier = Scripted::new();
    verifier.found(&p);
    VerificationService::new(&store, &verifier)
        .verify(&[record(&store, &p).await], VerifyMode::Force, at(1))
        .await
        .unwrap();
    assert!(store.latest_verification(p.id()).await.unwrap().is_some());
    store.close().await;
    // Reopening applies nothing twice.
    let store = SqliteJobStore::open(&path).await.unwrap();
    assert_eq!(store.verification_history(p.id()).await.unwrap().len(), 1);
}
