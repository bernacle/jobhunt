//! The discovery pipeline.
//!
//! ```text
//! begin run
//!   │
//! source.fetch(request)     adapter: HTTP fetch (conditional when a validator
//!   │                       is known), parsing, conversion into canonical
//!   │                       JobPostings (URLs normalized, text cleaned)
//!   ▼
//! prepare                   validate every posting, drop in-batch duplicates,
//!   │                       decide whether the listing may close missing jobs
//!   ▼
//! JobRepository::apply_scan lifecycle plan (NEW / UNCHANGED / UPDATED /
//!   │                       REOPENED / CLOSED), history, evidence, scan record:
//!   │                       one transaction per source
//!   ▼
//! identity::group           cross-source equivalence → opportunities
//!   ▼
//! finish run
//! ```
//!
//! Sources are fetched concurrently (bounded) and persisted one at a time as
//! they arrive. A failing source is recorded and does not stop the others; a
//! storage failure stops the run, because continuing would silently lose
//! data.
//!
//! Closing is the dangerous step: a job is closed only when it is missing
//! from a listing the adapter vouched is complete, that contained no
//! unidentifiable records, and that is not a sudden empty listing (see
//! [`closing_decision`]).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures::StreamExt;
use jobhunt_core::{
    ErrorChain, FetchRequest, Fetched, IngestCounts, RecordError, RecordErrorReason, Source,
    SourceBatch, SourceError, SourceKey,
};
use tracing::{Instrument, debug, info, info_span, warn};

use crate::identity;
use crate::model::{CANONICAL_REVISION, JobId, JobPosting};
use crate::repository::{
    JobRepository, LastListing, RunId, RunSummary, ScanBody, ScanWrite, StorageError,
};

/// A source adapter that produces canonical job postings.
pub type JobSource = dyn Source<Record = JobPosting>;

/// How many rejected records are logged individually at `warn` per source.
/// The rest are logged at `debug` to keep default output readable.
const REJECTIONS_LOGGED: usize = 3;

pub struct Discovery<'a> {
    repository: &'a dyn JobRepository,
    concurrency: usize,
    validator_max_age: chrono::Duration,
}

impl<'a> Discovery<'a> {
    pub fn new(repository: &'a dyn JobRepository) -> Self {
        Self {
            repository,
            concurrency: 8,
            validator_max_age: chrono::Duration::hours(24),
        }
    }

    /// Maximum number of sources fetched at the same time (at least 1).
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// How long a listing's validator may be reused for conditional fetches.
    /// After that the listing is fetched in full even if unchanged, which
    /// bounds how long a stale validator could hide a change. Zero disables
    /// conditional fetches.
    pub fn with_validator_max_age(mut self, max_age: chrono::Duration) -> Self {
        self.validator_max_age = max_age;
        self
    }

