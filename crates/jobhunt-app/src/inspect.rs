//! One opportunity, in full: what `jobhunt show` / `why` print and the MCP
//! `get_job` / `verify_job` tools return.

use chrono::{DateTime, Utc};
use jobhunt_jobs::verification::VerificationAge;
use jobhunt_ranking::{OpportunityState, Ranking, RankingError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::resolve::{Opportunity, short_id};
use crate::verify::Checked;
use crate::views::{
    ApplicationState, CompensationView, DecisionView, EligibilityDetail, ListingState,
    PipelineStateView, SourceRecordView, VerificationBrief, time,
};

/// Characters of the description [`JobDetail`] includes unless asked for
/// all of it.
pub const DESCRIPTION_SUMMARY: usize = 1200;

/// Everything known about one opportunity.
#[derive(Debug, Clone)]
pub struct Inspection {
    pub opportunity: Opportunity,
    pub checked: Checked,
    /// The ranking and decision brief (`None` without a profile).
    pub ranking: Option<Ranking>,
    /// The ranking was read from the store rather than computed now.
    pub reused_ranking: bool,
    /// The person's state before this inspection.
    pub state: OpportunityState,
}

impl LocalApp {
    /// Verification, eligibility, ranking and pipeline state of one
    /// opportunity, from what is stored. With `mark_seen`, looking at it is
    /// recorded (once), as `jobhunt show` and `why` do.
    pub async fn inspect(
        &self,
        opportunity: &Opportunity,
        mark_seen: bool,
        now: DateTime<Utc>,
    ) -> Result<Inspection, AppError> {
        let checked = self.check(opportunity, now).await?;
        let ranking = self.ranking();
        let (ranked, reused) = match ranking.explain(&opportunity.records, now).await {
            Ok(explained) => (Some(explained.ranking), explained.reused),
            Err(RankingError::NoProfile) => (None, false),
            Err(other) => return Err(other.into()),
        };
        let state = ranking.state(&opportunity.records).await?;
        if mark_seen {
            self.exclusive(ranking.mark_seen(&opportunity.records, now))
                .await?;
        }
        Ok(Inspection {
            opportunity: opportunity.clone(),
            checked,
            ranking: ranked,
            reused_ranking: reused,
            state,
        })
    }
}

/// The answer of `get_job`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobDetail {
    /// `opp_…`: the logical opportunity.
    pub id: String,
    pub short_id: String,
    /// `job_…` of the record the details are read from.
    pub job_id: String,
    pub title: String,
    pub company: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_url: Option<String>,
    /// Every location the source lists, primary first.
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<String>,
    pub compensation: CompensationView,
    /// The description as text: the first part, unless the full text was
    /// requested.
    pub description: String,
    pub description_truncated: bool,
    pub verification: VerificationBrief,
    /// Eligibility with its reasons (`None` without a profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<EligibilityDetail>,
    /// The decision brief (`None` without a profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionView>,
    pub pipeline: PipelineStateView,
    /// How many sources list it.
    pub source_count: usize,
    /// Every source record (provenance and per-source verification), when
    /// requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceRecordView>>,
}

fn summarize(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_owned(), false);
    }
    let cut: String = text.chars().take(limit).collect();
    // End at a sentence or at least a word.
    let end = cut
        .rfind(". ")
        .map(|i| i + 1)
        .filter(|i| *i > limit / 2)
        .or_else(|| cut.rfind(char::is_whitespace))
        .unwrap_or(cut.len());
    (format!("{}…", cut[..end].trim_end()), true)
}

impl JobDetail {
    pub fn of(i: &Inspection, include_sources: bool, full_description: bool) -> Self {
        let record = i
            .checked
            .trust
            .best
            .and_then(|b| i.checked.verified.get(b))
            .map_or_else(|| i.opportunity.main(), |v| &v.record);
        let p = &record.posting;
        let (description, truncated) = summarize(
            p.description_text.as_deref().unwrap_or(""),
            if full_description {
                usize::MAX
            } else {
                DESCRIPTION_SUMMARY
            },
        );
        let mut locations: Vec<String> = p.location.iter().cloned().collect();
        for l in &p.locations {
            if let Some(name) = &l.name
                && !locations.contains(name)
            {
                locations.push(name.clone());
            }
        }
        Self {
            id: record.opportunity_id.to_string(),
            short_id: short_id(&record.opportunity_id),
            job_id: record.id.to_string(),
            title: p.title.clone(),
            company: p.company.clone(),
            url: p.url.to_string(),
            apply_url: p.apply_url.as_ref().map(ToString::to_string),
            locations,
            workplace: p.workplace_type.as_ref().map(|w| w.as_str().to_owned()),
            remote: p.is_remote,
            employment: p.employment_type.as_ref().map(|e| e.as_str().to_owned()),
            department: p.department.clone(),
            team: p.team.clone(),
            posted_at: p.posted_at.map(time),
            compensation: CompensationView::best(record, &i.checked.trust),
            description,
            description_truncated: truncated,
            verification: VerificationBrief::of(&i.checked.trust),
            eligibility: i
                .checked
                .assessment
                .as_ref()
                .map(|a| EligibilityDetail::of(&a.decision)),
            decision: i.ranking.as_ref().map(DecisionView::of),
            pipeline: PipelineStateView::of(&i.state),
            source_count: i.checked.verified.len(),
            sources: include_sources.then(|| {
                i.checked
                    .verified
                    .iter()
                    .zip(&i.checked.trust.records)
                    .map(|(v, t)| SourceRecordView::of(&v.record, t))
                    .collect()
            }),
        }
    }
}

