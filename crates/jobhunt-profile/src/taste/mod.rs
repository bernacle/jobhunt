//! The candidate taste profile: what kind of company and role the person
//! would genuinely want, kept apart from what they can practically take.
//!
//! Two questions, two models:
//!
//! * **taste** (this module) answers "what would this developer want?":
//!   seniority, the shape of the engineering work, how specialized, how
//!   much ownership, the kind of company, team and culture, domains, and
//!   what they avoid. It is semantic and often fuzzy, so every statement
//!   carries its provenance, a confidence, and the person's review;
//! * **practical constraints** answer "can they realistically pursue this
//!   job?": work setup, where they live and may work, relocation, time
//!   zones, a pay floor. Those stay the deterministic, explicit
//!   [`crate::Preference`] records eligibility and ranking already read.
//!   Pay is never taste.
//!
//! A taste profile is composed ([`compose()`]) from:
//!
//! * [`TasteAssertion`]s stored in the profile: Narrow's reading of the
//!   person's words (by a model or the built-in rules, [`reading`]), what
//!   it inferred from their confirmed career evidence, and everything the
//!   person confirmed, corrected, added or removed;
//! * the structured preferences set before this model existed, read live
//!   ([`vocab::from_preference`]) so nothing is migrated destructively and
//!   ranking keeps reading exactly what it read before;
//! * patterns learned from feedback, read live and kept in their own
//!   section ([`LearnedSignal`]).
//!
//! The person's decisions always win: a confirmed, corrected or removed
//! statement is never overwritten by a new reading, a profile import or
//! learned feedback ([`edit`]).
//!
//! Nothing here ranks jobs. BRU-322 consumes [`TasteProfile`].

pub mod compose;
pub mod edit;
pub mod reading;
pub mod rules;
pub mod vocab;

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{PreferenceId, StatementId, TasteBriefId, TasteId};

pub use compose::{ComposedAssertion, LearnedSignal, TasteProfile, compose};
pub use reading::{InterpretError, ReadAssertion, TasteInterpreter, TasteReading, TasteRequest};
pub use rules::RulesInterpreter;

/// What a taste statement is about. Deliberately few: values are open
/// (a canonical token when Narrow knows one, the person's words otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasteDimension {
    /// The level of the role: `early_career`, `mid`, `senior`, `staff_plus`.
    Seniority,
    /// The shape of the engineering work: `backend`, `platform`,
    /// `infrastructure`, `product`, `database_internals`, `ml_research`, …
    /// Not technologies: using PostgreSQL is not building storage engines.
    WorkShape,
    /// How specialized: `broad` (generalist), `moderate`, `deep`.
    Specialization,
    /// Ownership and autonomy: `high`.
    Ownership,
    /// The kind of company: `startup`, `early_stage`, `growth`,
    /// `small_company`, `large_company`, `founder_led`, …
    Company,
    /// The team: `small_team`, `large_team`, `distributed`.
    Team,
    /// Engineering culture and pace: `strong_engineering`,
    /// `process_heavy`, `fast_paced`, `mentorship`, `remote_first`.
    Culture,
    /// A domain or product: `developer tools`, `fintech`.
    Domain,
    /// A technology, only when it shapes the work ("Rust systems work").
    Technology,
    /// How the work is done: `individual_contributor`, `management`,
    /// `greenfield`, `on_call`, `async_communication`, …
    WorkStyle,
    /// Anything else the person said that fits nowhere above.
    Other,
}

impl TasteDimension {
    pub const ALL: [TasteDimension; 11] = [
        Self::Seniority,
        Self::WorkShape,
        Self::Specialization,
        Self::Ownership,
        Self::Company,
        Self::Team,
        Self::Culture,
        Self::Domain,
        Self::Technology,
        Self::WorkStyle,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seniority => "seniority",
            Self::WorkShape => "work_shape",
            Self::Specialization => "specialization",
            Self::Ownership => "ownership",
            Self::Company => "company",
            Self::Team => "team",
            Self::Culture => "culture",
            Self::Domain => "domain",
            Self::Technology => "technology",
            Self::WorkStyle => "work_style",
            Self::Other => "other",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == value)
    }

    /// As shown to the person.
    pub fn label(self) -> &'static str {
        match self {
            Self::Seniority => "Level",
            Self::WorkShape => "Kind of work",
            Self::Specialization => "Specialization",
            Self::Ownership => "Ownership",
            Self::Company => "Company",
            Self::Team => "Team",
            Self::Culture => "Culture",
            Self::Domain => "Domain",
            Self::Technology => "Technology",
            Self::WorkStyle => "Way of working",
            Self::Other => "Other",
        }
    }
}