    /// Fetches every source, persists what they return, and refreshes
    /// cross-source opportunities.
    ///
    /// Returns a per-source report. Source failures are part of the report;
    /// only storage failures abort the run.
    pub async fn run(&self, sources: &[Box<JobSource>]) -> Result<DiscoveryReport, StorageError> {
        let started_at = Utc::now();
        let clock = Instant::now();
        let run = self.repository.begin_run(started_at).await?;
        info!(
            run = run.0,
            sources = sources.len(),
            "discovery run started"
        );

        let mut last_listings = Vec::with_capacity(sources.len());
        for source in sources {
            last_listings.push(self.repository.last_listing(source.key()).await?);
        }

        let mut fetches = futures::stream::iter(sources.iter().zip(&last_listings).enumerate())
            .map(|(index, (source, last))| {
                let request = FetchRequest {
                    validator: self.reusable_validator(last.as_ref(), started_at),
                };
                async move {
                    let key = source.key().clone();
                    let span = info_span!("source", source = %key);
                    let started = Instant::now();
                    let scan_started_at = Utc::now();
                    let result = async {
                        info!(conditional = request.validator.is_some(), "source started");
                        source.fetch(&request).await
                    }
                    .instrument(span.clone())
                    .await;
                    (index, key, span, started, scan_started_at, result)
                }
            })
            .buffer_unordered(self.concurrency);

        let mut reports = Vec::with_capacity(sources.len());
        while let Some((index, key, span, started, scan_started_at, result)) = fetches.next().await
        {
            let last = last_listings[index].as_ref();
            let result = match result {
                Ok(fetched) => Ok(self
                    .persist(run, &key, scan_started_at, fetched, last, started)
                    .instrument(span)
                    .await?),
                Err(error) => {
                    let message = ErrorChain(&error).to_string();
                    span.in_scope(|| warn!(error = %message, "source failed"));
                    self.repository
                        .apply_scan(&ScanWrite {
                            run,
                            source: &key,
                            started_at: scan_started_at,
                            observed_at: Utc::now(),
                            counts: IngestCounts::default(),
                            body: ScanBody::Failed { error: &message },
                        })
                        .await?;
                    Err(error)
                }
            };
            reports.push((
                index,
                SourceReport {
                    source: key,
                    result,
                    elapsed: started.elapsed(),
                },
            ));
        }
        drop(fetches);
        reports.sort_by_key(|(index, _)| *index);

        let dedupe = self.refresh_opportunities().await?;

        let report = DiscoveryReport {
            run,
            started_at,
            elapsed: clock.elapsed(),
            sources: reports.into_iter().map(|(_, report)| report).collect(),
            dedupe,
        };
        let totals = report.totals();
        self.repository
            .finish_run(
                run,
                &RunSummary {
                    finished_at: Utc::now(),
                    sources: report.sources.len(),
                    failed: report.failures().count(),
                    counts: totals,
                    multi_source_opportunities: report.dedupe.multi_source_opportunities,
                },
            )
            .await?;
        info!(
            run = run.0,
            sources = report.sources.len(),
            failed = report.failures().count(),
            received = totals.received,
            normalized = totals.normalized,
            rejected = totals.rejected,
            new = totals.inserted,
            updated = totals.updated,
            unchanged = totals.unchanged,
            reopened = totals.reopened,
            closed = totals.closed,
            multi_source_opportunities = report.dedupe.multi_source_opportunities,
            elapsed_ms = report.elapsed.as_millis() as u64,
            "discovery run completed"
        );
        Ok(report)
    }

    fn reusable_validator(&self, last: Option<&LastListing>, now: DateTime<Utc>) -> Option<String> {
        let last = last?;
        let fresh = now.signed_duration_since(last.finished_at) < self.validator_max_age;
        (last.complete && fresh && last.revision == CANONICAL_REVISION)
            .then(|| last.validator.clone())
            .flatten()
    }

