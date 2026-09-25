//! Local ↔ cloud sync, end to end: two local SQLite databases (a laptop
//! and a second machine) and one cloud account in a real Postgres, all
//! running the same application use cases.
//!
//! Needs `JOBHUNT_TEST_DATABASE_URL` (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_app::preferences::PreferenceUpdate;
use jobhunt_app::sync::Side;
use jobhunt_app::{
    App, AppConfig, AppError, DiscoveryMode, ErrorKind, LoadedConfig, Remote, SyncTransport,
};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::{JobPosting, ScanBody, ScanWrite, WorkplaceType};
use jobhunt_profile::{ExperienceEdit, Verification};
use jobhunt_ranking::{FeedbackAction, Stage};
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};
use jobhunt_storage::postgres::testing::TestDatabase;
use jobhunt_storage::postgres::{Keyring, PgStore, PgUserStore, UserId};
use jobhunt_storage::sync::{PullRequest, PullResponse, PushError, PushRequest, PushResponse};
use jobhunt_storage::{SqliteJobStore, Store};

fn at(minute: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap() + Duration::minutes(minute)
}

fn loaded() -> Arc<LoadedConfig> {
    Arc::new(LoadedConfig {
        config: AppConfig::default(),
        file: None,
        default_file: None,
        database: PathBuf::from(":memory:"),
    })
}

async fn laptop() -> App {
    App::with_store(
        (*loaded()).clone(),
        SqliteJobStore::open_in_memory().await.unwrap(),
    )
}

/// The cloud, in process: the same calls the HTTP API makes.
struct Direct {
    store: PgUserStore,
    online: AtomicBool,
}

#[async_trait]
impl SyncTransport for Direct {
    async fn pull(&self, request: &PullRequest) -> Result<PullResponse, AppError> {
        if !self.online.load(Ordering::SeqCst) {
            return Err(AppError::CloudUnavailable("offline".into()));
        }
        self.store
            .sync_pull(request, Utc::now())
            .await
            .map_err(|e| AppError::CloudUnavailable(e.to_string()))
    }

    async fn push(&self, request: &PushRequest) -> Result<PushResponse, AppError> {
        if !self.online.load(Ordering::SeqCst) {
            return Err(AppError::CloudUnavailable("offline".into()));
        }
        self.store
            .sync_push(request, Utc::now())
            .await
            .map_err(|e| match e {
                PushError::Invalid(m) => AppError::InvalidArguments(m),
                PushError::Storage(e) => AppError::CloudUnavailable(e.to_string()),
            })
    }
}

struct Cloud {
    db: TestDatabase,
    shared: PgStore,
    user: UserId,
}

impl Cloud {
    async fn new() -> Option<Self> {
        let db = TestDatabase::create().await?;
        let shared = db.store(Keyring::ephemeral()).await;
        let user = shared
            .sign_in("https://issuer.test/", "ana", Utc::now())
            .await
            .unwrap()
            .id;
        Some(Self { db, shared, user })
    }

    fn transport(&self) -> Direct {
        Direct {
            store: self.shared.for_user(self.user.clone()),
            online: AtomicBool::new(true),
        }
    }

    /// The hosted application for the account (as the API and MCP use it).
    fn app(&self) -> App {
        App::from_parts(
            loaded(),
            Arc::new(self.shared.for_user(self.user.clone())),
            DiscoveryMode::Background,
        )
    }

    fn remote(&self) -> Remote {
        Remote {
            server: "https://cloud.test".into(),
            user_id: self.user.to_string(),
        }
    }

    async fn finish(self) {
        self.shared.close().await;
        self.db.drop_database().await;
    }
}

async fn import_resume(app: &App, fixture: &str, now: DateTime<Utc>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jobhunt-resume/tests/fixtures")
        .join(fixture);
    let file = ResumeFile::read(&path).unwrap();
    let parser = DeterministicParser;
    let parsed = parser.parse(&file.text);
    app.profiles()
        .import_resume(file.source_document(parser.name(), now), &parsed, now)
        .await
        .unwrap();
}

