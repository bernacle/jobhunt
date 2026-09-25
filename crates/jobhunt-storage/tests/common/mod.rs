//! Shared helpers for the storage contract and Postgres tests.

#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationCheck, ApplicationStatus, ListingVerifier, ObserveError,
    ObservedListing, SourceObservation, VerificationMethod,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, EmploymentType, JobPosting, JobRecord,
    PayInterval, ScanBody, ScanResult, ScanWrite, SourceLocation, WorkplaceType,
};
use jobhunt_profile::ProfileService;
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};
use jobhunt_storage::postgres::testing::TestDatabase;
use jobhunt_storage::postgres::{Keyring, PgStore, PgUserStore};
use jobhunt_storage::{SqliteJobStore, Store};

pub fn at(minute: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap() + Duration::minutes(minute)
}

pub fn posting(instance: &str, native_id: &str, title: &str) -> JobPosting {
    JobPosting {
        provenance: Provenance {
            source: SourceKey::new("ashby", instance).unwrap(),
            source_record_id: Some(native_id.to_owned()),
            fetched_from: Some(
                CanonicalUrl::parse(&format!(
                    "https://api.ashbyhq.com/posting-api/job-board/{instance}?includeCompensation=true"
                ))
                .unwrap(),
            ),
        },
        url: CanonicalUrl::parse(&format!("https://jobs.ashbyhq.com/{instance}/{native_id}"))
            .unwrap(),
        apply_url: Some(
            CanonicalUrl::parse(&format!(
                "https://jobs.ashbyhq.com/{instance}/{native_id}/application"
            ))
            .unwrap(),
        ),
        company: instance.to_uppercase(),
        title: title.to_owned(),
        department: Some("Engineering".into()),
        team: Some("Platform".into()),
        location: Some("Remote - Worldwide".into()),
        locations: vec![SourceLocation {
            name: Some("Remote - Worldwide".into()),
            ..Default::default()
        }],
        employment_type: Some(EmploymentType::FullTime),
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: Some(true),
        compensation: Some(Compensation {
            summary: Some("$150K – $190K".into()),
            components: vec![CompensationComponent {
                kind: CompensationKind::Salary,
                label: None,
                currency: Some("USD".into()),
                min: Some(150_000.0),
                max: Some(190_000.0),
                interval: Some(PayInterval::Year),
            }],
        }),
        work_authorization: None,
        description_text: Some(
            "Requirements\nStrong experience with Rust and PostgreSQL.\nBuild backend services."
                .into(),
        ),
        description_html: Some("<p>Build things.</p>".into()),
        posted_at: Some(Utc.with_ymd_and_hms(2026, 9, 1, 17, 12, 35).unwrap()),
        source_updated_at: None,
    }
}

/// Persists one listing of `ashby:<instance>` in its own run.
pub async fn scan(
    store: &dyn Store,
    instance: &str,
    postings: &[JobPosting],
    observed_at: DateTime<Utc>,
    close_missing: bool,
) -> ScanResult {
    let source = SourceKey::new("ashby", instance).unwrap();
    let run = store.begin_run(observed_at).await.unwrap();
    store
        .apply_scan(&ScanWrite {
            run,
            source: &source,
            started_at: observed_at,
            observed_at,
            counts: IngestCounts {
                received: postings.len(),
                normalized: postings.len(),
                ..IngestCounts::default()
            },
            body: ScanBody::Listing {
                postings,
                complete: close_missing,
                close_missing,
                closing_withheld: None,
                retain: &[],
                validator: Some("\"etag\""),
            },
        })
        .await
        .unwrap()
}

/// Finds whatever posting it is asked about, from a list it is given.
pub struct Live(pub Mutex<Vec<JobPosting>>);

impl Live {
    pub fn new(postings: &[JobPosting]) -> Self {
        Self(Mutex::new(postings.to_vec()))
    }
}

#[async_trait]
impl ListingVerifier for Live {
    async fn observe(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let posting = self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id() == record.id)
            .cloned();
        let Some(posting) = posting else {
            return Ok(SourceObservation {
                method: VerificationMethod::GreenhouseJobApi,
                checked_url: record.posting.url.to_string(),
                listing: ObservedListing::NotFound {
                    detail: "404".into(),
                },
                application: ApplicationCheck {
                    status: ApplicationStatus::Closed,
                    basis: ApplicationBasis::Probed,
                    url: None,
                    http_status: Some(404),
                    detail: None,
                },
                chain: Vec::new(),
                unknowns: Vec::new(),
            });
        };
        Ok(SourceObservation {
            method: VerificationMethod::GreenhouseJobApi,
            checked_url: posting.url.to_string(),
            listing: ObservedListing::Found(Box::new(posting.clone())),
            application: ApplicationCheck {
                status: ApplicationStatus::Active,
                basis: ApplicationBasis::Probed,
                url: Some(posting.url.to_string()),
                http_status: Some(200),
                detail: None,
            },
            chain: Vec::new(),
            unknowns: Vec::new(),
        })
    }
}

/// Imports a fixture resume into the store's profile.
pub async fn import_resume(store: &dyn Store, fixture: &str, now: DateTime<Utc>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jobhunt-resume/tests/fixtures")
        .join(fixture);
    let file = ResumeFile::read(&path).unwrap();
    let parser = DeterministicParser;
    let parsed = parser.parse(&file.text);
    ProfileService::new(store)
        .import_resume(file.source_document(parser.name(), now), &parsed, now)
        .await
        .unwrap();
}

pub async fn sqlite() -> Arc<dyn Store> {
    Arc::new(SqliteJobStore::open_in_memory().await.unwrap())
}

/// A migrated Postgres store with one account, or `None` without a test
/// server.
pub struct PgFixture {
    pub db: TestDatabase,
    pub shared: PgStore,
}

impl PgFixture {
    pub async fn new() -> Option<Self> {
        let db = TestDatabase::create().await?;
        let shared = db.store(Keyring::ephemeral()).await;
        Some(Self { db, shared })
    }

    pub async fn user(&self, subject: &str) -> PgUserStore {
        let account = self
            .shared
            .sign_in("https://issuer.test/", subject, Utc::now())
            .await
            .unwrap();
        self.shared.for_user(account.id)
    }

    pub async fn finish(self) {
        self.shared.close().await;
        self.db.drop_database().await;
    }
}
