//! Running the rules: per job, per opportunity (several source records),
//! and with the listing gate on top.
//!
//! A job offers one or more ways of doing it (remote in some scope, an
//! office, a contractor path). Every option goes through every rule; the
//! decision is the best option's, and the others stay listed. For an
//! opportunity with several source records, each record is evaluated on
//! its own: records that agree give their answer, a silent record defers
//! to one that answers, and records that contradict each other make the
//! answer uncertain, with both kept.
//!
//! Verification and eligibility are separate answers. The listing gate
//! (rule 1) does not change the eligibility decision; it says whether the
//! job is trusted enough to recommend at all ([`Assessment::recommendable`]).

use chrono::{DateTime, Utc};
use jobhunt_jobs::JobRecord;
use jobhunt_jobs::verification::{
    Authority, FreshnessPolicy, OpportunityTrust, RecordVerification, Standing,
};

use crate::decision::{
    ConflictNote, Eligibility, EligibilityDecision, EvidenceRef, OptionDecision, RULES_VERSION,
    Reason, RuleId, Verdict,
};
use crate::job::{JobRequirements, requirements};
use crate::profile::ProfileFacts;
use crate::rules::{Context, RULES};

/// Every option of one job, through every rule.
pub fn options(job: &JobRequirements, profile: &ProfileFacts) -> Vec<OptionDecision> {
    job.options
        .iter()
        .map(|option| {
            let ctx = Context {
                job,
                option,
                profile,
            };
            let reasons: Vec<Reason> = RULES.iter().flat_map(|(_, rule)| rule(&ctx)).collect();
            OptionDecision::from_reasons(option.label(), reasons)
        })
        .collect()
}

/// The decision for one job's requirements.
pub fn evaluate(job: &JobRequirements, profile: &ProfileFacts) -> EligibilityDecision {
    let conflicts: Vec<ConflictNote> = job
        .conflicts
        .iter()
        .map(|c| ConflictNote {
            summary: c.summary.clone(),
            evidence: c.evidence.iter().map(EvidenceRef::from).collect(),
        })
        .collect();
    let mut decided = options(job, profile);
    if decided.is_empty() {
        let reason = Reason::new(
            RuleId::Ambiguity,
            Verdict::Unknown,
            "The posting doesn't say where or how the job is done",
        )
        .evidence(&job.unrecognized);
        return EligibilityDecision {
            status: Eligibility::Uncertain,
            headline: reason.conclusion.clone(),
            option: None,
            reasons: vec![reason],
            other_options: Vec::new(),
            conflicts,
            sources: vec![job.source.to_string()],
            rules_version: RULES_VERSION.to_owned(),
        };
    }
    // The best option; the earliest wins ties (remote comes first).
    let mut best = 0;
    for (i, d) in decided.iter().enumerate() {
        if d.status > decided[best].status {
            best = i;
        }
    }
    let chosen = decided.remove(best);
    EligibilityDecision {
        status: chosen.status,
        headline: chosen.headline(),
        option: Some(chosen.option),
        reasons: chosen.reasons,
        other_options: decided,
        conflicts,
        sources: vec![job.source.to_string()],
        rules_version: RULES_VERSION.to_owned(),
    }
}

/// The decision for one stored job.
pub fn evaluate_record(record: &JobRecord, profile: &ProfileFacts) -> EligibilityDecision {
    evaluate(&requirements(record), profile)
}

/// A source record to evaluate, with how much it is trusted.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub record: &'a JobRecord,
    pub authority: Authority,
}

