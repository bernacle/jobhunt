use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, TimeZone};
use jobhunt_core::{IngestCounts, SourceKey};

use super::*;
use crate::memory::MemoryRepository;
use crate::model::tests::posting;
use crate::model::{Compensation, CompensationComponent, CompensationKind, JobStatus, PayInterval};
use crate::repository::{JobEventKind, RunId, ScanBody, ScanWrite};

fn at(hours: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap() + Duration::hours(hours)
}

/// Answers from a script, counting calls.
#[derive(Default)]
struct Scripted {
    answers: Mutex<HashMap<JobId, Result<SourceObservation, ObserveError>>>,
    calls: AtomicUsize,
}

impl Scripted {
    fn set(&self, job: JobId, answer: Result<SourceObservation, ObserveError>) {
        self.answers.lock().unwrap().insert(job, answer);
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ListingVerifier for Scripted {
    async fn observe(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answers
            .lock()
            .unwrap()
            .get(&record.id)
            .cloned()
            .unwrap_or(Err(ObserveError::NotSupported {
                kind: record.posting.provenance.source.kind().to_owned(),
            }))
    }
}

fn found(p: &JobPosting) -> Result<SourceObservation, ObserveError> {
    Ok(SourceObservation {
        method: VerificationMethod::GreenhouseJobApi,
        checked_url: "https://boards-api.example/jobs/1".into(),
        listing: ObservedListing::Found(Box::new(p.clone())),
        application: ApplicationCheck {
            status: ApplicationStatus::Active,
            basis: ApplicationBasis::Probed,
            url: Some("https://jobs.example.com/apply".into()),
            http_status: Some(200),
            detail: None,
        },
        chain: vec![AuthorityLink {
            kind: LinkKind::AtsApi,
            url: "https://boards-api.example/jobs/1".into(),
            checked: true,
            note: "the board's API".into(),
        }],
        unknowns: Vec::new(),
    })
}

fn not_found() -> Result<SourceObservation, ObserveError> {
    Ok(SourceObservation {
        method: VerificationMethod::GreenhouseJobApi,
        checked_url: "https://boards-api.example/jobs/1".into(),
        listing: ObservedListing::NotFound {
            detail: "HTTP 404".into(),
        },
        application: ApplicationCheck::unknown("listing closed"),
        chain: Vec::new(),
        unknowns: Vec::new(),
    })
}

fn usd(min: f64) -> Compensation {
    Compensation {
        summary: None,
        components: vec![CompensationComponent {
            kind: CompensationKind::Salary,
            label: None,
            currency: Some("USD".into()),
            min: Some(min),
            max: Some(min + 40_000.0),
            interval: Some(PayInterval::Year),
        }],
    }
}

/// A repository holding `postings`, discovered by one scan at `at(0)`.
async fn discovered(postings: &[JobPosting]) -> MemoryRepository {
    let repo = MemoryRepository::default();
    scan(&repo, postings, true, at(0)).await;
    repo
}

async fn scan(repo: &MemoryRepository, postings: &[JobPosting], close: bool, when: DateTime<Utc>) {
    let source: SourceKey = postings
        .first()
        .map(|p| p.provenance.source.clone())
        .unwrap_or_else(|| "greenhouse:acme".parse().unwrap());
    repo.apply_scan(&ScanWrite {
        run: RunId(1),
        source: &source,
        started_at: when,
        observed_at: when,
        counts: IngestCounts::default(),
        body: ScanBody::Listing {
            postings,
            complete: true,
            close_missing: close,
            closing_withheld: None,
            retain: &[],
            validator: None,
        },
    })
    .await
    .unwrap();
}

async fn record(repo: &MemoryRepository, p: &JobPosting) -> JobRecord {
    repo.get(p.id()).await.unwrap().unwrap()
}

#[tokio::test]
async fn an_active_listing_is_verified_and_marks_the_job_seen() {
    let mut p = posting("greenhouse:acme", Some("1"), "Engineer");
    p.compensation = Some(usd(100_000.0));
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(p.id(), found(&p));
    let service = VerificationService::new(&repo, &verifier);

    let out = service
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(5))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    assert_eq!(v.listing, ListingStatus::Active);
    assert_eq!(v.application.status, ApplicationStatus::Active);
    assert_eq!(v.authority, Authority::EmployerConfiguredAts);
    assert_eq!(v.lifecycle.as_deref(), Some("unchanged"));
    assert!(v.changed_fields.is_empty());
    assert_eq!(v.changed_since_last_verification, None);
    assert_eq!(v.compensation.status, CompensationStatus::Published);
    assert_eq!(v.compensation.change, CompensationChange::FirstVerification);
    assert_eq!(v.published.locations, ["Remote"]);
    assert!(v.succeeded());
    assert_eq!(out[0].record.last_seen_at, at(5), "the job was seen listed");
    assert_eq!(out[0].last_success.as_ref(), Some(v));
    assert!(!out[0].reused);
    assert_eq!(repo.verification_history(p.id()).await.unwrap().len(), 1);
    // Verification records no scan.
    assert_eq!(repo.scans().len(), 1);
}

#[tokio::test]
async fn a_changed_listing_is_updated_through_the_lifecycle() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    let mut changed = p.clone();
    changed.title = "Senior Engineer".into();
    changed.compensation = Some(usd(150_000.0));
    verifier.set(p.id(), found(&changed));
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(5))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    assert_eq!(v.changed_fields, ["title", "compensation"]);
    assert_eq!(v.lifecycle.as_deref(), Some("updated"));
    assert_eq!(out[0].record.posting.title, "Senior Engineer");
    let history = repo.history(p.id()).await.unwrap();
    assert_eq!(history.last().unwrap().kind, JobEventKind::Updated);
    assert_eq!(history.last().unwrap().run, None, "not a discovery run");
    assert_eq!(
        history.last().unwrap().changed_fields,
        ["title", "compensation"]
    );
}