fn posting(id: &str, title: &str) -> JobPosting {
    JobPosting {
        provenance: Provenance {
            source: SourceKey::new("greenhouse", "acme").unwrap(),
            source_record_id: Some(id.into()),
            fetched_from: None,
        },
        url: CanonicalUrl::parse(&format!("https://job-boards.greenhouse.io/acme/jobs/{id}"))
            .unwrap(),
        apply_url: None,
        company: "Acme".into(),
        title: title.into(),
        department: None,
        team: None,
        location: Some("Remote - Worldwide".into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: Some(true),
        compensation: None,
        work_authorization: None,
        description_text: Some("Rust and PostgreSQL".into()),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    }
}

async fn discover(store: &dyn Store, postings: &[JobPosting], when: DateTime<Utc>) {
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
                close_missing: false,
                closing_withheld: None,
                retain: &[],
                validator: None,
            },
        })
        .await
        .unwrap();
}

async fn feedback(app: &App, id: &str, action: FeedbackAction, reason: Option<&str>, when: i64) {
    let opportunity = app.resolve(id).await.unwrap();
    app.record_feedback(&opportunity, action, reason, at(when))
        .await
        .unwrap();
}

#[tokio::test]
async fn first_sync_repeat_and_a_second_machine() {
    let Some(cloud) = Cloud::new().await else {
        return;
    };
    let transport = cloud.transport();
    let local = laptop().await;
    import_resume(&local, "ana_lima.md", at(0)).await;
    let data = local.profiles().require().await.unwrap();
    let claim = data.claims[0].id;
    local
        .profiles()
        .decide_claims(&[claim.to_string()], Verification::Confirmed, None, at(1))
        .await
        .unwrap();
    local
        .update_preferences(
            &PreferenceUpdate {
                statement: Some("Remote only, backend or platform work".into()),
                ..PreferenceUpdate::default()
            },
            at(2),
        )
        .await
        .unwrap();
    let job = posting("1", "Senior Rust Engineer");
    discover(local.store(), std::slice::from_ref(&job), at(3)).await;
    feedback(
        &local,
        &job.id().to_string(),
        FeedbackAction::Applied,
        Some("referral from a friend"),
        4,
    )
    .await;

    // First sync: everything goes up.
    let first = local
        .sync(&transport, &cloud.remote(), None, at(5))
        .await
        .unwrap();
    assert!(first.pushed > 10, "{first:?}");
    assert_eq!(first.feedback_pushed, 1);
    assert!(first.conflicts.is_empty());
    let hosted = cloud.app();
    let in_cloud = hosted.profiles().require().await.unwrap();
    let here = local.profiles().require().await.unwrap();
    assert_eq!(in_cloud.claims, here.claims);
    assert_eq!(in_cloud.preferences, here.preferences);
    assert_eq!(
        in_cloud.claim(claim).unwrap().verification,
        Verification::Confirmed
    );
    let pipeline = hosted.pipeline(false).await.unwrap();
    assert_eq!(pipeline.len(), 1);
    assert_eq!(pipeline[0].state.stage, Stage::Applied);

    // Syncing again changes nothing, on either side.
    let revision = here.profile.revision;
    let again = local
        .sync(&transport, &cloud.remote(), None, at(6))
        .await
        .unwrap();
    assert_eq!(
        (again.pushed, again.applied_locally, again.feedback_pushed),
        (0, 0, 0)
    );
    assert_eq!(
        local.profiles().require().await.unwrap().profile.revision,
        revision
    );
    assert_eq!(
        hosted.profiles().require().await.unwrap().profile.revision,
        in_cloud.profile.revision
    );

    // A second machine gets the same profile, feedback and job.
    let desktop = laptop().await;
    let pulled = desktop
        .sync(&transport, &cloud.remote(), None, at(7))
        .await
        .unwrap();
    assert!(pulled.applied_locally > 10);
    assert_eq!(pulled.feedback_pulled, 1);
    assert_eq!(pulled.jobs_pulled, 1);
    let there = desktop.profiles().require().await.unwrap();
    assert_eq!(there.claims, here.claims);
    assert_eq!(there.statements, here.statements);
    let desk_pipeline = desktop.pipeline(false).await.unwrap();
    assert_eq!(desk_pipeline[0].state.stage, Stage::Applied);
    assert_eq!(
        desk_pipeline[0].state.events[0].reason.as_deref(),
        Some("referral from a friend")
    );
    cloud.finish().await;
}

