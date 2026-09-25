//! Stored eligibility decisions, keyed by everything that produced them.
//!
//! A decision depends on the person (profile id and revision), the job
//! (every source record's material content and lifecycle status), the
//! verification each record rests on, and the rules
//! ([`RULES_VERSION`]). The cache key is a digest of all of them, so a
//! changed preference, an UPDATED job, a new verification or a rule change
//! each produce a new key and a stored decision can never outlive what it
//! was computed from. Old decisions are kept as an audit trail.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::StableId;
use jobhunt_jobs::verification::{FreshnessPolicy, RecordVerification, VERIFICATION_REVISION};
use jobhunt_jobs::{OpportunityId, StorageError};

use crate::decision::{EligibilityDecision, RULES_VERSION};
use crate::evaluate::{Assessment, assess, trust};
use crate::profile::ProfileFacts;

const CACHE_NAMESPACE: &str = "jobhunt.eligibility.v1";

/// The identity of a decision's inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    pub opportunity: OpportunityId,
    pub profile_id: String,
    pub profile_revision: u64,
    /// Digest of every input, `elig_<32 hex>`.
    pub key: String,
}

impl CacheKey {
    pub fn of(verified: &[RecordVerification], profile: &ProfileFacts) -> Option<Self> {
        let profile_id = profile.profile_id.clone()?;
        let first = verified.first()?;
        let revision = profile.revision.to_string();
        let mut parts: Vec<String> = vec![
            RULES_VERSION.to_owned(),
            VERIFICATION_REVISION.to_owned(),
            profile_id.clone(),
            revision,
        ];
        let mut records: Vec<String> = verified
            .iter()
            .map(|v| {
                format!(
                    "{}|{}|{}|{}",
                    v.record.id,
                    v.record.posting.content_fingerprint().to_hex(),
                    v.record.status.as_str(),
                    v.latest
                        .as_ref()
                        .map(|l| l.id.to_string())
                        .unwrap_or_else(|| "unverified".into()),
                )
            })
            .collect();
        records.sort();
        parts.extend(records);
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        Some(Self {
            opportunity: first.record.opportunity_id,
            profile_id,
            profile_revision: profile.revision,
            key: format!("elig_{}", StableId::derive(CACHE_NAMESPACE, &refs)),
        })
    }
}

/// Storage for decisions.
#[async_trait]
pub trait EligibilityRepository: Send + Sync {
    /// The decision stored under exactly this key, if any.
    async fn cached_decision(
        &self,
        key: &CacheKey,
    ) -> Result<Option<EligibilityDecision>, StorageError>;

    /// Stores a decision under its key (replacing one stored under the same
    /// key; other keys are kept).
    async fn store_decision(
        &self,
        key: &CacheKey,
        decision: &EligibilityDecision,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError>;

    /// The decisions stored under these keys, by [`CacheKey::key`] (keys
    /// without one are absent). Backends where a lookup is a network round
    /// trip answer in one query; the default asks one key at a time.
    async fn cached_decisions(
        &self,
        keys: &[CacheKey],
    ) -> Result<HashMap<String, EligibilityDecision>, StorageError> {
        let mut out = HashMap::new();
        for key in keys {
            if let Some(decision) = self.cached_decision(key).await? {
                out.insert(key.key.clone(), decision);
            }
        }
        Ok(out)
    }

    /// Stores several decisions (as [`store_decision`](Self::store_decision)
    /// does each). The default stores one at a time.
    async fn store_decisions(
        &self,
        decisions: &[(CacheKey, EligibilityDecision)],
        at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        for (key, decision) in decisions {
            self.store_decision(key, decision, at).await?;
        }
        Ok(())
    }
}

/// An assessment, with its decision taken from the store when the inputs
/// are unchanged (and stored when computed). Trust is always recomputed:
/// it depends on the clock.
pub async fn cached_assess<R: EligibilityRepository + ?Sized>(
    repository: &R,
    verified: &[RecordVerification],
    profile: &ProfileFacts,
    policy: &FreshnessPolicy,
    now: DateTime<Utc>,
) -> Result<(Assessment, bool), StorageError> {
    let Some(key) = CacheKey::of(verified, profile) else {
        return Ok((assess(verified, profile, policy, now), false));
    };
    if let Some(stored) = repository.cached_decision(&key).await? {
        let assessment = Assessment {
            trust: trust(verified, policy, now),
            decision: stored,
        };
        return Ok((assessment, true));
    }
    let assessment = assess(verified, profile, policy, now);
    repository
        .store_decision(&key, &assessment.decision, now)
        .await?;
    Ok((assessment, false))
}

/// [`cached_assess`] for many opportunities at once: one lookup for every
/// stored decision and one write for the new ones, so a search over the
/// whole corpus costs two queries instead of two per opportunity. The
/// assessments come back in the order of `candidates`.
pub async fn cached_assess_many<R: EligibilityRepository + ?Sized>(
    repository: &R,
    candidates: &[Vec<RecordVerification>],
    profile: &ProfileFacts,
    policy: &FreshnessPolicy,
    now: DateTime<Utc>,
) -> Result<Vec<Assessment>, StorageError> {
    let keys: Vec<Option<CacheKey>> = candidates
        .iter()
        .map(|verified| CacheKey::of(verified, profile))
        .collect();
    let lookup: Vec<CacheKey> = keys.iter().flatten().cloned().collect();
    let stored = if lookup.is_empty() {
        HashMap::new()
    } else {
        repository.cached_decisions(&lookup).await?
    };
    let mut out = Vec::with_capacity(candidates.len());
    let mut fresh: Vec<(CacheKey, EligibilityDecision)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (verified, key) in candidates.iter().zip(keys) {
        match key {
            Some(key) => match stored.get(&key.key) {
                Some(decision) => out.push(Assessment {
                    trust: trust(verified, policy, now),
                    decision: decision.clone(),
                }),
                None => {
                    let assessment = assess(verified, profile, policy, now);
                    if seen.insert(key.key.clone()) {
                        fresh.push((key, assessment.decision.clone()));
                    }
                    out.push(assessment);
                }
            },
            None => out.push(assess(verified, profile, policy, now)),
        }
    }
    if !fresh.is_empty() {
        repository.store_decisions(&fresh, now).await?;
    }
    Ok(out)
}