#[tokio::test]
async fn a_closed_job_found_again_is_reopened_with_its_history() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let other = posting("greenhouse:acme", Some("2"), "Designer");
    let repo = discovered(&[p.clone(), other.clone()]).await;
    scan(&repo, std::slice::from_ref(&other), true, at(1)).await;
    assert_eq!(record(&repo, &p).await.status, JobStatus::Closed);

    let verifier = Scripted::default();
    verifier.set(p.id(), found(&p));
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(2))
        .await
        .unwrap();
    assert_eq!(
        out[0].latest.as_ref().unwrap().lifecycle.as_deref(),
        Some("reopened")
    );
    assert_eq!(out[0].record.status, JobStatus::Open);
    let kinds: Vec<JobEventKind> = repo
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
            JobEventKind::Closed,
            JobEventKind::Reopened
        ],
        "not a brand-new job"
    );
    assert_eq!(out[0].record.first_seen_at, at(0));
}

#[tokio::test]
async fn a_deleted_listing_is_verified_closed_without_touching_the_job() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(p.id(), not_found());
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(5))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    assert_eq!(v.listing, ListingStatus::Closed);
    assert!(v.succeeded(), "a definitive answer");
    assert_eq!(v.compensation.status, CompensationStatus::NotObserved);
    assert_eq!(v.lifecycle, None);
    assert_eq!(
        out[0].record.status,
        JobStatus::Open,
        "closing is discovery's call"
    );
    assert_eq!(out[0].record.last_seen_at, at(0));
    let trust = out[0].trust(&FreshnessPolicy::default(), at(5));
    assert_eq!(trust.state, TrustState::VerifiedClosed);
}

