//! What the person did with an opportunity, and why.
//!
//! Every action is a [`FeedbackEvent`], stored once and never changed: the
//! opportunity it was about, the source record the person was looking at,
//! the action, the reason in their own words (verbatim; normalization never
//! replaces it) and when. The state of an opportunity
//! ([`OpportunityState`]) is folded from its events, so the history behind
//! "rejected" or "applied" is always there.
//!
//! Feedback belongs to the logical opportunity (`opp_…`), not to one
//! source's record: a job listed on two boards is saved, rejected or
//! applied to once. Events also keep the record id, so if identity grouping
//! later merges opportunities the feedback follows its record.
//!
//! State has two independent parts:
//!
//! * the pipeline [`Stage`]: unseen, seen, saved, rejected, applied,
//!   interviewing, offer;
//! * a [`Sentiment`]: liked or disliked. Liking a job is not a step toward
//!   applying, and disliking one is not rejecting it, so they are kept
//!   apart rather than forced into one enum.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Timelike, Utc};
use jobhunt_core::{ParseIdError, StableId};
use jobhunt_jobs::{JobId, OpportunityId};
use serde::{Deserialize, Serialize};

/// Identifier of a feedback event, `fb_<32 hex>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeedbackId(StableId);

impl FeedbackId {
    const PREFIX: &'static str = "fb_";

    pub fn derive(
        profile: &str,
        job: JobId,
        action: FeedbackAction,
        at: DateTime<Utc>,
        reason: Option<&str>,
    ) -> Self {
        let at = at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let job = job.to_string();
        Self(StableId::derive(
            "jobhunt.feedback.v1",
            &[
                profile,
                &job,
                action.as_str(),
                &at,
                reason.unwrap_or_default(),
            ],
        ))
    }
}

impl fmt::Display for FeedbackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::PREFIX, self.0)
    }
}

impl fmt::Debug for FeedbackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FeedbackId({self})")
    }
}

impl FromStr for FeedbackId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| ParseIdError(s.to_owned()))?;
        hex.parse()
            .map(Self)
            .map_err(|_| ParseIdError(s.to_owned()))
    }
}

impl Serialize for FeedbackId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FeedbackId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What the person did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackAction {
    /// Looked at it (recorded by `show` and `why`).
    Seen,
    Save,
    /// Took it off the saved list, without rejecting it.
    Unsave,
    /// Not interested.
    Reject,
    Applied,
    Interview,
    Offer,
    Like,
    Dislike,
}

impl FeedbackAction {
    pub const ALL: [FeedbackAction; 9] = [
        Self::Seen,
        Self::Save,
        Self::Unsave,
        Self::Reject,
        Self::Applied,
        Self::Interview,
        Self::Offer,
        Self::Like,
        Self::Dislike,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seen => "seen",
            Self::Save => "save",
            Self::Unsave => "unsave",
            Self::Reject => "reject",
            Self::Applied => "applied",
            Self::Interview => "interview",
            Self::Offer => "offer",
            Self::Like => "like",
            Self::Dislike => "dislike",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == value)
    }

    /// Past tense, as shown: "rejected", "applied".
    pub fn past(self) -> &'static str {
        match self {
            Self::Seen => "looked at",
            Self::Save => "saved",
            Self::Unsave => "unsaved",
            Self::Reject => "rejected",
            Self::Applied => "applied to",
            Self::Interview => "interviewing for",
            Self::Offer => "got an offer for",
            Self::Like => "liked",
            Self::Dislike => "disliked",
        }
    }

    /// Which way the action points when a reason doesn't say: a reason
    /// given while rejecting describes what is wrong with the job.
    pub fn polarity(self) -> Option<crate::key::Direction> {
        use crate::key::Direction;
        match self {
            Self::Save | Self::Like | Self::Applied => Some(Direction::Prefer),
            Self::Reject | Self::Dislike | Self::Unsave => Some(Direction::Avoid),
            // Being interviewed or offered says how the company saw the
            // person, not what the person wants.
            Self::Seen | Self::Interview | Self::Offer => None,
        }
    }
}

impl fmt::Display for FeedbackAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One action on one opportunity. Never updated or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub id: FeedbackId,
    /// `prof_…`.
    pub profile_id: String,
    /// The opportunity when the action was taken.
    pub opportunity: OpportunityId,
    /// The source record the person was looking at.
    pub job: JobId,
    pub action: FeedbackAction,
    /// The person's words, exactly as given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The job as it was then, for display.
    pub title: String,
    pub company: String,
    pub at: DateTime<Utc>,
}

impl FeedbackEvent {
    pub fn new(
        profile_id: &str,
        opportunity: OpportunityId,
        job: JobId,
        action: FeedbackAction,
        reason: Option<&str>,
        (title, company): (&str, &str),
        at: DateTime<Utc>,
    ) -> Self {
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        // Storage keeps microseconds; so does the event, so it round-trips.
        let at = at
            .with_nanosecond(at.nanosecond() / 1_000 * 1_000)
            .unwrap_or(at);
        Self {
            id: FeedbackId::derive(profile_id, job, action, at, reason),
            profile_id: profile_id.to_owned(),
            opportunity,
            job,
            action,
            reason: reason.map(str::to_owned),
            title: title.to_owned(),
            company: company.to_owned(),
            at,
        }
    }
}

/// Where an opportunity is in the person's pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Unseen,
    Seen,
    Saved,
    Rejected,
    Applied,
    Interviewing,
    Offer,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unseen => "unseen",
            Self::Seen => "seen",
            Self::Saved => "saved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Interviewing => "interviewing",
            Self::Offer => "offer",
        }
    }

    /// Progress of an application: applied, interviewing, offer.
    fn progress(self) -> u8 {
        match self {
            Self::Applied => 1,
            Self::Interviewing => 2,
            Self::Offer => 3,
            _ => 0,
        }
    }

    /// The person has applied (and hasn't withdrawn).
    pub fn in_progress(self) -> bool {
        self.progress() > 0
    }
}

