//! The person's portable state: `narrow export` and `narrow import`.
//!
//! A state file holds what belongs to the person and cannot be rebuilt:
//!
//! * the whole profile, as the existing versioned profile format
//!   ([`ProfileExport`], `jobhunt.profile`): experiences, projects,
//!   education, skills, every claim with its evidence and the person's
//!   decision about it, preferences and statements in their own words;
//! * every piece of feedback, verbatim (save, reject, applied, interview,
//!   offer, like, dislike, unsave, with reasons). The pipeline stage of
//!   every opportunity is folded from it, and learned taste is derived
//!   from it, so neither is stored separately and both come back exactly
//!   on import. "Looked at" marks are left out: they are bookkeeping;
//! * the source records those feedback events are about (their postings
//!   and lifecycle dates, no verification history), so the pipeline
//!   survives on a machine that has not discovered those jobs yet. Job
//!   ids are stable across machines, so the next discovery run updates
//!   them in place.
//!
//! Everything else is rebuildable and left out: the rest of the
//! discovered jobs, discovery runs and HTTP validators, verification
//! history, eligibility decisions and stored rankings.
//!
//! Import is all or nothing: the file is parsed strictly and validated,
//! then written in one transaction ([`jobhunt_storage::SqliteJobStore::import_state`]).
//! Feedback and jobs already present are left alone, so importing the same
//! file twice changes nothing. Replacing an existing profile needs an
//! explicit `replace_profile`.
//!
//! Version history:
//!
//! * `1`: first version.

use std::collections::{BTreeSet, HashSet};

use chrono::{DateTime, Utc};
use jobhunt_jobs::{JobRecord, OpportunityId};
use jobhunt_profile::{ProfileEvent, ProfileEventKind, ProfileExport};
use jobhunt_ranking::{FeedbackAction, FeedbackEvent};
use jobhunt_storage::{ProfileWrite, StateImport, StateImported};
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;

pub const STATE_FORMAT: &str = "jobhunt.state";
pub const STATE_VERSION: u32 = 1;

/// A state file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateExport {
    pub format: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    /// The program that wrote the file (`narrow 0.1.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    /// The profile, in the profile export format (absent when there is no
    /// profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileExport>,
    /// Feedback, oldest first.
    #[serde(default)]
    pub feedback: Vec<FeedbackEvent>,
    /// The source records the feedback is about.
    #[serde(default)]
    pub jobs: Vec<JobRecord>,
}

/// What an import did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Imported {
    pub stored: StateImported,
    /// Opportunities with feedback in the file.
    pub opportunities: usize,
}

impl StateExport {
    /// Parses and validates a state file. The format name and version are
    /// checked first, so an unsupported version gets a clear message.
    pub fn parse(json: &str) -> Result<Self, AppError> {
        let invalid = |m: String| AppError::InvalidImport(m);
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| invalid(format!("not a JSON document: {e}")))?;
        let format = value.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != STATE_FORMAT {
            let hint = if format == jobhunt_profile::EXPORT_FORMAT {
                " (this is a profile file: import it with `narrow profile import`)"
            } else {
                ""
            };
            return Err(invalid(format!(
                "not a JobHunt state file (format is {format:?}, expected {STATE_FORMAT:?}){hint}"
            )));
        }
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(v) if v == u64::from(STATE_VERSION) => {}
            other => {
                return Err(invalid(format!(
                    "state file version {} is not supported (this JobHunt reads version \
                     {STATE_VERSION}); update JobHunt",
                    other.map_or_else(|| "missing".to_owned(), |v| v.to_string())
                )));
            }
        }
        let export: Self = serde_json::from_value(value)
            .map_err(|e| invalid(format!("invalid state file: {e}")))?;
        export.validate()?;
        Ok(export)
    }

    /// Internal consistency: the embedded profile is valid, ids are unique,
    /// every job's id matches its posting, and every feedback event is
    /// about a job in the file.
    pub fn validate(&self) -> Result<(), AppError> {
        let mut problems = Vec::new();
        if let Some(profile) = &self.profile
            && let Err(e) = profile.validate()
        {
            problems.push(format!("profile: {e}"));
        }
        let mut jobs = HashSet::new();
        for job in &self.jobs {
            if !jobs.insert(job.id) {
                problems.push(format!("job {} appears twice", job.id));
            }
            if job.posting.id() != job.id {
                problems.push(format!(
                    "job {} does not match its source and source id",
                    job.id
                ));
            }
        }
        let mut events = HashSet::new();
        for e in &self.feedback {
            if !events.insert(e.id) {
                problems.push(format!("feedback {} appears twice", e.id));
            }
            if !jobs.contains(&e.job) {
                problems.push(format!(
                    "feedback {} is about job {} which is not in the file",
                    e.id, e.job
                ));
            }
            if e.action == FeedbackAction::Seen {
                problems.push(format!("feedback {} is a \"seen\" mark", e.id));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(AppError::InvalidImport(format!(
                "invalid state file:\n  - {}",
                problems.join("\n  - ")
            )))
        }
    }

    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| AppError::storage("encoding the state file", e))
    }
}

