//! Ranking one opportunity: the gate it passes (or doesn't), a coarse
//! tier, the signals behind both, and the decision brief.
//!
//! Ranking sits on top of eligibility and never replaces it. Eligibility
//! answers "can this person plausibly work this job?"; ranking answers
//! "would they likely want it?". So the gate comes first, from existing
//! answers:
//!
//! | Situation | Gate |
//! | --- | --- |
//! | rejected, or already applied / interviewing / offered | not recommended (it is in the pipeline, or ruled out by the person) |
//! | the listing is closed | not recommended |
//! | ineligible | not recommended |
//! | verified pay below a *required* minimum | not recommended |
//! | eligibility uncertain | shown separately, with why |
//! | eligible or conditional, but not trusted (never verified, stale, failed) | "verify first" |
//! | eligible or conditional, trusted ([`Assessment::recommendable`]) | recommended |
//!
//! Within a gate, the tier ([`Tier`]) is what people see: strong fit, worth
//! reviewing, maybe, low priority. The numeric score only orders jobs and
//! is shown only in details; it is not a match percentage. A strong fit
//! needs a reason in terms of what the person wants (a stated preference,
//! learned taste, their own feedback), not just eligibility and
//! freshness, and nothing they said they don't want.

use chrono::{DateTime, Utc};
use jobhunt_eligibility::{Assessment, Eligibility};
use jobhunt_jobs::verification::CompensationCheck;
use jobhunt_jobs::{JobId, JobRecord, OpportunityId};
use serde::{Deserialize, Serialize};

use crate::brief::{DecisionBrief, brief};
use crate::facets::{JobFacets, facets_of};
use crate::feedback::{OpportunityState, Stage};
use crate::person::Person;
use crate::signals::{self, Inputs, PayEvidence, Signal, SignalKind};
use crate::taste::TasteModel;

/// Revision of the signals, weights, gates and tiers. Part of every stored
/// ranking's key; bump it with any change that can rank a job differently.
pub const RANKING_VERSION: &str = "2";

/// Why an opportunity is not among the recommendations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Exclusion {
    Rejected,
    /// Applied, interviewing or offered: it is in the pipeline.
    InPipeline {
        stage: Stage,
    },
    Closed {
        why: String,
    },
    Ineligible {
        why: String,
    },
    /// Verified pay below a required minimum.
    BelowMinimum {
        why: String,
    },
    /// The posting states the opposite of a required company or team kind
    /// ("a public company" against "small companies required").
    UnmetRequirement {
        why: String,
    },
}

impl Exclusion {
    pub fn label(&self) -> String {
        match self {
            Self::Rejected => "you rejected it".into(),
            Self::InPipeline { stage } => format!("in your pipeline ({})", stage.as_str()),
            Self::Closed { why } => format!("closed: {why}"),
            Self::Ineligible { why } => format!("you can't take it: {why}"),
            Self::BelowMinimum { why } | Self::UnmetRequirement { why } => why.clone(),
        }
    }
}

/// Whether, and how, an opportunity is offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Gate {
    /// Trusted, and eligible (possibly on a condition).
    Recommended,
    /// Eligible (possibly on a condition), but the listing isn't verified
    /// recently enough to spend time on it yet.
    VerifyFirst {
        why: String,
    },
    /// Nothing rules the person out, but nothing confirms they can take it.
    EligibilityUnclear {
        why: String,
    },
    Excluded {
        exclusion: Exclusion,
    },
}

impl Gate {
    /// Higher is offered more readily.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Recommended => 3,
            Self::VerifyFirst { .. } => 2,
            Self::EligibilityUnclear { .. } => 1,
            Self::Excluded { .. } => 0,
        }
    }

    pub fn is_excluded(&self) -> bool {
        matches!(self, Self::Excluded { .. })
    }
}

/// How worth the person's time a job looks. Deliberately coarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    LowPriority,
    Maybe,
    WorthReviewing,
    StrongFit,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LowPriority => "low_priority",
            Self::Maybe => "maybe",
            Self::WorthReviewing => "worth_reviewing",
            Self::StrongFit => "strong_fit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LowPriority => "Low priority",
            Self::Maybe => "Maybe",
            Self::WorthReviewing => "Worth reviewing",
            Self::StrongFit => "Strong fit",
        }
    }
}

/// One ranked opportunity, with everything behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ranking {
    pub opportunity: OpportunityId,
    /// The record the ranking was read from (the strongest live one).
    pub job: JobId,
    pub title: String,
    pub company: String,
    pub gate: Gate,
    pub tier: Tier,
    /// Orders jobs within a tier. Not a percentage; shown only in details.
    pub score: f64,
    pub signals: Vec<Signal>,
    pub brief: DecisionBrief,
    /// The job, as read for ranking.
    pub facets: JobFacets,
    pub ranking_version: String,
    pub taste_digest: String,
}

/// One opportunity to rank.
pub struct Candidate<'a> {
    /// Every source record of the opportunity, in the order the assessment
    /// was made from.
    pub records: &'a [JobRecord],
    pub assessment: &'a Assessment,
    pub state: &'a OpportunityState,
}

impl Candidate<'_> {
    /// The record the opportunity's status rests on.
    pub fn representative(&self) -> Option<&JobRecord> {
        self.assessment
            .trust
            .best
            .and_then(|i| self.records.get(i))
            .or_else(|| self.records.first())
    }

    /// Verified compensation facts when there are any, else what discovery
    /// stored for the representative record.
    pub fn pay(&self) -> Option<PayEvidence> {
        let record = self.representative()?;
        let verified = self
            .assessment
            .trust
            .best()
            .and_then(|b| b.last_success.as_ref())
            .filter(|v| {
                v.compensation.status != jobhunt_jobs::verification::CompensationStatus::NotObserved
            });
        Some(match verified {
            Some(v) => PayEvidence {
                check: v.compensation.clone(),
                verified_at: Some(v.attempted_at),
                source: v.source.to_string(),
            },
            None => PayEvidence {
                check: CompensationCheck::observe(record.posting.compensation.as_ref(), None),
                verified_at: None,
                source: record.posting.provenance.source.to_string(),
            },
        })
    }
}

