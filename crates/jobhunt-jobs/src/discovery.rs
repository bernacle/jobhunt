//! The discovery pipeline.
//!
//! ```text
//! source.fetch()            adapter: HTTP fetch, payload parsing, conversion
//!   │                       into canonical JobPostings (URLs normalized)
//!   ▼
//! Discovery::ingest()       validate every posting, drop in-batch duplicates
//!   │
//!   ▼
//! JobRepository::upsert     insert / update / mark unchanged, atomically
//! ```
//!
//! Sources are fetched concurrently and persisted one batch at a time as they
//! arrive. A failing source is reported and does not stop the others; a
//! storage failure stops the run, because continuing would silently lose data.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures::StreamExt;
use jobhunt_core::{
    ErrorChain, IngestCounts, RecordError, RecordErrorReason, Source, SourceBatch, SourceError,
    SourceKey,
};
use tracing::{Instrument, debug, info, info_span, warn};

use crate::model::JobPosting;
use crate::repository::{JobRepository, StorageError};

/// A source adapter that produces canonical job postings.
pub type JobSource = dyn Source<Record = JobPosting>;

/// How many rejected records are logged individually at `warn` per source.
/// The rest are logged at `debug` to keep default output readable.
const REJECTIONS_LOGGED: usize = 3;

pub struct Discovery<'a> {
    repository: &'a dyn JobRepository,
    concurrency: usize,
}

impl<'a> Discovery<'a> {
    pub fn new(repository: &'a dyn JobRepository) -> Self {
        Self {
            repository,
            concurrency: 4,
        }
    }

