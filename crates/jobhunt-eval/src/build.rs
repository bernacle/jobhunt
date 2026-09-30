//! From fixtures to the canonical domain models production ranks: a
//! [`ProfileData`] built the way `narrow init` builds one (the resume read
//! by the production parser and folded in by the production import rules,
//! then the preferences set), stored [`JobRecord`]s, and a successful
//! verification for the listings Today would have verified.
//!
//! Nothing here reaches a network, a database or a clock: every time is
//! derived from [`now`].

use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, Provenance, SourceKey, StableId};
use jobhunt_eligibility::{Assessment, ProfileFacts, assess};
use jobhunt_jobs::verification::{
    ApplicationCheck, Authority, CompensationCheck, FreshnessPolicy, ListingStatus, PublishedPlace,
    RecordVerification, VERIFICATION_REVISION, VerificationId, VerificationMethod,
    VerificationRecord,
};
use jobhunt_jobs::{
    EmploymentType, JobPosting, JobRecord, JobStatus, OpportunityId, WorkplaceType,
};
use jobhunt_profile::taste::{
    TasteAssertion, TasteBrief, TasteConfidence, TasteOrigin, TasteReview, TasteSource, vocab,
};
use jobhunt_profile::{
    Certainty, Preference, PreferenceId, PreferenceOrigin, ProfileData, ProfileId, TasteId,
    merge_resume,
};
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};

use crate::fixture::{CandidateFixture, JobFixture};

/// The benchmark's fixed clock.
pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0)
        .single()
        .unwrap_or_default()
}

/// How long before [`now`] every job was first seen.
const FIRST_SEEN_DAYS: i64 = 2;
/// How long before [`now`] verified listings were verified.
const VERIFIED_HOURS: i64 = 6;

/// Why a fixture can't be turned into domain models.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("candidate {candidate}: could not read the resume: {message}")]
    Resume { candidate: String, message: String },
    #[error("job {job}: {message}")]
    Job { job: String, message: String },
}

/// A candidate's profile, built as production builds one.
pub fn profile(candidate: &CandidateFixture) -> Result<ProfileData, BuildError> {
    let at = now() - Duration::days(30);
    let id = ProfileId::derive(&["benchmark", &candidate.id]);
    let mut data = ProfileData::new(id, at);
    let file = ResumeFile::from_bytes(candidate.resume.as_bytes(), Some("resume.md".into()))
        .map_err(|e| BuildError::Resume {
            candidate: candidate.id.clone(),
            message: e.to_string(),
        })?;
    let parser = DeterministicParser;
    let parsed = parser.parse(&file.text);
    merge_resume(
        &mut data,
        file.source_document(parser.name(), at),
        &parsed,
        at,
    );
    for (i, p) in candidate.preference.iter().enumerate() {
        let key = p.value.key();
        data.preferences.push(Preference {
            id: PreferenceId::derive(&[&id.to_string(), "benchmark", &key, &i.to_string()]),
            value: p.value.clone(),
            stance: p.stance,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: at,
            updated_at: at,
        });
    }
    if let Some(taste) = &candidate.taste {
        data.taste_brief = Some(TasteBrief {
            id: TasteBrief::id_for(id),
            text: taste.looking_for.clone(),
            statement: None,
            interpretation: None,
            confirmed_at: Some(at),
            created_at: at,
            updated_at: at,
        });
        for s in &taste.statement {
            let key = jobhunt_profile::taste::key(s.dimension, &s.value);
            data.taste.push(TasteAssertion {
                id: TasteId::derive(&[&id.to_string(), "benchmark", &key]),
                dimension: s.dimension,
                value: s.value.clone(),
                polarity: s.polarity,
                text: s
                    .text
                    .clone()
                    .unwrap_or_else(|| vocab::sentence(s.dimension, &s.value, s.polarity)),
                confidence: TasteConfidence::High,
                origin: TasteOrigin::Stated,
                review: TasteReview::Confirmed,
                sources: vec![TasteSource::Person],
                explanation: None,
                interpreter: None,
                original: None,
                superseded_by: None,
                created_at: at,
                updated_at: at,
            });
        }
    }
    data.profile.revision = 1;
    Ok(data)
}

/// The stored record of a job fixture: open, first seen two days ago.
pub fn record(job: &JobFixture) -> Result<JobRecord, BuildError> {
    let bad = |message: String| BuildError::Job {
        job: job.id.clone(),
        message,
    };
    let source: SourceKey = job
        .source
        .parse()
        .map_err(|e| bad(format!("source: {e}")))?;
    let native = StableId::derive("jobhunt.eval.job", &[&job.id]).to_hex();
    let url = CanonicalUrl::parse(&format!(
        "https://jobs.example.com/{}/{native}",
        source.instance()
    ))
    .map_err(|e| bad(format!("url: {e}")))?;
    let posting = JobPosting {
        provenance: Provenance {
            source,
            source_record_id: Some(native),
            fetched_from: None,
        },
        url,
        apply_url: None,
        company: job.company.clone(),
        title: job.title.clone(),
        department: job.department.clone(),
        team: job.team.clone(),
        location: job.location.clone(),
        locations: Vec::new(),
        employment_type: job
            .employment
            .as_deref()
            .map(EmploymentType::from_canonical),
        workplace_type: job.workplace.as_deref().map(WorkplaceType::from_canonical),
        is_remote: job.remote,
        compensation: job.compensation.clone(),
        work_authorization: job.work_authorization.clone(),
        description_text: Some(job.description.trim().to_owned()),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    };
    let seen = now() - Duration::days(FIRST_SEEN_DAYS);
    Ok(JobRecord {
        id: posting.id(),
        opportunity_id: OpportunityId::founded_by(posting.id()),
        posting,
        first_seen_at: seen,
        last_seen_at: seen,
        content_updated_at: seen,
        status: JobStatus::Open,
        closed_at: None,
    })
}

/// A successful verification finding the listing active at its employer's
/// own board, as Today's verification step records one.
fn verification(record: &JobRecord) -> VerificationRecord {
    let at = now() - Duration::hours(VERIFIED_HOURS);
    VerificationRecord {
        id: VerificationId::derive(record.id, at),
        job_id: record.id,
        opportunity_id: record.opportunity_id,
        source: record.posting.provenance.source.clone(),
        source_record_id: record.posting.provenance.source_record_id.clone(),
        attempted_at: at,
        method: VerificationMethod::AshbyBoardApi,
        listing: ListingStatus::Active,
        application: ApplicationCheck::unknown("not checked by the benchmark"),
        authority: Authority::EmployerConfiguredAts,
        authority_chain: Vec::new(),
        checked_url: None,
        listing_url: None,
        content_fingerprint: None,
        changed_fields: Vec::new(),
        changed_since_last_verification: None,
        lifecycle: None,
        compensation: CompensationCheck::observe(record.posting.compensation.as_ref(), None),
        published: PublishedPlace::default(),
        unknowns: Vec::new(),
        failure: None,
        revision: VERIFICATION_REVISION.to_owned(),
    }
}

/// The eligibility assessment of a job for a person, with the job's
/// verification (when the fixture says it was verified), exactly as the
/// ranking service assesses it.
pub fn assessment(job: &JobFixture, record: &JobRecord, facts: &ProfileFacts) -> Assessment {
    let v = job.verified.then(|| verification(record));
    let rv = RecordVerification {
        record: record.clone(),
        latest: v.clone(),
        last_success: v,
        reused: true,
    };
    assess(&[rv], facts, &FreshnessPolicy::default(), now())
}