/// The answer of `verify_job` (and `jobhunt verify --json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VerificationReport {
    /// `opp_…`.
    pub id: String,
    pub title: String,
    pub company: String,
    /// Records whose recent attempt (within the reuse window) was reused
    /// instead of asking the source again.
    pub reused: usize,
    /// The listing state of the record the opportunity rests on.
    pub listing: ListingState,
    pub application: ApplicationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_url: Option<String>,
    /// Who publishes the listing (`employer_configured_ats`, …).
    pub authority: String,
    pub verification: VerificationBrief,
    /// The latest attempt at the record the state rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    /// The latest attempt that reached an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    pub compensation: CompensationView,
    /// `None` without a profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<EligibilityDetail>,
    /// Trusted and the person appears able to take it.
    pub recommendable: bool,
    /// What is uncertain: unknowns from the attempts, failures, and
    /// disagreements between sources.
    pub uncertainty: Vec<String>,
    /// Every source record with its latest attempt.
    pub sources: Vec<SourceRecordView>,
}

impl VerificationReport {
    pub fn of(opportunity: &Opportunity, checked: &Checked) -> Self {
        let trust = &checked.trust;
        let best = trust.best();
        let record = trust
            .best
            .and_then(|b| checked.verified.get(b))
            .map_or_else(|| opportunity.main(), |v| &v.record);
        let latest = best.and_then(|b| b.latest.as_ref());
        let mut uncertainty: Vec<String> = Vec::new();
        for r in &trust.records {
            if let Some(v) = &r.latest {
                for u in &v.unknowns {
                    if !uncertainty.contains(u) {
                        uncertainty.push(u.clone());
                    }
                }
                if let Some(f) = &v.failure {
                    uncertainty.push(format!("{}: {}", r.source, f.detail));
                }
            }
        }
        uncertainty.extend(trust.conflicts.iter().cloned());
        if best.and_then(|b| b.age) == Some(VerificationAge::Stale) {
            uncertainty.push("The last successful verification is stale".into());
        }
        if let Some(a) = &checked.assessment {
            uncertainty.extend(a.decision.conflicts.iter().map(|c| c.summary.clone()));
        }
        Self {
            id: record.opportunity_id.to_string(),
            title: record.posting.title.clone(),
            company: record.posting.company.clone(),
            reused: checked.reused(),
            listing: latest.map_or(ListingState::Unknown, |v| v.listing.into()),
            application: latest.map_or(ApplicationState::Unknown, |v| v.application.status.into()),
            application_url: latest.and_then(|v| v.application.url.clone()),
            authority: best
                .map_or_else(|| "unknown".to_owned(), |b| b.authority.as_str().to_owned()),
            verification: VerificationBrief::of(trust),
            last_attempt_at: latest.map(|v| time(v.attempted_at)),
            last_success_at: best
                .and_then(|b| b.last_success.as_ref())
                .map(|v| time(v.attempted_at)),
            compensation: CompensationView::best(record, trust),
            eligibility: checked
                .assessment
                .as_ref()
                .map(|a| EligibilityDetail::of(&a.decision)),
            recommendable: checked
                .assessment
                .as_ref()
                .is_some_and(|a| a.recommendable()),
            uncertainty,
            sources: checked
                .verified
                .iter()
                .zip(&trust.records)
                .map(|(v, t)| SourceRecordView::of(&v.record, t))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_end_at_a_sentence_or_word() {
        let text = "First sentence here. Second sentence that goes on and on.";
        let (s, cut) = summarize(text, 30);
        assert!(cut);
        assert_eq!(s, "First sentence here.…");
        let (s, cut) = summarize("short", 30);
        assert_eq!((s.as_str(), cut), ("short", false));
        let (s, _) = summarize("wordwordword wordword", 15);
        assert_eq!(s, "wordwordword…");
    }
}
