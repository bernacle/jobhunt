//! Changing the profile from a client that is not the `narrow` command
//! (the web app, over the HTTP API): importing a resume from its bytes,
//! and reviewing the claims JobHunt could not settle on its own.
//!
//! The rules are the profile domain's: a re-import merges into the
//! profile without undoing the person's edits, confirmations or
//! rejections ([`jobhunt_profile::merge_resume`]); a claim is usable only
//! as the evidence policy says. Nothing here creates a claim.

use chrono::{DateTime, Utc};
use jobhunt_profile::{ImportReport, Tally, Verification};
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::profile_view::{ProfileView, UnresolvedClaim};

/// Counts of one kind of record in an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TallyView {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// No longer in the resume (kept, marked stale).
    pub stale: usize,
    /// Back in the resume after being stale.
    pub restored: usize,
    /// Already in the profile from another source (or entered by the
    /// person): this source was added as evidence, not as a duplicate.
    #[serde(default)]
    pub corroborated: usize,
}

impl From<&Tally> for TallyView {
    fn from(t: &Tally) -> Self {
        Self {
            added: t.added,
            updated: t.updated,
            unchanged: t.unchanged,
            stale: t.stale,
            restored: t.restored,
            corroborated: t.corroborated,
        }
    }
}

/// What importing a resume changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResumeImportResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// The profile had no resume before.
    pub first_import: bool,
    /// This exact file was imported before (nothing new to read).
    pub same_file: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    pub experiences: TallyView,
    pub projects: TallyView,
    pub education: TallyView,
    pub skills: TallyView,
    pub claims: TallyView,
    /// Confirmed claims found again (still confirmed).
    pub kept_confirmed: usize,
    /// Rejected claims found again (still rejected).
    pub kept_rejected: usize,
    /// Confirmed claims whose wording changed: they need confirming again.
    pub reconfirm: usize,
    /// Confirmed claims whose source left the resume.
    pub stale_confirmed: usize,
    /// Records where the person's own edits were kept over the resume.
    pub preserved_edits: usize,
    /// Doubts about particular records ("Freelance: no dates found").
    pub notes: Vec<String>,
    /// Lines of the resume that were not understood.
    pub ignored_lines: usize,
    /// Claims now waiting for the person's review.
    pub needs_review: usize,
    /// The profile after the import.
    pub profile: ProfileView,
}

impl ResumeImportResult {
    fn of(file: &ResumeFile, report: &ImportReport, profile: ProfileView) -> Self {
        Self {
            file_name: file.file_name.clone(),
            first_import: report.first_import,
            same_file: report.same_file,
            pages: Some(file.text.pages).filter(|p| *p > 0),
            experiences: (&report.experiences).into(),
            projects: (&report.projects).into(),
            education: (&report.education).into(),
            skills: (&report.skills).into(),
            claims: (&report.claims).into(),
            kept_confirmed: report.kept_confirmed,
            kept_rejected: report.kept_rejected,
            reconfirm: report.reconfirm,
            stale_confirmed: report.stale_confirmed,
            preserved_edits: report.preserved_edits,
            notes: report.notes.clone(),
            ignored_lines: report.ignored.len(),
            needs_review: profile.needs_review_total,
            profile,
        }
    }
}

/// The claims waiting for the person's decision, with the words behind
/// each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimReview {
    pub claims: Vec<UnresolvedClaim>,
    pub total: usize,
}

/// A decision about claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClaimDecision {
    /// True: it may be used as evidence.
    Confirm,
    /// Not true: never used, and stays rejected across re-imports.
    Reject,
    /// Back to unreviewed.
    Reset,
}

/// One claim after a decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecidedClaim {
    pub id: String,
    pub text: String,
    /// `confirmed`, `rejected`, `extracted`, `inferred`, …
    pub state: String,
    /// Whether it may now be used as evidence.
    pub usable: bool,
}

/// The answer of a claim decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimDecisionResult {
    pub decided: Vec<DecidedClaim>,
    /// Claims still waiting for review.
    pub needs_review_total: usize,
}

impl LocalApp {
    /// Imports (or re-imports) a resume from its bytes. `file_name` decides
    /// how it is read (`.pdf`, `.txt`, `.md`).
    pub async fn import_resume(
        &self,
        bytes: &[u8],
        file_name: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<ResumeImportResult, AppError> {
        let file = ResumeFile::from_bytes(bytes, file_name)
            .map_err(|e| AppError::InvalidArguments(format!("could not read the resume: {e}")))?;
        let parser = DeterministicParser;
        let parsed = parser.parse(&file.text);
        let (report, data) = self
            .exclusive(async {
                Ok(self
                    .profiles()
                    .import_resume(file.source_document(parser.name(), now), &parsed, now)
                    .await?)
            })
            .await?;
        Ok(ResumeImportResult::of(
            &file,
            &report,
            ProfileView::of(&data),
        ))
    }

    /// Every claim waiting for review (or the first `limit`).
    pub async fn claims_for_review(&self, limit: Option<usize>) -> Result<ClaimReview, AppError> {
        let data = self.profiles().require().await?;
        let queue = data.review_queue();
        Ok(ClaimReview {
            total: queue.len(),
            claims: queue
                .iter()
                .take(limit.unwrap_or(usize::MAX))
                .map(|c| UnresolvedClaim::of(&data, c))
                .collect(),
        })
    }

    /// Confirms, rejects or resets claims (`clm_…` ids). `note` is kept on
    /// them verbatim (why they were rejected, typically).
    pub async fn decide_claims(
        &self,
        ids: &[String],
        decision: ClaimDecision,
        note: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<ClaimDecisionResult, AppError> {
        if ids.is_empty() {
            return Err(AppError::InvalidArguments(
                "give at least one claim id (clm_…)".into(),
            ));
        }
        let verification = match decision {
            ClaimDecision::Confirm => Verification::Confirmed,
            ClaimDecision::Reject => Verification::Rejected,
            ClaimDecision::Reset => Verification::Unverified,
        };
        let note = note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        let changed = self
            .exclusive(async {
                Ok(self
                    .profiles()
                    .decide_claims(ids, verification, note, now)
                    .await?)
            })
            .await?;
        let data = self.profiles().require().await?;
        Ok(ClaimDecisionResult {
            decided: changed
                .iter()
                .map(|c| {
                    let current = data.claim(c.id).unwrap_or(c);
                    DecidedClaim {
                        id: c.id.to_string(),
                        text: current.text.clone(),
                        state: current.state_label().to_owned(),
                        usable: data.standing(current).is_usable(),
                    }
                })
                .collect(),
            needs_review_total: data.review_queue().len(),
        })
    }
}