    async fn persist(
        &self,
        run: RunId,
        key: &SourceKey,
        started_at: DateTime<Utc>,
        fetched: Fetched<JobPosting>,
        last: Option<&LastListing>,
        clock: Instant,
    ) -> Result<ScanStats, StorageError> {
        let batch = match fetched {
            Fetched::NotModified => {
                let result = self
                    .repository
                    .apply_scan(&ScanWrite {
                        run,
                        source: key,
                        started_at,
                        observed_at: Utc::now(),
                        counts: IngestCounts::default(),
                        body: ScanBody::NotModified,
                    })
                    .await?;
                let counts = IngestCounts {
                    unchanged: result.touched,
                    ..IngestCounts::default()
                };
                info!(
                    unchanged = counts.unchanged,
                    elapsed_ms = clock.elapsed().as_millis() as u64,
                    "source not modified"
                );
                return Ok(ScanStats {
                    counts,
                    kind: ScanKind::NotModified,
                    closing_withheld: None,
                });
            }
            Fetched::Batch(batch) => batch,
        };
        info!(
            received = batch.received(),
            complete = batch.complete,
            elapsed_ms = clock.elapsed().as_millis() as u64,
            "fetch completed"
        );

        let prepared = prepare(key, batch);
        log_rejections(&prepared.rejected);
        let decision = closing_decision(
            prepared.complete,
            prepared.unidentified_rejections,
            prepared.counts.received,
            last,
        );
        let closing_withheld = match decision {
            Closing::Allowed => None,
            Closing::Incomplete => None,
            Closing::Withheld(reason) => {
                warn!(reason, "not closing missing jobs");
                Some(reason)
            }
        };
        let close_missing = decision == Closing::Allowed;

        let result = self
            .repository
            .apply_scan(&ScanWrite {
                run,
                source: key,
                started_at,
                observed_at: Utc::now(),
                counts: prepared.counts,
                body: ScanBody::Listing {
                    postings: &prepared.valid,
                    complete: prepared.complete,
                    close_missing,
                    closing_withheld,
                    retain: &prepared.retain,
                    // A validator is only worth keeping for a listing that
                    // was fully applied; otherwise "not modified" next time
                    // would skip work this scan left undone.
                    validator: prepared.validator.as_deref().filter(|_| close_missing),
                },
            })
            .await?;

        let mut counts = prepared.counts;
        for outcome in &result.outcomes {
            counts.record(*outcome);
        }
        counts.closed = result.closed.len();
        for id in &result.closed {
            debug!(job = %id, "job closed");
        }
        info!(
            received = counts.received,
            normalized = counts.normalized,
            rejected = counts.rejected,
            skipped = counts.skipped,
            duplicates = counts.duplicates,
            new = counts.inserted,
            updated = counts.updated,
            unchanged = counts.unchanged,
            reopened = counts.reopened,
            closed = counts.closed,
            complete = prepared.complete,
            "source completed"
        );
        Ok(ScanStats {
            counts,
            kind: if prepared.complete {
                ScanKind::Complete
            } else {
                ScanKind::Partial
            },
            closing_withheld,
        })
    }

    /// Recomputes cross-source opportunities over every stored record.
    async fn refresh_opportunities(&self) -> Result<DedupeStats, StorageError> {
        let index = self.repository.identity_index().await?;
        let grouping = identity::group(&index);
        for link in &grouping.links {
            debug!(a = %link.a, b = %link.b, evidence = %link.evidence, "equivalent records");
        }
        for (link, reason) in &grouping.rejected {
            debug!(a = %link.a, b = %link.b, evidence = %link.evidence, reason, "equivalence rejected");
        }
        for (job, opportunity) in &grouping.assignments {
            info!(job = %job, opportunity = %opportunity, "duplicate detected: record joins opportunity");
        }
        for jobs in &grouping.look_alikes {
            debug!(jobs = ?jobs, "look-alike records kept apart (no shared evidence)");
        }
        self.repository
            .assign_opportunities(&grouping.assignments)
            .await?;
        Ok(DedupeStats {
            links: grouping.links.len(),
            reassigned: grouping.assignments.len(),
            multi_source_opportunities: grouping.multi_source_groups,
            look_alikes: grouping.look_alikes.len(),
        })
    }
}

/// A batch validated and ready to persist.
struct Prepared {
    valid: Vec<JobPosting>,
    rejected: Vec<RecordError>,
    retain: Vec<JobId>,
    unidentified_rejections: usize,
    counts: IngestCounts,
    complete: bool,
    validator: Option<String>,
}