#[tokio::test]
async fn failures_keep_the_last_successful_verification() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(p.id(), found(&p));
    let service = VerificationService::new(&repo, &verifier);
    service
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(1))
        .await
        .unwrap();

    for (error, listing, kind) in [
        (
            ObserveError::Timeout { url: "u".into() },
            ListingStatus::Unreachable,
            FailureKind::Timeout,
        ),
        (
            ObserveError::Unavailable {
                url: "u".into(),
                status: Some(503),
                detail: "HTTP 503".into(),
            },
            ListingStatus::Unreachable,
            FailureKind::Unavailable,
        ),
        (
            ObserveError::Malformed {
                url: "u".into(),
                detail: "not JSON".into(),
            },
            ListingStatus::Ambiguous,
            FailureKind::Malformed,
        ),
    ] {
        verifier.set(p.id(), Err(error));
        let out = service
            .verify(&[record(&repo, &p).await], VerifyMode::Force, at(30))
            .await
            .unwrap();
        let latest = out[0].latest.as_ref().unwrap();
        assert_eq!(latest.listing, listing);
        assert_eq!(latest.failure.as_ref().unwrap().kind, kind);
        assert!(!latest.succeeded());
        let success = out[0].last_success.as_ref().unwrap();
        assert_eq!(success.attempted_at, at(1), "the success is kept");
        assert_eq!(
            repo.latest_successful_verification(p.id())
                .await
                .unwrap()
                .unwrap()
                .attempted_at,
            at(1)
        );
        let trust = out[0].trust(&FreshnessPolicy::default(), at(30));
        assert_eq!(trust.state, TrustState::CouldNotVerify);
        assert_eq!(trust.age, Some(VerificationAge::Aging));
    }
    assert_eq!(repo.verification_history(p.id()).await.unwrap().len(), 4);
}

#[tokio::test]
async fn recent_attempts_are_reused_unless_forced() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(p.id(), found(&p));
    let service = VerificationService::new(&repo, &verifier);
    let job = record(&repo, &p).await;
    service
        .verify(std::slice::from_ref(&job), VerifyMode::IfDue, at(1))
        .await
        .unwrap();
    let again = service
        .verify(
            std::slice::from_ref(&job),
            VerifyMode::IfDue,
            at(1) + Duration::minutes(5),
        )
        .await
        .unwrap();
    assert!(again[0].reused);
    assert_eq!(verifier.calls(), 1, "a fresh attempt is not repeated");
    service
        .verify(
            std::slice::from_ref(&job),
            VerifyMode::Force,
            at(1) + Duration::minutes(6),
        )
        .await
        .unwrap();
    assert_eq!(verifier.calls(), 2);
    service
        .verify(std::slice::from_ref(&job), VerifyMode::IfDue, at(2))
        .await
        .unwrap();
    assert_eq!(
        verifier.calls(),
        3,
        "an attempt older than the reuse window is repeated"
    );
    let cached = service.cached(std::slice::from_ref(&job)).await.unwrap();
    assert!(cached[0].latest.is_some());
}

#[tokio::test]
async fn unsupported_sources_are_unknown_not_failed_silently() {
    let p = posting("workday:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(1))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    assert_eq!(v.listing, ListingStatus::Unknown);
    assert_eq!(v.authority, Authority::Unknown);
    assert_eq!(v.failure.as_ref().unwrap().kind, FailureKind::NotSupported);
    assert!(!v.unknowns.is_empty());
}

#[tokio::test]
async fn a_different_record_is_ambiguous() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(
        p.id(),
        found(&posting("greenhouse:acme", Some("9"), "Other")),
    );
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(1))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    assert_eq!(v.listing, ListingStatus::Ambiguous);
    assert_eq!(v.failure.as_ref().unwrap().kind, FailureKind::Mismatch);
}

