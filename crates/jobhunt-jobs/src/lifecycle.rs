//! Lifecycle classification of one source scan.
//!
//! [`plan_scan`] compares a source's current postings with what is stored
//! for that source and decides, for each posting, what happened to it, and
//! which stored jobs are gone. It is pure (no I/O), so every storage backend
//! applies exactly the same rules:
//!
//! | stored state                   | in this scan                  | result     |
//! |--------------------------------|-------------------------------|------------|
//! | not stored                     | listed                        | NEW        |
//! | open, same material content    | listed                        | UNCHANGED  |
//! | open, material content changed | listed                        | UPDATED    |
//! | closed                         | listed                        | REOPENED   |
//! | open                           | missing from a complete scan  | CLOSED     |
//! | open                           | missing from a partial scan   | stays open |
//!
//! "Material content" is [`JobPosting::content_fingerprint`]. A posting whose
//! stored row differs only in non-material ways (description markup, the
//! source's update timestamp) is UNCHANGED but its row is refreshed.

use std::collections::{HashMap, HashSet};

use jobhunt_core::UpsertOutcome;

use crate::model::{JobId, JobPosting, JobStatus};

/// What a backend has stored about one job, as needed for planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJob {
    pub status: JobStatus,
    /// Hex [`JobPosting::fingerprint`] of the stored row.
    pub fingerprint: String,
    /// Hex [`JobPosting::content_fingerprint`], or `None` for rows written
    /// before material fingerprints existed.
    pub content_fingerprint: Option<String>,
}

/// What to do with one posting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// NEW: insert the row and record a `new` event.
    Insert,
    /// UNCHANGED and byte-for-byte identical: only mark it seen.
    Touch,
    /// UNCHANGED materially, but the stored row is out of date (non-material
    /// edit, or a legacy row being baselined): rewrite it without an event.
    Refresh,
    /// UPDATED: rewrite the row and record an `updated` event.
    Update,
    /// REOPENED: reopen, rewrite the row and record a `reopened` event.
    Reopen {
        /// Whether the material content differs from the closed version.
        content_changed: bool,
    },
}

impl Action {
    pub fn outcome(self) -> UpsertOutcome {
        match self {
            Self::Insert => UpsertOutcome::Inserted,
            Self::Touch | Self::Refresh => UpsertOutcome::Unchanged,
            Self::Update => UpsertOutcome::Updated,
            Self::Reopen { .. } => UpsertOutcome::Reopened,
        }
    }

    /// Whether the stored row's content columns must be rewritten.
    pub fn rewrites(self) -> bool {
        !matches!(self, Self::Touch)
    }
}

/// The plan for one posting, with the fingerprints the backend must store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPosting {
    pub id: JobId,
    pub action: Action,
    pub fingerprint: String,
    pub content_fingerprint: String,
}

