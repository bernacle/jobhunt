//! Cloud-only storage behavior against a real Postgres: multi-user
//! isolation, the shared corpus, encryption at rest, sync, concurrency,
//! distributed leases, accounts and migrations.
//!
//! Needs `JOBHUNT_TEST_DATABASE_URL`; skipped without it (and failing when
//! `JOBHUNT_REQUIRE_POSTGRES` is set, as in CI's cloud job).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use common::*;
use jobhunt_core::SourceKey;
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Eligibility, cached_assess};
use jobhunt_jobs::verification::{FreshnessPolicy, VerificationService, VerifyMode, cached};
use jobhunt_jobs::{JobQuery, JobRepository};
use jobhunt_profile::entities::{EntityKind, decompose};
use jobhunt_profile::{PreferenceValue, ProfileId, ProfileRepository, ProfileService, Stance};
use jobhunt_ranking::{FeedbackAction, FeedbackRepository, RankingService, RuleReader};
use jobhunt_storage::Store;
use jobhunt_storage::postgres::testing::TestDatabase;
use jobhunt_storage::postgres::{
    Keyring, PgSettings, PgStore, ScheduleSettings, ScheduledSource, SourceOutcome,
    VerificationPick, WorkerKind,
};
use jobhunt_storage::sync::{EntityMutation, PullRequest, PushRequest, SYNC_PROTOCOL};
use sqlx::Row;

macro_rules! pg {
    () => {
        match PgFixture::new().await {
            Some(f) => f,
            None => return,
        }
    };
}

async fn raw_pool(fixture: &PgFixture) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(fixture.db.options())
        .await
        .unwrap()
}

