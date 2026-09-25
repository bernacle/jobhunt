use std::sync::Mutex;

use chrono::{Duration, TimeZone};
use jobhunt_core::{CanonicalUrl, IngestCounts, Provenance, SourceKey};
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationCheck, ApplicationStatus, ListingVerifier, ObserveError,
    ObservedListing, SourceObservation, VerificationMethod, VerificationService, VerifyMode,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, JobRecord, JobRepository,
    OpportunityId, PayInterval, ScanBody, ScanWrite, WorkplaceType,
};
use jobhunt_profile::{CompensationBound, PayPeriod, PreferenceValue, ProfileService, Stance};
use jobhunt_ranking::{
    Dimension, Exclusion, Gate, RankQuery, RankingService, RuleReader, Stage, TasteKey, Tier,
};

use super::*;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
}

fn posting(id: &str, title: &str, location: &str, pay: Option<(f64, f64)>) -> JobPosting {
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
        location: Some(location.into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: Some(true),
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
        description_text: Some("Requirements\nStrong experience with Rust.".into()),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    }
}

async fn discover(store: &SqliteJobStore, postings: &[JobPosting]) {
    let source = SourceKey::new("greenhouse", "acme").unwrap();
    let when = now() - Duration::hours(1);
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

/// Finds whatever posting it is asked about, as stored.
struct Live(Mutex<Vec<JobPosting>>);

#[async_trait]
impl ListingVerifier for Live {
    async fn observe(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let posting = self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id() == record.id)
            .cloned()
            .unwrap();
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

async fn get(store: &SqliteJobStore, p: &JobPosting) -> JobRecord {
    store.get(p.id()).await.unwrap().unwrap()
}

#[tokio::test]
async fn feedback_round_trips_and_is_found_by_record() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let a = posting("1", "Backend Engineer", "Remote - Worldwide", None);
    let b = posting("2", "Frontend Engineer", "Remote - Worldwide", None);
    discover(&store, &[a.clone(), b.clone()]).await;
    let (ra, rb) = (get(&store, &a).await, get(&store, &b).await);
    let service = RankingService::new(&store, &RuleReader);
    let saved = service
        .record(std::slice::from_ref(&ra), FeedbackAction::Save, None, now())
        .await
        .unwrap();
    let rejected = service
        .record(
            std::slice::from_ref(&rb),
            FeedbackAction::Reject,
            Some("  stack is too frontend-heavy "),
            now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(
        rejected.event.reason.as_deref(),
        Some("stack is too frontend-heavy")
    );
    assert_eq!(rejected.before.stage, Stage::Unseen);
    assert_eq!(rejected.after.stage, Stage::Rejected);
    assert!(!rejected.reading.unwrap().is_unread());

    let all = store.feedback(&saved.event.profile_id).await.unwrap();
    assert_eq!(all, [saved.event.clone(), rejected.event.clone()]);
    let only_a = store
        .feedback_for_jobs(&saved.event.profile_id, &[ra.id])
        .await
        .unwrap();
    assert_eq!(only_a, std::slice::from_ref(&saved.event));
    assert!(store.feedback("prof_other").await.unwrap().is_empty());
    assert!(
        store
            .feedback_for_jobs(&saved.event.profile_id, &[])
            .await
            .unwrap()
            .is_empty()
    );
    // Looking at a job is recorded once.
    assert!(
        service
            .mark_seen(std::slice::from_ref(&ra), now())
            .await
            .is_ok()
    );
    let unseen = posting("3", "Data Engineer", "Remote - Worldwide", None);
    discover(&store, &[a.clone(), b.clone(), unseen.clone()]).await;
    let ru = get(&store, &unseen).await;
    assert!(
        service
            .mark_seen(std::slice::from_ref(&ru), now())
            .await
            .unwrap()
    );
    assert!(
        !service
            .mark_seen(std::slice::from_ref(&ru), now())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn duplicates_share_state_and_feedback_follows_merges() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let a = posting("1", "Backend Engineer", "Remote - Worldwide", None);
    let b = posting("2", "Backend Engineer", "Remote - Worldwide", None);
    discover(&store, &[a.clone(), b.clone()]).await;
    let ra = get(&store, &a).await;
    let service = RankingService::new(&store, &RuleReader);
    service
        .record(
            std::slice::from_ref(&ra),
            FeedbackAction::Applied,
            None,
            now(),
        )
        .await
        .unwrap();
    // Identity grouping later finds they are one job, named after b.
    let merged = OpportunityId::founded_by(b.id());
    store
        .assign_opportunities(&[(a.id(), merged), (b.id(), merged)])
        .await
        .unwrap();
    let records = store.opportunity_records(merged).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(service.state(&records).await.unwrap().stage, Stage::Applied);
    let pipeline = service.pipeline(false).await.unwrap();
    assert_eq!(pipeline.len(), 1);
    assert_eq!(pipeline[0].opportunity, merged);
    assert_eq!(pipeline[0].state.stage, Stage::Applied);
}

#[tokio::test]
async fn ranks_explains_and_keeps_what_was_shown() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let profile = ProfileService::new(&store);
    let set = |value, stance| profile.set_preference(value, stance, now() - Duration::days(1));
    set(
        PreferenceValue::CurrentLocation {
            place: "Berlin, Germany".into(),
        },
        Stance::Required,
    )
    .await
    .unwrap();
    set(
        PreferenceValue::Role {
            role: "backend".into(),
        },
        Stance::Wanted,
    )
    .await
    .unwrap();
    set(
        PreferenceValue::Compensation {
            bound: CompensationBound::Minimum,
            amount: 150_000,
            currency: Some("USD".into()),
            period: PayPeriod::Year,
            arrangement: None,
        },
        Stance::Required,
    )
    .await
    .unwrap();

    let good = posting(
        "1",
        "Senior Backend Engineer",
        "Remote - Worldwide",
        Some((170_000.0, 200_000.0)),
    );
    let sre = posting("2", "Site Reliability Engineer", "Remote - Worldwide", None);
    let us = posting("3", "Backend Engineer", "Remote (US)", None);
    let cheap = posting(
        "4",
        "Backend Engineer",
        "Remote - Worldwide",
        Some((90_000.0, 100_000.0)),
    );
    let all = [good.clone(), sre.clone(), us.clone(), cheap.clone()];
    discover(&store, &all).await;
    let verifier = Live(Mutex::new(all.to_vec()));
    for p in [&good, &cheap] {
        let r = get(&store, p).await;
        VerificationService::new(&store, &verifier)
            .verify(&[r], VerifyMode::Force, now() - Duration::minutes(5))
            .await
            .unwrap();
    }

    let service = RankingService::new(&store, &RuleReader);
    let query = RankQuery {
        text: String::new(),
        store_top: 10,
        all: false,
    };
    let report = service.rank(&query, now()).await.unwrap();
    assert_eq!(report.considered, 4);
    let titles: Vec<(&str, &Gate)> = report
        .rankings
        .iter()
        .map(|r| (r.title.as_str(), &r.gate))
        .collect();
    assert_eq!(titles.len(), 2, "{titles:?}");
    assert_eq!(titles[0].0, "Senior Backend Engineer");
    assert_eq!(report.rankings[0].gate, Gate::Recommended);
    assert_eq!(report.rankings[0].tier, Tier::StrongFit);
    assert!(matches!(report.rankings[1].gate, Gate::VerifyFirst { .. }));
    assert_eq!(report.excluded.ineligible, 1);
    assert_eq!(report.excluded.below_minimum, 1);

    // Rejecting with a reason: gone from the list, learned from.
    let rs = get(&store, &sre).await;
    service
        .record(
            std::slice::from_ref(&rs),
            FeedbackAction::Reject,
            Some("pure SRE"),
            now(),
        )
        .await
        .unwrap();
    let report = service.rank(&query, now()).await.unwrap();
    assert_eq!(report.rankings.len(), 1);
    assert_eq!(report.excluded.rejected, 1);
    let sre_taste = report.taste.get(&TasteKey::role("sre")).unwrap();
    assert_eq!(sre_taste.reasons, 1);
    assert!(
        report
            .taste
            .get(&TasteKey::new(Dimension::Company, "acme"))
            .is_some()
    );

    // What was shown is stored under its inputs and explained from there.
    let rg = store
        .opportunity_records(get(&store, &good).await.opportunity_id)
        .await
        .unwrap();
    let explained = service.explain(&rg, now()).await.unwrap();
    assert!(explained.reused);
    assert_eq!(explained.ranking, report.rankings[0]);
    // A new piece of feedback is a new key.
    service
        .record(&rg, FeedbackAction::Like, Some("great product"), now())
        .await
        .unwrap();
    let again = service.explain(&rg, now()).await.unwrap();
    assert!(!again.reused);
    assert!(
        again
            .ranking
            .brief
            .history
            .iter()
            .any(|h| h.contains("great product"))
    );

    // The cheap job, verified below a required minimum, says why.
    let rc = get(&store, &cheap).await;
    let explained = service
        .explain(std::slice::from_ref(&rc), now())
        .await
        .unwrap();
    assert!(matches!(
        explained.ranking.gate,
        Gate::Excluded {
            exclusion: Exclusion::BelowMinimum { .. }
        }
    ));
}

#[tokio::test]
async fn ranking_needs_a_profile() {
    let store = SqliteJobStore::open_in_memory().await.unwrap();
    let service = RankingService::new(&store, &RuleReader);
    let error = service
        .rank(&RankQuery::default(), now())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("needs a career profile"),
        "{error}"
    );
}
