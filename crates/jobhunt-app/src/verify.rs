//! Verification and eligibility of one opportunity, as `jobhunt verify`,
//! `check`, `show` and the MCP `verify_job` / `get_job` tools use them.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use jobhunt_eligibility::evaluate::trust;
use jobhunt_eligibility::profile::ProfileFacts;
use jobhunt_eligibility::{Assessment, cached_assess};
use jobhunt_jobs::verification::{
    OpportunityTrust, RecordVerification, VerificationService, VerifyMode, cached,
};
use jobhunt_profile::ProfileService;
use jobhunt_sources::{HttpClient, HttpVerifier, VerifierHosts};

use crate::error::AppError;
use crate::resolve::Opportunity;
use crate::{LocalApp, Progress, ProgressEvent};

/// Sends every verification request to this base URL instead of the real
/// hosts. For offline tests against a local server; not a user setting.
pub const VERIFY_ENDPOINT_OVERRIDE: &str = "JOBHUNT_VERIFY_ENDPOINT";

/// An opportunity's verification state, checked against the profile.
#[derive(Debug, Clone)]
pub struct Checked {
    pub verified: Vec<RecordVerification>,
    pub trust: OpportunityTrust,
    /// `None` without a profile.
    pub profile: Option<ProfileFacts>,
    /// `None` without a profile.
    pub assessment: Option<Assessment>,
}

impl Checked {
    /// How many records reused a recent attempt instead of asking again.
    pub fn reused(&self) -> usize {
        self.verified.iter().filter(|v| v.reused).count()
    }
}

/// The verifier that asks sources over HTTP, from configuration: the one
/// verifier, used by `verify`, `find` and the cloud's scheduled
/// re-verification alike.
pub fn http_verifier(config: &crate::AppConfig) -> Result<HttpVerifier, AppError> {
    let http = HttpClient::new(config.discovery.http_settings())
        .map_err(|e| AppError::VerificationUnavailable(format!("HTTP client: {e}")))?;
    Ok(match std::env::var(VERIFY_ENDPOINT_OVERRIDE) {
        Ok(base) if !base.trim().is_empty() => {
            HttpVerifier::with_hosts(http, VerifierHosts::all_at(&base))
        }
        _ => HttpVerifier::new(http),
    })
}

impl LocalApp {
    fn verifier(&self) -> Result<HttpVerifier, AppError> {
        http_verifier(self.config())
    }

    /// The profile as eligibility reads it, when there is one.
    pub async fn profile_facts(&self) -> Result<Option<ProfileFacts>, AppError> {
        let data = ProfileService::new(self.store()).load().await?;
        Ok(data.as_ref().map(ProfileFacts::from_profile))
    }

    /// Asks the opportunity's authoritative sources (reusing an attempt
    /// from the last few minutes unless `mode` is [`VerifyMode::Force`]),
    /// stores what they say, and checks the result against the profile.
    pub async fn verify(
        &self,
        opportunity: &Opportunity,
        mode: VerifyMode,
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<Checked, AppError> {
        let verified = self
            .verify_records(&opportunity.records, mode, progress, now)
            .await?;
        self.check_verified(verified, now).await
    }

    pub(crate) async fn verify_records(
        &self,
        records: &[jobhunt_jobs::JobRecord],
        mode: VerifyMode,
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<Vec<RecordVerification>, AppError> {
        let verifier = self.verifier()?;
        progress.note(ProgressEvent::Verifying {
            records: records.len(),
            sources: records
                .iter()
                .map(|r| r.posting.provenance.source.to_string())
                .collect::<BTreeSet<_>>()
                .len(),
        });
        let config = &self.config().verification;
        // Each attempt is its own row and each refreshed posting its own
        // transaction, so concurrent verifications need no lock: at worst
        // one opportunity gets two attempts in its history.
        VerificationService::new(self.store(), &verifier)
            .with_policy(config.policy())
            .with_concurrency(config.concurrency)
            .verify(records, mode, now)
            .await
            .map_err(|e| AppError::storage("saving the verification", e))
    }

    /// The stored verification state of an opportunity, checked against
    /// the profile, without asking anyone.
    pub async fn check(
        &self,
        opportunity: &Opportunity,
        now: DateTime<Utc>,
    ) -> Result<Checked, AppError> {
        let verified = cached(self.store(), &opportunity.records).await?;
        self.check_verified(verified, now).await
    }

    async fn check_verified(
        &self,
        verified: Vec<RecordVerification>,
        now: DateTime<Utc>,
    ) -> Result<Checked, AppError> {
        let policy = self.policy();
        let profile = self.profile_facts().await?;
        let (assessment, trust) = match &profile {
            Some(p) => {
                let (a, _) = cached_assess(self.store(), &verified, p, &policy, now)
                    .await
                    .map_err(|e| AppError::storage("storing the eligibility decision", e))?;
                let trust = a.trust.clone();
                (Some(a), trust)
            }
            None => (None, trust(&verified, &policy, now)),
        };
        Ok(Checked {
            verified,
            trust,
            profile,
            assessment,
        })
    }
}
