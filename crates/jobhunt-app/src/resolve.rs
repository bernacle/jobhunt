//! Turning what someone typed (`opp_…`, `job_…`, or a unique prefix of
//! either) into a logical opportunity and its source records.

use jobhunt_jobs::{JobId, JobRecord, JobRepository, OpportunityId};
use jobhunt_storage::SqliteJobStore;

use crate::error::AppError;

/// Characters after `opp_` / `job_` a short id needs, so a stray prefix
/// never silently matches a random job.
pub const MIN_SHORT_ID: usize = 4;

/// Characters after the prefix shown in short ids (`opp_1a2b3c4d`).
pub const SHORT_ID: usize = 8;

/// One logical opportunity, with every source record that lists it.
#[derive(Debug, Clone)]
pub struct Opportunity {
    pub id: OpportunityId,
    /// Every source record; when a job id was given, that record first.
    pub records: Vec<JobRecord>,
}

impl Opportunity {
    /// The record the person asked about (or the first one).
    pub fn main(&self) -> &JobRecord {
        &self.records[0]
    }
}

/// `opp_1a2b3c4d` for display. [`LocalApp::resolve`](crate::LocalApp::resolve) accepts it back as long as it is
/// unique.
pub fn short_id(id: &impl ToString) -> String {
    let text = id.to_string();
    match text.split_once('_') {
        Some((prefix, hex)) if hex.len() > SHORT_ID => format!("{prefix}_{}", &hex[..SHORT_ID]),
        _ => text,
    }
}

enum Wanted {
    Job(JobId),
    Opportunity(OpportunityId),
}

pub(crate) async fn resolve(store: &SqliteJobStore, input: &str) -> Result<Opportunity, AppError> {
    let input = input.trim().to_ascii_lowercase();
    let wanted = if let Ok(job) = input.parse::<JobId>() {
        Wanted::Job(job)
    } else if let Ok(opportunity) = input.parse::<OpportunityId>() {
        Wanted::Opportunity(opportunity)
    } else {
        by_prefix(store, &input).await?
    };
    let records = match wanted {
        Wanted::Job(job) => match store.get(job).await? {
            Some(record) => {
                let mut members = store.opportunity_records(record.opportunity_id).await?;
                members.retain(|m| m.id != record.id);
                members.insert(0, record);
                members
            }
            None => Vec::new(),
        },
        Wanted::Opportunity(opportunity) => store.opportunity_records(opportunity).await?,
    };
    match records.first() {
        Some(first) => Ok(Opportunity {
            id: first.opportunity_id,
            records,
        }),
        None => Err(AppError::UnknownOpportunity { input }),
    }
}

async fn by_prefix(store: &SqliteJobStore, input: &str) -> Result<Wanted, AppError> {
    let (kind, hex) = input.split_once('_').unwrap_or(("", input));
    let valid =
        hex.len() >= MIN_SHORT_ID && hex.len() < 32 && hex.chars().all(|c| c.is_ascii_hexdigit());
    if !matches!(kind, "opp" | "job") || !valid {
        return Err(AppError::InvalidArguments(format!(
            "{input:?} is not an opportunity id (opp_…) or job id (job_…); a short id needs at \
             least {MIN_SHORT_ID} characters after the underscore"
        )));
    }
    // Two candidates are enough to know it is ambiguous; a few more make
    // the message useful.
    const SHOWN: usize = 5;
    if kind == "opp" {
        let found = store.opportunities_with_prefix(input, SHOWN).await?;
        match found.as_slice() {
            [] => Err(AppError::UnknownOpportunity {
                input: input.to_owned(),
            }),
            [one] => Ok(Wanted::Opportunity(*one)),
            many => Err(AppError::AmbiguousId {
                input: input.to_owned(),
                candidates: many.iter().map(ToString::to_string).collect(),
            }),
        }
    } else {
        let found = store.jobs_with_prefix(input, SHOWN).await?;
        match found.as_slice() {
            [] => Err(AppError::UnknownOpportunity {
                input: input.to_owned(),
            }),
            [one] => Ok(Wanted::Job(*one)),
            many => Err(AppError::AmbiguousId {
                input: input.to_owned(),
                candidates: many.iter().map(ToString::to_string).collect(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ids_keep_the_prefix() {
        assert_eq!(
            short_id(&"opp_02e51190085f8a9a0772e845ddd9f329"),
            "opp_02e51190"
        );
        assert_eq!(short_id(&"opp_02e5"), "opp_02e5");
    }
}