/// The decision for an opportunity from its source records (live ones;
/// see [`assess`]).
pub fn evaluate_sources(sources: &[Source<'_>], profile: &ProfileFacts) -> EligibilityDecision {
    let mut decisions: Vec<(Authority, EligibilityDecision)> = sources
        .iter()
        .map(|s| (s.authority, evaluate_record(s.record, profile)))
        .collect();
    // Strongest source first (stable, so earlier records win ties).
    decisions.sort_by_key(|(authority, _)| std::cmp::Reverse(authority.rank()));
    let Some((_, first)) = decisions.first() else {
        return EligibilityDecision {
            status: Eligibility::Uncertain,
            headline: "No source records".into(),
            option: None,
            reasons: Vec::new(),
            other_options: Vec::new(),
            conflicts: Vec::new(),
            sources: Vec::new(),
            rules_version: RULES_VERSION.to_owned(),
        };
    };
    if decisions.len() == 1 {
        return first.clone();
    }
    let decisive: Vec<&(Authority, EligibilityDecision)> = decisions
        .iter()
        .filter(|(_, d)| d.status != Eligibility::Uncertain)
        .collect();
    let positive = decisive
        .iter()
        .any(|(_, d)| d.status >= Eligibility::Conditional);
    let negative = decisive
        .iter()
        .any(|(_, d)| d.status == Eligibility::Ineligible);
    let all_sources: Vec<String> = decisions
        .iter()
        .flat_map(|(_, d)| d.sources.clone())
        .collect();
    let mut conflicts: Vec<ConflictNote> = Vec::new();
    for (_, d) in &decisions {
        for c in &d.conflicts {
            if !conflicts.contains(c) {
                conflicts.push(c.clone());
            }
        }
    }
    if positive && negative {
        // The sources contradict each other: keep both answers.
        let mut reasons = vec![Reason::new(
            RuleId::Ambiguity,
            Verdict::Unknown,
            "The job's sources disagree about whether you can take it",
        )];
        for (_, d) in &decisive {
            let mut r = Reason::new(
                RuleId::Ambiguity,
                Verdict::NotApplicable,
                format!(
                    "{}: {} — {}",
                    d.sources.join(", "),
                    d.status.label(),
                    d.headline
                ),
            );
            if let Some(key) = d
                .reasons
                .iter()
                .find(|r| matches!(r.verdict, Verdict::Fail | Verdict::Pass))
            {
                r.evidence = key.evidence.clone();
            }
            reasons.push(r);
        }
        conflicts.push(ConflictNote {
            summary: format!(
                "Sources disagree: {}",
                decisive
                    .iter()
                    .map(|(_, d)| format!("{} says {}", d.sources.join(", "), d.status))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            evidence: Vec::new(),
        });
        return EligibilityDecision {
            status: Eligibility::Uncertain,
            headline: "The job's sources disagree about whether you can take it".into(),
            option: None,
            reasons,
            other_options: Vec::new(),
            conflicts,
            sources: all_sources,
            rules_version: RULES_VERSION.to_owned(),
        };
    }
    // Agreement, or silence from some: the strongest decisive answer.
    let mut chosen = decisive
        .first()
        .map(|(_, d)| d.clone())
        .unwrap_or_else(|| first.clone());
    for (_, d) in &decisions {
        if d.sources != chosen.sources {
            chosen.reasons.push(Reason::new(
                RuleId::Ambiguity,
                Verdict::NotApplicable,
                format!(
                    "Also listed on {}: {} — {}",
                    d.sources.join(", "),
                    d.status.label(),
                    d.headline
                ),
            ));
        }
    }
    chosen.conflicts = conflicts;
    chosen.sources = all_sources;
    chosen
}

/// A job's verification and eligibility, side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub trust: OpportunityTrust,
    pub decision: EligibilityDecision,
}

impl Assessment {
    /// Rule 1, the listing gate, as a reason.
    pub fn listing_reason(&self) -> Reason {
        match &self.trust.standing {
            Standing::Trusted => Reason::new(
                RuleId::Listing,
                Verdict::Pass,
                "The listing is verified active at an authoritative source",
            ),
            Standing::NotTrusted(why) => Reason::new(
                RuleId::Listing,
                if self.trust.state.is_closed() {
                    Verdict::Fail
                } else {
                    Verdict::Unknown
                },
                format!("Not trusted enough to recommend: {why}"),
            ),
        }
    }

    /// Trusted, and the person appears able to take it (possibly on a
    /// condition they have not ruled out).
    pub fn recommendable(&self) -> bool {
        self.trust.standing == Standing::Trusted && self.decision.status >= Eligibility::Conditional
    }
}

/// Verification trust and eligibility for an opportunity's records. Only
/// live records decide eligibility when there are any (a closed source
/// record says nothing about the job that is still open elsewhere).
pub fn assess(
    verified: &[RecordVerification],
    profile: &ProfileFacts,
    policy: &FreshnessPolicy,
    now: DateTime<Utc>,
) -> Assessment {
    let trust = trust(verified, policy, now);
    let live: Vec<Source<'_>> = verified
        .iter()
        .zip(&trust.records)
        .filter(|(_, t)| !t.state.is_closed())
        .map(|(v, t)| Source {
            record: &v.record,
            authority: t.authority,
        })
        .collect();
    let sources: Vec<Source<'_>> = if live.is_empty() {
        verified
            .iter()
            .zip(&trust.records)
            .map(|(v, t)| Source {
                record: &v.record,
                authority: t.authority,
            })
            .collect()
    } else {
        live
    };
    let decision = evaluate_sources(&sources, profile);
    Assessment { trust, decision }
}

/// The trust view of an opportunity's verified records.
pub fn trust(
    verified: &[RecordVerification],
    policy: &FreshnessPolicy,
    now: DateTime<Utc>,
) -> OpportunityTrust {
    let trusts: Vec<_> = verified.iter().map(|v| v.trust(policy, now)).collect();
    OpportunityTrust::of(trusts, policy, now)
}
