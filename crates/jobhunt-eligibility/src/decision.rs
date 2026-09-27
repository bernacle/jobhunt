//! Eligibility decisions and the reasons behind them.
//!
//! A decision is one of four explicit states, never a percentage, and
//! every state carries the reasons that produced it: which rule, what it
//! concluded, the posting's words and the profile fact it used. That is
//! what "why do we think you're eligible?" is answered from, without
//! reconstructing anything from logs. Decisions are plain data so they can
//! be stored (see [`crate::cache`]).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::job::Evidence;
use crate::profile::{FactBasis, ProfileLocation};

/// Revision of the eligibility rules and the normalization they read
/// (geography tables, cue lists). Part of every stored decision's cache
/// key, so changing a rule never keeps conclusions reached under the old
/// one. Bump it with any change that can change a decision.
pub const RULES_VERSION: &str = "3";

/// Whether a person appears able to work a job, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eligibility {
    /// An explicit restriction of the posting excludes the person.
    Ineligible,
    /// Information is missing or contradictory; nothing rules the person
    /// out and nothing confirms them.
    Uncertain,
    /// Eligible only if a condition the person has not ruled out holds:
    /// they relocate, or the company grants the sponsorship it offers.
    Conditional,
    /// Every explicit requirement is met by stated facts.
    Eligible,
}

impl Eligibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ineligible => "ineligible",
            Self::Uncertain => "uncertain",
            Self::Conditional => "conditional",
            Self::Eligible => "eligible",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "ineligible" => Some(Self::Ineligible),
            "uncertain" => Some(Self::Uncertain),
            "conditional" => Some(Self::Conditional),
            "eligible" => Some(Self::Eligible),
            _ => None,
        }
    }

    /// As shown to the person.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ineligible => "INELIGIBLE",
            Self::Uncertain => "UNCLEAR",
            Self::Conditional => "CONDITIONAL",
            Self::Eligible => "ELIGIBLE",
        }
    }
}

impl fmt::Display for Eligibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one rule concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Conditional,
    Unknown,
    Fail,
    /// Informational: explains why something does not apply.
    NotApplicable,
}

/// The rules, in the order they are applied. Hard restrictions come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleId {
    /// Rule 1: The listing is active and verifiable enough to recommend (a gate
    /// applied on top of the decision, see [`crate::evaluate`](mod@crate::evaluate)).
    Listing,
    /// Rule 2: A work mode the person requires.
    WorkMode,
    /// Rule 2: On-site / hybrid presence at a place.
    Presence,
    /// Rule 3: Countries (and cities) the job allows or rules out.
    CountryConstraint,
    /// Rule 4: Regions the job allows or rules out, including "anywhere".
    RegionConstraint,
    /// Rule 4: A remote option whose scope is not published.
    RemoteScope,
    /// Rule 5: Work authorization and visa sponsorship.
    Authorization,
    /// Rule 6: Contractor, B2B and employer-of-record engagement.
    Engagement,
    /// Rule 7: Time-zone requirements.
    Timezone,
    /// Rule 8: Contradictory or unreadable information.
    Ambiguity,
}

impl RuleId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listing => "listing",
            Self::WorkMode => "work_mode",
            Self::Presence => "presence",
            Self::CountryConstraint => "country_constraint",
            Self::RegionConstraint => "region_constraint",
            Self::RemoteScope => "remote_scope",
            Self::Authorization => "authorization",
            Self::Engagement => "engagement",
            Self::Timezone => "timezone",
            Self::Ambiguity => "ambiguity",
        }
    }
}

/// The posting's words behind a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    /// `ashby:linear`.
    pub source: String,
    /// `locations`, `description`, …
    pub field: String,
    pub text: String,
}

impl From<&Evidence> for EvidenceRef {
    fn from(e: &Evidence) -> Self {
        Self {
            source: e.source.to_string(),
            field: e.field.to_owned(),
            text: e.text.clone(),
        }
    }
}

/// A profile fact a reason used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileFact {
    /// `location`, `relocation`, `sponsorship`, `time zone`, …
    pub what: String,
    pub value: String,
    /// `preference`, `resume`, `derived`, or `missing`.
    pub basis: String,
}