#[tokio::test]
async fn compensation_changes_between_verifications() {
    let mut p = posting("greenhouse:acme", Some("1"), "Engineer");
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    let service = VerificationService::new(&repo, &verifier);
    let run = |p: JobPosting, hours: i64| {
        verifier.set(p.id(), found(&p));
        let repo = &repo;
        let service = &service;
        async move {
            let job = repo.get(p.id()).await.unwrap().unwrap();
            service
                .verify(&[job], VerifyMode::Force, at(hours))
                .await
                .unwrap()
                .remove(0)
                .latest
                .unwrap()
        }
    };
    let first = run(p.clone(), 1).await;
    assert_eq!(first.compensation.status, CompensationStatus::NotPublished);
    p.compensation = Some(usd(100_000.0));
    let published = run(p.clone(), 2).await;
    assert_eq!(
        published.compensation.change,
        CompensationChange::NewlyPublished
    );
    assert_eq!(published.changed_since_last_verification, Some(true));
    let same = run(p.clone(), 3).await;
    assert_eq!(same.compensation.change, CompensationChange::Unchanged);
    assert_eq!(same.changed_since_last_verification, Some(false));
    p.compensation = Some(usd(120_000.0));
    let changed = run(p.clone(), 4).await;
    assert_eq!(changed.compensation.change, CompensationChange::Changed);
    assert_eq!(changed.compensation.previous, Some(usd(100_000.0)));
    p.compensation = None;
    let removed = run(p.clone(), 5).await;
    assert_eq!(removed.compensation.change, CompensationChange::Removed);
}

#[tokio::test]
async fn a_board_publishing_the_employer_page_is_recorded_in_the_chain() {
    let mut p = posting("greenhouse:stripe", Some("7"), "Engineer");
    p.url = CanonicalUrl::parse("https://stripe.com/jobs/search?gh_jid=7").unwrap();
    let repo = discovered(std::slice::from_ref(&p)).await;
    let verifier = Scripted::default();
    verifier.set(p.id(), found(&p));
    let out = VerificationService::new(&repo, &verifier)
        .verify(&[record(&repo, &p).await], VerifyMode::IfDue, at(1))
        .await
        .unwrap();
    let v = out[0].latest.as_ref().unwrap();
    let page = v
        .authority_chain
        .iter()
        .find(|l| l.kind == LinkKind::EmployerPage)
        .unwrap();
    assert!(page.url.starts_with("https://stripe.com/"));
    assert!(!page.checked);
    // Named by the board, not checked by JobHunt: the board is the authority.
    assert_eq!(v.authority, Authority::EmployerConfiguredAts);
}

fn trust_of(
    p: &JobPosting,
    latest: Option<ListingStatus>,
    success_hours_ago: Option<i64>,
    now: DateTime<Utc>,
) -> RecordTrust {
    let record = JobRecord {
        id: p.id(),
        posting: p.clone(),
        first_seen_at: at(0),
        last_seen_at: at(0),
        content_updated_at: at(0),
        status: JobStatus::Open,
        closed_at: None,
        opportunity_id: crate::model::OpportunityId::founded_by(p.id()),
    };
    let make = |listing: ListingStatus, when: DateTime<Utc>| {
        let (mut v, _) = build_record(
            &record,
            match listing {
                ListingStatus::Active => found(p),
                ListingStatus::Closed => not_found(),
                _ => Err(ObserveError::Timeout { url: "u".into() }),
            },
            None,
            when,
        );
        v.authority = Authority::of_kind(p.provenance.source.kind());
        v
    };
    let success = success_hours_ago.map(|h| make(ListingStatus::Active, now - Duration::hours(h)));
    let latest = latest.map(|l| match (&success, l) {
        (Some(s), ListingStatus::Active) => s.clone(),
        _ => make(l, now),
    });
    RecordTrust::of(
        &record,
        latest.as_ref(),
        success
            .as_ref()
            .or(latest.as_ref().filter(|v| v.succeeded())),
        &FreshnessPolicy::default(),
        now,
    )
}

