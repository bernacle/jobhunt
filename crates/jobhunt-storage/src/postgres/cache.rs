//! A request-scoped read cache for ranking.
//!
//! Ranking every open opportunity needs, per opportunity, its records and
//! the latest (and latest successful) verification of each record. The
//! ranking asks for them in batches (`opportunity_records_many`,
//! `latest_verifications_of`: one query each); a [`super::PgUserStore`]
//! lives for one request and keeps what those batches returned, so the
//! single lookups that follow in the same request (classifying the feed,
//! checking what is shown) are answered from memory. Any write through the
//! same store clears the cache; writes by other requests are not seen,
//! exactly as within one transaction snapshot. What is decided from the
//! data does not change.

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