    /// Maximum number of sources fetched at the same time (at least 1).
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Fetches every source and persists what they return.
    ///
    /// Returns a per-source report. Source failures are part of the report;
    /// only storage failures abort the run.
    pub async fn run(&self, sources: &[Box<JobSource>]) -> Result<DiscoveryReport, StorageError> {
        let started_at = Utc::now();
        let clock = Instant::now();
        info!(sources = sources.len(), "discovery started");

        let mut fetches = futures::stream::iter(sources.iter().enumerate())
            .map(|(index, source)| async move {
                let key = source.key().clone();
                let span = info_span!("source", source = %key);
                let started = Instant::now();
                let result = async {
                    info!("fetch started");
                    source.fetch().await
                }
                .instrument(span.clone())
                .await;
                (index, key, span, started, result)
            })
            .buffer_unordered(self.concurrency);

        let mut reports = Vec::with_capacity(sources.len());
        while let Some((index, key, span, started, result)) = fetches.next().await {
            let result = match result {
                Ok(batch) => {
                    span.in_scope(|| {
                        info!(
                            received = batch.received(),
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "fetch completed"
                        );
                    });
                    let counts = self
                        .ingest(&key, batch, Utc::now())
                        .instrument(span)
                        .await?;
                    Ok(counts)
                }
                Err(error) => {
                    span.in_scope(|| warn!(error = %ErrorChain(&error), "fetch failed"));
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
        reports.sort_by_key(|(index, _)| *index);

        let report = DiscoveryReport {
            started_at,
            elapsed: clock.elapsed(),
            sources: reports.into_iter().map(|(_, report)| report).collect(),
        };
        let totals = report.totals();
        info!(
            sources = report.sources.len(),
            failed = report.failures().count(),
            received = totals.received,
            inserted = totals.inserted,
            updated = totals.updated,
            unchanged = totals.unchanged,
            rejected = totals.rejected,
            elapsed_ms = report.elapsed.as_millis() as u64,
            "discovery completed"
        );
        Ok(report)
    }

    /// Validates, de-duplicates and persists one source batch.
    pub async fn ingest(
        &self,
        source: &SourceKey,
        batch: SourceBatch<JobPosting>,
        observed_at: DateTime<Utc>,
    ) -> Result<IngestCounts, StorageError> {
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
        log_rejections(&rejected);

        let outcomes = self.repository.upsert_postings(&valid, observed_at).await?;
        for outcome in outcomes {
            counts.record(outcome);
        }

        info!(
            received = counts.received,
            normalized = counts.normalized,
            rejected = counts.rejected,
            skipped = counts.skipped,
            duplicates = counts.duplicates,
            inserted = counts.inserted,
            updated = counts.updated,
            unchanged = counts.unchanged,
            "ingest completed"
        );
        Ok(counts)
    }
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

/// Outcome of one source within a discovery run.
#[derive(Debug)]
pub struct SourceReport {
    pub source: SourceKey,
    pub result: Result<IngestCounts, SourceError>,
    pub elapsed: Duration,
}

/// Outcome of a whole discovery run.
#[derive(Debug)]
pub struct DiscoveryReport {
    /// When the run started. Every job seen during the run has
    /// `last_seen_at >= started_at`.
    pub started_at: DateTime<Utc>,
    pub elapsed: Duration,
    /// One report per source, in the order the sources were given.
    pub sources: Vec<SourceReport>,
}

impl DiscoveryReport {
    /// Counts summed over the sources that succeeded.
    pub fn totals(&self) -> IngestCounts {
        let mut totals = IngestCounts::default();
        for report in &self.sources {
            if let Ok(counts) = &report.result {
                totals.merge(counts);
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
    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use jobhunt_core::{Fingerprint, UpsertOutcome};

    use super::*;
    use crate::model::tests::posting;
    use crate::model::{JobId, JobRecord};
    use crate::repository::JobQuery;

    /// Minimal in-memory repository honoring the upsert contract.
    #[derive(Default)]
    struct MemoryRepository {
        jobs: Mutex<HashMap<JobId, (Fingerprint, JobRecord)>>,
    }

    #[async_trait]
    impl JobRepository for MemoryRepository {
        async fn upsert_postings(
            &self,
            postings: &[JobPosting],
            observed_at: DateTime<Utc>,
        ) -> Result<Vec<UpsertOutcome>, StorageError> {
            let mut jobs = self.jobs.lock().unwrap();
            Ok(postings
                .iter()
                .map(|p| {
                    let fp = p.fingerprint();
                    match jobs.get_mut(&p.id()) {
                        None => {
                            jobs.insert(
                                p.id(),
                                (
                                    fp,
                                    JobRecord {
                                        id: p.id(),
                                        posting: p.clone(),
                                        first_seen_at: observed_at,
                                        last_seen_at: observed_at,
                                        content_updated_at: observed_at,
                                    },
                                ),
                            );
                            UpsertOutcome::Inserted
                        }
                        Some((stored, record)) => {
                            record.last_seen_at = observed_at;
                            if *stored == fp {
                                UpsertOutcome::Unchanged
                            } else {
                                *stored = fp;
                                record.posting = p.clone();
                                record.content_updated_at = observed_at;
                                UpsertOutcome::Updated
                            }
                        }
                    }
                })
                .collect())
        }

        async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError> {
            Ok(self.jobs.lock().unwrap().get(&id).map(|(_, r)| r.clone()))
        }

        async fn search(&self, _query: &JobQuery) -> Result<Vec<JobRecord>, StorageError> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .values()
                .map(|(_, r)| r.clone())
                .collect())
        }

        async fn count(&self, _query: &JobQuery) -> Result<u64, StorageError> {
            Ok(self.jobs.lock().unwrap().len() as u64)
        }
    }

    /// A source that returns a fixed batch (or a fixed failure).
    struct StaticSource {
        key: SourceKey,
        postings: Vec<JobPosting>,
        fail: bool,
    }

    impl StaticSource {
        fn ok(key: &str, postings: Vec<JobPosting>) -> Box<JobSource> {
            Box::new(Self {
                key: key.parse().unwrap(),
                postings,
                fail: false,
            })
        }

        fn failing(key: &str) -> Box<JobSource> {
            Box::new(Self {
                key: key.parse().unwrap(),
                postings: vec![],
                fail: true,
            })
        }
    }

    #[async_trait]
    impl Source for StaticSource {
        type Record = JobPosting;

        fn key(&self) -> &SourceKey {
            &self.key
        }

        async fn fetch(&self) -> Result<SourceBatch<JobPosting>, SourceError> {
            if self.fail {
                return Err(SourceError::Status {
                    url: "https://example.com".into(),
                    status: 503,
                });
            }
            Ok(SourceBatch {
                records: self.postings.clone(),
                rejected: vec![RecordError::new(
                    Some("bad".into()),
                    RecordErrorReason::MissingField("title"),
                )],
                skipped: 1,
            })
        }
    }

    #[tokio::test]
    async fn repeated_runs_do_not_duplicate() {
        let repo = MemoryRepository::default();
        let discovery = Discovery::new(&repo);
        let sources = vec![StaticSource::ok(
            "test:acme",
            vec![
                posting("test:acme", Some("1"), "Engineer"),
                posting("test:acme", Some("2"), "Designer"),
            ],
        )];

        let first = discovery.run(&sources).await.unwrap();
        let totals = first.totals();
        assert_eq!(totals.received, 4);
        assert_eq!(totals.normalized, 2);
        assert_eq!(totals.rejected, 1);
        assert_eq!(totals.skipped, 1);
        assert_eq!(totals.inserted, 2);

        let second = discovery.run(&sources).await.unwrap();
        let totals = second.totals();
        assert_eq!(
            (totals.inserted, totals.updated, totals.unchanged),
            (0, 0, 2)
        );
        assert_eq!(repo.count(&JobQuery::default()).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn changed_content_is_reported_as_updated() {
        let repo = MemoryRepository::default();
        let discovery = Discovery::new(&repo);
        let key: SourceKey = "test:acme".parse().unwrap();
        let original = posting("test:acme", Some("1"), "Engineer");
        let t0 = Utc::now();
        let batch = |p: JobPosting| SourceBatch {
            records: vec![p],
            ..Default::default()
        };

        discovery
            .ingest(&key, batch(original.clone()), t0)
            .await
            .unwrap();
        let mut edited = original.clone();
        edited.title = "Senior Engineer".into();
        let t1 = t0 + chrono::Duration::seconds(60);
        let counts = discovery.ingest(&key, batch(edited), t1).await.unwrap();
        assert_eq!(counts.updated, 1);

        let record = repo.get(original.id()).await.unwrap().unwrap();
        assert_eq!(record.posting.title, "Senior Engineer");
        assert_eq!(record.first_seen_at, t0);
        assert_eq!(record.content_updated_at, t1);
    }

    #[tokio::test]
    async fn in_batch_duplicates_and_invalid_postings_are_not_persisted() {
        let repo = MemoryRepository::default();
        let discovery = Discovery::new(&repo);
        let key: SourceKey = "test:acme".parse().unwrap();
        let mut untitled = posting("test:acme", Some("3"), "x");
        untitled.title = " ".into();
        let batch = SourceBatch {
            records: vec![
                posting("test:acme", Some("1"), "Engineer"),
                posting("test:acme", Some("1"), "Engineer"),
                untitled,
                posting("test:other", Some("4"), "Misattributed"),
            ],
            ..Default::default()
        };

        let counts = discovery.ingest(&key, batch, Utc::now()).await.unwrap();
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
    async fn one_failing_source_does_not_stop_the_others() {
        let repo = MemoryRepository::default();
        let discovery = Discovery::new(&repo).with_concurrency(2);
        let sources = vec![
            StaticSource::failing("test:down"),
            StaticSource::ok(
                "test:acme",
                vec![posting("test:acme", Some("1"), "Engineer")],
            ),
        ];

        let report = discovery.run(&sources).await.unwrap();
        assert_eq!(report.sources.len(), 2);
        assert_eq!(report.sources[0].source.to_string(), "test:down");
        assert!(report.sources[0].result.is_err());
        assert_eq!(report.succeeded(), 1);
        assert_eq!(report.failures().count(), 1);
        assert_eq!(report.totals().inserted, 1);
    }
}