/// Liked or disliked, independent of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sentiment {
    Liked,
    Disliked,
}

/// The person's state on one opportunity, from its events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpportunityState {
    pub stage: Stage,
    pub sentiment: Option<Sentiment>,
    /// The furthest application stage ever reached (kept when the person
    /// later withdraws by rejecting).
    pub furthest: Stage,
    /// Oldest first.
    pub events: Vec<FeedbackEvent>,
}

impl Default for OpportunityState {
    fn default() -> Self {
        Self {
            stage: Stage::Unseen,
            sentiment: None,
            furthest: Stage::Unseen,
            events: Vec::new(),
        }
    }
}

impl OpportunityState {
    /// Folds events (in any order; they are sorted by time).
    pub fn of(events: impl IntoIterator<Item = FeedbackEvent>) -> Self {
        let mut events: Vec<FeedbackEvent> = events.into_iter().collect();
        events.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
        let mut state = Self::default();
        for event in &events {
            state.apply(event.action);
        }
        state.events = events;
        state
    }

    fn apply(&mut self, action: FeedbackAction) {
        use FeedbackAction as A;
        let stage = self.stage;
        self.stage = match action {
            A::Seen if stage == Stage::Unseen => Stage::Seen,
            A::Save if matches!(stage, Stage::Unseen | Stage::Seen | Stage::Rejected) => {
                Stage::Saved
            }
            A::Unsave if stage == Stage::Saved => Stage::Seen,
            A::Reject => Stage::Rejected,
            A::Applied if stage.progress() < 1 => Stage::Applied,
            A::Interview if stage.progress() < 2 => Stage::Interviewing,
            A::Offer => Stage::Offer,
            _ => stage,
        };
        match action {
            A::Like => self.sentiment = Some(Sentiment::Liked),
            A::Dislike => self.sentiment = Some(Sentiment::Disliked),
            _ => {}
        }
        if self.stage.progress() > self.furthest.progress() {
            self.furthest = self.stage;
        }
    }

    /// The last event with a reason for an action.
    pub fn last_reason(&self, action: FeedbackAction) -> Option<&FeedbackEvent> {
        self.events
            .iter()
            .rev()
            .find(|e| e.action == action && e.reason.is_some())
    }

    /// When the current stage was reached.
    pub fn since(&self) -> Option<DateTime<Utc>> {
        self.events
            .iter()
            .rev()
            .find(|e| e.action != FeedbackAction::Seen)
            .or_else(|| self.events.last())
            .map(|e| e.at)
    }

    /// Whether the person ever took an action other than looking.
    pub fn has_feedback(&self) -> bool {
        self.events.iter().any(|e| e.action != FeedbackAction::Seen)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::testing::{event, now};

    #[test]
    fn ids_round_trip() {
        let e = event(FeedbackAction::Reject, Some("too corporate"), 0);
        let text = e.id.to_string();
        assert!(text.starts_with("fb_"));
        assert_eq!(text.parse::<FeedbackId>().unwrap(), e.id);
        assert!("clm_1".parse::<FeedbackId>().is_err());
    }

    #[test]
    fn reasons_are_kept_verbatim() {
        let e = event(
            FeedbackAction::Reject,
            Some("  Too corporate — sorry ;)  "),
            0,
        );
        assert_eq!(e.reason.as_deref(), Some("Too corporate — sorry ;)"));
        let e = event(FeedbackAction::Reject, Some("   "), 0);
        assert_eq!(e.reason, None);
    }

    #[test]
    fn pipeline_state_folds_events() {
        use FeedbackAction as A;
        let state = |actions: &[A]| {
            OpportunityState::of(
                actions
                    .iter()
                    .enumerate()
                    .map(|(i, a)| event(*a, None, i as i64)),
            )
        };
        assert_eq!(state(&[]).stage, Stage::Unseen);
        assert_eq!(state(&[A::Seen]).stage, Stage::Seen);
        assert_eq!(state(&[A::Save]).stage, Stage::Saved);
        assert_eq!(state(&[A::Save, A::Unsave]).stage, Stage::Seen);
        // Saving a rejected job brings it back.
        assert_eq!(state(&[A::Reject, A::Save]).stage, Stage::Saved);
        // Applying is not undone by saving again.
        assert_eq!(state(&[A::Applied, A::Save]).stage, Stage::Applied);
        assert_eq!(
            state(&[A::Applied, A::Interview, A::Applied]).stage,
            Stage::Interviewing
        );
        // Withdrawing keeps how far it went.
        let s = state(&[A::Applied, A::Interview, A::Reject]);
        assert_eq!(
            (s.stage, s.furthest),
            (Stage::Rejected, Stage::Interviewing)
        );
        // Liking is orthogonal to the pipeline.
        let s = state(&[A::Save, A::Like, A::Seen]);
        assert_eq!(
            (s.stage, s.sentiment),
            (Stage::Saved, Some(Sentiment::Liked))
        );
        let s = state(&[A::Like, A::Dislike]);
        assert_eq!(
            (s.stage, s.sentiment),
            (Stage::Unseen, Some(Sentiment::Disliked))
        );
    }

    #[test]
    fn events_are_ordered_by_time() {
        let late = event(FeedbackAction::Reject, None, 10);
        let early = event(FeedbackAction::Save, None, 1);
        let s = OpportunityState::of([late, early]);
        assert_eq!(s.stage, Stage::Rejected);
        assert_eq!(s.since(), Some(now() + Duration::minutes(10)));
    }
}