#[test]
fn opportunity_trust_prefers_the_live_authoritative_record() {
    let policy = FreshnessPolicy::default();
    let now = at(100);
    let yc = posting("yc:acme", Some("1"), "Engineer");
    let ashby = posting("ashby:acme", Some("2"), "Engineer");

    // One source dead, one authoritative source active.
    let t = OpportunityTrust::of(
        vec![
            trust_of(&yc, Some(ListingStatus::Closed), None, now),
            trust_of(&ashby, Some(ListingStatus::Active), Some(1), now),
        ],
        &policy,
        now,
    );
    assert_eq!(t.state, TrustState::VerifiedActive);
    assert_eq!(t.best().unwrap().source.to_string(), "ashby:acme");
    assert_eq!(t.standing, Standing::Trusted);
    assert_eq!(t.records.len(), 2, "every record stays inspectable");
    assert_eq!(t.conflicts.len(), 1);
    assert!(t.conflicts[0].contains("yc:acme"));

    // Two active authoritative sources: the employer's board wins.
    let t = OpportunityTrust::of(
        vec![
            trust_of(&yc, Some(ListingStatus::Active), Some(1), now),
            trust_of(&ashby, Some(ListingStatus::Active), Some(2), now),
        ],
        &policy,
        now,
    );
    assert_eq!(
        t.best().unwrap().authority,
        Authority::EmployerConfiguredAts
    );
    assert!(t.conflicts.is_empty());

    // Stale, failed and unverified listings are not trusted.
    let stale = OpportunityTrust::of(
        vec![trust_of(
            &ashby,
            Some(ListingStatus::Active),
            Some(100),
            now,
        )],
        &policy,
        now,
    );
    assert!(matches!(stale.standing, Standing::NotTrusted(ref r) if r.contains("72 hours")));
    let failed = OpportunityTrust::of(
        vec![trust_of(
            &ashby,
            Some(ListingStatus::Unreachable),
            Some(50),
            now,
        )],
        &policy,
        now,
    );
    assert_eq!(failed.state, TrustState::CouldNotVerify);
    assert!(matches!(failed.standing, Standing::NotTrusted(ref r) if r.contains("2 days ago")));
    let never = OpportunityTrust::of(vec![trust_of(&ashby, None, None, now)], &policy, now);
    assert_eq!(never.state, TrustState::NotVerified);
    assert!(matches!(never.standing, Standing::NotTrusted(_)));

    // A secondary source alone is never trusted to recommend.
    let repost = posting("jobboard:acme", Some("3"), "Engineer");
    let mut t = trust_of(&repost, Some(ListingStatus::Active), Some(1), now);
    t.authority = Authority::SecondarySource;
    let t = OpportunityTrust::of(vec![t], &policy, now);
    assert!(matches!(t.standing, Standing::NotTrusted(ref r) if r.contains("secondary")));
}

#[test]
fn a_job_closed_by_discovery_is_never_shown_verified_active() {
    let p = posting("ashby:acme", Some("2"), "Engineer");
    let now = at(10);
    let mut t = trust_of(&p, Some(ListingStatus::Active), Some(5), now);
    assert_eq!(t.state, TrustState::VerifiedActive);
    let record = JobRecord {
        id: p.id(),
        posting: p.clone(),
        first_seen_at: at(0),
        last_seen_at: at(0),
        content_updated_at: at(0),
        status: JobStatus::Closed,
        closed_at: Some(now - Duration::hours(1)),
        opportunity_id: crate::model::OpportunityId::founded_by(p.id()),
    };
    t = RecordTrust::of(
        &record,
        t.latest.as_ref(),
        t.last_success.as_ref(),
        &FreshnessPolicy::default(),
        now,
    );
    assert_eq!(t.state, TrustState::ClosedByDiscovery);
    let o = OpportunityTrust::of(vec![t], &FreshnessPolicy::default(), now);
    assert!(matches!(o.standing, Standing::NotTrusted(ref r) if r.contains("closed")));
}

#[test]
fn records_serialize_for_storage() {
    let p = posting("greenhouse:acme", Some("1"), "Engineer");
    let record = JobRecord {
        id: p.id(),
        posting: p.clone(),
        first_seen_at: at(0),
        last_seen_at: at(0),
        content_updated_at: at(0),
        status: JobStatus::Open,
        closed_at: None,
        opportunity_id: crate::model::OpportunityId::founded_by(p.id()),
    };
    let (v, posting) = build_record(&record, found(&p), None, at(1));
    assert!(posting.is_some());
    let json = serde_json::to_string(&v).unwrap();
    let back: VerificationRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, v);
}
