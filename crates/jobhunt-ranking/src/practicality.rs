//! Practicality: can the person realistically pursue a job, and what still
//! needs checking?
//!
//! Practicality is shown after fit and never creates it. It can make a job
//! impossible (eligibility rules it out, the person's stated work setup or
//! relocation conflicts), add a known concern (pay below a floor or a
//! target), leave something to check (pay not published, remote scope
//! unclear, travel expected, the listing not verified yet), or add a useful
//! fact (the pay reaches the person's target). Being remote, verified or
//! well paid makes a job possible, not wanted.
//!
//! It is read from what eligibility, verification and the pay reading
//! already concluded (the gate and the practical signals); nothing here
//! re-decides them.

use serde::{Deserialize, Serialize};

use crate::rank::{Exclusion, Gate};
use crate::signals::{Signal, SignalGroup, SignalKind};

/// Where a job stands practically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PracticalStatus {
    /// Nothing practical stands in the way, as far as the posting says.
    Clear,
    /// Something practical isn't known yet: check it.
    Check,
    /// A known practical downside (pay below a floor or target).
    Concern,
    /// A concrete condition rules it out.
    Impossible,
}

impl PracticalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Check => "check",
            Self::Concern => "concern",
            Self::Impossible => "impossible",
        }
    }
}

/// How the published pay reads against what the person wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayStanding {
    /// The person set no pay preference.
    #[default]
    NoPreference,
    /// Not published, or not as a comparable salary.
    Unpublished,
    /// Published for another location than where the person lives: says
    /// nothing about what the job pays them.
    ForAnotherLocation,
    /// Published, but not comparable (another currency or period).
    NotComparable,
    /// Meets what they want.
    Meets,
    /// Below a target (or partly below a minimum).
    BelowTarget,
    /// Below a minimum.
    BelowMinimum,
}

/// The practical side of one job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Practicality {
    pub status: PracticalStatus,
    /// What rules it out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    /// Known downsides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concerns: Vec<String>,
    /// Things to check: what the posting leaves open, the person's stated
    /// requirements first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// Useful practical facts (pay reaching a target, remote from where
    /// they want).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<String>,
    #[serde(default)]
    pub pay: PayStanding,
}

impl Default for Practicality {
    fn default() -> Self {
        Self {
            status: PracticalStatus::Clear,
            blockers: Vec::new(),
            concerns: Vec::new(),
            checks: Vec::new(),
            facts: Vec::new(),
            pay: PayStanding::NoPreference,
        }
    }
}

/// Groups whose signals are practical, not fit.
fn practical(group: SignalGroup) -> bool {
    matches!(
        group,
        SignalGroup::Eligibility
            | SignalGroup::Verification
            | SignalGroup::Compensation
            | SignalGroup::WorkMode
            | SignalGroup::Freshness
    )
}

/// A stated requirement the posting leaves open ("Unresolved: you require
/// …"): about what the person can take, so it is a thing to check whatever
/// group it came from.
fn unresolved_requirement(s: &Signal) -> bool {
    s.kind == SignalKind::Unknown && s.summary.starts_with("Unresolved:")
}

/// Reads a job's practicality from its gate and signals.
pub fn assess(gate: &Gate, signals: &[Signal], pay: PayStanding) -> Practicality {
    let mut out = Practicality {
        pay,
        ..Practicality::default()
    };
    let push = |list: &mut Vec<String>, text: &str| {
        if !list.iter().any(|x| x == text) {
            list.push(text.to_owned());
        }
    };
    for s in signals {
        if !(practical(s.group) || unresolved_requirement(s)) {
            continue;
        }
        match s.kind {
            SignalKind::Blocker => push(&mut out.blockers, &s.summary),
            SignalKind::Unknown | SignalKind::Condition => push(&mut out.checks, &s.summary),
            SignalKind::Minus => push(&mut out.concerns, &s.summary),
            SignalKind::Plus | SignalKind::Context => {
                if matches!(s.group, SignalGroup::Compensation | SignalGroup::WorkMode) {
                    push(&mut out.facts, &s.summary);
                }
            }
        }
    }
    // The person's own requirements first.
    out.checks.sort_by_key(|c| !c.starts_with("Unresolved:"));
    match gate {
        Gate::Excluded { exclusion } => match exclusion {
            Exclusion::BelowMinimum { why } => {
                push(&mut out.concerns, why);
                out.status = PracticalStatus::Concern;
            }
            Exclusion::Ineligible { .. }
            | Exclusion::UnmetRequirement { .. }
            | Exclusion::Closed { .. }
            | Exclusion::PayUnknown { .. }
            | Exclusion::EligibilityUnconfirmed { .. } => {
                // The eligibility or work-setup blocker already says it.
                if out.blockers.is_empty() {
                    push(&mut out.blockers, &exclusion.label());
                }
                out.status = PracticalStatus::Impossible;
            }
            // The person's own decision, not practicality.
            Exclusion::Rejected | Exclusion::InPipeline { .. } => {}
        },
        Gate::VerifyFirst { why } if !why.is_empty() => {
            push(&mut out.checks, &format!("Verify it's still open: {why}"));
        }
        _ => {}
    }
    if out.status == PracticalStatus::Clear {
        out.status = if !out.blockers.is_empty() {
            PracticalStatus::Impossible
        } else if !out.concerns.is_empty() {
            PracticalStatus::Concern
        } else if !out.checks.is_empty() {
            PracticalStatus::Check
        } else {
            PracticalStatus::Clear
        };
    }
    out
}