impl LocalApp {
    /// Everything the person owns (see the module docs).
    pub async fn export_state(
        &self,
        now: DateTime<Utc>,
        generator: Option<String>,
    ) -> Result<StateExport, AppError> {
        let profile = self
            .profiles()
            .load()
            .await?
            .map(|data| ProfileExport::from_data(&data, now, generator.clone()));
        let feedback: Vec<FeedbackEvent> = self
            .ranking()
            .feedback()
            .await?
            .into_iter()
            .filter(|e| e.action != FeedbackAction::Seen)
            .collect();
        // Every record of every opportunity with feedback, plus each record
        // an event names (it may have left that opportunity since).
        let mut opportunities: BTreeSet<OpportunityId> = BTreeSet::new();
        for e in &feedback {
            opportunities.insert(e.opportunity);
            if let Some(r) = self.store().get(e.job).await? {
                opportunities.insert(r.opportunity_id);
            }
        }
        let mut jobs: Vec<JobRecord> = Vec::new();
        let mut seen = HashSet::new();
        for o in opportunities {
            for r in self.store().opportunity_records(o).await? {
                if seen.insert(r.id) {
                    jobs.push(r);
                }
            }
        }
        for e in &feedback {
            if !seen.contains(&e.job)
                && let Some(r) = self.store().get(e.job).await?
            {
                seen.insert(r.id);
                jobs.push(r);
            }
        }
        Ok(StateExport {
            format: STATE_FORMAT.to_owned(),
            version: STATE_VERSION,
            exported_at: now,
            generator,
            profile,
            feedback,
            jobs,
        })
    }

    /// Imports a validated state file in one transaction. An existing
    /// profile is replaced only with `replace_profile`.
    pub async fn import_state(
        &self,
        export: StateExport,
        replace_profile: bool,
        now: DateTime<Utc>,
    ) -> Result<Imported, AppError> {
        export.validate()?;
        self.exclusive(async {
            let profiles = self.profiles();
            let current = profiles.load().await?;
            let profile_id = profiles.profile_id();
            if export.profile.is_some() && current.is_some() && !replace_profile {
                return Err(AppError::InvalidImport(
                    "a profile already exists; importing would replace it. Export it first \
                     (narrow export) and import again with --replace"
                        .into(),
                ));
            }
            let data = export.profile.clone().map(|p| {
                let mut data = p.into_data(profile_id);
                data.profile.revision = current.as_ref().map_or(0, |d| d.profile.revision) + 1;
                data.profile.updated_at = now;
                data
            });
            let events = data
                .as_ref()
                .map(|d| {
                    vec![ProfileEvent::new(
                        now,
                        ProfileEventKind::ProfileImported,
                        None,
                        format!(
                            "state file: {} experiences, {} claims, {} preferences",
                            d.experiences.len(),
                            d.claims.len(),
                            d.preferences.len()
                        ),
                    )]
                })
                .unwrap_or_default();
            let local = profile_id.to_string();
            let feedback: Vec<FeedbackEvent> = export
                .feedback
                .iter()
                .cloned()
                .map(|mut e| {
                    e.profile_id = local.clone();
                    e
                })
                .collect();
            let opportunities = feedback
                .iter()
                .map(|e| e.opportunity)
                .collect::<HashSet<_>>()
                .len();
            let stored = self
                .store()
                .import_state(StateImport {
                    profile: data.as_ref().map(|d| ProfileWrite {
                        data: d,
                        expected_revision: current.as_ref().map_or(0, |c| c.profile.revision),
                        events: &events,
                    }),
                    jobs: &export.jobs,
                    feedback: &feedback,
                })
                .await?;
            Ok(Imported {
                stored,
                opportunities,
            })
        })
        .await
    }
}