#[tokio::test]
async fn changes_flow_both_ways_and_non_conflicting_edits_merge() {
    let Some(cloud) = Cloud::new().await else {
        return;
    };
    let transport = cloud.transport();
    let local = laptop().await;
    import_resume(&local, "ana_lima.md", at(0)).await;
    local
        .sync(&transport, &cloud.remote(), None, at(1))
        .await
        .unwrap();
    let hosted = cloud.app();
    let data = local.profiles().require().await.unwrap();
    let (first, second) = (data.claims[0].id, data.claims[1].id);

    // Cloud → local: a preference stated through the hosted MCP/API, and
    // a claim confirmed there.
    hosted
        .update_preferences(
            &PreferenceUpdate {
                statement: Some("At least USD 150k".into()),
                ..PreferenceUpdate::default()
            },
            at(2),
        )
        .await
        .unwrap();
    hosted
        .profiles()
        .decide_claims(&[first.to_string()], Verification::Confirmed, None, at(3))
        .await
        .unwrap();
    // Local, at the same time: another claim rejected (no overlap).
    local
        .profiles()
        .decide_claims(&[second.to_string()], Verification::Rejected, None, at(3))
        .await
        .unwrap();

    let report = local
        .sync(&transport, &cloud.remote(), None, at(4))
        .await
        .unwrap();
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert!(report.applied_locally >= 2);
    assert!(report.pushed >= 1);
    for app in [&local, &hosted] {
        let d = app.profiles().require().await.unwrap();
        assert_eq!(
            d.claim(first).unwrap().verification,
            Verification::Confirmed
        );
        assert_eq!(
            d.claim(second).unwrap().verification,
            Verification::Rejected
        );
        assert_eq!(d.statements.len(), 1);
    }

    // The same record edited on both sides, in different fields: merged.
    let experience = local.profiles().require().await.unwrap().experiences[0].clone();
    local
        .profiles()
        .edit_experience(
            &experience.id.to_string(),
            ExperienceEdit {
                title: Some(Some("Staff Engineer".into())),
                ..ExperienceEdit::default()
            },
            at(5),
        )
        .await
        .unwrap();
    hosted
        .profiles()
        .edit_experience(
            &experience.id.to_string(),
            ExperienceEdit {
                summary: Some(Some("Led the payments platform".into())),
                ..ExperienceEdit::default()
            },
            at(6),
        )
        .await
        .unwrap();
    let merged = local
        .sync(&transport, &cloud.remote(), None, at(7))
        .await
        .unwrap();
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
    assert_eq!(merged.merged, 1);
    for app in [&local, &hosted] {
        let e = app
            .profiles()
            .require()
            .await
            .unwrap()
            .experience(experience.id)
            .unwrap()
            .clone();
        assert_eq!(e.title.as_deref(), Some("Staff Engineer"));
        assert_eq!(e.summary.as_deref(), Some("Led the payments platform"));
        assert!(e.meta.is_edited("title") && e.meta.is_edited("summary"));
    }
    cloud.finish().await;
}