impl fmt::Display for TasteDimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which way a statement leans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    /// Wanted.
    Prefer,
    /// Fine, not sought ("open to startups").
    Open,
    /// Not wanted.
    Avoid,
    /// The person said it doesn't matter to them. Kept so it is never
    /// inferred again.
    Neutral,
}

impl Polarity {
    pub const ALL: [Polarity; 4] = [Self::Prefer, Self::Open, Self::Avoid, Self::Neutral];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prefer => "prefer",
            Self::Open => "open",
            Self::Avoid => "avoid",
            Self::Neutral => "neutral",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == value)
    }
}

/// How sure Narrow is of a statement it read or inferred. Deliberately
/// not a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasteConfidence {
    Low,
    Medium,
    High,
}

impl TasteConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// How a statement was arrived at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasteOrigin {
    /// The person wrote it (a correction or an added sentence).
    Stated,
    /// Narrow's reading of the person's words (by a model or the rules).
    Interpreted,
    /// Inferred from the person's confirmed career evidence.
    Profile,
    /// A pattern learned from their feedback (saved, rejected, applied).
    Learned,
    /// A structured preference set before the taste profile existed.
    Legacy,
}

impl TasteOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stated => "stated",
            Self::Interpreted => "interpreted",
            Self::Profile => "profile",
            Self::Learned => "learned",
            Self::Legacy => "legacy",
        }
    }
}

/// The person's decision about a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TasteReview {
    /// Narrow's reading, not yet looked at by the person.
    #[default]
    Unreviewed,
    /// The person said it is right.
    Confirmed,
    /// The person changed it (its polarity, its words, or both). What
    /// Narrow had read is kept in [`TasteAssertion::original`].
    Corrected,
    /// The person removed it. Kept, so no later reading brings it back.
    Removed,
}

impl TasteReview {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreviewed => "unreviewed",
            Self::Confirmed => "confirmed",
            Self::Corrected => "corrected",
            Self::Removed => "removed",
        }
    }

    /// The person decided something about it: it is theirs, and wins.
    pub fn is_reviewed(self) -> bool {
        self != Self::Unreviewed
    }
}

/// Where a statement comes from. Every statement has at least one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TasteSource {
    /// The person's own words, verbatim.
    Words {
        quote: String,
        /// The statement the words were stored as, when there is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        statement: Option<StatementId>,
    },
    /// A structured preference set earlier.
    Preference {
        preference: PreferenceId,
        text: String,
    },
    /// Career evidence in the profile ("latest title: Senior Engineer").
    Evidence {
        text: String,
        /// The experiences and claims it rests on (`exp_…`, `clm_…`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        records: Vec<String>,
    },
    /// A pattern learned from feedback (`company_trait:startup`), with
    /// what backs it ("2 saves, 1 application").
    Feedback { pattern: String, text: String },
    /// The person, directly (a confirmation or a correction).
    Person,
}

impl TasteSource {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Words { .. } => "words",
            Self::Preference { .. } => "preference",
            Self::Evidence { .. } => "evidence",
            Self::Feedback { .. } => "feedback",
            Self::Person => "person",
        }
    }

    /// One line, for display: “small technical teams”, "your earlier
    /// setting: small teams".
    pub fn describe(&self) -> String {
        match self {
            Self::Words { quote, .. } => format!("You wrote: “{quote}”"),
            Self::Preference { text, .. } => format!("Your earlier setting: {text}"),
            Self::Evidence { text, .. } => format!("Your profile: {text}"),
            Self::Feedback { text, .. } => format!("Your feedback: {text}"),
            Self::Person => "You reviewed it".to_owned(),
        }
    }
}

/// What Narrow had read before the person corrected it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalReading {
    pub dimension: TasteDimension,
    pub value: String,
    pub polarity: Polarity,
    pub text: String,
    pub origin: TasteOrigin,
}

