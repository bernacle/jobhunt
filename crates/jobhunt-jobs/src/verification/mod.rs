//! Verification: is a stored job still open at its authoritative source,
//! who stands behind the listing, and what does it publish right now?
//!
//! Discovery records what sources published during a scan. Verification
//! asks one job's authoritative source directly, records what it found as
//! a [`VerificationRecord`] (one per attempt, kept as history), and feeds a
//! live posting back through the existing lifecycle, so a changed job is
//! UPDATED and a reappearing one REOPENED exactly as discovery would record
//! it. Eligibility (whether a person can take the job) is a separate
//! question answered elsewhere from the records this produces.
//!
//! * [`model`]: records, listing / application / authority states.
//! * [`compensation`]: published pay as facts, with currency evidence.
//! * [`trust`]: the freshness policy and the trust view of a record and of
//!   an opportunity.
//! * [`ListingVerifier`]: the fetch contract source adapters implement
//!   (plain HTTP today). A browser-backed implementation would be another
//!   implementation of this trait; nothing here would change.
//! * [`VerificationRepository`]: the persistence boundary.
//! * [`VerificationService`]: the use case.

pub mod compensation;
pub mod model;
pub mod trust;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use jobhunt_core::{CanonicalUrl, UpsertOutcome};
use tracing::{debug, info};

pub use compensation::{
    CompensationChange, CompensationCheck, CompensationStatus, CurrencyEvidence, PayRange,
};
pub use model::{
    ApplicationBasis, ApplicationCheck, ApplicationStatus, Authority, AuthorityLink, FailureKind,
    LinkKind, ListingStatus, PublishedPlace, VERIFICATION_REVISION, VerificationFailure,
    VerificationId, VerificationMethod, VerificationRecord,
};
pub use trust::{
    FreshnessPolicy, OpportunityTrust, RecordTrust, Reverify, Standing, TrustState,
    VerificationAge, ago,
};

use crate::model::{JobId, JobPosting, JobRecord};
use crate::repository::{JobRepository, StorageError};

/// What the authoritative source said about the job's listing.
#[derive(Debug, Clone, PartialEq)]
pub enum ObservedListing {
    /// The source lists the job; this is what it publishes now, converted
    /// by the same adapter code discovery uses.
    Found(Box<JobPosting>),
    /// The job's own endpoint answered "not found".
    NotFound { detail: String },
    /// A complete listing of the source no longer has the job.
    MissingFromCompleteListing { detail: String },
}

/// What a verifier observed.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceObservation {
    pub method: VerificationMethod,
    pub checked_url: String,
    pub listing: ObservedListing,
    pub application: ApplicationCheck,
    /// Links the verifier followed or read, employer side first.
    pub chain: Vec<AuthorityLink>,
    /// What the verifier could not establish.
    pub unknowns: Vec<String>,
}

/// A verifier could not reach a definitive answer. "Not found" is not an
/// error: it is [`ObservedListing::NotFound`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObserveError {
    #[error("no verification mechanism for {kind} sources")]
    NotSupported { kind: String },
    #[error("{url} timed out")]
    Timeout { url: String },
    #[error("{url} is unavailable: {detail}")]
    Unavailable {
        url: String,
        status: Option<u16>,
        detail: String,
    },
    #[error("{url} answered with something unreadable: {detail}")]
    Malformed { url: String, detail: String },
    #[error("request to {url} failed: {detail}")]
    Request { url: String, detail: String },
}

impl ObserveError {
    fn failure(&self) -> VerificationFailure {
        let (kind, url, http_status) = match self {
            Self::NotSupported { .. } => (FailureKind::NotSupported, None, None),
            Self::Timeout { url } => (FailureKind::Timeout, Some(url.clone()), None),
            Self::Unavailable { url, status, .. } => {
                (FailureKind::Unavailable, Some(url.clone()), *status)
            }
            Self::Malformed { url, .. } => (FailureKind::Malformed, Some(url.clone()), None),
            Self::Request { url, .. } => (FailureKind::Request, Some(url.clone()), None),
        };
        VerificationFailure {
            kind,
            detail: self.to_string(),
            url,
            http_status,
        }
    }

