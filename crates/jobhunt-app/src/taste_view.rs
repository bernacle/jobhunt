//! What JobHunt believes the person wants, in two parts kept apart:
//! what they said (preferences and statements, which always win) and what
//! it learned from their feedback (patterns, each with its evidence). The
//! answer of `narrow taste --json`'s equivalent, the MCP `get_taste` tool
//! and `GET /api/v1/taste`.
//!
//! Nothing is computed here: the patterns, their status and confidence are
//! [`jobhunt_ranking::taste`]'s.

use jobhunt_ranking::signals::describe_evidence;
use jobhunt_ranking::taste::{EvidenceKind, TasteEvidence};
use jobhunt_ranking::{LearnedTaste, TasteModel, TasteStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::controls::PreferenceControls;
use crate::error::AppError;
use crate::preferences::{PreferenceView, StatementView};
use crate::views::time;

/// One piece of feedback behind a learned pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteEvidenceView {
    /// `opp_…` the feedback was about.
    pub opportunity: String,
    pub title: String,
    pub company: String,
    /// `save`, `reject`, `applied`, `like`, …
    pub action: String,
    pub at: String,
    /// `reason` (the person's words) or `behavior` (what they did).
    pub kind: String,
    /// The reason, verbatim (reasons only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The fact of the job it counted for (behavior), or the part of the
    /// reason that was read.
    pub read_as: String,
    /// One line, for display.
    pub summary: String,
}

impl TasteEvidenceView {
    fn of(e: &TasteEvidence) -> Self {
        let (kind, reason, read_as) = match &e.kind {
            EvidenceKind::Reason {
                text,
                phrase,
                resolved,
            } => (
                "reason",
                Some(text.clone()),
                resolved.clone().unwrap_or_else(|| phrase.clone()),
            ),
            EvidenceKind::Behavior { what, fact } => ("behavior", None, format!("{what}: {fact}")),
        };
        Self {
            opportunity: e.opportunity.to_string(),
            title: e.title.clone(),
            company: e.company.clone(),
            action: e.action.as_str().to_owned(),
            at: time(e.at),
            kind: kind.to_owned(),
            reason,
            read_as,
            summary: describe_evidence(e),
        }
    }
}

/// A pattern learned from feedback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LearnedView {
    /// `role:sre`, `domain:developer_tools`, …
    pub key: String,
    /// `role`, `domain`, `company`, `technology`, …
    pub dimension: String,
    /// The value, readable: `SRE / DevOps`, `large companies`.
    pub value: String,
    /// `prefer` or `avoid`: the way the evidence leans.
    pub direction: String,
    /// `active` (used in ranking), `mixed` (contradictory, not used),
    /// `not_enough` (too little evidence yet) or `explicit` (the person
    /// said something about it, which is used instead).
    pub status: String,
    /// `tentative`, `established` or `strong` (active patterns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// What the person said about it (explicit patterns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stated: Option<String>,
    /// Whether the feedback agrees with what they said (explicit patterns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agrees: Option<bool>,
    /// "2 reasons in your words and 1 action without a reason".
    pub basis: String,
    /// Reasons in the person's words among the support.
    pub reasons: usize,
    /// Distinct jobs behind the support.
    pub opportunities: usize,
    pub last_reinforced: String,
    /// Evidence for it, newest first.
    pub support: Vec<TasteEvidenceView>,
    /// Evidence against it, newest first.
    pub against: Vec<TasteEvidenceView>,
}

impl LearnedView {
    fn of(l: &LearnedTaste) -> Self {
        let (status, confidence, stated, agrees) = match &l.status {
            TasteStatus::Active { confidence } => {
                ("active", Some(confidence.as_str().to_owned()), None, None)
            }
            TasteStatus::Mixed => ("mixed", None, None, None),
            TasteStatus::NotEnough => ("not_enough", None, None, None),
            TasteStatus::Explicit { preference, agrees } => {
                ("explicit", None, Some(preference.clone()), Some(*agrees))
            }
        };
        Self {
            key: l.key.to_string(),
            dimension: l.key.dimension.label().to_owned(),
            value: l.key.value_label(),
            direction: l.direction.as_str().to_owned(),
            status: status.to_owned(),
            confidence,
            stated,
            agrees,
            basis: l.basis(),
            reasons: l.reasons,
            opportunities: l.opportunities,
            last_reinforced: time(l.last_reinforced),
            support: l.support.iter().map(TasteEvidenceView::of).collect(),
            against: l.against.iter().map(TasteEvidenceView::of).collect(),
        }
    }
}