/// What every ranking shares.
pub struct Context<'a> {
    pub person: &'a Person,
    pub taste: &'a TasteModel,
    pub now: DateTime<Utc>,
}

fn gate(
    a: &Assessment,
    state: &OpportunityState,
    below_minimum: Option<String>,
    unmet_requirement: Option<String>,
) -> Gate {
    let excluded = |exclusion| Gate::Excluded { exclusion };
    if state.stage == Stage::Rejected {
        return excluded(Exclusion::Rejected);
    }
    if state.stage.in_progress() {
        return excluded(Exclusion::InPipeline { stage: state.stage });
    }
    if a.trust.state.is_closed() {
        return excluded(Exclusion::Closed {
            why: a.listing_reason().conclusion,
        });
    }
    if a.decision.status == Eligibility::Ineligible {
        return excluded(Exclusion::Ineligible {
            why: a.decision.headline.clone(),
        });
    }
    if let Some(why) = below_minimum {
        return excluded(Exclusion::BelowMinimum { why });
    }
    if let Some(why) = unmet_requirement {
        return excluded(Exclusion::UnmetRequirement { why });
    }
    match a.decision.status {
        Eligibility::Uncertain => Gate::EligibilityUnclear {
            why: a.decision.headline.clone(),
        },
        _ if !a.recommendable() => Gate::VerifyFirst {
            why: match &a.trust.standing {
                jobhunt_jobs::verification::Standing::NotTrusted(why) => why.clone(),
                jobhunt_jobs::verification::Standing::Trusted => String::new(),
            },
        },
        _ => Gate::Recommended,
    }
}

fn tier(signals: &[Signal], score: f64) -> Tier {
    let personal = signals
        .iter()
        .any(|s| s.weight > 0.0 && s.basis.is_personal());
    let refused = signals.iter().any(|s| s.weight <= -2.0);
    let tier = if score >= 4.0 && personal {
        Tier::StrongFit
    } else if score >= 1.5 {
        Tier::WorthReviewing
    } else if score >= -1.0 {
        Tier::Maybe
    } else {
        Tier::LowPriority
    };
    // Something the person said they don't want caps it.
    if refused { tier.min(Tier::Maybe) } else { tier }
}

/// Ranks one opportunity.
pub fn rank(candidate: &Candidate<'_>, ctx: &Context<'_>) -> Option<Ranking> {
    let record = candidate.representative()?;
    let facets = JobFacets::clone(&facets_of(record));
    let pay = candidate.pay()?;
    let inputs = Inputs {
        facets: &facets,
        person: ctx.person,
        taste: ctx.taste,
        state: candidate.state,
        assessment: candidate.assessment,
        compensation: &pay,
        now: ctx.now,
    };
    let mut all: Vec<Signal> = vec![
        signals::eligibility(candidate.assessment),
        signals::verification(candidate.assessment, ctx.now),
    ];
    all.extend(signals::feedback(&inputs));
    all.extend(signals::notes(&inputs, record.opportunity_id));
    all.extend(signals::role(&inputs));
    all.extend(signals::seniority(&inputs));
    all.extend(signals::stack(&inputs));
    all.extend(signals::domain(&inputs));
    let (pay_signals, below_minimum) = signals::compensation(&inputs);
    all.extend(pay_signals);
    let company = signals::company(&inputs);
    all.extend(company.signals);
    all.extend(signals::work_style(&inputs));
    all.extend(signals::work_mode(&inputs));
    all.extend(signals::freshness(&inputs));

    let score = all.iter().map(|s| s.weight).sum::<f64>();
    let score = (score * 100.0).round() / 100.0;
    let gate = gate(
        candidate.assessment,
        candidate.state,
        below_minimum,
        company.ruled_out,
    );
    let tier = tier(&all, score);
    // A requirement the posting says nothing about is not met: never a
    // strong fit on the strength of everything else.
    let tier = if company.unresolved {
        tier.min(Tier::WorthReviewing)
    } else {
        tier
    };
    let brief = brief(&facets, &pay, &gate, tier, &all, ctx.person);
    Some(Ranking {
        opportunity: record.opportunity_id,
        job: record.id,
        title: record.posting.title.clone(),
        company: record.posting.company.clone(),
        gate,
        tier,
        score,
        signals: all,
        brief,
        facets,
        ranking_version: RANKING_VERSION.to_owned(),
        taste_digest: ctx.taste.digest.clone(),
    })
}

/// Recommendations first, best first: by gate, tier, score, then the most
/// recently listed.
pub fn order(rankings: &mut [Ranking]) {
    rankings.sort_by(|a, b| {
        b.gate
            .rank()
            .cmp(&a.gate.rank())
            .then(b.tier.cmp(&a.tier))
            .then(b.score.total_cmp(&a.score))
            .then(b.facets.listed_at.cmp(&a.facets.listed_at))
            .then(a.opportunity.cmp(&b.opportunity))
    });
}

impl Ranking {
    /// The signals of one kind.
    pub fn of_kind(&self, kind: SignalKind) -> impl Iterator<Item = &Signal> {
        self.signals.iter().filter(move |s| s.kind == kind)
    }
}

#[cfg(test)]
mod tests;
