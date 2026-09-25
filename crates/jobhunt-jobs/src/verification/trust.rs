//! Freshness policy and the trust view of a job: what the latest
//! verification attempts and the discovery lifecycle together say about
//! whether a listing is live, and whether it is trusted enough to
//! recommend.
//!
//! A failed attempt never erases an earlier success: the view keeps both
//! the latest attempt and the latest successful verification. A job the
//! discovery lifecycle closed after its last successful verification is
//! shown as closed, never as "verified active".

use chrono::{DateTime, Duration, Utc};
use jobhunt_core::SourceKey;
use serde::{Deserialize, Serialize};

use crate::model::{JobId, JobRecord, JobStatus};
use crate::verification::model::{Authority, ListingStatus, VerificationRecord};

/// When a verification is recent enough to rely on, and when to ask the
/// source again. Callers (the CLI, later a server) choose the numbers; the
/// rules that use them live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessPolicy {
    /// An attempt this recent is reused instead of asking the source again
    /// (unless verification is forced), so repeated commands never hammer
    /// a site.
    pub reuse_within: Duration,
    /// A successful verification this recent is fresh.
    pub fresh_for: Duration,
    /// Older than this, a verification is stale and the job is not trusted
    /// enough to recommend until it is verified again.
    pub stale_after: Duration,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            reuse_within: Duration::minutes(15),
            fresh_for: Duration::hours(24),
            stale_after: Duration::hours(72),
        }
    }
}

/// How old a verification is, under a [`FreshnessPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationAge {
    Stale,
    Aging,
    Fresh,
}

impl VerificationAge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::Aging => "aging",
            Self::Fresh => "fresh",
        }
    }
}

/// Whether to ask the source again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reverify {
    /// Never verified.
    Never,
    /// The last successful verification is no longer fresh, or the last
    /// attempt failed.
    Due,
    /// A recent attempt is reused.
    NotYet,
    /// Forced by the caller.
    Forced,
}

impl Reverify {
    pub fn fetch(self) -> bool {
        !matches!(self, Self::NotYet)
    }
}

impl FreshnessPolicy {
    pub fn age(&self, verified_at: DateTime<Utc>, now: DateTime<Utc>) -> VerificationAge {
        let age = now - verified_at;
        if age <= self.fresh_for {
            VerificationAge::Fresh
        } else if age <= self.stale_after {
            VerificationAge::Aging
        } else {
            VerificationAge::Stale
        }
    }

    /// Whether a verification with this latest attempt should ask the
    /// source again.
    pub fn reverify(
        &self,
        latest: Option<&VerificationRecord>,
        force: bool,
        now: DateTime<Utc>,
    ) -> Reverify {
        match latest {
            _ if force => Reverify::Forced,
            None => Reverify::Never,
            Some(v) if now - v.attempted_at <= self.reuse_within => Reverify::NotYet,
            Some(_) => Reverify::Due,
        }
    }
}

/// What JobHunt currently believes about one source record's listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    /// The latest attempt found the listing live at its source.
    VerifiedActive,
    /// The latest attempt found the listing gone.
    VerifiedClosed,
    /// Discovery found it missing from a complete listing after its last
    /// successful verification (or it was never verified).
    ClosedByDiscovery,
    /// The latest attempt could not reach an answer.
    CouldNotVerify,
    /// Never verified; only discovery has seen it.
    NotVerified,
}

impl TrustState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedActive => "verified_active",
            Self::VerifiedClosed => "verified_closed",
            Self::ClosedByDiscovery => "closed_by_discovery",
            Self::CouldNotVerify => "could_not_verify",
            Self::NotVerified => "not_verified",
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::VerifiedClosed | Self::ClosedByDiscovery)
    }
}

/// One source record, seen through its verifications and lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordTrust {
    pub job_id: JobId,
    pub source: SourceKey,
    pub authority: Authority,
    pub state: TrustState,
    pub latest: Option<VerificationRecord>,
    pub last_success: Option<VerificationRecord>,
    /// Age of the last successful verification.
    pub age: Option<VerificationAge>,
    /// When discovery last saw the record listed.
    pub last_seen_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl RecordTrust {
    pub fn of(
        record: &JobRecord,
        latest: Option<&VerificationRecord>,
        last_success: Option<&VerificationRecord>,
        policy: &FreshnessPolicy,
        now: DateTime<Utc>,
    ) -> Self {
        let authority = latest
            .or(last_success)
            .map(|v| v.authority)
            .unwrap_or_else(|| Authority::of_kind(record.posting.provenance.source.kind()));
        let success_at = last_success.map(|v| v.attempted_at);
        let closed_since_success = record.status == JobStatus::Closed
            && match (record.closed_at, success_at) {
                (Some(closed), Some(ok)) => closed >= ok,
                (_, None) => true,
                (None, Some(_)) => false,
            };
        let state = match latest {
            _ if closed_since_success => TrustState::ClosedByDiscovery,
            None => TrustState::NotVerified,
            Some(v) => match v.listing {
                ListingStatus::Active => TrustState::VerifiedActive,
                ListingStatus::Closed => TrustState::VerifiedClosed,
                _ => TrustState::CouldNotVerify,
            },
        };
        Self {
            job_id: record.id,
            source: record.posting.provenance.source.clone(),
            authority,
            state,
            latest: latest.cloned(),
            last_success: last_success.cloned(),
            age: success_at.map(|at| policy.age(at, now)),
            last_seen_at: record.last_seen_at,
            closed_at: record.closed_at,
        }
    }
}