    fn listing(&self) -> ListingStatus {
        match self {
            Self::NotSupported { .. } => ListingStatus::Unknown,
            Self::Timeout { .. } | Self::Unavailable { .. } | Self::Request { .. } => {
                ListingStatus::Unreachable
            }
            Self::Malformed { .. } => ListingStatus::Ambiguous,
        }
    }
}

/// Asks a job's authoritative source about it. Implementations own the
/// source-specific part (endpoints, payloads) and must only read the one
/// job (or, where the source has no single-job endpoint, one listing).
#[async_trait]
pub trait ListingVerifier: Send + Sync {
    async fn observe(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError>;
}

/// Storage for verification records.
#[async_trait]
pub trait VerificationRepository: Send + Sync {
    /// Stores one attempt. Records are never updated or deleted.
    async fn save_verification(&self, record: &VerificationRecord) -> Result<(), StorageError>;

    /// Every attempt for a job, newest first.
    async fn verification_history(
        &self,
        job: JobId,
    ) -> Result<Vec<VerificationRecord>, StorageError>;

    /// The most recent attempt, successful or not.
    async fn latest_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError>;

    /// The most recent attempt that reached a definitive answer.
    async fn latest_successful_verification(
        &self,
        job: JobId,
    ) -> Result<Option<VerificationRecord>, StorageError>;

    /// Applies a posting the verifier found live through the job
    /// lifecycle, exactly as a scan that listed only this job would
    /// (UNCHANGED / UPDATED / REOPENED, with history), without closing
    /// anything and without recording a scan.
    async fn record_observation(
        &self,
        posting: &JobPosting,
        observed_at: DateTime<Utc>,
    ) -> Result<UpsertOutcome, StorageError>;
}

/// Whether to ask sources again when a recent attempt exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyMode {
    /// Reuse an attempt within [`FreshnessPolicy::reuse_within`].
    IfDue,
    /// Always ask.
    Force,
}

/// One record after verification.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordVerification {
    /// The stored record, as refreshed by the verification.
    pub record: JobRecord,
    pub latest: Option<VerificationRecord>,
    pub last_success: Option<VerificationRecord>,
    /// The latest attempt was reused rather than made now.
    pub reused: bool,
}

impl RecordVerification {
    pub fn trust(&self, policy: &FreshnessPolicy, now: DateTime<Utc>) -> RecordTrust {
        RecordTrust::of(
            &self.record,
            self.latest.as_ref(),
            self.last_success.as_ref(),
            policy,
            now,
        )
    }
}

/// The stored verification state of records, without asking anyone (for
/// displaying a job without fetching it).
pub async fn cached<R: VerificationRepository + ?Sized>(
    repository: &R,
    records: &[JobRecord],
) -> Result<Vec<RecordVerification>, StorageError> {
    let mut out = Vec::with_capacity(records.len());
    for record in records {
        out.push(RecordVerification {
            record: record.clone(),
            latest: repository.latest_verification(record.id).await?,
            last_success: repository.latest_successful_verification(record.id).await?,
            reused: true,
        });
    }
    Ok(out)
}

/// Verifies stored jobs against their sources and records the results.
pub struct VerificationService<'a, R: ?Sized> {
    repository: &'a R,
    verifier: &'a dyn ListingVerifier,
    policy: FreshnessPolicy,
    concurrency: usize,
}

