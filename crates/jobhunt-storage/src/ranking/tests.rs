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
    // The work is what they want and nothing else is known to fit (pay
    // meeting the minimum adds nothing): worth reviewing, not Today.
    assert_eq!(report.rankings[0].tier, Tier::WorthReviewing);
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

/// The semantic review stage, end to end on SQLite with a scripted
/// reviewer: what is shortlisted, cached, held back or raised, and what a
/// failing reviewer leaves (the rules' assessment, never weaker jobs).
mod semantic_review {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use jobhunt_profile::CompanyTrait;
    use jobhunt_ranking::review::{FitReview, FitReviewRequest, ReviewBudget, ReviewPoint};
    use jobhunt_ranking::{FitLevel, FitReviewer, ReviewError, ReviewState};

    use super::*;

    /// Answers by job title; counts calls.
    struct Scripted {
        answers: HashMap<&'static str, Result<FitLevel, ReviewError>>,
        calls: AtomicUsize,
    }

    impl Scripted {
        fn new(answers: &[(&'static str, Result<FitLevel, ReviewError>)]) -> Self {
            Self {
                answers: answers.iter().cloned().collect(),
                calls: AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    fn point(aspect: &str, quote: &str) -> ReviewPoint {
        ReviewPoint {
            aspect: aspect.into(),
            reason: format!("About the {aspect}"),
            quote: quote.into(),
        }
    }

    #[async_trait]
    impl FitReviewer for Scripted {
        fn name(&self) -> String {
            "model/test:scripted".into()
        }

        async fn review(&self, request: &FitReviewRequest) -> Result<FitReview, ReviewError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let fit = self
                .answers
                .get(request.job.title.as_str())
                .cloned()
                .unwrap_or(Err(ReviewError::Transport("no script".into())))?;
            Ok(FitReview {
                fit,
                role_fit: "match".into(),
                company_fit: "unknown".into(),
                seniority: "unknown".into(),
                specialization: "unknown".into(),
                affirmative: vec![
                    point("role", "backend services"),
                    point("ownership", "end to end"),
                ],
                contradictions: vec![point("specialization", "backend services")],
                uncertainties: Vec::new(),
                reviewer: self.name(),
                rejected: 0,
                input_tokens: 2_000,
                output_tokens: 300,
            })
        }
    }

    const SMALL_TEAM: &str = "You'll join a team of 6 engineers and own backend services end \
        to end.\nRequirements\nStrong experience with Rust.";
    const PLAIN: &str = "You will build backend services end to end.\nRequirements\nStrong \
        experience with Rust.";

    /// Someone in Berlin wanting backend roles on small teams; a strong
    /// fit (backend, small team) and a plausible one (backend only).
    async fn setup() -> SqliteJobStore {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let profile = ProfileService::new(&store);
        let at = now() - Duration::days(1);
        for (value, stance) in [
            (
                PreferenceValue::CurrentLocation {
                    place: "Berlin, Germany".into(),
                },
                Stance::Required,
            ),
            (
                PreferenceValue::Role {
                    role: "backend".into(),
                },
                Stance::Wanted,
            ),
            (
                PreferenceValue::Company {
                    company: CompanyTrait::SmallTeam,
                },
                Stance::Wanted,
            ),
        ] {
            profile.set_preference(value, stance, at).await.unwrap();
        }
        let mut strong = posting("1", "Senior Backend Engineer", "Remote - Worldwide", None);
        strong.description_text = Some(SMALL_TEAM.into());
        let mut plausible = posting("2", "Backend Engineer", "Remote - Worldwide", None);
        plausible.description_text = Some(PLAIN.into());
        let all = [strong, plausible];
        discover(&store, &all).await;
        let verifier = Live(Mutex::new(all.to_vec()));
        for p in &all {
            let r = get(&store, p).await;
            VerificationService::new(&store, &verifier)
                .verify(&[r], VerifyMode::Force, now() - Duration::minutes(5))
                .await
                .unwrap();
        }
        store
    }

    fn tiers(report: &jobhunt_ranking::RankReport) -> Vec<(String, Tier)> {
        let mut out: Vec<(String, Tier)> = report
            .rankings
            .iter()
            .map(|r| (r.title.clone(), r.tier))
            .collect();
        out.sort();
        out
    }

    const STRONG: &str = "Senior Backend Engineer";
    const PLAUSIBLE: &str = "Backend Engineer";

    #[tokio::test]
    async fn the_rules_alone_without_a_reviewer() {
        let store = setup().await;
        let report = RankingService::new(&store, &RuleReader)
            .rank(&RankQuery::default(), now())
            .await
            .unwrap();
        assert_eq!(
            tiers(&report),
            [
                (PLAUSIBLE.to_owned(), Tier::WorthReviewing),
                (STRONG.to_owned(), Tier::StrongFit)
            ]
        );
        assert_eq!(report.review, jobhunt_ranking::ReviewStats::default());
        assert!(
            report
                .rankings
                .iter()
                .all(|r| r.fit.review == ReviewState::NotReviewed)
        );
    }

    #[tokio::test]
    async fn reviews_are_stored_and_never_paid_for_twice() {
        let store = setup().await;
        let reviewer = Scripted::new(&[
            (STRONG, Ok(FitLevel::Strong)),
            (PLAUSIBLE, Ok(FitLevel::Plausible)),
        ]);
        let service = RankingService::new(&store, &RuleReader)
            .with_reviewer(&reviewer, ReviewBudget::default());
        let first = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!(first.review.shortlisted, 2);
        assert_eq!(first.review.calls, 2);
        assert_eq!(first.review.input_tokens, 4_000);
        let again = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!(
            reviewer.calls(),
            2,
            "the same inputs are never reviewed again"
        );
        assert_eq!(again.review.cache_hits, 2);
        assert_eq!(again.review.calls, 0);
        assert_eq!(tiers(&first), tiers(&again));
        // Explaining one job reads the stored review; it never calls.
        let r = store
            .opportunity_records(first.rankings[0].opportunity)
            .await
            .unwrap();
        let explained = service.explain(&r, now()).await.unwrap();
        assert!(matches!(
            explained.ranking.fit.review,
            ReviewState::Reviewed { .. }
        ));
        assert_eq!(reviewer.calls(), 2);
    }

    #[tokio::test]
    async fn a_review_holds_back_a_strong_fit_and_raises_a_plausible_one_on_quotes() {
        let store = setup().await;
        let reviewer = Scripted::new(&[
            (STRONG, Ok(FitLevel::Plausible)),
            (PLAUSIBLE, Ok(FitLevel::Strong)),
        ]);
        let report = RankingService::new(&store, &RuleReader)
            .with_reviewer(&reviewer, ReviewBudget::default())
            .rank(&RankQuery::default(), now())
            .await
            .unwrap();
        assert_eq!(
            tiers(&report),
            [
                (PLAUSIBLE.to_owned(), Tier::StrongFit),
                (STRONG.to_owned(), Tier::WorthReviewing)
            ]
        );
        assert_eq!(report.review.changed, 2);
        let raised = report
            .rankings
            .iter()
            .find(|r| r.title == PLAUSIBLE)
            .unwrap();
        assert!(
            raised
                .brief
                .worth
                .iter()
                .any(|w| w.contains("“end to end”")),
            "the review's quoted reasons are shown: {:?}",
            raised.brief.worth
        );
        assert!(raised.fit.assessor.contains("model/test:scripted"));
    }

    #[tokio::test]
    async fn a_failing_reviewer_leaves_the_rules_assessment_and_asks_again_later() {
        for error in [
            ReviewError::Status {
                status: 429,
                retryable: true,
            },
            ReviewError::Transport("timed out".into()),
            ReviewError::Malformed("not the expected JSON".into()),
            ReviewError::Refused,
        ] {
            let store = setup().await;
            let reviewer = Scripted::new(&[
                (STRONG, Err(error.clone())),
                (PLAUSIBLE, Err(error.clone())),
            ]);
            let service = RankingService::new(&store, &RuleReader)
                .with_reviewer(&reviewer, ReviewBudget::default());
            let report = service.rank(&RankQuery::default(), now()).await.unwrap();
            assert_eq!(
                tiers(&report),
                [
                    (PLAUSIBLE.to_owned(), Tier::WorthReviewing),
                    (STRONG.to_owned(), Tier::StrongFit)
                ],
                "{error}: the rules stand, nothing weaker is promoted"
            );
            assert_eq!(report.review.failures, 2, "{error}");
            assert!(report.rankings.iter().all(|r| matches!(
                &r.fit.review,
                ReviewState::Unavailable { why } if *why == error.short()
            )));
            service.rank(&RankQuery::default(), now()).await.unwrap();
            assert_eq!(reviewer.calls(), 4, "{error}: nothing stored, asked again");
        }
    }

    #[tokio::test]
    async fn a_partial_failure_keeps_what_was_reviewed() {
        let store = setup().await;
        let reviewer = Scripted::new(&[
            (STRONG, Ok(FitLevel::Poor)),
            (
                PLAUSIBLE,
                Err(ReviewError::Status {
                    status: 503,
                    retryable: true,
                }),
            ),
        ]);
        let report = RankingService::new(&store, &RuleReader)
            .with_reviewer(&reviewer, ReviewBudget::default())
            .rank(&RankQuery::default(), now())
            .await
            .unwrap();
        assert_eq!(
            tiers(&report),
            [
                (PLAUSIBLE.to_owned(), Tier::WorthReviewing),
                (STRONG.to_owned(), Tier::LowPriority)
            ]
        );
        assert_eq!((report.review.calls, report.review.failures), (2, 1));
    }

    #[tokio::test]
    async fn a_corrupt_stored_review_is_reviewed_again() {
        let store = setup().await;
        let reviewer = Scripted::new(&[
            (STRONG, Ok(FitLevel::Strong)),
            (PLAUSIBLE, Ok(FitLevel::Plausible)),
        ]);
        let service = RankingService::new(&store, &RuleReader)
            .with_reviewer(&reviewer, ReviewBudget::default());
        service.rank(&RankQuery::default(), now()).await.unwrap();
        sqlx::query("UPDATE fit_reviews SET review = '{not json'")
            .execute(&store.pool)
            .await
            .unwrap();
        let report = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!(report.review.cache_hits, 0);
        assert_eq!(
            report.review.calls, 2,
            "read as a miss, reviewed and stored again"
        );
        let again = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!(again.review.cache_hits, 2);
    }

    #[tokio::test]
    async fn the_budget_bounds_calls_and_defers_the_rest() {
        let store = setup().await;
        let reviewer = Scripted::new(&[
            (STRONG, Ok(FitLevel::Strong)),
            (PLAUSIBLE, Ok(FitLevel::Plausible)),
        ]);
        let budget = ReviewBudget {
            new_reviews: 1,
            ..ReviewBudget::default()
        };
        let service = RankingService::new(&store, &RuleReader).with_reviewer(&reviewer, budget);
        let report = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!((report.review.calls, report.review.deferred), (1, 1));
        // The strong fit (what Today would show) is reviewed first.
        let strong = report.rankings.iter().find(|r| r.title == STRONG).unwrap();
        assert!(matches!(strong.fit.review, ReviewState::Reviewed { .. }));
        let report = service.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!((report.review.cache_hits, report.review.calls), (1, 1));
    }
}

/// Ranking asks the repository a fixed number of questions, whatever the
/// size of the corpus: records, verification state and eligibility come
/// in batches, never one call per job (thousands of round trips against a
/// networked database).
mod repository_calls {
    use std::collections::{BTreeMap, HashMap};

    use jobhunt_core::UpsertOutcome;
    use jobhunt_eligibility::{CacheKey, EligibilityDecision, EligibilityRepository};
    use jobhunt_jobs::verification::{
        LatestVerifications, VerificationRecord, VerificationRepository,
    };
    use jobhunt_jobs::{
        IdentityEntry, JobEvent, JobId, JobQuery, LastListing, RunId, RunSummary, ScanResult,
        StorageError,
    };
    use jobhunt_profile::{
        Claim, ClaimQuery, ProfileData, ProfileEvent, ProfileId, ProfileRepository,
    };
    use jobhunt_ranking::{FeedbackEvent, FeedbackRepository, RankKey, Ranking, RankingRepository};

    use super::*;

    /// A repository that counts the calls made to it.
    struct Counting<'a> {
        inner: &'a SqliteJobStore,
        calls: Mutex<BTreeMap<&'static str, usize>>,
    }

    impl<'a> Counting<'a> {
        fn new(inner: &'a SqliteJobStore) -> Self {
            Self {
                inner,
                calls: Mutex::default(),
            }
        }
        fn count(&self, method: &'static str) {
            *self.calls.lock().unwrap().entry(method).or_default() += 1;
        }
        fn calls(&self) -> BTreeMap<&'static str, usize> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl JobRepository for Counting<'_> {
        async fn begin_run(&self, at: DateTime<Utc>) -> Result<RunId, StorageError> {
            self.count("begin_run");
            self.inner.begin_run(at).await
        }
        async fn finish_run(&self, run: RunId, summary: &RunSummary) -> Result<(), StorageError> {
            self.count("finish_run");
            self.inner.finish_run(run, summary).await
        }
        async fn last_listing(
            &self,
            source: &SourceKey,
        ) -> Result<Option<LastListing>, StorageError> {
            self.count("last_listing");
            self.inner.last_listing(source).await
        }
        async fn apply_scan(&self, scan: &ScanWrite<'_>) -> Result<ScanResult, StorageError> {
            self.count("apply_scan");
            self.inner.apply_scan(scan).await
        }
        async fn identity_index(&self) -> Result<Vec<IdentityEntry>, StorageError> {
            self.count("identity_index");
            self.inner.identity_index().await
        }
        async fn assign_opportunities(
            &self,
            assignments: &[(JobId, OpportunityId)],
        ) -> Result<(), StorageError> {
            self.count("assign_opportunities");
            self.inner.assign_opportunities(assignments).await
        }
        async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError> {
            self.count("get");
            self.inner.get(id).await
        }
        async fn get_many(&self, ids: &[JobId]) -> Result<HashMap<JobId, JobRecord>, StorageError> {
            self.count("get_many");
            self.inner.get_many(ids).await
        }
        async fn opportunity_records(
            &self,
            id: OpportunityId,
        ) -> Result<Vec<JobRecord>, StorageError> {
            self.count("opportunity_records");
            self.inner.opportunity_records(id).await
        }
        async fn opportunity_records_many(
            &self,
            ids: &[OpportunityId],
        ) -> Result<HashMap<OpportunityId, Vec<JobRecord>>, StorageError> {
            self.count("opportunity_records_many");
            self.inner.opportunity_records_many(ids).await
        }
        async fn history(&self, id: JobId) -> Result<Vec<JobEvent>, StorageError> {
            self.count("history");
            self.inner.history(id).await
        }
        async fn histories(
            &self,
            ids: &[JobId],
        ) -> Result<HashMap<JobId, Vec<JobEvent>>, StorageError> {
            self.count("histories");
            self.inner.histories(ids).await
        }
        async fn search(&self, query: &JobQuery) -> Result<Vec<JobRecord>, StorageError> {
            self.count("search");
            self.inner.search(query).await
        }
        async fn count(&self, query: &JobQuery) -> Result<u64, StorageError> {
            self.count("count");
            self.inner.count(query).await
        }
    }

    #[async_trait]
    impl VerificationRepository for Counting<'_> {
        async fn save_verification(&self, record: &VerificationRecord) -> Result<(), StorageError> {
            self.count("save_verification");
            self.inner.save_verification(record).await
        }
        async fn verification_history(
            &self,
            job: JobId,
        ) -> Result<Vec<VerificationRecord>, StorageError> {
            self.count("verification_history");
            self.inner.verification_history(job).await
        }
        async fn latest_verification(
            &self,
            job: JobId,
        ) -> Result<Option<VerificationRecord>, StorageError> {
            self.count("latest_verification");
            self.inner.latest_verification(job).await
        }
        async fn latest_successful_verification(
            &self,
            job: JobId,
        ) -> Result<Option<VerificationRecord>, StorageError> {
            self.count("latest_successful_verification");
            self.inner.latest_successful_verification(job).await
        }
        async fn latest_verifications_of(
            &self,
            jobs: &[JobId],
        ) -> Result<HashMap<JobId, LatestVerifications>, StorageError> {
            self.count("latest_verifications_of");
            self.inner.latest_verifications_of(jobs).await
        }
        async fn record_observation(
            &self,
            posting: &JobPosting,
            at: DateTime<Utc>,
        ) -> Result<UpsertOutcome, StorageError> {
            self.count("record_observation");
            self.inner.record_observation(posting, at).await
        }
    }

    #[async_trait]
    impl EligibilityRepository for Counting<'_> {
        async fn cached_decision(
            &self,
            key: &CacheKey,
        ) -> Result<Option<EligibilityDecision>, StorageError> {
            self.count("cached_decision");
            self.inner.cached_decision(key).await
        }
        async fn store_decision(
            &self,
            key: &CacheKey,
            decision: &EligibilityDecision,
            at: DateTime<Utc>,
        ) -> Result<(), StorageError> {
            self.count("store_decision");
            self.inner.store_decision(key, decision, at).await
        }
        async fn cached_decisions(
            &self,
            keys: &[CacheKey],
        ) -> Result<HashMap<String, EligibilityDecision>, StorageError> {
            self.count("cached_decisions");
            self.inner.cached_decisions(keys).await
        }
        async fn store_decisions(
            &self,
            decisions: &[(CacheKey, EligibilityDecision)],
            at: DateTime<Utc>,
        ) -> Result<(), StorageError> {
            self.count("store_decisions");
            self.inner.store_decisions(decisions, at).await
        }
    }

    #[async_trait]
    impl FeedbackRepository for Counting<'_> {
        async fn record_feedback(&self, event: &FeedbackEvent) -> Result<(), StorageError> {
            self.count("record_feedback");
            self.inner.record_feedback(event).await
        }
        async fn feedback(&self, profile_id: &str) -> Result<Vec<FeedbackEvent>, StorageError> {
            self.count("feedback");
            self.inner.feedback(profile_id).await
        }
        async fn feedback_for_jobs(
            &self,
            profile_id: &str,
            jobs: &[JobId],
        ) -> Result<Vec<FeedbackEvent>, StorageError> {
            self.count("feedback_for_jobs");
            self.inner.feedback_for_jobs(profile_id, jobs).await
        }
    }

