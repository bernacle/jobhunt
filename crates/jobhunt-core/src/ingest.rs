//! Bookkeeping for ingesting source batches into storage.

use serde::Serialize;

/// What persisting a single record did (its lifecycle classification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpsertOutcome {
    /// NEW: the record was never stored before.
    Inserted,
    /// UPDATED: the record was open and its material content changed.
    Updated,
    /// UNCHANGED: the record was open with the same material content; only
    /// observation metadata ("last seen") moved.
    Unchanged,
    /// The record had been closed and is listed again. Its content may or
    /// may not have changed meanwhile.
    Reopened,
}

/// Counters describing one ingest of a source batch.
///
/// `received = normalized + rejected + skipped`, and every normalized record
/// ends up as exactly one of inserted/updated/unchanged/reopened/duplicate.
/// `closed` counts previously open records that this ingest found missing
/// from a complete listing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IngestCounts {
    /// Records the source returned.
    pub received: usize,
    /// Records that became valid canonical records.
    pub normalized: usize,
    /// Records that could not be converted (malformed, missing required data).
    pub rejected: usize,
    /// Records intentionally ignored by the source (for example unlisted).
    pub skipped: usize,
    /// Records repeated within the same batch (same identity).
    pub duplicates: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub reopened: usize,
    pub closed: usize,
}

impl IngestCounts {
    pub fn record(&mut self, outcome: UpsertOutcome) {
        match outcome {
            UpsertOutcome::Inserted => self.inserted += 1,
            UpsertOutcome::Updated => self.updated += 1,
            UpsertOutcome::Unchanged => self.unchanged += 1,
            UpsertOutcome::Reopened => self.reopened += 1,
        }
    }

    pub fn merge(&mut self, other: &IngestCounts) {
        self.received += other.received;
        self.normalized += other.normalized;
        self.rejected += other.rejected;
        self.skipped += other.skipped;
        self.duplicates += other.duplicates;
        self.inserted += other.inserted;
        self.updated += other.updated;
        self.unchanged += other.unchanged;
        self.reopened += other.reopened;
        self.closed += other.closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_merges() {
        let mut a = IngestCounts {
            received: 2,
            normalized: 2,
            ..Default::default()
        };
        a.record(UpsertOutcome::Inserted);
        a.record(UpsertOutcome::Unchanged);

        let mut b = IngestCounts {
            received: 1,
            rejected: 1,
            ..Default::default()
        };
        b.record(UpsertOutcome::Updated);
        b.record(UpsertOutcome::Reopened);
        b.closed = 2;

        a.merge(&b);
        assert_eq!(a.received, 3);
        assert_eq!(a.normalized, 2);
        assert_eq!(a.rejected, 1);
        assert_eq!((a.inserted, a.updated, a.unchanged), (1, 1, 1));
        assert_eq!((a.reopened, a.closed), (1, 2));
    }
}
