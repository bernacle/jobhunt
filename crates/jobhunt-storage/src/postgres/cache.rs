//! A request-scoped read cache for ranking.
//!
//! Ranking every open opportunity asks, per opportunity, for its records and
//! for the latest (and latest successful) verification of each record:
//! cheap against a local SQLite file, thousands of round trips against a
//! networked Postgres. A [`super::PgUserStore`] lives for one request, so
//! when it answers the ranking's "every open opportunity" search it loads
//! those in three batched queries and serves the follow-up lookups from
//! memory. Any write through the same store clears the cache; writes by
//! other requests are not seen, exactly as within one transaction
//! snapshot. What is decided from the data does not change.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use jobhunt_jobs::verification::VerificationRecord;
use jobhunt_jobs::{JobId, JobRecord, OpportunityId};

#[derive(Default)]
pub(crate) struct ReadCache {
    pub records: HashMap<OpportunityId, Vec<JobRecord>>,
    pub latest: HashMap<JobId, Option<VerificationRecord>>,
    pub latest_success: HashMap<JobId, Option<VerificationRecord>>,
}

#[derive(Default)]
pub(crate) struct Cache(Mutex<ReadCache>);

impl Cache {
    pub fn lock(&self) -> MutexGuard<'_, ReadCache> {
        match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn clear(&self) {
        *self.lock() = ReadCache::default();
    }
}
