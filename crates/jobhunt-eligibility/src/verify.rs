//! Whether a job's facts are first-party and current.
//!
//! JobHunt reads jobs from where companies publish them: their own Ashby,
//! Greenhouse or Lever boards, and the listings they post themselves on Y
//! Combinator's Work at a Startup. A job is *first-party* when at least one
//! of its records comes from such a source, so its location, remote and pay
//! fields are the company's own words rather than a reposting. *Freshness*
//! is how recently JobHunt saw the job still listed there; a closed record
//! was missing from a complete listing of its source.

use std::fmt;

use chrono::{DateTime, Duration, Utc};
use jobhunt_core::{CanonicalUrl, SourceKey};
use jobhunt_jobs::{JobRecord, JobStatus};

/// How recently a job was seen listed at its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Freshness {
    /// Missing from its source's last complete listing.
    Closed,
    /// Not seen for more than a week.
    Stale,
    /// Seen within the last week.
    Aging,
    /// Seen within the last two days.
    Fresh,
}

impl Freshness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Stale => "stale",
            Self::Aging => "aging",
            Self::Fresh => "fresh",
        }
    }

    pub fn of(status: JobStatus, last_seen_at: DateTime<Utc>, now: DateTime<Utc>) -> Self {
        let age = now - last_seen_at;
        match status {
            JobStatus::Closed => Self::Closed,
            JobStatus::Open if age <= Duration::hours(48) => Self::Fresh,
            JobStatus::Open if age <= Duration::days(7) => Self::Aging,
            JobStatus::Open => Self::Stale,
        }
    }
}

impl fmt::Display for Freshness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Who publishes a source's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Publisher {
    /// "the company's own Ashby job board".
    pub description: &'static str,
    /// The company itself writes the records.
    pub first_party: bool,
}

/// The publisher of a source kind.
pub fn publisher(kind: &str) -> Publisher {
    let (description, first_party) = match kind {
        "ashby" => ("the company's own Ashby job board", true),
        "greenhouse" => ("the company's own Greenhouse job board", true),
        "lever" => ("the company's own Lever job site", true),
        "yc" => (
            "the company's own listing on Y Combinator's Work at a Startup",
            true,
        ),
        _ => ("a source JobHunt does not know as first-party", false),
    };
    Publisher {
        description,
        first_party,
    }
}

/// One record of the job, as verified.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCheck {
    pub source: SourceKey,
    pub publisher: Publisher,
    pub url: CanonicalUrl,
    pub status: JobStatus,
    pub last_seen_at: DateTime<Utc>,
    pub freshness: Freshness,
}

/// Every record of a job, verified.
#[derive(Debug, Clone, PartialEq)]
pub struct Verification {
    /// Freshest first.
    pub sources: Vec<SourceCheck>,
    pub now: DateTime<Utc>,
}

impl Verification {
    /// Whether any record is the company's own.
    pub fn first_party(&self) -> bool {
        self.sources.iter().any(|s| s.publisher.first_party)
    }

    /// The best freshness across records.
    pub fn freshness(&self) -> Freshness {
        self.sources
            .iter()
            .map(|s| s.freshness)
            .max()
            .unwrap_or(Freshness::Stale)
    }

    /// One line: "First-party: the company's own Ashby job board, seen 2
    /// hours ago".
    pub fn summary(&self) -> String {
        let Some(best) = self.sources.first() else {
            return "No source records".into();
        };
        let party = if best.publisher.first_party {
            "First-party"
        } else {
            "Not first-party"
        };
        let when = ago(self.now - best.last_seen_at);
        match best.freshness {
            Freshness::Closed => format!(
                "{party}: {}; no longer listed (last seen {when})",
                best.publisher.description
            ),
            Freshness::Stale => format!(
                "{party}: {}; last seen {when}, may be gone",
                best.publisher.description
            ),
            _ => format!("{party}: {}, seen {when}", best.publisher.description),
        }
    }
}

/// Verifies a job's records (every record of one opportunity, or one).
pub fn verify<'a>(
    records: impl IntoIterator<Item = &'a JobRecord>,
    now: DateTime<Utc>,
) -> Verification {
    let mut sources: Vec<SourceCheck> = records
        .into_iter()
        .map(|r| SourceCheck {
            source: r.posting.provenance.source.clone(),
            publisher: publisher(r.posting.provenance.source.kind()),
            url: r.posting.url.clone(),
            status: r.status,
            last_seen_at: r.last_seen_at,
            freshness: Freshness::of(r.status, r.last_seen_at, now),
        })
        .collect();
    sources.sort_by(|a, b| {
        (b.freshness, b.publisher.first_party, b.last_seen_at).cmp(&(
            a.freshness,
            a.publisher.first_party,
            a.last_seen_at,
        ))
    });
    Verification { sources, now }
}

/// "just now", "5 minutes ago", "3 hours ago", "12 days ago".
pub fn ago(age: Duration) -> String {
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    if age < Duration::minutes(1) {
        "just now".into()
    } else if age < Duration::hours(1) {
        plural(age.num_minutes(), "minute")
    } else if age < Duration::days(2) {
        plural(age.num_hours(), "hour")
    } else {
        plural(age.num_days(), "day")
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn freshness_and_age() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        let open = |hours| Freshness::of(JobStatus::Open, now - Duration::hours(hours), now);
        assert_eq!(open(3), Freshness::Fresh);
        assert_eq!(open(72), Freshness::Aging);
        assert_eq!(open(24 * 9), Freshness::Stale);
        assert_eq!(
            Freshness::of(JobStatus::Closed, now, now),
            Freshness::Closed
        );
        assert_eq!(ago(Duration::minutes(90)), "1 hour ago");
        assert_eq!(ago(Duration::days(12)), "12 days ago");
        assert!(publisher("greenhouse").first_party);
        assert!(!publisher("somewhere").first_party);
    }
}