/// Validates postings and drops in-batch duplicates.
fn prepare(source: &SourceKey, batch: SourceBatch<JobPosting>) -> Prepared {
    let mut counts = IngestCounts {
        received: batch.received(),
        skipped: batch.skipped,
        ..IngestCounts::default()
    };
    let mut rejected = batch.rejected;
    let mut seen = HashSet::with_capacity(batch.records.len());
    let mut valid = Vec::with_capacity(batch.records.len());

    for posting in batch.records {
        if posting.provenance.source != *source {
            rejected.push(RecordError::new(
                posting.provenance.source_record_id.clone(),
                RecordErrorReason::InvalidValue {
                    field: "provenance.source",
                    detail: format!(
                        "adapter for {source} produced a posting attributed to {}",
                        posting.provenance.source
                    ),
                },
            ));
            continue;
        }
        if let Err(error) = posting.validate() {
            rejected.push(error);
            continue;
        }
        if seen.insert(posting.id()) {
            valid.push(posting);
        } else {
            counts.duplicates += 1;
        }
    }
    counts.rejected = rejected.len();
    counts.normalized = valid.len() + counts.duplicates;

    // A rejected record that carries its source id still exists at the
    // source: it must not be closed. One without an id could be any job.
    let mut retain = Vec::new();
    let mut unidentified_rejections = 0;
    for error in &rejected {
        match error.record_id.as_deref() {
            Some(record_id) => retain.push(JobId::derive_from_record_id(source, record_id)),
            None => unidentified_rejections += 1,
        }
    }
    Prepared {
        valid,
        rejected,
        retain,
        unidentified_rejections,
        counts,
        complete: batch.complete,
        validator: batch.validator,
    }
}

/// Whether a listing may close the source's missing jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    Allowed,
    /// The adapter did not vouch for completeness.
    Incomplete,
    /// Complete, but a safeguard applies.
    Withheld(&'static str),
}

/// Decides whether a listing is trustworthy enough to close missing jobs.
///
/// * The adapter must mark the listing complete.
/// * Every rejected record must be identifiable, so it can be kept open.
/// * An empty listing right after a non-empty one is treated as a glitch
///   (sources occasionally return `[]`); it closes jobs only once a second
///   consecutive complete listing is also empty.
pub fn closing_decision(
    complete: bool,
    unidentified_rejections: usize,
    received: usize,
    last: Option<&LastListing>,
) -> Closing {
    if !complete {
        return Closing::Incomplete;
    }
    if unidentified_rejections > 0 {
        return Closing::Withheld("the listing had unreadable records without an id");
    }
    if received == 0 && last.is_some_and(|l| l.received > 0) {
        return Closing::Withheld("the listing was empty; waiting for a second empty listing");
    }
    Closing::Allowed
}

fn log_rejections(rejected: &[RecordError]) {
    if rejected.is_empty() {
        return;
    }
    for error in rejected.iter().take(REJECTIONS_LOGGED) {
        warn!(error = %error, "rejected record");
    }
    for error in rejected.iter().skip(REJECTIONS_LOGGED) {
        debug!(error = %error, "rejected record");
    }
    if rejected.len() > REJECTIONS_LOGGED {
        warn!(
            count = rejected.len(),
            "rejected records (run with debug logging to see all)"
        );
    }
}

/// How a source's scan went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    /// A complete listing.
    Complete,
    /// A listing the adapter could not vouch was complete: nothing closed.
    Partial,
    /// The source confirmed nothing changed since the last complete listing.
    NotModified,
}

/// Statistics of one successful source scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanStats {
    pub counts: IngestCounts,
    pub kind: ScanKind,
    /// Why a complete listing did not close missing jobs, if it did not.
    pub closing_withheld: Option<&'static str>,
}

/// Outcome of one source within a discovery run.
#[derive(Debug)]
pub struct SourceReport {
    pub source: SourceKey,
    pub result: Result<ScanStats, SourceError>,
    pub elapsed: Duration,
}

/// Cross-source grouping after a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DedupeStats {
    /// Accepted links between records of different sources.
    pub links: usize,
    /// Records whose opportunity changed in this run.
    pub reassigned: usize,
    /// Opportunities backed by more than one source record.
    pub multi_source_opportunities: usize,
    /// Jobs that look listed by several sources (same company and title)
    /// but share no evidence, so were kept apart.
    pub look_alikes: usize,
}

/// Outcome of a whole discovery run.
#[derive(Debug)]
pub struct DiscoveryReport {
    pub run: RunId,
    /// When the run started. Every job seen during the run has
    /// `last_seen_at >= started_at`.
    pub started_at: DateTime<Utc>,
    pub elapsed: Duration,
    /// One report per source, in the order the sources were given.
    pub sources: Vec<SourceReport>,
    pub dedupe: DedupeStats,
}

