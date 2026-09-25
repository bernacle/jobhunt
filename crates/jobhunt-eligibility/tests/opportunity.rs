//! Opportunities with several source records, the listing gate, and the
//! decision cache.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use common::{job, now};
use jobhunt_eligibility::decision::{Eligibility, EligibilityDecision, RuleId, Verdict};
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{CacheKey, EligibilityRepository, assess, cached_assess};
use jobhunt_jobs::verification::{
    ApplicationCheck, Authority, CompensationCheck, FreshnessPolicy, ListingStatus, PublishedPlace,
    RecordVerification, Standing, TrustState, VERIFICATION_REVISION, VerificationId,
    VerificationMethod, VerificationRecord,
};
use jobhunt_jobs::{JobRecord, JobStatus, OpportunityId, StorageError, WorkplaceType};

fn verification(
    record: &JobRecord,
    listing: ListingStatus,
    at: DateTime<Utc>,
) -> VerificationRecord {
    VerificationRecord {
        id: VerificationId::derive(record.id, at),
        job_id: record.id,
        opportunity_id: record.opportunity_id,
        source: record.posting.provenance.source.clone(),
        source_record_id: record.posting.provenance.source_record_id.clone(),
        attempted_at: at,
        method: VerificationMethod::AshbyBoardApi,
        listing,
        application: ApplicationCheck::unknown("test"),
        authority: Authority::of_kind(record.posting.provenance.source.kind()),
        authority_chain: Vec::new(),
        checked_url: None,
        listing_url: None,
        content_fingerprint: None,
        changed_fields: Vec::new(),
        changed_since_last_verification: None,
        lifecycle: None,
        compensation: CompensationCheck::not_observed(),
        published: PublishedPlace::default(),
        unknowns: Vec::new(),
        failure: None,
        revision: VERIFICATION_REVISION.to_owned(),
    }
}

/// A record in `opportunity`, verified (or not) at `now() - 1h`.
fn verified(
    mut record: JobRecord,
    opportunity: OpportunityId,
    listing: Option<ListingStatus>,
) -> RecordVerification {
    record.opportunity_id = opportunity;
    let latest = listing.map(|l| verification(&record, l, now() - Duration::hours(1)));
    RecordVerification {
        last_success: latest.clone().filter(VerificationRecord::succeeded),
        latest,
        record,
        reused: true,
    }
}

fn opp() -> OpportunityId {
    "opp_00000000000000000000000000000001".parse().unwrap()
}

fn brazil() -> ProfileFacts {
    let mut p = ProfileFacts::living_in("Recife, Brazil");
    p.profile_id = Some("prof_1".into());
    p.revision = 3;
    p
}

fn policy() -> FreshnessPolicy {
    FreshnessPolicy::default()
}

#[test]
fn a_dead_source_defers_to_the_live_authoritative_one() {
    let yc = job("yc:example", "Remote (US)", Some(WorkplaceType::Remote), "");
    let ashby = job(
        "ashby:example",
        "Remote - Americas",
        Some(WorkplaceType::Remote),
        "",
    );
    let records = [
        verified(yc, opp(), Some(ListingStatus::Closed)),
        verified(ashby, opp(), Some(ListingStatus::Active)),
    ];
    let a = assess(&records, &brazil(), &policy(), now());
    assert_eq!(a.trust.state, TrustState::VerifiedActive);
    assert_eq!(a.trust.best().unwrap().source.to_string(), "ashby:example");
    assert_eq!(a.trust.standing, Standing::Trusted);
    assert_eq!(
        a.decision.status,
        Eligibility::Eligible,
        "the closed record doesn't decide"
    );
    assert_eq!(a.decision.sources, ["ashby:example"]);
    assert_eq!(
        a.trust.records.len(),
        2,
        "the closed record stays inspectable"
    );
    assert!(a.recommendable());
    assert_eq!(a.listing_reason().verdict, Verdict::Pass);
}