/// One statement of the taste profile: "prefers small technical teams".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasteAssertion {
    pub id: TasteId,
    pub dimension: TasteDimension,
    /// A canonical token (`small_team`, `database_internals`) or, for
    /// something Narrow has no token for, the words normalized.
    pub value: String,
    pub polarity: Polarity,
    /// What the person sees: a short phrase ("Small technical teams").
    pub text: String,
    pub confidence: TasteConfidence,
    pub origin: TasteOrigin,
    #[serde(default)]
    pub review: TasteReview,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<TasteSource>,
    /// Why Narrow concluded it, when it was read or inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// What read it (`rules/1`, `model/anthropic:claude-opus-5-5`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
    /// Narrow's reading before the person corrected it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<OriginalReading>,
    /// Set when a correction replaced it with another statement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<TasteId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TasteAssertion {
    /// Identity: a statement about the same dimension and value is the same
    /// statement, whoever made it.
    pub fn key(&self) -> String {
        key(self.dimension, &self.value)
    }

    /// In effect: neither removed nor replaced.
    pub fn is_active(&self) -> bool {
        self.review != TasteReview::Removed && self.superseded_by.is_none()
    }

    /// The person's own: they wrote it, confirmed it or corrected it.
    pub fn is_persons(&self) -> bool {
        self.origin == TasteOrigin::Stated
            || matches!(self.review, TasteReview::Confirmed | TasteReview::Corrected)
    }
}

/// `dimension:value`.
pub fn key(dimension: TasteDimension, value: &str) -> String {
    format!("{}:{}", dimension.as_str(), value)
}

/// How an interpretation went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterpretationOutcome {
    /// Read by the configured interpreter.
    Read,
    /// The model could not be used; the built-in rules read it instead.
    Fallback,
}

impl InterpretationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Fallback => "fallback",
        }
    }
}

/// The last time the person's words were interpreted, and what came of it
/// beyond the statements themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    /// `rules/1`, `model/anthropic:claude-opus-5-5`.
    pub interpreter: String,
    pub outcome: InterpretationOutcome,
    /// Digest of everything the interpreter was given: the same input is
    /// never interpreted twice unless the person asks.
    pub input_digest: String,
    pub at: DateTime<Utc>,
    /// Why the fallback was used ("the model did not answer").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// One sentence, when the interpreter wrote one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What was unclear, for the person to settle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguities: Vec<String>,
    /// Practical constraints the words mentioned ("remote"). Never turned
    /// into taste, and never set automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints_noted: Vec<String>,
    /// Statements the interpreter produced that failed validation.
    #[serde(default)]
    pub rejected: u32,
}

/// What the person is looking for, in their own words: the answer to
/// "What kind of job are you looking for?". One per profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasteBrief {
    pub id: TasteBriefId,
    /// Verbatim.
    pub text: String,
    /// The preference statement the same words were stored as (the rule
    /// parser still reads practical constraints from it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<StatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<Interpretation>,
    /// When the person last said the summary looks right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TasteBrief {
    /// The brief's id for a profile: there is only one.
    pub fn id_for(profile: crate::ProfileId) -> TasteBriefId {
        TasteBriefId::derive(&[&profile.to_string(), "brief"])
    }
}

/// A value as a token: lowercase words joined by `_`, at most 60 bytes.
/// `"Developer Tools"` → `developer_tools`.
pub fn normalize_value(raw: &str) -> String {
    let key = jobhunt_core::text::search_key(raw);
    let mut out = String::new();
    for word in key.split(' ').filter(|w| !w.is_empty()) {
        if out.len() + word.len() + 1 > 60 {
            break;
        }
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_normalize_to_tokens() {
        assert_eq!(normalize_value("Developer Tools"), "developer_tools");
        assert_eq!(normalize_value("  small-team "), "small_team");
        assert_eq!(normalize_value("!!"), "");
        assert!(normalize_value(&"word ".repeat(40)).len() <= 60);
    }

    #[test]
    fn enums_round_trip() {
        for d in TasteDimension::ALL {
            assert_eq!(TasteDimension::from_canonical(d.as_str()), Some(d));
            let json = serde_json::to_string(&d).unwrap();
            assert_eq!(json, format!("\"{}\"", d.as_str()));
        }
        for p in Polarity::ALL {
            assert_eq!(Polarity::from_canonical(p.as_str()), Some(p));
        }
        assert_eq!(
            serde_json::to_string(&TasteSource::Person).unwrap(),
            r#"{"kind":"person"}"#
        );
    }
}
