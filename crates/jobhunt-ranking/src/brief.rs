//! The decision brief: what a person needs to decide whether a job is
//! worth their time, in a few lines, fit first and practicality after.
//!
//! * a one-line **verdict**: how well it fits what they want, and what
//!   stands in the way;
//! * a **summary** of what the job is (role, level, stack, domain, pay,
//!   where);
//! * **why it may be worth attention**: the fit's affirmative reasons,
//!   specific to this job ("A startup of 15 people, the kind of company
//!   you want"), never pay, remote work or freshness;
//! * **caveats**: what goes against the fit, then practical concerns and
//!   what rules it out;
//! * **unknowns**: the things to check (pay not published, remote scope
//!   unclear), the person's stated requirements first;
//! * **your history** with it.
//!
//! Every line traces to the fit assessment, the practicality or a
//! [`Signal`] with its evidence (`narrow why --details`).

use serde::{Deserialize, Serialize};

use crate::facets::{JobFacets, JobFunction};
use crate::fit::{FitAssessment, FitLevel, Severity};
use crate::practicality::Practicality;
use crate::rank::Gate;
use crate::signals::{Basis, PayEvidence, Signal};

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

fn verdict(gate: &Gate, fit: &FitAssessment, has_taste: bool) -> String {
    let lead = match fit.level {
        FitLevel::Strong => "Looks unusually aligned with what you want".to_owned(),
        FitLevel::Plausible => match fit.main_contradiction() {
            Some(c) => format!("Worth a look, not a strong fit: {}", lower_first(&c.text)),
            None => {
                "Worth a look: some of it fits what you want, not enough to recommend it".to_owned()
            }
        },
        FitLevel::Insufficient => "Not enough points to what you want".to_owned(),
        FitLevel::Poor => match fit.material().next() {
            Some(c) => format!("Not a fit: {}", lower_first(&c.text)),
            None => "Not a fit".to_owned(),
        },
    };
    let thin = if has_taste {
        ""
    } else {
        " (you haven't said what you want yet, so there is little to go on)"
    };
    match gate {
        Gate::Recommended => format!("{lead}{thin}."),
        Gate::VerifyFirst { why } => format!("{lead}{thin}, but verify it first: {why}."),
        Gate::EligibilityUnclear { why } => format!("{lead}{thin}, if you can take it: {why}."),
        Gate::Excluded { exclusion } => format!("Not recommended: {}.", exclusion.label()),
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase() => text.to_owned(),
        (Some(a), _) => a.to_lowercase().chain(text.chars().skip(1)).collect(),
        _ => String::new(),
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

/// Builds the brief: fit first, then practicality.
pub fn brief(
    facets: &JobFacets,
    pay: &PayEvidence,
    gate: &Gate,
    fit: &FitAssessment,
    practicality: &Practicality,
    signals: &[Signal],
    has_taste: bool,
) -> DecisionBrief {
    let history: Vec<String> = signals
        .iter()
        .filter(|s| s.basis == Basis::Feedback)
        .map(|s| s.summary.clone())
        .collect();
    let mut out = DecisionBrief {
        verdict: String::new(),
        summary: summary(facets, pay),
        worth: Vec::new(),
        caveats: Vec::new(),
        unknowns: practicality.checks.clone(),
        history,
    };
    refit(&mut out, gate, fit, practicality, has_taste);
    out
}

/// Rewrites what a brief says about fit (the verdict, the reasons, the
/// caveats) after the fit changed (a semantic review).
pub fn refit(
    brief: &mut DecisionBrief,
    gate: &Gate,
    fit: &FitAssessment,
    practicality: &Practicality,
    has_taste: bool,
) {
    let mut caveats: Vec<String> = Vec::new();
    let mut push = |text: &str| {
        if !caveats.iter().any(|c| c == text) {
            caveats.push(text.to_owned());
        }
    };
    // What goes against the fit (material first), then practical concerns
    // and what rules it out.
    for c in &fit.contradictions {
        if c.severity >= Severity::Minor {
            push(&c.text);
        }
    }
    for c in &practicality.concerns {
        push(c);
    }
    for b in &practicality.blockers {
        push(b);
    }
    brief.verdict = verdict(gate, fit, has_taste);
    brief.worth = fit.reason_texts().into_iter().take(MAX_LINES).collect();
    brief.caveats = caveats.into_iter().take(MAX_LINES).collect();
}