impl ProfileFact {
    pub fn new(what: &str, value: impl Into<String>, basis: FactBasis) -> Self {
        Self {
            what: what.to_owned(),
            value: value.into(),
            basis: basis.as_str().to_owned(),
        }
    }

    pub fn missing(what: &str) -> Self {
        Self {
            what: what.to_owned(),
            value: "not in your profile".to_owned(),
            basis: "missing".to_owned(),
        }
    }

    pub fn location(location: &ProfileLocation) -> Self {
        Self::new("location", location.raw.clone(), location.basis)
    }
}

/// One step of the reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason {
    pub rule: RuleId,
    pub verdict: Verdict,
    /// One sentence: "Brazil is within the listed Americas region".
    pub conclusion: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<ProfileFact>,
}

impl Reason {
    pub fn new(rule: RuleId, verdict: Verdict, conclusion: impl Into<String>) -> Self {
        Self {
            rule,
            verdict,
            conclusion: conclusion.into(),
            evidence: Vec::new(),
            profile: Vec::new(),
        }
    }

    pub fn evidence<'a>(mut self, evidence: impl IntoIterator<Item = &'a Evidence>) -> Self {
        for e in evidence {
            let e = EvidenceRef::from(e);
            if !self.evidence.contains(&e) {
                self.evidence.push(e);
            }
        }
        self
    }

    pub fn fact(mut self, fact: ProfileFact) -> Self {
        if !self.profile.contains(&fact) {
            self.profile.push(fact);
        }
        self
    }
}

/// The outcome for one way of doing the job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionDecision {
    /// "Remote (the Americas)", "On-site in New York".
    pub option: String,
    pub status: Eligibility,
    pub reasons: Vec<Reason>,
}

impl OptionDecision {
    /// Combines rule verdicts: any failure excludes; else anything unknown
    /// leaves it uncertain; else a condition makes it conditional.
    pub fn from_reasons(option: String, reasons: Vec<Reason>) -> Self {
        let has = |v: Verdict| reasons.iter().any(|r| r.verdict == v);
        let status = if has(Verdict::Fail) {
            Eligibility::Ineligible
        } else if has(Verdict::Unknown) {
            Eligibility::Uncertain
        } else if has(Verdict::Conditional) {
            Eligibility::Conditional
        } else {
            Eligibility::Eligible
        };
        Self {
            option,
            status,
            reasons,
        }
    }

    /// The reason that best explains the status.
    pub fn headline(&self) -> String {
        let wanted = match self.status {
            Eligibility::Ineligible => Verdict::Fail,
            Eligibility::Uncertain => Verdict::Unknown,
            Eligibility::Conditional => Verdict::Conditional,
            Eligibility::Eligible => Verdict::Pass,
        };
        // A failure is explained by the first (hardest) rule that failed; a
        // positive answer by where the job can be done from, not by a
        // work-mode match.
        let geographic = |r: &&Reason| {
            matches!(
                r.rule,
                RuleId::Presence
                    | RuleId::CountryConstraint
                    | RuleId::RegionConstraint
                    | RuleId::RemoteScope
            )
        };
        let matching = || self.reasons.iter().filter(|r| r.verdict == wanted);
        matching()
            .find(|r| wanted == Verdict::Fail || geographic(r))
            .or_else(|| matching().next())
            .map(|r| r.conclusion.clone())
            .unwrap_or_else(|| self.option.clone())
    }
}

/// A disagreement between statements, kept visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictNote {
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
}

/// Whether a person appears able to work a job, and why.
///
/// This is a compatibility signal computed from what the posting publishes
/// and what the person told JobHunt, not a legal determination of work
/// authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EligibilityDecision {
    pub status: Eligibility,
    /// One line.
    pub headline: String,
    /// The way of doing the job the status is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    /// Why, in rule order.
    pub reasons: Vec<Reason>,
    /// The other ways of doing the job, and how each came out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_options: Vec<OptionDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<ConflictNote>,
    /// The source records the decision rests on (`greenhouse:stripe`).
    #[serde(default)]
    pub sources: Vec<String>,
    pub rules_version: String,
}
