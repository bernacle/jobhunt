//! Importing a person's portable state (`narrow import`) in one
//! transaction: their profile, the jobs their feedback is about, and the
//! feedback itself. Either all of it is stored or none of it is.

use jobhunt_jobs::JobRecord;
use jobhunt_profile::{ProfileData, ProfileEvent, StorageError};
use jobhunt_ranking::FeedbackEvent;

use crate::profile::save_profile_in;
use crate::ranking::insert_feedback;
use crate::sqlite::{SqliteJobStore, import_job};

/// A profile to store, replacing the current one.
#[derive(Debug, Clone, Copy)]
pub struct ProfileWrite<'a> {
    pub data: &'a ProfileData,
    /// The revision the stored profile must still have (0 when there is
    /// none); anything else is a conflict and nothing is written.
    pub expected_revision: u64,
    pub events: &'a [ProfileEvent],
}

/// Everything one import writes.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateImport<'a> {
    pub profile: Option<ProfileWrite<'a>>,
    /// Jobs the feedback refers to; ones already stored are left alone.
    pub jobs: &'a [JobRecord],
    /// Feedback events; ones already stored (same id) are left alone.
    pub feedback: &'a [FeedbackEvent],
}

/// What an import changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateImported {
    pub profile: bool,
    pub jobs_added: usize,
    pub jobs_present: usize,
    pub feedback_added: usize,
    pub feedback_present: usize,
}

impl SqliteJobStore {
    /// Stores an imported state atomically.
    pub async fn import_state(
        &self,
        state: StateImport<'_>,
    ) -> Result<StateImported, StorageError> {
        let query_error = |operation: &'static str| {
            move |source: sqlx::Error| StorageError::Query {
                operation,
                source: Box::new(source),
            }
        };
        let mut tx = self
            .begin_write()
            .await
            .map_err(query_error("starting an import"))?;
        let mut out = StateImported::default();
        if let Some(p) = state.profile {
            save_profile_in(&mut tx, p.data, p.expected_revision, p.events).await?;
            out.profile = true;
        }
        // Job and feedback failures are reported in the profile domain's
        // error type, which is what import callers already handle.
        let wrap = |operation: &'static str| {
            move |e: jobhunt_jobs::StorageError| StorageError::Query {
                operation,
                source: Box::new(e),
            }
        };
        for record in state.jobs {
            if import_job(&mut tx, record)
                .await
                .map_err(wrap("importing a job"))?
            {
                out.jobs_added += 1;
            } else {
                out.jobs_present += 1;
            }
        }
        for event in state.feedback {
            if insert_feedback(&mut tx, event, true)
                .await
                .map_err(wrap("importing feedback"))?
            {
                out.feedback_added += 1;
            } else {
                out.feedback_present += 1;
            }
        }
        tx.commit()
            .await
            .map_err(query_error("committing an import"))?;
        Ok(out)
    }
}