/// A reason about one job only (not generalized).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobNoteView {
    pub opportunity: String,
    /// The reason, verbatim.
    pub reason: String,
    /// What it was read as ("avoid: this domain").
    pub read_as: String,
    pub direction: String,
    pub action: String,
    pub at: String,
}

/// A reason nothing could be read from, kept as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnreadReasonView {
    pub opportunity: String,
    pub title: String,
    pub company: String,
    pub action: String,
    pub reason: String,
    pub at: String,
}

/// The answer of `get_taste`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteView {
    /// What the person said: preferences in effect (they always win over
    /// anything learned).
    pub stated: Vec<PreferenceView>,
    /// The same preferences as structured controls (work setup,
    /// relocation, location, pay, company and team), each in its layer.
    pub controls: PreferenceControls,
    /// The person's statements, verbatim, with what was not understood.
    pub statements: Vec<StatementView>,
    /// Patterns learned from feedback that affect ranking now.
    pub learned: Vec<LearnedView>,
    /// Patterns the evidence contradicts: not used.
    pub contradictory: Vec<LearnedView>,
    /// Patterns about something the person also stated: what they said is
    /// used.
    pub covered_by_stated: Vec<LearnedView>,
    /// Patterns with too little evidence to use yet.
    pub emerging: Vec<LearnedView>,
    /// Reasons about single jobs, not generalized.
    pub job_notes: Vec<JobNoteView>,
    /// Reasons nothing could be read from, kept as written.
    pub unread_reasons: Vec<UnreadReasonView>,
    /// Pieces of feedback (not counting "looked at").
    pub feedback_events: usize,
    /// Jobs with feedback.
    pub opportunities: usize,
}

impl TasteView {
    pub fn of(
        model: &TasteModel,
        stated: Vec<PreferenceView>,
        controls: PreferenceControls,
        statements: Vec<StatementView>,
    ) -> Self {
        let with = |status: &str| -> Vec<LearnedView> {
            model
                .learned
                .iter()
                .map(LearnedView::of)
                .filter(|l| l.status == status)
                .collect()
        };
        Self {
            stated,
            controls,
            statements,
            learned: with("active"),
            contradictory: with("mixed"),
            covered_by_stated: with("explicit"),
            emerging: with("not_enough"),
            job_notes: model
                .notes
                .iter()
                .map(|n| JobNoteView {
                    opportunity: n.opportunity.to_string(),
                    reason: n.reason.clone(),
                    read_as: n.key.value_label(),
                    direction: n.direction.as_str().to_owned(),
                    action: n.action.as_str().to_owned(),
                    at: time(n.at),
                })
                .collect(),
            unread_reasons: model
                .unread
                .iter()
                .map(|u| UnreadReasonView {
                    opportunity: u.opportunity.to_string(),
                    title: u.title.clone(),
                    company: u.company.clone(),
                    action: u.action.as_str().to_owned(),
                    reason: u.reason.clone(),
                    at: time(u.at),
                })
                .collect(),
            feedback_events: model.events,
            opportunities: model.opportunities,
        }
    }
}

impl LocalApp {
    /// What the person said and what JobHunt learned (see [`TasteView`]).
    pub async fn taste_view(&self) -> Result<TasteView, AppError> {
        let ranking = self.ranking();
        let (data, person) = ranking.person().await?;
        let data = data.ok_or(AppError::NoProfile)?;
        let model = ranking.taste(&person).await?;
        Ok(TasteView::of(
            &model,
            data.preferences
                .iter()
                .filter(|p| p.active)
                .map(PreferenceView::of)
                .collect(),
            PreferenceControls::of(&data),
            data.statements.iter().map(StatementView::of).collect(),
        ))
    }
}