/// Whether jobs missing from the scan may be closed.
#[derive(Debug, Clone, Copy)]
pub enum Closing<'a> {
    /// The scan is not a trustworthy complete listing: close nothing.
    Skip,
    /// Close every open job of the source that is neither in the scan nor in
    /// `retain` (records the source listed but that could not be converted).
    CloseMissing { retain: &'a HashSet<JobId> },
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanPlan {
    /// One entry per input posting, in order.
    pub postings: Vec<PlannedPosting>,
    /// Open jobs to close, sorted.
    pub close: Vec<JobId>,
}

/// Plans a scan. `stored` must hold every stored job of the scanned source
/// (open and closed); `postings` must already be free of duplicate ids.
pub fn plan_scan(
    stored: &HashMap<JobId, StoredJob>,
    postings: &[JobPosting],
    closing: Closing<'_>,
) -> ScanPlan {
    let mut plan = ScanPlan {
        postings: Vec::with_capacity(postings.len()),
        close: Vec::new(),
    };
    let mut seen = HashSet::with_capacity(postings.len());

    for posting in postings {
        let id = posting.id();
        seen.insert(id);
        let fingerprint = posting.fingerprint().to_hex();
        let content_fingerprint = posting.content_fingerprint().to_hex();
        let action = match stored.get(&id) {
            None => Action::Insert,
            Some(old) => {
                let material_same = match &old.content_fingerprint {
                    Some(stored_fp) => *stored_fp == content_fingerprint,
                    // A legacy row has no material fingerprint to compare
                    // against: adopt the current content as its baseline.
                    None => true,
                };
                match old.status {
                    JobStatus::Closed => Action::Reopen {
                        content_changed: !material_same,
                    },
                    JobStatus::Open if !material_same => Action::Update,
                    JobStatus::Open
                        if old.fingerprint == fingerprint && old.content_fingerprint.is_some() =>
                    {
                        Action::Touch
                    }
                    JobStatus::Open => Action::Refresh,
                }
            }
        };
        plan.postings.push(PlannedPosting {
            id,
            action,
            fingerprint,
            content_fingerprint,
        });
    }

    if let Closing::CloseMissing { retain } = closing {
        plan.close = stored
            .iter()
            .filter(|(id, job)| {
                job.status == JobStatus::Open && !seen.contains(id) && !retain.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        plan.close.sort();
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::posting;

    fn stored(p: &JobPosting, status: JobStatus) -> (JobId, StoredJob) {
        (
            p.id(),
            StoredJob {
                status,
                fingerprint: p.fingerprint().to_hex(),
                content_fingerprint: Some(p.content_fingerprint().to_hex()),
            },
        )
    }

    fn actions(plan: &ScanPlan) -> Vec<Action> {
        plan.postings.iter().map(|p| p.action).collect()
    }

    #[test]
    fn classifies_new_unchanged_and_updated() {
        let same = posting("test:acme", Some("1"), "Engineer");
        let edited_before = posting("test:acme", Some("2"), "Designer");
        let mut edited = edited_before.clone();
        edited.title = "Senior Designer".into();
        let mut cosmetic = posting("test:acme", Some("3"), "Writer");
        let store: HashMap<_, _> = [
            stored(&same, JobStatus::Open),
            stored(&edited_before, JobStatus::Open),
            stored(&cosmetic, JobStatus::Open),
        ]
        .into();
        cosmetic.description_html = Some("<p>new markup</p>".into());
        let new = posting("test:acme", Some("4"), "Analyst");

        let plan = plan_scan(&store, &[same, edited, cosmetic, new], Closing::Skip);
        assert_eq!(
            actions(&plan),
            vec![
                Action::Touch,
                Action::Update,
                Action::Refresh,
                Action::Insert
            ]
        );
        let outcomes: Vec<_> = plan.postings.iter().map(|p| p.action.outcome()).collect();
        assert_eq!(
            outcomes,
            vec![
                UpsertOutcome::Unchanged,
                UpsertOutcome::Updated,
                UpsertOutcome::Unchanged,
                UpsertOutcome::Inserted
            ]
        );
        assert!(plan.close.is_empty());
    }

    #[test]
    fn closes_missing_jobs_only_for_complete_scans() {
        let kept = posting("test:acme", Some("1"), "Engineer");
        let gone = posting("test:acme", Some("2"), "Designer");
        let already_closed = posting("test:acme", Some("3"), "Writer");
        let store: HashMap<_, _> = [
            stored(&kept, JobStatus::Open),
            stored(&gone, JobStatus::Open),
            stored(&already_closed, JobStatus::Closed),
        ]
        .into();

        let partial = plan_scan(&store, std::slice::from_ref(&kept), Closing::Skip);
        assert!(partial.close.is_empty(), "a partial scan closes nothing");

        let retain = HashSet::new();
        let complete = plan_scan(
            &store,
            std::slice::from_ref(&kept),
            Closing::CloseMissing { retain: &retain },
        );
        assert_eq!(complete.close, vec![gone.id()]);
    }

    #[test]
    fn retained_records_are_not_closed() {
        let unreadable = posting("test:acme", Some("1"), "Engineer");
        let store: HashMap<_, _> = [stored(&unreadable, JobStatus::Open)].into();
        let retain: HashSet<_> = [unreadable.id()].into();
        let plan = plan_scan(&store, &[], Closing::CloseMissing { retain: &retain });
        assert!(plan.close.is_empty());
    }

    #[test]
    fn closed_jobs_reopen() {
        let job = posting("test:acme", Some("1"), "Engineer");
        let store: HashMap<_, _> = [stored(&job, JobStatus::Closed)].into();
        let mut changed = job.clone();
        changed.title = "Staff Engineer".into();

        let same = plan_scan(&store, std::slice::from_ref(&job), Closing::Skip);
        assert_eq!(
            actions(&same),
            vec![Action::Reopen {
                content_changed: false
            }]
        );
        let edited = plan_scan(&store, &[changed], Closing::Skip);
        assert_eq!(
            actions(&edited),
            vec![Action::Reopen {
                content_changed: true
            }]
        );
        assert_eq!(edited.postings[0].action.outcome(), UpsertOutcome::Reopened);
    }

    #[test]
    fn legacy_rows_are_baselined_not_updated() {
        let job = posting("test:acme", Some("1"), "Engineer");
        let store: HashMap<_, _> = [(
            job.id(),
            StoredJob {
                status: JobStatus::Open,
                fingerprint: job.fingerprint().to_hex(),
                content_fingerprint: None,
            },
        )]
        .into();
        let plan = plan_scan(&store, std::slice::from_ref(&job), Closing::Skip);
        assert_eq!(actions(&plan), vec![Action::Refresh]);
    }
}
