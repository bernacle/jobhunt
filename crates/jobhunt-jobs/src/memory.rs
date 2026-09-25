//! In-memory [`JobRepository`] for pipeline tests. It applies the same
//! [`plan_scan`] rules as real backends, so tests exercise the actual
//! lifecycle logic.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::SourceKey;

use crate::identity::{IdentityEntry, evidence_keys};
use crate::lifecycle::{Action, Closing, StoredJob, plan_scan};
use crate::model::JobPosting;
use crate::model::{CANONICAL_REVISION, JobId, JobRecord, JobStatus, OpportunityId};
use crate::repository::{
    JobEvent, JobEventKind, JobQuery, JobRepository, LastListing, RunId, RunSummary, ScanBody,
    ScanResult, ScanWrite, StorageError,
};
use crate::verification::{VerificationRecord, VerificationRepository};

#[derive(Debug, Clone)]
pub struct ScanRow {
    pub source: SourceKey,
    pub status: &'static str,
    pub finished_at: DateTime<Utc>,
    pub complete: bool,
    pub received: usize,
    pub validator: Option<String>,
}

#[derive(Default)]
struct State {
    runs: i64,
    jobs: HashMap<JobId, (StoredJob, JobRecord)>,
    history: HashMap<JobId, Vec<JobEvent>>,
    scans: Vec<ScanRow>,
    verifications: Vec<VerificationRecord>,
}

#[derive(Default)]
pub struct MemoryRepository {
    state: Mutex<State>,
}

impl MemoryRepository {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn scans(&self) -> Vec<ScanRow> {
        self.lock().scans.clone()
    }
}

#[async_trait]
impl JobRepository for MemoryRepository {
    async fn begin_run(&self, _started_at: DateTime<Utc>) -> Result<RunId, StorageError> {
        let mut state = self.lock();
        state.runs += 1;
        Ok(RunId(state.runs))
    }

    async fn finish_run(&self, _run: RunId, _summary: &RunSummary) -> Result<(), StorageError> {
        Ok(())
    }

    async fn last_listing(&self, source: &SourceKey) -> Result<Option<LastListing>, StorageError> {
        Ok(self
            .lock()
            .scans
            .iter()
            .rev()
            .find(|s| s.source == *source && s.status == "listing")
            .map(|s| LastListing {
                finished_at: s.finished_at,
                complete: s.complete,
                received: s.received,
                validator: s.validator.clone(),
                revision: CANONICAL_REVISION.to_owned(),
            }))
    }

    async fn apply_scan(&self, scan: &ScanWrite<'_>) -> Result<ScanResult, StorageError> {
        let mut state = self.lock();
        let at = scan.observed_at;
        let event = |kind, changed_fields: Vec<String>, previous| JobEvent {
            kind,
            at,
            run: Some(scan.run),
            changed_fields,
            previous,
        };
        let mut result = ScanResult::default();
        let (status, complete, validator) = match &scan.body {
            ScanBody::Failed { .. } => ("failed", false, None),
            ScanBody::NotModified => {
                for (stored, record) in state.jobs.values_mut() {
                    if record.posting.provenance.source == *scan.source
                        && stored.status == JobStatus::Open
                    {
                        record.last_seen_at = at;
                        result.touched += 1;
                    }
                }
                ("not_modified", true, None)
            }
            ScanBody::Listing {
                postings,
                complete,
                close_missing,
                retain,
                validator,
                ..
            } => {
                let stored: HashMap<JobId, StoredJob> = state
                    .jobs
                    .iter()
                    .filter(|(_, (_, r))| r.posting.provenance.source == *scan.source)
                    .map(|(id, (s, _))| (*id, s.clone()))
                    .collect();
                let retain: HashSet<JobId> = retain.iter().copied().collect();
                let closing = if *close_missing {
                    Closing::CloseMissing { retain: &retain }
                } else {
                    Closing::Skip
                };
                let plan = plan_scan(&stored, postings, closing);
                for (posting, planned) in postings.iter().zip(&plan.postings) {
                    let new_stored = StoredJob {
                        status: JobStatus::Open,
                        fingerprint: planned.fingerprint.clone(),
                        content_fingerprint: Some(planned.content_fingerprint.clone()),
                    };
                    let previous = state.jobs.get(&planned.id).map(|(_, r)| r.clone());
                    let diff = |old: &JobRecord| {
                        let before = old.posting.snapshot();
                        let fields = before
                            .changed_fields(&posting.snapshot())
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<Vec<_>>();
                        (fields, Some(before))
                    };
                    let new_event = match (planned.action, &previous) {
                        (Action::Insert, _) => Some(event(JobEventKind::New, vec![], None)),
                        (Action::Update, Some(old)) => {
                            let (fields, before) = diff(old);
                            Some(event(JobEventKind::Updated, fields, before))
                        }
                        (Action::Reopen { content_changed }, Some(old)) => {
                            let (fields, before) = if content_changed {
                                diff(old)
                            } else {
                                (vec![], None)
                            };
                            Some(event(JobEventKind::Reopened, fields, before))
                        }
                        _ => None,
                    };
                    let record = match previous {
                        None => JobRecord {
                            id: planned.id,
                            posting: posting.clone(),
                            first_seen_at: at,
                            last_seen_at: at,
                            content_updated_at: at,
                            status: JobStatus::Open,
                            closed_at: None,
                            opportunity_id: OpportunityId::founded_by(planned.id),
                        },
                        Some(mut old) => {
                            old.last_seen_at = at;
                            old.status = JobStatus::Open;
                            old.closed_at = None;
                            if planned.action.rewrites() {
                                if matches!(
                                    planned.action,
                                    Action::Update
                                        | Action::Reopen {
                                            content_changed: true
                                        }
                                ) {
                                    old.content_updated_at = at;
                                }
                                old.posting = posting.clone();
                            }
                            old
                        }
                    };
                    state.jobs.insert(planned.id, (new_stored, record));
                    if let Some(e) = new_event {
                        state.history.entry(planned.id).or_default().push(e);
                    }
                    result.outcomes.push(planned.action.outcome());
                }
                for id in &plan.close {
                    if let Some((stored, record)) = state.jobs.get_mut(id) {
                        stored.status = JobStatus::Closed;
                        record.status = JobStatus::Closed;
                        record.closed_at = Some(at);
                    }
                    state.history.entry(*id).or_default().push(event(
                        JobEventKind::Closed,
                        vec![],
                        None,
                    ));
                }
                result.closed = plan.close;
                ("listing", *complete, validator.map(str::to_owned))
            }
        };
        state.scans.push(ScanRow {
            source: scan.source.clone(),
            status,
            finished_at: at,
            complete,
            received: scan.counts.received,
            validator,
        });
        Ok(result)
    }

