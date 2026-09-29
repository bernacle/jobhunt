//! Feedback on an opportunity: save, reject, applied, like, …
//!
//! Recording goes through [`jobhunt_ranking::RankingService::record`],
//! the one place feedback is stored, so `narrow reject <id> --reason "…"`
//! and the MCP `reject_job` tool produce the same event, the same state and
//! the same learned taste. A repeat of an action whose effect is already in
//! place (saving a saved job, the same rejection with the same reason) is
//! not stored again, so a client retrying a request can't pile up
//! duplicate feedback.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use jobhunt_ranking::reason::{ReadSignal, Scope, Target};
use jobhunt_ranking::{FeedbackAction, Recorded, Stage, TasteModel};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::resolve::Opportunity;
use crate::views::PipelineStateView;

/// A reason, read: "avoid company: large companies".
pub fn describe_signal(signal: &ReadSignal) -> String {
    let what = match &signal.target {
        Target::Key { key } => key.label(),
        Target::Job { reference } => reference.label().to_owned(),
    };
    let scope = match signal.scope {
        Scope::General => "",
        Scope::ThisOpportunity => " (this job only)",
    };
    format!("{} {what}{scope}", signal.direction.as_str())
}

fn active_patterns(model: &TasteModel) -> BTreeSet<String> {
    model
        .active()
        .filter_map(|l| {
            l.effect().map(|(direction, confidence)| {
                format!(
                    "{} {} ({})",
                    direction.as_str(),
                    l.key.label(),
                    confidence.as_str()
                )
            })
        })
        .collect()
}

/// What recording feedback did, including what it taught.
#[derive(Debug, Clone)]
pub struct FeedbackOutcome {
    pub recorded: Recorded,
    /// Patterns affecting ranking now that didn't before (with their
    /// confidence).
    pub learned: Vec<String>,
    /// Patterns that affected ranking before and no longer do (or changed
    /// confidence).
    pub unlearned: Vec<String>,
}

impl FeedbackOutcome {
    pub fn taste_changed(&self) -> bool {
        !self.learned.is_empty() || !self.unlearned.is_empty()
    }
}

impl LocalApp {
    /// Records an action on an opportunity (its records, the one the person
    /// looked at first). `reason` is kept verbatim.
    pub async fn record_feedback(
        &self,
        opportunity: &Opportunity,
        action: FeedbackAction,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<FeedbackOutcome, AppError> {
        self.exclusive(async {
            let ranking = self.ranking();
            let (_, person) = ranking.person().await?;
            let before = active_patterns(&ranking.taste(&person).await?);
            let recorded = ranking
                .record(&opportunity.records, action, reason, now)
                .await?;
            let after = if recorded.recorded {
                active_patterns(&ranking.taste(&person).await?)
            } else {
                before.clone()
            };
            Ok(FeedbackOutcome {
                learned: after.difference(&before).cloned().collect(),
                unlearned: before.difference(&after).cloned().collect(),
                recorded,
            })
        })
        .await
    }
}

/// How a reason was read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReasonInterpretation {
    /// The reason, verbatim (it is always kept as written).
    pub reason: String,
    /// Something in it was recognized.
    pub understood: bool,
    /// What was recognized ("avoid company: large companies").
    pub read_as: Vec<String>,
    /// Which reader read it (`rules/1`).
    pub reader: String,
}

/// The answer of `save_job`, `reject_job`, `mark_applied` and
/// `record_feedback`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedbackResult {
    /// `opp_…`.
    pub id: String,
    pub title: String,
    pub company: String,
    /// `save`, `reject`, `applied`, …
    pub action: String,
    /// Whether a new feedback event was stored. `false` means the request
    /// repeated one already in effect (same action, same reason): nothing
    /// changed, and retrying is safe.
    pub recorded: bool,
    /// `fb_…` of the stored event (or the earlier one this repeats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<ReasonInterpretation>,
    /// The state before this request.
    pub previous: PipelineStateView,
    /// The state now.
    pub state: PipelineStateView,
    /// Whether what JobHunt learned from feedback changed.
    pub taste_changed: bool,
    /// Patterns that now affect ranking.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub learned: Vec<String>,
    /// Patterns that no longer do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlearned: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl FeedbackResult {
    pub fn of(outcome: &FeedbackOutcome) -> Self {
        let r = &outcome.recorded;
        let e = &r.event;
        let note = if !r.recorded {
            Some("Already recorded: nothing changed.".to_owned())
        } else if r.after.stage == Stage::Rejected {
            Some(format!(
                "It won't be recommended again. To undo, save it: narrow save {}",
                e.opportunity
            ))
        } else {
            None
        };
        Self {
            id: e.opportunity.to_string(),
            title: e.title.clone(),
            company: e.company.clone(),
            action: e.action.as_str().to_owned(),
            recorded: r.recorded,
            feedback_id: Some(e.id.to_string()),
            interpretation: match (&e.reason, &r.reading) {
                (Some(reason), Some(reading)) => Some(ReasonInterpretation {
                    reason: reason.clone(),
                    understood: !reading.is_unread(),
                    read_as: reading.signals.iter().map(describe_signal).collect(),
                    reader: reading.reader.clone(),
                }),
                _ => None,
            },
            previous: PipelineStateView::of(&r.before),
            state: PipelineStateView::of(&r.after),
            taste_changed: outcome.taste_changed(),
            learned: outcome.learned.clone(),
            unlearned: outcome.unlearned.clone(),
            note,
        }
    }
}

/// One opportunity in the pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PipelineEntryView {
    /// `opp_…`.
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub company: String,
    pub stage: crate::views::PipelineStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<crate::views::SentimentView>,
    /// When the current stage was reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Every source record of it is closed.
    pub listing_closed: bool,
    /// The latest reason the person gave, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reason: Option<String>,
}

/// The answer of `get_pipeline`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PipelineView {
    /// Offers first, then interviewing, applied, saved (and rejected, when
    /// asked for); latest first within a stage.
    pub entries: Vec<PipelineEntryView>,
}

impl PipelineView {
    pub fn of(entries: &[jobhunt_ranking::PipelineEntry]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|e| PipelineEntryView {
                    id: e.opportunity.to_string(),
                    short_id: crate::short_id(&e.opportunity),
                    title: e.title.clone(),
                    company: e.company.clone(),
                    stage: e.state.stage.into(),
                    sentiment: e.state.sentiment.map(Into::into),
                    since: e.state.since().map(crate::views::time),
                    listing_closed: e.closed,
                    last_reason: e.state.events.iter().rev().find_map(|ev| ev.reason.clone()),
                })
                .collect(),
        }
    }
}

impl LocalApp {
    /// Opportunities the person saved, applied to, is interviewing for or
    /// got an offer from (and, with `include_rejected`, rejected ones).
    pub async fn pipeline(
        &self,
        include_rejected: bool,
    ) -> Result<Vec<jobhunt_ranking::PipelineEntry>, AppError> {
        Ok(self.ranking().pipeline(include_rejected).await?)
    }
}