/// Whether an opportunity is trusted enough to recommend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Verified active, recently, at a source that speaks for the employer
    /// or a platform the employer posts to.
    Trusted,
    /// Everything else, with why.
    NotTrusted(String),
}

/// Every source record of an opportunity, with the strongest one chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct OpportunityTrust {
    /// In the order given.
    pub records: Vec<RecordTrust>,
    /// The record the opportunity's status rests on.
    pub best: Option<usize>,
    pub state: TrustState,
    pub standing: Standing,
    /// Disagreements between records, in words (one live, another gone).
    pub conflicts: Vec<String>,
}

impl OpportunityTrust {
    pub fn best(&self) -> Option<&RecordTrust> {
        self.best.map(|i| &self.records[i])
    }

    /// Combines the records: the strongest live record wins (authority,
    /// then the most recent success). Records that disagree stay listed
    /// and are named in `conflicts`.
    pub fn of(records: Vec<RecordTrust>, policy: &FreshnessPolicy, now: DateTime<Utc>) -> Self {
        let score = |r: &RecordTrust| {
            let state = match r.state {
                TrustState::VerifiedActive => 4,
                TrustState::CouldNotVerify => 3,
                TrustState::NotVerified => 2,
                TrustState::VerifiedClosed => 1,
                TrustState::ClosedByDiscovery => 0,
            };
            (
                state,
                r.authority.rank(),
                r.last_success.as_ref().map(|v| v.attempted_at),
                r.last_seen_at,
            )
        };
        let best = records
            .iter()
            .enumerate()
            .max_by(|(ia, a), (ib, b)| score(a).cmp(&score(b)).then(ib.cmp(ia)))
            .map(|(i, _)| i);
        let state = best.map_or(TrustState::NotVerified, |i| records[i].state);

        let mut conflicts = Vec::new();
        if state == TrustState::VerifiedActive {
            for r in records.iter().filter(|r| r.state.is_closed()) {
                conflicts.push(format!(
                    "{} no longer lists the job, but {} does",
                    r.source,
                    best.map(|i| records[i].source.to_string())
                        .unwrap_or_default()
                ));
            }
        }

        let standing = match best.map(|i| &records[i]) {
            None => Standing::NotTrusted("no source records".into()),
            Some(b) => match b.state {
                TrustState::VerifiedActive => {
                    let age = b.age.unwrap_or(VerificationAge::Stale);
                    if b.authority.rank() < Authority::TrustedSource.rank() {
                        Standing::NotTrusted(format!("the listing is on {}", b.authority.label()))
                    } else if age == VerificationAge::Stale {
                        Standing::NotTrusted(format!(
                            "last verified more than {} hours ago",
                            policy.stale_after.num_hours()
                        ))
                    } else {
                        Standing::Trusted
                    }
                }
                TrustState::VerifiedClosed | TrustState::ClosedByDiscovery => {
                    Standing::NotTrusted("the listing is closed".into())
                }
                TrustState::CouldNotVerify => Standing::NotTrusted(match &b.last_success {
                    Some(v) => format!(
                        "the latest verification failed (last successful {})",
                        ago(now - v.attempted_at)
                    ),
                    None => "the listing could not be verified".into(),
                }),
                TrustState::NotVerified => {
                    Standing::NotTrusted("the listing has not been verified".into())
                }
            },
        };
        Self {
            records,
            best,
            state,
            standing,
            conflicts,
        }
    }
}

/// "just now", "18 minutes ago", "3 hours ago", "yesterday", "3 days ago".
pub fn ago(age: Duration) -> String {
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    if age < Duration::minutes(1) {
        match age.num_seconds() {
            s if s < 5 => "just now".into(),
            s => format!("{s} seconds ago"),
        }
    } else if age < Duration::hours(1) {
        plural(age.num_minutes(), "minute")
    } else if age < Duration::hours(24) {
        plural(age.num_hours(), "hour")
    } else if age < Duration::hours(48) {
        "yesterday".into()
    } else {
        plural(age.num_days(), "day")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_ages() {
        assert_eq!(ago(Duration::seconds(2)), "just now");
        assert_eq!(ago(Duration::seconds(12)), "12 seconds ago");
        assert_eq!(ago(Duration::minutes(18)), "18 minutes ago");
        assert_eq!(ago(Duration::minutes(61)), "1 hour ago");
        assert_eq!(ago(Duration::hours(30)), "yesterday");
        assert_eq!(ago(Duration::days(3)), "3 days ago");
    }

    #[test]
    fn policy_ages_and_reuse() {
        let p = FreshnessPolicy::default();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        assert_eq!(p.age(now - Duration::hours(2), now), VerificationAge::Fresh);
        assert_eq!(
            p.age(now - Duration::hours(30), now),
            VerificationAge::Aging
        );
        assert_eq!(p.age(now - Duration::days(5), now), VerificationAge::Stale);
        assert_eq!(p.reverify(None, false, now), Reverify::Never);
        assert!(p.reverify(None, false, now).fetch());
        assert!(p.reverify(None, true, now).fetch());
    }
}