#[test]
fn two_live_sources_that_agree() {
    let a1 = job(
        "ashby:example",
        "Remote - LATAM",
        Some(WorkplaceType::Remote),
        "",
    );
    let g1 = job(
        "greenhouse:example",
        "Remote - Americas",
        Some(WorkplaceType::Remote),
        "",
    );
    let records = [
        verified(a1, opp(), Some(ListingStatus::Active)),
        verified(g1, opp(), Some(ListingStatus::Active)),
    ];
    let a = assess(&records, &brazil(), &policy(), now());
    assert_eq!(a.decision.status, Eligibility::Eligible);
    assert_eq!(a.decision.sources.len(), 2, "both provenances visible");
    assert!(
        a.decision
            .reasons
            .iter()
            .any(|r| r.conclusion.starts_with("Also listed on"))
    );
}

#[test]
fn sources_that_contradict_each_other_are_uncertain() {
    let us = job(
        "ashby:example",
        "Remote (US)",
        Some(WorkplaceType::Remote),
        "",
    );
    let americas = job(
        "greenhouse:example",
        "Remote - Americas",
        Some(WorkplaceType::Remote),
        "",
    );
    let records = [
        verified(us, opp(), Some(ListingStatus::Active)),
        verified(americas, opp(), Some(ListingStatus::Active)),
    ];
    let a = assess(&records, &brazil(), &policy(), now());
    assert_eq!(a.decision.status, Eligibility::Uncertain);
    assert!(a.decision.headline.contains("sources disagree"));
    let per_source: Vec<&str> = a
        .decision
        .reasons
        .iter()
        .filter(|r| r.rule == RuleId::Ambiguity && r.verdict == Verdict::NotApplicable)
        .map(|r| r.conclusion.as_str())
        .collect();
    assert_eq!(per_source.len(), 2, "both answers are kept: {per_source:?}");
    assert!(
        a.decision
            .conflicts
            .iter()
            .any(|c| c.summary.starts_with("Sources disagree"))
    );
}

#[test]
fn a_silent_source_defers_to_one_that_answers() {
    let silent = job("yc:example", "Remote", Some(WorkplaceType::Remote), "");
    let stated = job(
        "ashby:example",
        "Remote - Brazil",
        Some(WorkplaceType::Remote),
        "",
    );
    let records = [
        verified(silent, opp(), Some(ListingStatus::Active)),
        verified(stated, opp(), Some(ListingStatus::Active)),
    ];
    let a = assess(&records, &brazil(), &policy(), now());
    assert_eq!(a.decision.status, Eligibility::Eligible);
    assert!(
        a.decision
            .reasons
            .iter()
            .any(|r| r.conclusion.contains("Also listed on yc:example: UNCLEAR"))
    );
}

#[test]
fn verification_and_eligibility_stay_separate() {
    // Verified active, first-party, and ineligible.
    let us = job(
        "ashby:example",
        "Remote (US)",
        Some(WorkplaceType::Remote),
        "",
    );
    let a = assess(
        &[verified(us, opp(), Some(ListingStatus::Active))],
        &brazil(),
        &policy(),
        now(),
    );
    assert_eq!(a.trust.standing, Standing::Trusted);
    assert_eq!(a.decision.status, Eligibility::Ineligible);
    assert!(!a.recommendable());

    // Could not verify, but geographically compatible: not recommended.
    let latam = job(
        "ashby:example",
        "Remote - LATAM",
        Some(WorkplaceType::Remote),
        "",
    );
    let a = assess(
        &[verified(
            latam.clone(),
            opp(),
            Some(ListingStatus::Unreachable),
        )],
        &brazil(),
        &policy(),
        now(),
    );
    assert_eq!(a.decision.status, Eligibility::Eligible);
    assert_eq!(a.trust.state, TrustState::CouldNotVerify);
    assert!(!a.recommendable());
    assert_eq!(a.listing_reason().verdict, Verdict::Unknown);

    // Never verified: the same.
    let a = assess(
        &[verified(latam.clone(), opp(), None)],
        &brazil(),
        &policy(),
        now(),
    );
    assert!(!a.recommendable());

    // A secondary source alone is not trusted.
    let repost = job(
        "jobboard:example",
        "Remote - LATAM",
        Some(WorkplaceType::Remote),
        "",
    );
    let a = assess(
        &[verified(repost, opp(), Some(ListingStatus::Active))],
        &brazil(),
        &policy(),
        now(),
    );
    assert!(matches!(a.trust.standing, Standing::NotTrusted(_)));

    // Closed by discovery after its last verification.
    let mut closed = verified(latam, opp(), Some(ListingStatus::Active));
    closed.record.status = JobStatus::Closed;
    closed.record.closed_at = Some(now());
    let a = assess(&[closed], &brazil(), &policy(), now());
    assert_eq!(a.trust.state, TrustState::ClosedByDiscovery);
    assert_eq!(a.listing_reason().verdict, Verdict::Fail);
}