    #[async_trait]
    impl RankingRepository for Counting<'_> {
        async fn cached_ranking(&self, key: &RankKey) -> Result<Option<Ranking>, StorageError> {
            self.count("cached_ranking");
            self.inner.cached_ranking(key).await
        }
        async fn store_ranking(
            &self,
            key: &RankKey,
            ranking: &Ranking,
            at: DateTime<Utc>,
        ) -> Result<(), StorageError> {
            self.count("store_ranking");
            self.inner.store_ranking(key, ranking, at).await
        }
        async fn cached_fit_review(
            &self,
            profile_id: &str,
            key: &str,
        ) -> Result<Option<jobhunt_ranking::FitReview>, StorageError> {
            self.count("cached_fit_review");
            self.inner.cached_fit_review(profile_id, key).await
        }
        async fn store_fit_review(
            &self,
            profile_id: &str,
            key: &str,
            review: &jobhunt_ranking::FitReview,
            at: DateTime<Utc>,
        ) -> Result<(), StorageError> {
            self.count("store_fit_review");
            self.inner
                .store_fit_review(profile_id, key, review, at)
                .await
        }
    }

    #[async_trait]
    impl ProfileRepository for Counting<'_> {
        async fn load_profile(
            &self,
            id: ProfileId,
        ) -> Result<Option<ProfileData>, jobhunt_profile::StorageError> {
            self.count("load_profile");
            self.inner.load_profile(id).await
        }
        async fn save_profile(
            &self,
            data: &ProfileData,
            expected_revision: u64,
            events: &[ProfileEvent],
        ) -> Result<(), jobhunt_profile::StorageError> {
            self.count("save_profile");
            self.inner
                .save_profile(data, expected_revision, events)
                .await
        }
        async fn find_claims(
            &self,
            profile: ProfileId,
            query: &ClaimQuery,
        ) -> Result<Vec<Claim>, jobhunt_profile::StorageError> {
            self.count("find_claims");
            self.inner.find_claims(profile, query).await
        }
        async fn profile_events(
            &self,
            profile: ProfileId,
            limit: usize,
        ) -> Result<Vec<ProfileEvent>, jobhunt_profile::StorageError> {
            self.count("profile_events");
            self.inner.profile_events(profile, limit).await
        }
    }

    /// The repository calls one ranking of `jobs` open postings makes, for
    /// someone who has looked at every one, saved every third, and whose
    /// jobs all have history (a second scan changed their pay).
    async fn calls_ranking(jobs: usize) -> BTreeMap<&'static str, usize> {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        ProfileService::new(&store)
            .set_preference(
                PreferenceValue::CurrentLocation {
                    place: "Berlin, Germany".into(),
                },
                Stance::Required,
                now() - Duration::days(1),
            )
            .await
            .unwrap();
        let postings: Vec<JobPosting> = (0..jobs)
            .map(|i| {
                posting(
                    &i.to_string(),
                    "Backend Engineer",
                    "Remote - Worldwide",
                    None,
                )
            })
            .collect();
        discover(&store, &postings).await;
        let changed: Vec<JobPosting> = (0..jobs)
            .map(|i| {
                posting(
                    &i.to_string(),
                    "Backend Engineer",
                    "Remote - Worldwide",
                    Some((150_000.0, 180_000.0)),
                )
            })
            .collect();
        discover(&store, &changed).await;
        let service = RankingService::new(&store, &RuleReader);
        for (i, p) in postings.iter().enumerate() {
            let r = get(&store, p).await;
            let action = if i % 3 == 0 {
                FeedbackAction::Save
            } else {
                FeedbackAction::Seen
            };
            service
                .record(std::slice::from_ref(&r), action, None, now())
                .await
                .unwrap();
            assert!(!store.history(r.id).await.unwrap().is_empty());
        }
        let counting = Counting::new(&store);
        let ranking = RankingService::new(&counting, &RuleReader);
        let report = ranking.rank(&RankQuery::default(), now()).await.unwrap();
        assert_eq!(report.considered, jobs);
        let pipeline = ranking.pipeline(false).await.unwrap();
        assert_eq!(pipeline.len(), jobs.div_ceil(3));
        counting.calls()
    }

    #[tokio::test]
    async fn ranking_makes_the_same_calls_for_3_jobs_as_for_30() {
        let few = calls_ranking(3).await;
        let many = calls_ranking(30).await;
        assert_eq!(few, many, "per-job repository calls crept back in");
        for per_job in [
            "opportunity_records",
            "latest_verification",
            "latest_successful_verification",
            "cached_decision",
            "get",
            "history",
        ] {
            assert!(!many.contains_key(per_job), "{per_job} called: {many:?}");
        }
        assert!(many.contains_key("get_many"), "feedback is read: {many:?}");
    }
}