    async fn identity_index(&self) -> Result<Vec<IdentityEntry>, StorageError> {
        Ok(self
            .lock()
            .jobs
            .values()
            .map(|(_, r)| IdentityEntry {
                job: r.id,
                source: r.posting.provenance.source.clone(),
                company: r.posting.company.clone(),
                title: r.posting.title.clone(),
                first_seen_at: r.first_seen_at,
                opportunity: r.opportunity_id,
                evidence: evidence_keys(&r.posting),
            })
            .collect())
    }

    async fn assign_opportunities(
        &self,
        assignments: &[(JobId, OpportunityId)],
    ) -> Result<(), StorageError> {
        let mut state = self.lock();
        for (job, opportunity) in assignments {
            if let Some((_, record)) = state.jobs.get_mut(job) {
                record.opportunity_id = *opportunity;
            }
        }
        Ok(())
    }

    async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError> {
        Ok(self.lock().jobs.get(&id).map(|(_, r)| r.clone()))
    }

    async fn opportunity_records(&self, id: OpportunityId) -> Result<Vec<JobRecord>, StorageError> {
        let mut records: Vec<JobRecord> = self
            .lock()
            .jobs
            .values()
            .filter(|(_, r)| r.opportunity_id == id)
            .map(|(_, r)| r.clone())
            .collect();
        records.sort_by_key(|r| (r.first_seen_at, r.id));
        Ok(records)
    }

    async fn history(&self, id: JobId) -> Result<Vec<JobEvent>, StorageError> {
        Ok(self.lock().history.get(&id).cloned().unwrap_or_default())
    }

    async fn search(&self, query: &JobQuery) -> Result<Vec<JobRecord>, StorageError> {
        let state = self.lock();
        let mut records: Vec<JobRecord> = state
            .jobs
            .values()
            .map(|(_, r)| r)
            .filter(|r| {
                let doc = r.posting.search_document();
                query.terms.iter().all(|t| doc.contains(&format!(" {t}")))
                    && (query.sources.is_empty()
                        || query.sources.contains(&r.posting.provenance.source))
                    && query.status.is_none_or(|s| r.status == s)
                    && query.seen_since.is_none_or(|t| r.last_seen_at >= t)
            })
            .cloned()
            .collect();
        records.sort_by_key(|r| (r.first_seen_at, r.id));
        if query.distinct_opportunities {
            let mut seen = HashSet::new();
            records.retain(|r| seen.insert(r.opportunity_id));
        }
        if let Some(limit) = query.limit {
            records.truncate(limit);
        }
        Ok(records)
    }

    async fn count(&self, query: &JobQuery) -> Result<u64, StorageError> {
        let query = JobQuery {
            limit: None,
            ..query.clone()
        };
        Ok(self.search(&query).await?.len() as u64)
    }
}

#[async_trait]
impl VerificationRepository for MemoryRepository {
    async fn save_verification(&self, record: &VerificationRecord) -> Result<(), StorageError> {
        self.lock().verifications.push(record.clone());
        Ok(())
    }

    async fn verification_history(
        &self,
        job: JobId,
    ) -> Result<Vec<VerificationRecord>, StorageError> {
        let mut out: Vec<VerificationRecord> = self
            .lock()
            .verifications
            .iter()
            .filter(|v| v.job_id == job)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.attempted_at.cmp(&a.attempted_at));
        Ok(out)
    }

    async fn latest_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError> {
        Ok(self.verification_history(job).await?.into_iter().next())
    }

    async fn latest_successful_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError> {
        Ok(self
            .verification_history(job)
            .await?
            .into_iter()
            .find(VerificationRecord::succeeded))
    }

    async fn record_observation(
        &self,
        posting: &JobPosting,
        observed_at: DateTime<Utc>,
    ) -> Result<jobhunt_core::UpsertOutcome, StorageError> {
        let postings = [posting.clone()];
        let scan = ScanWrite {
            run: RunId(0),
            source: &posting.provenance.source,
            started_at: observed_at,
            observed_at,
            counts: jobhunt_core::IngestCounts::default(),
            body: ScanBody::Listing {
                postings: &postings,
                complete: false,
                close_missing: false,
                closing_withheld: None,
                retain: &[],
                validator: None,
            },
        };
        let result = self.apply_scan(&scan).await?;
        let mut state = self.lock();
        // An observation is not a scan: no scan row, no run.
        state.scans.pop();
        if let Some(events) = state.history.get_mut(&posting.id())
            && let Some(last) = events.last_mut()
            && last.run == Some(RunId(0))
        {
            last.run = None;
        }
        Ok(result.outcomes[0])
    }
}
