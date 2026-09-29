//! The decision brief: what a person needs to decide whether a job is
//! worth their time, in a few lines.
//!
//! * a one-line **verdict** (the tier, in words, and what to do first);
//! * a **summary** of what the job is (role, level, stack, domain, pay,
//!   where);
//! * **why it may be worth attention**: the strongest reasons, in terms of
//!   what the person wants;
//! * **caveats**: what counts against it, and conditions;
//! * **unknowns**: what the posting doesn't say that matters to them;
//! * **your history** with it.
//!
//! Every line comes from a [`Signal`], so each one can be traced to its
//! evidence (`narrow why --details`).

use serde::{Deserialize, Serialize};

use crate::facets::{JobFacets, JobFunction};
use crate::person::Person;
use crate::rank::{Gate, Tier};
use crate::signals::{Basis, PayEvidence, Signal, SignalKind};

/// What someone needs to decide whether to spend time on a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionBrief {
    /// One sentence.
    pub verdict: String,
    /// "Backend · senior · Rust, PostgreSQL · payments · USD 150,000 –
    /// 180,000 per year · remote".
    pub summary: String,
    pub worth: Vec<String>,
    pub caveats: Vec<String>,
    pub unknowns: Vec<String>,
    pub history: Vec<String>,
}

const MAX_LINES: usize = 5;

fn verdict(gate: &Gate, tier: Tier, person: &Person) -> String {
    let fit = match tier {
        Tier::StrongFit => "A strong fit for what you want",
        Tier::WorthReviewing => "Worth reviewing",
        Tier::Maybe => "Maybe: mixed signals",
        Tier::LowPriority => "Low priority: little points to it",
    };
    let thin = if person.has_preferences() {
        ""
    } else {
        " (you haven't said what you want yet, so this rests on eligibility, experience and freshness)"
    };
    match gate {
        Gate::Recommended => format!("{fit}{thin}."),
        Gate::VerifyFirst { why } => {
            format!("{fit}{thin}, but verify it first: {why}.")
        }
        Gate::EligibilityUnclear { why } => {
            format!("{fit}{thin}, if you can take it: {why}.")
        }
        Gate::Excluded { exclusion } => format!("Not recommended: {}.", exclusion.label()),
    }
}

fn summary(f: &JobFacets, pay: &PayEvidence) -> String {
    let mut parts: Vec<String> = Vec::new();
    let roles: Vec<String> = f
        .roles
        .iter()
        .take(2)
        .map(|r| r.key.value_label())
        .collect();
    if f.function.is_engineering() && !roles.is_empty() {
        parts.push(roles.join(", "));
    } else if f.function != JobFunction::Engineering {
        parts.push(f.function.label().to_owned());
    }
    if let Some((level, _)) = &f.level {
        parts.push(level.as_str().to_owned());
    }
    let techs: Vec<&str> = f
        .technologies_at(crate::facets::Requirement::Required)
        .into_iter()
        .take(3)
        .map(|t| t.name.as_str())
        .collect();
    if !techs.is_empty() {
        parts.push(techs.join(", "));
    }
    let domains: Vec<&str> = f
        .domains
        .iter()
        .take(2)
        .map(|d| d.key.value.as_str())
        .collect();
    if !domains.is_empty() {
        parts.push(domains.join(", "));
    }
    let salary = pay.check.ranges.iter().find(|r| r.kind == "salary");
    parts.push(match salary {
        Some(r) => r.describe(),
        None => "pay not published".to_owned(),
    });
    let modes: Vec<&str> = f
        .work_modes
        .iter()
        .map(|m| match m {
            jobhunt_profile::WorkMode::Remote => "remote",
            jobhunt_profile::WorkMode::Hybrid => "hybrid",
            jobhunt_profile::WorkMode::Onsite => "on-site",
        })
        .collect();
    if !modes.is_empty() {
        parts.push(modes.join(" or "));
    }
    parts.join(" · ")
}

/// Builds the brief from the signals.
pub fn brief(
    facets: &JobFacets,
    pay: &PayEvidence,
    gate: &Gate,
    tier: Tier,
    signals: &[Signal],
    person: &Person,
) -> DecisionBrief {
    let history: Vec<String> = signals
        .iter()
        .filter(|s| s.basis == Basis::Feedback)
        .map(|s| s.summary.clone())
        .collect();
    let others = || signals.iter().filter(|s| s.basis != Basis::Feedback);
    let mut worth: Vec<&Signal> = others().filter(|s| s.kind == SignalKind::Plus).collect();
    worth.sort_by(|a, b| {
        b.basis
            .is_personal()
            .cmp(&a.basis.is_personal())
            .then(b.weight.total_cmp(&a.weight))
    });
    let mut caveats: Vec<&Signal> = others()
        .filter(|s| {
            matches!(
                s.kind,
                SignalKind::Minus | SignalKind::Condition | SignalKind::Blocker
            )
        })
        .collect();
    caveats.sort_by(|a, b| {
        (b.kind == SignalKind::Blocker)
            .cmp(&(a.kind == SignalKind::Blocker))
            .then(a.weight.total_cmp(&b.weight))
    });
    // What the person stated comes first: a requirement the posting leaves
    // unresolved ("Unresolved: you require at least USD 140,000 per year")
    // matters more than a fact the posting simply omits, and a card may
    // show only the first unknown.
    let mut unknowns: Vec<&Signal> = others().filter(|s| s.kind == SignalKind::Unknown).collect();
    unknowns.sort_by_key(|s| s.basis != Basis::Stated);
    let unknowns: Vec<String> = unknowns.into_iter().map(|s| s.summary.clone()).collect();
    DecisionBrief {
        verdict: verdict(gate, tier, person),
        summary: summary(facets, pay),
        worth: worth
            .into_iter()
            .take(MAX_LINES)
            .map(|s| s.summary.clone())
            .collect(),
        caveats: caveats
            .into_iter()
            .take(MAX_LINES)
            .map(|s| s.summary.clone())
            .collect(),
        unknowns,
        history,
    }
}