impl<'a, R> VerificationService<'a, R>
where
    R: JobRepository + VerificationRepository + ?Sized,
{
    pub fn new(repository: &'a R, verifier: &'a dyn ListingVerifier) -> Self {
        Self {
            repository,
            verifier,
            policy: FreshnessPolicy::default(),
            concurrency: 4,
        }
    }

    pub fn with_policy(mut self, policy: FreshnessPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Records verified at the same time (each source's host limits apply
    /// on top).
    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    pub fn policy(&self) -> &FreshnessPolicy {
        &self.policy
    }

    /// Verifies every record (the source records of one opportunity, or
    /// one job), in order.
    pub async fn verify(
        &self,
        records: &[JobRecord],
        mode: VerifyMode,
        now: DateTime<Utc>,
    ) -> Result<Vec<RecordVerification>, StorageError> {
        let results: Vec<Result<RecordVerification, StorageError>> = futures::stream::iter(records)
            .map(|record| self.verify_one(record, mode, now))
            .buffered(self.concurrency)
            .collect()
            .await;
        results.into_iter().collect()
    }

    /// The stored verification state of records, without asking anyone.
    pub async fn cached(
        &self,
        records: &[JobRecord],
    ) -> Result<Vec<RecordVerification>, StorageError> {
        cached(self.repository, records).await
    }

    async fn verify_one(
        &self,
        record: &JobRecord,
        mode: VerifyMode,
        now: DateTime<Utc>,
    ) -> Result<RecordVerification, StorageError> {
        let latest = self.repository.latest_verification(record.id).await?;
        let prior_success = self
            .repository
            .latest_successful_verification(record.id)
            .await?;
        let decision = self
            .policy
            .reverify(latest.as_ref(), mode == VerifyMode::Force, now);
        if !decision.fetch() {
            debug!(job = %record.id, "reusing a recent verification");
            return Ok(RecordVerification {
                record: record.clone(),
                latest,
                last_success: prior_success,
                reused: true,
            });
        }

        let observed = self.verifier.observe(record).await;
        let (mut attempt, posting) = build_record(record, observed, prior_success.as_ref(), now);
        if let Some(posting) = &posting {
            let outcome = self.repository.record_observation(posting, now).await?;
            attempt.lifecycle = Some(outcome_name(outcome).to_owned());
        }
        self.repository.save_verification(&attempt).await?;
        info!(
            job = %record.id,
            source = %record.posting.provenance.source,
            listing = %attempt.listing,
            application = %attempt.application.status,
            "job verified"
        );
        let refreshed = self
            .repository
            .get(record.id)
            .await?
            .unwrap_or_else(|| record.clone());
        let last_success = if attempt.succeeded() {
            Some(attempt.clone())
        } else {
            prior_success
        };
        Ok(RecordVerification {
            record: refreshed,
            latest: Some(attempt),
            last_success,
            reused: false,
        })
    }
}

fn outcome_name(outcome: UpsertOutcome) -> &'static str {
    match outcome {
        UpsertOutcome::Inserted => "inserted",
        UpsertOutcome::Updated => "updated",
        UpsertOutcome::Unchanged => "unchanged",
        UpsertOutcome::Reopened => "reopened",
    }
}

fn published(posting: &JobPosting) -> PublishedPlace {
    let mut locations: Vec<String> = Vec::new();
    for text in posting
        .location
        .iter()
        .chain(posting.locations.iter().filter_map(|l| l.name.as_ref()))
    {
        if !locations.contains(text) {
            locations.push(text.clone());
        }
    }
    PublishedPlace {
        locations,
        workplace: posting
            .workplace_type
            .as_ref()
            .map(|w| w.as_str().to_owned()),
        is_remote: posting.is_remote,
        employment: posting
            .employment_type
            .as_ref()
            .map(|e| e.as_str().to_owned()),
        work_authorization: posting.work_authorization.clone(),
    }
}

/// Builds the record of one attempt, and the posting to feed back into the
/// lifecycle when the listing was found.
fn build_record(
    record: &JobRecord,
    observed: Result<SourceObservation, ObserveError>,
    prior_success: Option<&VerificationRecord>,
    now: DateTime<Utc>,
) -> (VerificationRecord, Option<JobPosting>) {
    let source = record.posting.provenance.source.clone();
    let base_authority = Authority::of_kind(source.kind());
    let mut out = VerificationRecord {
        id: VerificationId::derive(record.id, now),
        job_id: record.id,
        opportunity_id: record.opportunity_id,
        source,
        source_record_id: record.posting.provenance.source_record_id.clone(),
        attempted_at: now,
        method: VerificationMethod::Unsupported,
        listing: ListingStatus::Unknown,
        application: ApplicationCheck::unknown("not checked"),
        authority: base_authority,
        authority_chain: Vec::new(),
        checked_url: None,
        listing_url: None,
        content_fingerprint: None,
        changed_fields: Vec::new(),
        changed_since_last_verification: None,
        lifecycle: None,
        compensation: CompensationCheck::not_observed(),
        published: PublishedPlace::default(),
        unknowns: Vec::new(),
        failure: None,
        revision: VERIFICATION_REVISION.to_owned(),
    };
    let observation = match observed {
        Ok(o) => o,
        Err(error) => {
            out.listing = error.listing();
            out.failure = Some(error.failure());
            if let ObserveError::NotSupported { .. } = error {
                out.unknowns
                    .push("Narrow cannot verify listings from this source".into());
            } else {
                out.unknowns
                    .push("whether the listing is still open (the source did not answer)".into());
            }
            return (out, None);
        }
    };
    out.method = observation.method;
    out.checked_url = Some(observation.checked_url.clone());
    out.application = observation.application;
    out.unknowns = observation.unknowns;
    out.authority_chain = observation.chain;

    let posting = match observation.listing {
        ObservedListing::Found(posting) if posting.id() == record.id => Some(*posting),
        ObservedListing::Found(posting) => {
            out.listing = ListingStatus::Ambiguous;
            out.failure = Some(VerificationFailure {
                kind: FailureKind::Mismatch,
                detail: format!(
                    "the source answered with {:?} ({}), not this job",
                    posting.title,
                    posting
                        .provenance
                        .source_record_id
                        .as_deref()
                        .unwrap_or("no id")
                ),
                url: Some(observation.checked_url),
                http_status: None,
            });
            None
        }
        ObservedListing::NotFound { detail }
        | ObservedListing::MissingFromCompleteListing { detail } => {
            out.listing = ListingStatus::Closed;
            out.unknowns.retain(|u| !u.is_empty());
            out.failure = None;
            out.listing_url = Some(record.posting.url.clone());
            out.authority_chain.push(AuthorityLink {
                kind: LinkKind::AtsListingPage,
                url: record.posting.url.to_string(),
                checked: false,
                note: format!("no longer published: {detail}"),
            });
            None
        }
    };
    if let Some(posting) = &posting {
        out.listing = ListingStatus::Active;
        out.listing_url = Some(posting.url.clone());
        let fingerprint = posting.content_fingerprint().to_hex();
        out.changed_fields = record
            .posting
            .snapshot()
            .changed_fields(&posting.snapshot())
            .into_iter()
            .map(str::to_owned)
            .collect();
        out.changed_since_last_verification = prior_success
            .and_then(|p| p.content_fingerprint.as_ref())
            .map(|previous| *previous != fingerprint);
        out.content_fingerprint = Some(fingerprint);
        let previous = prior_success
            .filter(|p| p.listing == ListingStatus::Active)
            .map(|p| p.compensation.observed.as_ref());
        out.compensation = CompensationCheck::observe(posting.compensation.as_ref(), previous);
        out.published = published(posting);
        if !is_ats_host(&posting.url) && out.authority.is_employer() {
            out.authority_chain.push(AuthorityLink {
                kind: LinkKind::EmployerPage,
                url: posting.url.to_string(),
                checked: false,
                note: format!(
                    "the board publishes the employer's own page ({}) as the job's URL",
                    posting.url.host()
                ),
            });
        }
        // A page on the employer's own domain that JobHunt itself checked
        // raises the authority to first-party.
        if out.authority == Authority::EmployerConfiguredAts
            && out
                .authority_chain
                .iter()
                .any(|l| l.kind == LinkKind::EmployerPage && l.checked)
        {
            out.authority = Authority::EmployerFirstParty;
        }
    }
    (out, posting)
}

/// Hosts that belong to an ATS or platform rather than to the employer.
fn is_ats_host(url: &CanonicalUrl) -> bool {
    let host = url.host();
    [
        "greenhouse.io",
        "lever.co",
        "ashbyhq.com",
        "ycombinator.com",
        "workatastartup.com",
    ]
    .iter()
    .any(|ats| host == *ats || host.ends_with(&format!(".{ats}")))
}

#[cfg(test)]
mod tests;
