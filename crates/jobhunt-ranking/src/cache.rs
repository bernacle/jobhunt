//! Stored rankings, keyed by everything that produced them, and the
//! storage boundaries of this crate.
//!
//! A ranking depends on the person (profile id and revision), their
//! feedback (the taste digest, which covers every event and the reader and
//! taste revisions), their state on the opportunity, every source record's
//! content and status, the verification and eligibility answers, the rules
//! ([`RANKING_VERSION`]) and the day (freshness is counted in days). The
//! key is a digest of all of them, so a stored ranking never outlives its
//! inputs; older rows stay as a record of what was shown and why.
//!
//! Only rankings someone looked at are stored (the ones `rank` prints and
//! the one `why` explains), so the table grows with use, not with the
//! size of the job database.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::StableId;
use jobhunt_jobs::{JobId, OpportunityId, StorageError};

use crate::feedback::FeedbackEvent;
use crate::rank::{Candidate, Context, RANKING_VERSION, Ranking, rank};

const CACHE_NAMESPACE: &str = "jobhunt.ranking.v1";

/// The identity of a ranking's inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankKey {
    pub opportunity: OpportunityId,
    pub profile_id: String,
    /// `rank_<32 hex>`.
    pub key: String,
}

impl RankKey {
    pub fn of(candidate: &Candidate<'_>, ctx: &Context<'_>) -> Option<Self> {
        let first = candidate.records.first()?;
        let a = candidate.assessment;
        let mut parts: Vec<String> = vec![
            RANKING_VERSION.to_owned(),
            ctx.taste.digest.clone(),
            ctx.person.profile_id.clone(),
            ctx.person.revision.to_string(),
            ctx.now.format("%Y-%m-%d").to_string(),
            format!(
                "{}|{}|{}|{}|{}",
                a.decision.status,
                a.decision.rules_version,
                a.decision.headline,
                a.trust.state.as_str(),
                match &a.trust.standing {
                    jobhunt_jobs::verification::Standing::Trusted => "trusted".to_owned(),
                    jobhunt_jobs::verification::Standing::NotTrusted(why) => why.clone(),
                }
            ),
        ];
        let mut records: Vec<String> = candidate
            .records
            .iter()
            .zip(&a.trust.records)
            .map(|(r, t)| {
                format!(
                    "{}|{}|{}|{}|{}",
                    r.id,
                    r.posting.content_fingerprint().to_hex(),
                    r.status.as_str(),
                    t.latest
                        .as_ref()
                        .map(|v| v.id.to_string())
                        .unwrap_or_default(),
                    t.age.map(|a| a.as_str()).unwrap_or_default(),
                )
            })
            .collect();
        records.sort();
        parts.extend(records);
        parts.extend(candidate.state.events.iter().map(|e| e.id.to_string()));
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        Some(Self {
            opportunity: first.opportunity_id,
            profile_id: ctx.person.profile_id.clone(),
            key: format!("rank_{}", StableId::derive(CACHE_NAMESPACE, &refs)),
        })
    }
}

/// Storage for rankings.
#[async_trait]
pub trait RankingRepository: Send + Sync {
    /// The ranking stored under exactly this key, if any.
    async fn cached_ranking(&self, key: &RankKey) -> Result<Option<Ranking>, StorageError>;

    /// Stores a ranking under its key (replacing one under the same key).
    async fn store_ranking(
        &self,
        key: &RankKey,
        ranking: &Ranking,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError>;
}

/// Storage for feedback. Events are only ever added.
#[async_trait]
pub trait FeedbackRepository: Send + Sync {
    async fn record_feedback(&self, event: &FeedbackEvent) -> Result<(), StorageError>;

    /// Every event of a profile, oldest first.
    async fn feedback(&self, profile_id: &str) -> Result<Vec<FeedbackEvent>, StorageError>;

    /// Events of a profile about any of these source records, oldest
    /// first.
    async fn feedback_for_jobs(
        &self,
        profile_id: &str,
        jobs: &[JobId],
    ) -> Result<Vec<FeedbackEvent>, StorageError>;
}

/// A ranking, from the store when its inputs are unchanged. With `store`,
/// a newly computed ranking is saved.
pub async fn cached_rank<R: RankingRepository + ?Sized>(
    repository: &R,
    candidate: &Candidate<'_>,
    ctx: &Context<'_>,
    store: bool,
) -> Result<Option<(Ranking, bool)>, StorageError> {
    let Some(key) = RankKey::of(candidate, ctx) else {
        return Ok(rank(candidate, ctx).map(|r| (r, false)));
    };
    if let Some(stored) = repository.cached_ranking(&key).await? {
        return Ok(Some((stored, true)));
    }
    let Some(ranking) = rank(candidate, ctx) else {
        return Ok(None);
    };
    if store {
        repository.store_ranking(&key, &ranking, ctx.now).await?;
    }
    Ok(Some((ranking, false)))
}