#[derive(Default)]
struct Memory {
    decisions: Mutex<HashMap<String, EligibilityDecision>>,
    writes: Mutex<usize>,
}

#[async_trait::async_trait]
impl EligibilityRepository for Memory {
    async fn cached_decision(
        &self,
        key: &CacheKey,
    ) -> Result<Option<EligibilityDecision>, StorageError> {
        Ok(self.decisions.lock().unwrap().get(&key.key).cloned())
    }

    async fn store_decision(
        &self,
        key: &CacheKey,
        decision: &EligibilityDecision,
        _at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        *self.writes.lock().unwrap() += 1;
        self.decisions
            .lock()
            .unwrap()
            .insert(key.key.clone(), decision.clone());
        Ok(())
    }
}

#[tokio::test]
async fn cached_decisions_never_outlive_their_inputs() {
    let repo = Memory::default();
    let base = job(
        "ashby:example",
        "Remote - LATAM",
        Some(WorkplaceType::Remote),
        "",
    );
    let records = vec![verified(base.clone(), opp(), Some(ListingStatus::Active))];
    let profile = brazil();

    let (first, hit) = cached_assess(&repo, &records, &profile, &policy(), now())
        .await
        .unwrap();
    assert!(!hit);
    let (second, hit) = cached_assess(&repo, &records, &profile, &policy(), now())
        .await
        .unwrap();
    assert!(hit, "unchanged inputs reuse the stored decision");
    assert_eq!(second.decision, first.decision);
    let key = CacheKey::of(&records, &profile).unwrap();
    assert_eq!(key.profile_revision, 3);
    assert!(key.key.starts_with("elig_"));

    // A profile change (new revision) is a new key.
    let mut moved = profile.clone();
    moved.revision = 4;
    moved.location = ProfileFacts::living_in("Madrid").location;
    assert_ne!(CacheKey::of(&records, &moved).unwrap().key, key.key);
    let (changed, hit) = cached_assess(&repo, &records, &moved, &policy(), now())
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(changed.decision.status, Eligibility::Ineligible);

    // A job change (material content) is a new key.
    let mut updated = base.clone();
    updated.posting.location = Some("Remote (US)".into());
    let records_updated = vec![verified(updated, opp(), Some(ListingStatus::Active))];
    assert_ne!(
        CacheKey::of(&records_updated, &profile).unwrap().key,
        key.key
    );
    let (after, hit) = cached_assess(&repo, &records_updated, &profile, &policy(), now())
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(after.decision.status, Eligibility::Ineligible);

    // A new verification is a new key; a closed status too.
    let mut reverified = records.clone();
    reverified[0].latest = Some(verification(
        &reverified[0].record,
        ListingStatus::Active,
        now(),
    ));
    assert_ne!(CacheKey::of(&reverified, &profile).unwrap().key, key.key);
    let mut closed = records.clone();
    closed[0].record.status = JobStatus::Closed;
    assert_ne!(CacheKey::of(&closed, &profile).unwrap().key, key.key);

    // No stored profile: nothing is cached.
    let anonymous = ProfileFacts::living_in("Recife, Brazil");
    assert!(CacheKey::of(&records, &anonymous).is_none());
    let before = *repo.writes.lock().unwrap();
    let (_, hit) = cached_assess(&repo, &records, &anonymous, &policy(), now())
        .await
        .unwrap();
    assert!(!hit);
    assert_eq!(*repo.writes.lock().unwrap(), before);
}