#[tokio::test]
async fn private_data_is_isolated_between_accounts() {
    let f = pg!();
    let alice = f.user("alice").await;
    let bob = f.user("bob").await;
    import_resume(&alice, "ana_lima.md", at(0)).await;
    let job = posting("ramp", "1", "Senior Rust Engineer");
    scan(&alice, "ramp", std::slice::from_ref(&job), at(1), true).await;
    let record = alice.get(job.id()).await.unwrap().unwrap();
    RankingService::new(&alice, &RuleReader)
        .record(
            std::slice::from_ref(&record),
            FeedbackAction::Reject,
            Some("too corporate"),
            at(2),
        )
        .await
        .unwrap();

    // Bob sees the shared job, but none of Alice's private data, even with
    // the same (local) profile id.
    assert!(bob.get(job.id()).await.unwrap().is_some());
    assert!(ProfileService::new(&bob).load().await.unwrap().is_none());
    let pid = ProfileId::local().to_string();
    assert!(bob.feedback(&pid).await.unwrap().is_empty());
    assert!(
        bob.feedback_for_jobs(&pid, &[job.id()])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        bob.profile_events(ProfileId::local(), 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(bob.stats().await.unwrap().feedback, 0);
    assert_eq!(alice.stats().await.unwrap().feedback, 1);

    // Bob writing his own profile does not touch Alice's.
    ProfileService::new(&bob)
        .set_preference(
            PreferenceValue::CurrentLocation {
                place: "Porto Alegre, Brazil".into(),
            },
            Stance::Required,
            at(3),
        )
        .await
        .unwrap();
    let alice_data = ProfileService::new(&alice).require().await.unwrap();
    assert_eq!(alice_data.profile.revision, 1);
    let bob_data = ProfileService::new(&bob).require().await.unwrap();
    assert!(bob_data.experiences.is_empty());
    assert_eq!(alice.feedback(&pid).await.unwrap().len(), 1);

    // A deleted account takes its private data with it; shared jobs stay.
    assert!(f.shared.delete_account(alice.user()).await.unwrap());
    let pool = raw_pool(&f).await;
    let leftover: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM profile_entities WHERE user_id = $1) + \
         (SELECT COUNT(*) FROM feedback WHERE user_id = $1) + \
         (SELECT COUNT(*) FROM profile_events WHERE user_id = $1)",
    )
    .bind(alice.user().as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leftover, 0);
    assert!(bob.get(job.id()).await.unwrap().is_some());
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn shared_jobs_have_shared_verification_and_per_person_eligibility() {
    let f = pg!();
    let alice = f.user("alice").await;
    let bob = f.user("bob").await;
    let mut job = posting("ramp", "1", "Backend Engineer");
    job.location = Some("Remote - Brazil".into());
    job.locations.clear();
    scan(&alice, "ramp", std::slice::from_ref(&job), at(0), true).await;
    // The same job discovered again (by anyone) is the same row.
    scan(&bob, "ramp", std::slice::from_ref(&job), at(1), true).await;
    assert_eq!(bob.count(&JobQuery::default()).await.unwrap(), 1);

    for (store, place) in [(&alice, "São Paulo, Brazil"), (&bob, "Lisbon, Portugal")] {
        ProfileService::new(store)
            .set_preference(
                PreferenceValue::CurrentLocation {
                    place: place.into(),
                },
                Stance::Required,
                at(2),
            )
            .await
            .unwrap();
    }
    let record = alice.get(job.id()).await.unwrap().unwrap();
    let live = Live::new(std::slice::from_ref(&job));
    VerificationService::new(&alice, &live)
        .verify(std::slice::from_ref(&record), VerifyMode::Force, at(3))
        .await
        .unwrap();
    // Bob reuses Alice's verification: facts about a listing are shared.
    let bob_view = VerificationService::new(&bob, &live)
        .verify(std::slice::from_ref(&record), VerifyMode::IfDue, at(4))
        .await
        .unwrap();
    assert!(bob_view[0].reused);

    let policy = FreshnessPolicy::default();
    let mut decisions = Vec::new();
    for store in [&alice, &bob] {
        let data = ProfileService::new(store).require().await.unwrap();
        let facts = ProfileFacts::from_profile(&data);
        let verified = cached(store, std::slice::from_ref(&record)).await.unwrap();
        let (a, _) = cached_assess(store, &verified, &facts, &policy, at(5))
            .await
            .unwrap();
        decisions.push(a.decision.status);
    }
    assert_eq!(
        decisions,
        vec![Eligibility::Eligible, Eligibility::Ineligible],
        "one shared job, a decision per person"
    );

    // Feedback on the shared job stays separate.
    RankingService::new(&alice, &RuleReader)
        .record(
            std::slice::from_ref(&record),
            FeedbackAction::Save,
            None,
            at(6),
        )
        .await
        .unwrap();
    RankingService::new(&bob, &RuleReader)
        .record(
            std::slice::from_ref(&record),
            FeedbackAction::Reject,
            Some("Brazil only"),
            at(6),
        )
        .await
        .unwrap();
    let a_state = RankingService::new(&alice, &RuleReader)
        .state(std::slice::from_ref(&record))
        .await
        .unwrap();
    let b_state = RankingService::new(&bob, &RuleReader)
        .state(std::slice::from_ref(&record))
        .await
        .unwrap();
    assert_ne!(a_state.stage, b_state.stage);
    f.finish().await;
}

#[tokio::test]
async fn private_values_are_encrypted_at_rest_and_bound_to_their_owner() {
    let f = pg!();
    let alice = f.user("alice").await;
    let bob = f.user("bob").await;
    import_resume(&alice, "ana_lima.md", at(0)).await;
    import_resume(&bob, "ana_lima.md", at(0)).await;
    let job = posting("ramp", "1", "Engineer");
    scan(&alice, "ramp", std::slice::from_ref(&job), at(1), true).await;
    let record = alice.get(job.id()).await.unwrap().unwrap();
    RankingService::new(&alice, &RuleReader)
        .record(
            std::slice::from_ref(&record),
            FeedbackAction::Reject,
            Some("my manager works there"),
            at(2),
        )
        .await
        .unwrap();
    let pool = raw_pool(&f).await;
    let bodies: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT body FROM profile_entities WHERE NOT deleted UNION ALL \
         SELECT reason FROM feedback WHERE reason IS NOT NULL UNION ALL \
         SELECT detail FROM profile_events",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!bodies.is_empty());
    let data = ProfileService::new(&alice).require().await.unwrap();
    let needles: Vec<String> = [
        data.documents[0]
            .text
            .lines()
            .find(|l| l.len() > 20)
            .unwrap()
            .to_owned(),
        data.experiences[0].company.clone().unwrap(),
        "my manager works there".to_owned(),
        "ana_lima.md".to_owned(),
    ]
    .into_iter()
    .collect();
    for body in &bodies {
        for needle in &needles {
            assert!(
                !body.windows(needle.len()).any(|w| w == needle.as_bytes()),
                "plaintext {needle:?} found in the database"
            );
        }
    }
    // A ciphertext moved into another account's row does not decrypt.
    sqlx::query(
        "UPDATE profile_entities b SET body = a.body FROM profile_entities a \
         WHERE a.user_id = $1 AND b.user_id = $2 AND a.kind = 'profile' AND b.kind = 'profile'",
    )
    .bind(alice.user().as_str())
    .bind(bob.user().as_str())
    .execute(&pool)
    .await
    .unwrap();
    let fresh_bob = f.shared.for_user(bob.user().clone());
    assert!(ProfileService::new(&fresh_bob).load().await.is_err());
    assert!(ProfileService::new(&alice).load().await.unwrap().is_some());
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn key_rotation_reseals_old_values() {
    let Some(db) = TestDatabase::create().await else {
        return;
    };
    let old_key = "old:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let new_key = "new:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    let old = db.store(Keyring::parse(old_key).unwrap()).await;
    let account = old
        .sign_in("https://issuer.test/", "a", Utc::now())
        .await
        .unwrap();
    import_resume(&old.for_user(account.id.clone()), "ana_lima.md", at(0)).await;
    old.close().await;

    let rotated_keys = Keyring::parse(&format!("{new_key},{old_key}")).unwrap();
    let rotated = db.store(rotated_keys).await;
    let before = ProfileService::new(&rotated.for_user(account.id.clone()))
        .require()
        .await
        .unwrap();
    let report = rotated.reencrypt().await.unwrap();
    assert!(report.rewritten > 0);
    assert_eq!(
        rotated.reencrypt().await.unwrap().rewritten,
        0,
        "idempotent"
    );
    rotated.close().await;

    let only_new = db.store(Keyring::parse(new_key).unwrap()).await;
    let after = ProfileService::new(&only_new.for_user(account.id))
        .require()
        .await
        .unwrap();
    assert_eq!(before, after);
    only_new.close().await;
    db.drop_database().await;
}

#[tokio::test]
async fn sync_push_is_compare_and_set_and_idempotent() {
    let f = pg!();
    let alice = f.user("alice").await;
    // A profile made "on a laptop".
    let laptop = sqlite().await;
    import_resume(laptop.as_ref(), "ana_lima.md", at(0)).await;
    let data = ProfileService::new(laptop.as_ref())
        .require()
        .await
        .unwrap();
    let entities = decompose(&data).unwrap();
    let push = PushRequest {
        protocol: SYNC_PROTOCOL,
        push_id: "p1".into(),
        entities: entities
            .iter()
            .map(|e| EntityMutation {
                kind: e.key.kind,
                id: e.key.id.clone(),
                base_version: 0,
                body: Some(e.body.clone()),
            })
            .collect(),
        feedback: Vec::new(),
        jobs: Vec::new(),
    };
    let first = alice.sync_push(&push, at(1)).await.unwrap();
    assert!(first.conflicts.is_empty());
    assert!(first.applied.iter().all(|a| a.version == 1));
    assert_eq!(first.profile_revision, 1);
    let cloud = ProfileService::new(&alice).require().await.unwrap();
    assert_eq!(cloud.experiences, data.experiences);
    assert_eq!(cloud.claims, data.claims);

    // Retrying the same push changes nothing.
    let retry = alice.sync_push(&push, at(2)).await.unwrap();
    assert!(retry.conflicts.is_empty());
    assert_eq!(retry.profile_revision, 1);

    // A change based on an old version conflicts, and nothing is applied.
    let claim = entities
        .iter()
        .find(|e| e.key.kind == EntityKind::Claim)
        .unwrap();
    let mut confirmed = claim.body.clone();
    confirmed["verification"] = "confirmed".into();
    let ok = alice
        .sync_push(
            &PushRequest {
                protocol: SYNC_PROTOCOL,
                push_id: "p2".into(),
                entities: vec![EntityMutation {
                    kind: claim.key.kind,
                    id: claim.key.id.clone(),
                    base_version: 1,
                    body: Some(confirmed.clone()),
                }],
                feedback: Vec::new(),
                jobs: Vec::new(),
            },
            at(3),
        )
        .await
        .unwrap();
    assert_eq!(ok.applied[0].version, 2);
    assert_eq!(ok.profile_revision, 2);
    let mut rejected = claim.body.clone();
    rejected["verification"] = "rejected".into();
    let stale = alice
        .sync_push(
            &PushRequest {
                protocol: SYNC_PROTOCOL,
                push_id: "p3".into(),
                entities: vec![EntityMutation {
                    kind: claim.key.kind,
                    id: claim.key.id.clone(),
                    base_version: 1,
                    body: Some(rejected),
                }],
                feedback: Vec::new(),
                jobs: Vec::new(),
            },
            at(4),
        )
        .await
        .unwrap();
    assert!(stale.applied.is_empty());
    assert_eq!(stale.conflicts.len(), 1);
    assert_eq!(stale.conflicts[0].current.version, 2);
    assert_eq!(
        stale.conflicts[0].current.body.as_ref().unwrap()["verification"],
        "confirmed"
    );

    // A push leaving a dangling reference is refused as a whole.
    let experience = entities
        .iter()
        .find(|e| e.key.kind == EntityKind::Experience)
        .unwrap();
    let invalid = alice
        .sync_push(
            &PushRequest {
                protocol: SYNC_PROTOCOL,
                push_id: "p4".into(),
                entities: vec![EntityMutation {
                    kind: experience.key.kind,
                    id: experience.key.id.clone(),
                    base_version: 1,
                    body: None,
                }],
                feedback: Vec::new(),
                jobs: Vec::new(),
            },
            at(5),
        )
        .await;
    assert!(invalid.is_err());

    // Pull: everything after a cursor, tombstones and versions included.
    let all = alice
        .sync_pull(
            &PullRequest {
                protocol: SYNC_PROTOCOL,
                cursor: 0,
            },
            at(6),
        )
        .await
        .unwrap();
    assert_eq!(all.entities.len(), entities.len());
    assert_eq!(all.profile_revision, 2);
    let since = alice
        .sync_pull(
            &PullRequest {
                protocol: SYNC_PROTOCOL,
                cursor: all.cursor,
            },
            at(7),
        )
        .await
        .unwrap();
    assert!(since.entities.is_empty());
    assert_eq!(since.cursor, all.cursor);
    f.finish().await;
}

#[tokio::test]
async fn sync_feedback_carries_its_jobs_and_is_idempotent() {
    let f = pg!();
    let alice = f.user("alice").await;
    let laptop = sqlite().await;
    let job = posting("local-only-board", "1", "Engineer");
    scan(
        laptop.as_ref(),
        "local-only-board",
        std::slice::from_ref(&job),
        at(0),
        true,
    )
    .await;
    let record = laptop.get(job.id()).await.unwrap().unwrap();
    let recorded = RankingService::new(laptop.as_ref(), &RuleReader)
        .record(
            std::slice::from_ref(&record),
            FeedbackAction::Applied,
            Some("referral from Maria"),
            at(1),
        )
        .await
        .unwrap();
    let push = PushRequest {
        protocol: SYNC_PROTOCOL,
        push_id: "f1".into(),
        entities: Vec::new(),
        feedback: vec![recorded.event.clone()],
        jobs: vec![record.clone()],
    };
    let first = alice.sync_push(&push, at(2)).await.unwrap();
    assert_eq!((first.feedback_added, first.jobs_added), (1, 1));
    let again = alice.sync_push(&push, at(3)).await.unwrap();
    assert_eq!((again.feedback_added, again.feedback_present), (0, 1));
    // The cloud stores the synced job, marked for verification.
    let pool = raw_pool(&f).await;
    let origin: String = sqlx::query_scalar("SELECT origin FROM jobs WHERE id = $1")
        .bind(job.id().to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(origin, "sync");
    let pulled = alice
        .sync_pull(
            &PullRequest {
                protocol: SYNC_PROTOCOL,
                cursor: 0,
            },
            at(4),
        )
        .await
        .unwrap();
    assert_eq!(pulled.feedback, vec![recorded.event]);
    assert_eq!(pulled.jobs.len(), 1);
    // Feedback about a job nobody has is refused.
    let orphan = PushRequest {
        jobs: Vec::new(),
        feedback: vec![{
            let mut e = pulled.feedback[0].clone();
            e.id = jobhunt_ranking::FeedbackId::derive(
                "x",
                posting("nowhere", "1", "x").id(),
                FeedbackAction::Save,
                at(5),
                None,
            );
            e.job = posting("nowhere", "1", "x").id();
            e
        }],
        ..push.clone()
    };
    assert!(alice.sync_push(&orphan, at(5)).await.is_err());
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn concurrent_writers_are_serialized_per_person() {
    let f = pg!();
    let alice = Arc::new(f.user("alice").await);
    import_resume(alice.as_ref(), "ana_lima.md", at(0)).await;
    let data = ProfileService::new(alice.as_ref()).require().await.unwrap();

    // Two saves from the same revision: exactly one wins.
    let mut handles = Vec::new();
    for i in 0..2 {
        let store = Arc::clone(&alice);
        let mut next = data.clone();
        next.profile.revision += 1;
        next.profile.headline = Some(format!("Headline {i}"));
        handles.push(tokio::spawn(async move {
            store.save_profile(&next, 1, &[]).await
        }));
    }
    let mut ok = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(()) => ok += 1,
            Err(jobhunt_profile::StorageError::Conflict { .. }) => {}
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(ok, 1);

    // Many concurrent feedback writes: each gets its own sequence number.
    let job = posting("ramp", "1", "Engineer");
    scan(
        alice.as_ref(),
        "ramp",
        std::slice::from_ref(&job),
        at(1),
        true,
    )
    .await;
    let record = alice.get(job.id()).await.unwrap().unwrap();
    let mut handles = Vec::new();
    for i in 0..10 {
        let store = Arc::clone(&alice);
        let record = record.clone();
        handles.push(tokio::spawn(async move {
            let event = jobhunt_ranking::FeedbackEvent::new(
                &ProfileId::local().to_string(),
                record.opportunity_id,
                record.id,
                FeedbackAction::Like,
                Some(&format!("reason {i}")),
                (&record.posting.title, &record.posting.company),
                at(2) + Duration::seconds(i),
            );
            store.record_feedback(&event).await
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }
    let pool = raw_pool(&f).await;
    let seqs: Vec<i64> = sqlx::query_scalar("SELECT seq FROM feedback WHERE user_id = $1")
        .bind(alice.user().as_str())
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(seqs.iter().collect::<HashSet<_>>().len(), 10);

    // Concurrent scans of one source are serialized: no duplicate inserts.
    let postings: Vec<_> = (0..20)
        .map(|i| posting("busy", &i.to_string(), "Engineer"))
        .collect();
    let mut handles = Vec::new();
    for i in 0..4 {
        let store = Arc::clone(&alice);
        let postings = postings.clone();
        handles.push(tokio::spawn(async move {
            scan(store.as_ref(), "busy", &postings, at(10 + i), true).await
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE source_instance = 'busy'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 20);
    let news: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_events e JOIN jobs j ON j.id = e.job_id \
         WHERE j.source_instance = 'busy' AND e.kind = 'new'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(news, 20, "each job is NEW exactly once");
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn source_leases_are_exclusive_expire_and_back_off() {
    let f = pg!();
    let store = &f.shared;
    let sources: Vec<ScheduledSource> = ["a", "b", "c", "d"]
        .iter()
        .map(|i| ScheduledSource {
            key: SourceKey::new("ashby", i).unwrap(),
            company: None,
        })
        .collect();
    let now = Utc::now();
    store.register_sources(&sources, now).await.unwrap();
    let lease = StdDuration::from_secs(600);

    // Two workers at once get disjoint sources.
    let (one, two) = tokio::join!(
        store.claim_due_sources("worker-1", 3, lease, now),
        store.claim_due_sources("worker-2", 3, lease, now)
    );
    let (one, two) = (one.unwrap(), two.unwrap());
    let claimed: HashSet<_> = one.iter().chain(&two).map(|c| c.key.clone()).collect();
    assert_eq!(one.len() + two.len(), 4);
    assert_eq!(claimed.len(), 4, "no source claimed twice");
    assert!(
        store
            .claim_due_sources("worker-3", 10, lease, now)
            .await
            .unwrap()
            .is_empty()
    );

    // A crashed worker's lease expires and the source is taken over.
    let later = now + Duration::seconds(601);
    let taken = store
        .claim_due_sources("worker-3", 10, lease, later)
        .await
        .unwrap();
    assert_eq!(taken.len(), 4);
    // The original owner can no longer finish it.
    let settings = ScheduleSettings::default();
    assert!(
        !store
            .finish_source(
                "worker-1",
                &one[0].key,
                &SourceOutcome::Succeeded,
                &settings,
                later
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .finish_source(
                "worker-3",
                &taken[0].key,
                &SourceOutcome::Succeeded,
                &settings,
                later
            )
            .await
            .unwrap()
    );
    let failure = SourceOutcome::Failed {
        error: "HTTP 503".into(),
    };
    for _ in 0..3 {
        let due = store.schedule_overview().await.unwrap();
        let row = due
            .iter()
            .find(|r| r.source == taken[1].key.to_string())
            .unwrap();
        let claim_at = if row.leased_by.is_some() {
            later
        } else {
            row.next_due_at
        };
        if row.leased_by.is_none() {
            let got = store
                .claim_due_sources("worker-3", 10, lease, claim_at)
                .await
                .unwrap();
            assert!(got.iter().any(|c| c.key == taken[1].key));
        }
        store
            .finish_source("worker-3", &taken[1].key, &failure, &settings, claim_at)
            .await
            .unwrap();
    }
    let rows = store.schedule_overview().await.unwrap();
    let failing = rows
        .iter()
        .find(|r| r.source == taken[1].key.to_string())
        .unwrap();
    assert_eq!(failing.consecutive_failures, 3);
    // Retried after 1 h, then 2 h, then 4 h.
    let last = failing.last_finished_at.unwrap();
    assert_eq!(failing.next_due_at - last, Duration::hours(4));
    let ok = rows
        .iter()
        .find(|r| r.source == taken[0].key.to_string())
        .unwrap();
    assert_eq!(ok.consecutive_failures, 0);
    assert!(ok.next_due_at >= later + Duration::hours(11));

    // Removing a source from the configuration disables it.
    store.register_sources(&sources[..1], later).await.unwrap();
    let enabled = store
        .schedule_overview()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.enabled)
        .count();
    assert_eq!(enabled, 1);

    // Worker runs are recorded; abandoned ones are closed.
    let run = store
        .start_worker_run(
            WorkerKind::Discovery,
            "w1",
            StdDuration::from_secs(3600),
            now,
        )
        .await
        .unwrap();
    let next = store
        .start_worker_run(
            WorkerKind::Discovery,
            "w2",
            StdDuration::from_secs(60),
            now + Duration::hours(2),
        )
        .await
        .unwrap();
    store
        .finish_worker_run(next, &serde_json::json!({"sources": 1}), None, now)
        .await
        .unwrap();
    let pool = raw_pool(&f).await;
    let status: String = sqlx::query_scalar("SELECT status FROM worker_runs WHERE id = $1")
        .bind(run.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "failed");
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn verification_candidates_are_prioritized_and_claims_exclusive() {
    let f = pg!();
    let alice = f.user("alice").await;
    let now = Utc::now();
    let saved = posting("ramp", "1", "Saved job");
    let fresh = posting("ramp", "2", "Verified recently");
    let old = posting("ramp", "3", "Old and unwatched");
    let new = posting("ramp", "4", "Just discovered");
    let then = now - Duration::days(10);
    scan(
        &alice,
        "ramp",
        &[saved.clone(), fresh.clone(), old.clone()],
        then,
        true,
    )
    .await;
    scan(
        &alice,
        "ramp",
        &[saved.clone(), fresh.clone(), old.clone(), new.clone()],
        now,
        true,
    )
    .await;
    let saved_record = alice.get(saved.id()).await.unwrap().unwrap();
    RankingService::new(&alice, &RuleReader)
        .record(
            std::slice::from_ref(&saved_record),
            FeedbackAction::Save,
            None,
            now,
        )
        .await
        .unwrap();
    let fresh_record = alice.get(fresh.id()).await.unwrap().unwrap();
    let live = Live::new(std::slice::from_ref(&fresh));
    VerificationService::new(&alice, &live)
        .verify(std::slice::from_ref(&fresh_record), VerifyMode::Force, now)
        .await
        .unwrap();

    let pick = VerificationPick {
        fresh_for: StdDuration::from_secs(24 * 3600),
        recommended_within: StdDuration::from_secs(7 * 24 * 3600),
        new_within: StdDuration::from_secs(48 * 3600),
        limit: 10,
    };
    let candidates = f.shared.verification_candidates(&pick, now).await.unwrap();
    let ids: Vec<_> = candidates.iter().map(|c| c.job).collect();
    assert_eq!(ids.first(), Some(&saved.id()), "pipeline jobs first");
    assert!(ids.contains(&new.id()));
    assert!(!ids.contains(&fresh.id()), "recently verified: skipped");
    assert!(!ids.contains(&old.id()), "nobody cares about it: skipped");

    let lease = StdDuration::from_secs(300);
    let (a, b) = tokio::join!(
        f.shared.claim_verifications("v1", &ids, lease, now),
        f.shared.claim_verifications("v2", &ids, lease, now)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.len() + b.len(), ids.len(), "each job claimed once");
    f.shared.release_verifications("v1", &a).await.unwrap();
    let again = f
        .shared
        .claim_verifications("v3", &ids, lease, now)
        .await
        .unwrap();
    assert_eq!(again, a, "released claims are free again");
    f.finish().await;
}

#[tokio::test]
async fn accounts_tokens_and_revocation() {
    let f = pg!();
    let now = Utc::now();
    // Concurrent first sign-ins create one account.
    let (one, two) = tokio::join!(
        f.shared.sign_in("https://issuer.test/", "same", now),
        f.shared.sign_in("https://issuer.test/", "same", now)
    );
    let (one, two) = (one.unwrap(), two.unwrap());
    assert_eq!(one.id, two.id);
    assert!(one.created_now ^ two.created_now);
    let other = f
        .shared
        .sign_in("https://other.test/", "same", now)
        .await
        .unwrap();
    assert_ne!(other.id, one.id, "identities are per issuer");

    let created = f
        .shared
        .create_api_token(
            &one.id,
            "claude desktop",
            Some(now + Duration::days(30)),
            now,
        )
        .await
        .unwrap();
    assert!(created.secret.starts_with("jh_pat_"));
    let check = f
        .shared
        .check_api_token(&created.secret, now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(check.user, one.id);
    assert!(
        f.shared
            .check_api_token("jh_pat_nope", now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.shared
            .check_api_token(&created.secret, now + Duration::days(31))
            .await
            .unwrap()
            .is_none(),
        "expired"
    );
    let pool = raw_pool(&f).await;
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM api_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, created.secret.as_bytes(), "only a digest is stored");
    // Another account cannot revoke it.
    assert!(
        !f.shared
            .revoke_api_token(&other.id, &created.token.id, now)
            .await
            .unwrap()
    );
    f.shared.revoke_sessions(&one.id, now).await.unwrap();
    assert!(
        f.shared
            .check_api_token(&created.secret, now)
            .await
            .unwrap()
            .is_none()
    );
    let account = f.shared.account(&one.id).await.unwrap().unwrap();
    assert!(account.tokens_valid_after.is_some());
    pool.close().await;
    f.finish().await;
}

#[tokio::test]
async fn migrations_run_concurrently_and_report_schema() {
    let Some(db) = TestDatabase::create().await else {
        return;
    };
    let mut stores = Vec::new();
    for _ in 0..3 {
        stores.push(
            PgStore::connect_with(db.options(), &PgSettings::default(), Keyring::ephemeral())
                .await
                .unwrap(),
        );
    }
    let before = stores[0].schema().await.unwrap();
    assert_eq!(before.applied, 0);
    assert!(!before.is_current());
    let results = futures::future::join_all(stores.iter().map(|s| s.migrate())).await;
    for r in results {
        assert!(r.unwrap().is_current());
    }
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_with(db.options())
        .await
        .unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied as u64, before.known);
    let row = sqlx::query(
        "SELECT COUNT(*) AS n FROM information_schema.tables WHERE table_name = 'profile_entities'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("n"), 1);
    pool.close().await;
    for s in stores {
        s.close().await;
    }
    db.drop_database().await;
}

#[tokio::test]
async fn usage_events_are_recorded_and_summarized() {
    let f = pg!();
    let alice = f.user("alice").await;
    let now = Utc::now();
    f.shared
        .record_usage(&[
            jobhunt_storage::postgres::UsageEvent {
                user: Some(alice.user().clone()),
                at: now,
                event: "find".into(),
                metadata: serde_json::json!({"results": 5}),
            },
            jobhunt_storage::postgres::UsageEvent {
                user: None,
                at: now,
                event: "login".into(),
                metadata: serde_json::json!({}),
            },
        ])
        .await
        .unwrap();
    let summary = f
        .shared
        .usage_summary(now - Duration::hours(1))
        .await
        .unwrap();
    assert_eq!(
        summary,
        vec![("find".to_owned(), 1, 1), ("login".to_owned(), 1, 0)]
    );
    f.finish().await;
}