#[tokio::test]
async fn conflicting_decisions_are_surfaced_never_lost() {
    let Some(cloud) = Cloud::new().await else {
        return;
    };
    let transport = cloud.transport();
    let local = laptop().await;
    import_resume(&local, "ana_lima.md", at(0)).await;
    local
        .sync(&transport, &cloud.remote(), None, at(1))
        .await
        .unwrap();
    let hosted = cloud.app();
    let claim = local.profiles().require().await.unwrap().claims[0].id;
    local
        .profiles()
        .decide_claims(&[claim.to_string()], Verification::Confirmed, None, at(2))
        .await
        .unwrap();
    hosted
        .profiles()
        .decide_claims(&[claim.to_string()], Verification::Rejected, None, at(2))
        .await
        .unwrap();

    let report = local
        .sync(&transport, &cloud.remote(), None, at(3))
        .await
        .unwrap();
    assert_eq!(report.conflicts.len(), 1);
    assert!(report.conflicts[0].reason.contains("decided differently"));
    // Neither decision was overwritten.
    let here = local.profiles().require().await.unwrap();
    assert_eq!(
        here.claim(claim).unwrap().verification,
        Verification::Confirmed
    );
    let there = hosted.profiles().require().await.unwrap();
    assert_eq!(
        there.claim(claim).unwrap().verification,
        Verification::Rejected
    );
    // Syncing again keeps it open (and doesn't push the local side).
    let again = local
        .sync(&transport, &cloud.remote(), None, at(4))
        .await
        .unwrap();
    assert_eq!(again.conflicts.len(), 1);
    assert_eq!(
        hosted
            .profiles()
            .require()
            .await
            .unwrap()
            .claim(claim)
            .unwrap()
            .verification,
        Verification::Rejected
    );
    let status = local.sync_status().await.unwrap();
    assert_eq!(status.conflicts.len(), 1);

    // The person keeps the local decision: it wins everywhere.
    assert_eq!(
        local
            .resolve_sync_conflicts(&[], Side::Local, at(5))
            .await
            .unwrap(),
        1
    );
    let resolved = local
        .sync(&transport, &cloud.remote(), None, at(6))
        .await
        .unwrap();
    assert!(resolved.conflicts.is_empty());
    assert_eq!(
        hosted
            .profiles()
            .require()
            .await
            .unwrap()
            .claim(claim)
            .unwrap()
            .verification,
        Verification::Confirmed
    );

    // Another conflict, resolved the other way.
    hosted
        .profiles()
        .decide_claims(&[claim.to_string()], Verification::Unverified, None, at(7))
        .await
        .unwrap();
    local
        .profiles()
        .decide_claims(&[claim.to_string()], Verification::Rejected, None, at(7))
        .await
        .unwrap();
    let report = local
        .sync(&transport, &cloud.remote(), None, at(8))
        .await
        .unwrap();
    assert_eq!(report.conflicts.len(), 1);
    local
        .resolve_sync_conflicts(&[], Side::Cloud, at(9))
        .await
        .unwrap();
    assert_eq!(
        local
            .profiles()
            .require()
            .await
            .unwrap()
            .claim(claim)
            .unwrap()
            .verification,
        Verification::Unverified
    );
    assert!(
        local
            .sync(&transport, &cloud.remote(), None, at(10))
            .await
            .unwrap()
            .conflicts
            .is_empty()
    );
    cloud.finish().await;
}

#[tokio::test]
async fn offline_changes_sync_later_and_feedback_unions() {
    let Some(cloud) = Cloud::new().await else {
        return;
    };
    let transport = cloud.transport();
    let local = laptop().await;
    import_resume(&local, "ana_lima.md", at(0)).await;
    let jobs = [
        posting("1", "Backend Engineer"),
        posting("2", "Platform Engineer"),
    ];
    discover(local.store(), &jobs, at(1)).await;
    local
        .sync(&transport, &cloud.remote(), None, at(2))
        .await
        .unwrap();

    // Offline: work continues locally; sync fails without changing anything.
    transport.online.store(false, Ordering::SeqCst);
    feedback(
        &local,
        &jobs[0].id().to_string(),
        FeedbackAction::Reject,
        Some("on-call heavy"),
        3,
    )
    .await;
    local
        .update_preferences(
            &PreferenceUpdate {
                statement: Some("No on-call rotations".into()),
                ..PreferenceUpdate::default()
            },
            at(3),
        )
        .await
        .unwrap();
    let offline = local
        .sync(&transport, &cloud.remote(), None, at(4))
        .await
        .unwrap_err();
    assert_eq!(offline.kind(), ErrorKind::CloudUnavailable);
    assert_eq!(local.sync_status().await.unwrap().unsynced_feedback, 1);

    // Meanwhile the cloud records other feedback (from a hosted client).
    let hosted = cloud.app();
    discover(hosted.store(), &jobs, at(4)).await;
    feedback(
        &hosted,
        &jobs[1].id().to_string(),
        FeedbackAction::Save,
        None,
        5,
    )
    .await;

    // Back online: both sides end with both pieces of feedback.
    transport.online.store(true, Ordering::SeqCst);
    let report = local
        .sync(&transport, &cloud.remote(), None, at(6))
        .await
        .unwrap();
    assert_eq!((report.feedback_pushed, report.feedback_pulled), (1, 1));
    for app in [&local, &hosted] {
        let all = app.pipeline(true).await.unwrap();
        let stages: Vec<Stage> = all.iter().map(|e| e.state.stage).collect();
        assert!(stages.contains(&Stage::Rejected) && stages.contains(&Stage::Saved));
        assert!(
            app.profiles()
                .require()
                .await
                .unwrap()
                .statements
                .iter()
                .any(|s| s.text == "No on-call rotations")
        );
    }
    assert_eq!(local.sync_status().await.unwrap().unsynced_feedback, 0);
    cloud.finish().await;
}