impl DiscoveryReport {
    /// Counts summed over the sources that succeeded.
    pub fn totals(&self) -> IngestCounts {
        let mut totals = IngestCounts::default();
        for report in &self.sources {
            if let Ok(stats) = &report.result {
                totals.merge(&stats.counts);
            }
        }
        totals
    }

    pub fn failures(&self) -> impl Iterator<Item = (&SourceKey, &SourceError)> {
        self.sources
            .iter()
            .filter_map(|report| report.result.as_ref().err().map(|e| (&report.source, e)))
    }

    pub fn succeeded(&self) -> usize {
        self.sources.iter().filter(|r| r.result.is_ok()).count()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::memory::MemoryRepository;
    use crate::model::JobStatus;
    use crate::model::tests::posting;
    use crate::repository::{JobEventKind, JobQuery};

    /// A source whose response can be changed between runs.
    struct ScriptedSource {
        key: SourceKey,
        response: Mutex<Response>,
        requests: Mutex<Vec<FetchRequest>>,
    }

    #[derive(Clone)]
    enum Response {
        Listing {
            postings: Vec<JobPosting>,
            rejected: Vec<Option<&'static str>>,
            complete: bool,
            validator: Option<&'static str>,
        },
        NotModified,
        Fail,
    }

    impl ScriptedSource {
        fn new(key: &str, response: Response) -> Arc<Self> {
            Arc::new(Self {
                key: key.parse().unwrap(),
                response: Mutex::new(response),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn complete(key: &str, postings: Vec<JobPosting>) -> Arc<Self> {
            Self::new(key, listing(postings, true))
        }

        fn set(&self, response: Response) {
            *self.response.lock().unwrap() = response;
        }
    }

    fn listing(postings: Vec<JobPosting>, complete: bool) -> Response {
        Response::Listing {
            postings,
            rejected: vec![],
            complete,
            validator: None,
        }
    }

    #[async_trait]
    impl Source for ScriptedSource {
        type Record = JobPosting;

        fn key(&self) -> &SourceKey {
            &self.key
        }

        async fn fetch(&self, request: &FetchRequest) -> Result<Fetched<JobPosting>, SourceError> {
            self.requests.lock().unwrap().push(request.clone());
            match self.response.lock().unwrap().clone() {
                Response::Fail => Err(SourceError::Status {
                    url: "https://example.com".into(),
                    status: 503,
                }),
                Response::NotModified => Ok(Fetched::NotModified),
                Response::Listing {
                    postings,
                    rejected,
                    complete,
                    validator,
                } => Ok(Fetched::Batch(SourceBatch {
                    records: postings,
                    rejected: rejected
                        .into_iter()
                        .map(|id| {
                            RecordError::new(
                                id.map(str::to_owned),
                                RecordErrorReason::MissingField("title"),
                            )
                        })
                        .collect(),
                    skipped: 0,
                    complete,
                    validator: validator.map(str::to_owned),
                })),
            }
        }
    }

    /// Lets tests keep a handle on a source while discovery owns a box.
    struct Shared(Arc<ScriptedSource>);

    #[async_trait]
    impl Source for Shared {
        type Record = JobPosting;

        fn key(&self) -> &SourceKey {
            self.0.key()
        }

        async fn fetch(&self, request: &FetchRequest) -> Result<Fetched<JobPosting>, SourceError> {
            self.0.fetch(request).await
        }
    }

    async fn run(repo: &MemoryRepository, sources: &[&Arc<ScriptedSource>]) -> DiscoveryReport {
        let boxed: Vec<Box<JobSource>> = sources
            .iter()
            .map(|s| Box::new(Shared(Arc::clone(s))) as Box<JobSource>)
            .collect();
        Discovery::new(repo).run(&boxed).await.unwrap()
    }

    fn stats(report: &DiscoveryReport, index: usize) -> ScanStats {
        *report.sources[index].result.as_ref().unwrap()
    }

    fn lifecycle(c: &IngestCounts) -> (usize, usize, usize, usize, usize) {
        (c.inserted, c.updated, c.unchanged, c.reopened, c.closed)
    }

    async fn status(repo: &MemoryRepository, p: &JobPosting) -> JobStatus {
        repo.get(p.id()).await.unwrap().unwrap().status
    }

    #[tokio::test]
    async fn repeated_runs_classify_new_unchanged_updated_and_closed() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let b = posting("test:acme", Some("2"), "Designer");
        let c = posting("test:acme", Some("3"), "Writer");
        let source = ScriptedSource::complete("test:acme", vec![a.clone(), b.clone(), c.clone()]);

        let first = run(&repo, &[&source]).await;
        assert_eq!(lifecycle(&first.totals()), (3, 0, 0, 0, 0));
        assert_eq!(stats(&first, 0).kind, ScanKind::Complete);

        let second = run(&repo, &[&source]).await;
        assert_eq!(lifecycle(&second.totals()), (0, 0, 3, 0, 0));

        let mut b2 = b.clone();
        b2.title = "Senior Designer".into();
        source.set(listing(vec![a.clone(), b2], true));
        let third = run(&repo, &[&source]).await;
        assert_eq!(lifecycle(&third.totals()), (0, 1, 1, 0, 1));
        assert_eq!(status(&repo, &c).await, JobStatus::Closed);
        assert_eq!(
            repo.count(&JobQuery::default()).await.unwrap(),
            3,
            "closed jobs are kept"
        );

        let history = repo.history(b.id()).await.unwrap();
        let kinds: Vec<_> = history.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![JobEventKind::New, JobEventKind::Updated]);
        assert_eq!(history[1].changed_fields, vec!["title"]);
        assert_eq!(history[1].previous.as_ref().unwrap().title, "Designer");
    }

    #[tokio::test]
    async fn failed_and_partial_scans_never_close() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let b = posting("test:acme", Some("2"), "Designer");
        let source = ScriptedSource::complete("test:acme", vec![a.clone(), b.clone()]);
        run(&repo, &[&source]).await;

        source.set(Response::Fail);
        let failed = run(&repo, &[&source]).await;
        assert!(failed.sources[0].result.is_err());
        assert_eq!(status(&repo, &a).await, JobStatus::Open);

        source.set(listing(vec![a.clone()], false));
        let partial = run(&repo, &[&source]).await;
        assert_eq!(stats(&partial, 0).kind, ScanKind::Partial);
        assert_eq!(partial.totals().closed, 0);
        assert_eq!(status(&repo, &b).await, JobStatus::Open);
    }

    #[tokio::test]
    async fn closed_jobs_reopen_when_they_reappear() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let b = posting("test:acme", Some("2"), "Designer");
        let source = ScriptedSource::complete("test:acme", vec![a.clone(), b.clone()]);
        run(&repo, &[&source]).await;
        source.set(listing(vec![a.clone()], true));
        run(&repo, &[&source]).await;
        assert_eq!(status(&repo, &b).await, JobStatus::Closed);

        source.set(listing(vec![a.clone(), b.clone()], true));
        let report = run(&repo, &[&source]).await;
        assert_eq!(lifecycle(&report.totals()), (0, 0, 1, 1, 0));
        let record = repo.get(b.id()).await.unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Open);
        assert_eq!(record.closed_at, None);
        let kinds: Vec<_> = repo
            .history(b.id())
            .await
            .unwrap()
            .iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                JobEventKind::New,
                JobEventKind::Closed,
                JobEventKind::Reopened
            ]
        );
    }

    #[tokio::test]
    async fn rejected_records_with_ids_stay_open_and_without_ids_block_closing() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let b = posting("test:acme", Some("2"), "Designer");
        let c = posting("test:acme", Some("3"), "Writer");
        let source = ScriptedSource::complete("test:acme", vec![a.clone(), b.clone(), c.clone()]);
        run(&repo, &[&source]).await;

        // b is listed but unreadable (its id is known); c is gone.
        source.set(Response::Listing {
            postings: vec![a.clone()],
            rejected: vec![Some("2")],
            complete: true,
            validator: None,
        });
        let report = run(&repo, &[&source]).await;
        assert_eq!(report.totals().rejected, 1);
        assert_eq!(status(&repo, &b).await, JobStatus::Open);
        assert_eq!(status(&repo, &c).await, JobStatus::Closed);

        // An unreadable record without an id could be any job: close nothing.
        source.set(Response::Listing {
            postings: vec![],
            rejected: vec![None],
            complete: true,
            validator: None,
        });
        let report = run(&repo, &[&source]).await;
        assert_eq!(report.totals().closed, 0);
        assert!(stats(&report, 0).closing_withheld.is_some());
        assert_eq!(status(&repo, &a).await, JobStatus::Open);
    }

    #[tokio::test]
    async fn a_sudden_empty_listing_needs_confirmation() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let source = ScriptedSource::complete("test:acme", vec![a.clone()]);
        run(&repo, &[&source]).await;

        source.set(listing(vec![], true));
        let first_empty = run(&repo, &[&source]).await;
        assert_eq!(first_empty.totals().closed, 0);
        assert!(stats(&first_empty, 0).closing_withheld.is_some());
        assert_eq!(status(&repo, &a).await, JobStatus::Open);

        let second_empty = run(&repo, &[&source]).await;
        assert_eq!(second_empty.totals().closed, 1);
        assert_eq!(status(&repo, &a).await, JobStatus::Closed);
    }

    #[tokio::test]
    async fn not_modified_listings_mark_open_jobs_seen() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let source = ScriptedSource::new(
            "test:acme",
            Response::Listing {
                postings: vec![a.clone()],
                rejected: vec![],
                complete: true,
                validator: Some("etag-1"),
            },
        );
        run(&repo, &[&source]).await;
        let seen_before = repo.get(a.id()).await.unwrap().unwrap().last_seen_at;

        source.set(Response::NotModified);
        let report = run(&repo, &[&source]).await;
        assert_eq!(stats(&report, 0).kind, ScanKind::NotModified);
        assert_eq!(report.totals().unchanged, 1);
        let requests = source.requests.lock().unwrap().clone();
        assert_eq!(requests[0].validator, None);
        assert_eq!(requests[1].validator.as_deref(), Some("etag-1"));
        let record = repo.get(a.id()).await.unwrap().unwrap();
        assert!(record.last_seen_at >= seen_before);
        assert!(record.last_seen_at >= report.started_at);
    }

    #[tokio::test]
    async fn validators_are_not_reused_after_partial_listings() {
        let repo = MemoryRepository::default();
        let a = posting("test:acme", Some("1"), "Engineer");
        let source = ScriptedSource::new(
            "test:acme",
            Response::Listing {
                postings: vec![a.clone()],
                rejected: vec![],
                complete: false,
                validator: Some("etag-1"),
            },
        );
        run(&repo, &[&source]).await;
        run(&repo, &[&source]).await;
        let requests = source.requests.lock().unwrap().clone();
        assert_eq!(requests[1].validator, None);
    }

    #[tokio::test]
    async fn one_failing_source_does_not_stop_the_others() {
        let repo = MemoryRepository::default();
        let down = ScriptedSource::new("test:down", Response::Fail);
        let up = ScriptedSource::complete(
            "test:acme",
            vec![posting("test:acme", Some("1"), "Engineer")],
        );
        let report = run(&repo, &[&down, &up]).await;
        assert_eq!(report.sources.len(), 2);
        assert_eq!(report.sources[0].source.to_string(), "test:down");
        assert!(report.sources[0].result.is_err());
        assert_eq!(report.succeeded(), 1);
        assert_eq!(report.failures().count(), 1);
        assert_eq!(report.totals().inserted, 1);
        assert_eq!(repo.scans().len(), 2, "failed scans are recorded too");
    }

    #[tokio::test]
    async fn in_batch_duplicates_and_invalid_postings_are_not_persisted() {
        let repo = MemoryRepository::default();
        let mut untitled = posting("test:acme", Some("3"), "x");
        untitled.title = " ".into();
        let source = ScriptedSource::complete(
            "test:acme",
            vec![
                posting("test:acme", Some("1"), "Engineer"),
                posting("test:acme", Some("1"), "Engineer"),
                untitled,
                posting("test:other", Some("4"), "Misattributed"),
            ],
        );
        let report = run(&repo, &[&source]).await;
        let counts = report.totals();
        assert_eq!(counts.received, 4);
        assert_eq!(counts.normalized, 2);
        assert_eq!(counts.duplicates, 1);
        assert_eq!(counts.rejected, 2);
        assert_eq!(counts.inserted, 1);
        assert_eq!(
            counts.received,
            counts.normalized + counts.rejected + counts.skipped
        );
        assert_eq!(repo.count(&JobQuery::default()).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn equivalent_records_from_two_sources_share_an_opportunity() {
        let repo = MemoryRepository::default();
        let mut gh = posting("greenhouse:acme", Some("555"), "Engineer");
        gh.url = jobhunt_core::CanonicalUrl::parse("https://boards.greenhouse.io/acme/jobs/555")
            .unwrap();
        let mut site = posting("careers:acme", Some("x"), "Engineer");
        site.url =
            jobhunt_core::CanonicalUrl::parse("https://acme.com/careers?gh_jid=555").unwrap();
        let other = posting("careers:acme", Some("y"), "Engineer");

        let a = ScriptedSource::complete("greenhouse:acme", vec![gh.clone()]);
        let b = ScriptedSource::complete("careers:acme", vec![site.clone(), other.clone()]);
        let report = run(&repo, &[&a, &b]).await;
        assert_eq!(report.dedupe.links, 1);
        assert_eq!(report.dedupe.multi_source_opportunities, 1);

        let gh_record = repo.get(gh.id()).await.unwrap().unwrap();
        let site_record = repo.get(site.id()).await.unwrap().unwrap();
        let other_record = repo.get(other.id()).await.unwrap().unwrap();
        assert_eq!(gh_record.opportunity_id, site_record.opportunity_id);
        assert_ne!(gh_record.opportunity_id, other_record.opportunity_id);
        // Both source records stay individually stored.
        let members = repo
            .opportunity_records(gh_record.opportunity_id)
            .await
            .unwrap();
        assert_eq!(members.len(), 2);

        let distinct = JobQuery {
            distinct_opportunities: true,
            ..JobQuery::default()
        };
        assert_eq!(repo.count(&distinct).await.unwrap(), 2);

        // Stable on re-run.
        let again = run(&repo, &[&a, &b]).await;
        assert_eq!(again.dedupe.reassigned, 0);
        assert_eq!(
            lifecycle(&again.totals()),
            (0, 0, 3, 0, 0),
            "a repeat run is idempotent"
        );
    }

    #[test]
    fn closing_decisions() {
        let last = |received| LastListing {
            finished_at: Utc::now(),
            complete: true,
            received,
            validator: None,
            revision: CANONICAL_REVISION.into(),
        };
        assert_eq!(closing_decision(false, 0, 5, None), Closing::Incomplete);
        assert_eq!(closing_decision(true, 0, 5, None), Closing::Allowed);
        assert!(matches!(
            closing_decision(true, 1, 5, None),
            Closing::Withheld(_)
        ));
        assert!(matches!(
            closing_decision(true, 0, 0, Some(&last(3))),
            Closing::Withheld(_)
        ));
        assert_eq!(
            closing_decision(true, 0, 0, Some(&last(0))),
            Closing::Allowed
        );
        assert_eq!(closing_decision(true, 0, 0, None), Closing::Allowed);
    }
}
