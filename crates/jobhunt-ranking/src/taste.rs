//! Taste learned from feedback, with the evidence behind every conclusion.
//!
//! Three things are kept apart:
//!
//! * **explicit preferences**, which the person set or stated
//!   ([`crate::person`]); they always win;
//! * **observed feedback**, which is what they did with jobs
//!   ([`crate::feedback`]);
//! * **derived taste**, meaning patterns across that feedback (this
//!   module).
//!
//! Every [`LearnedTaste`] answers: what pattern was inferred, from which
//! events, how many, whether they were reasons in the person's words or
//! only behavior, when it was last reinforced, and what contradicts it.
//! Nothing is hidden: a pattern too weak to use, or contradicted, is
//! listed with its evidence all the same.
//!
//! How evidence is weighed (direction, not exact science):
//!
//! | Evidence | Weight |
//! | --- | --- |
//! | a reason naming something ("pure SRE") | 1.0 |
//! | a reason pointing at the job ("this domain") | 0.75 per fact it points at |
//! | liked / disliked | ±0.6 |
//! | applied (interviewing 0.6, offer 0.7) | +0.5 |
//! | saved | +0.25 |
//! | rejected without a readable reason | −0.2 |
//! | rejected with a readable reason (the reason carries it) | −0.1 |
//! | only looked at | 0 |
//!
//! Behavior is spread over the job's facets (role, level, domains, company
//! kind, work style, required technologies, employer). Reasons count once
//! per event for what they name. A pattern is used only when enough
//! evidence agrees: one reason makes it tentative, two established, three
//! strong; behavior alone needs three jobs (tentative) or five
//! (established) and is never strong, so passive behavior can't produce a
//! strong preference. When evidence on both sides is comparable, the
//! pattern is **mixed** and not used at all: one rejected fintech job does
//! not blacklist fintech after two fintech applications.
//!
//! Reasons about only one job ("boring product", "unclear remote policy")
//! are kept as notes on that job and not generalized.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jobhunt_core::StableId;
use jobhunt_jobs::OpportunityId;
use serde::{Deserialize, Serialize};

use crate::facets::JobFacets;
use crate::feedback::{FeedbackAction, FeedbackId, OpportunityState, Sentiment, Stage};
use crate::key::{Direction, TasteKey};
use crate::reason::{JobReference, ReasonReader, Scope, Target};

/// Revision of the derivation rules and weights. Part of every ranking's
/// cache key.
pub const TASTE_VERSION: &str = "1";

const REASON_WEIGHT: f64 = 1.0;
const REFERENCE_WEIGHT: f64 = 0.75;

/// What a piece of evidence is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EvidenceKind {
    /// The person's words.
    Reason {
        /// The whole reason, verbatim.
        text: String,
        /// The part that was read.
        phrase: String,
        /// For a reference to the job ("this domain"), the fact it resolved
        /// to.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved: Option<String>,
    },
    /// What they did, spread over the job's facets.
    Behavior {
        /// `applied`, `saved`, `rejected`, …, and `liked` / `disliked`.
        what: String,
        /// The job's fact it counted for.
        fact: String,
    },
}

/// One piece of evidence for or against a pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasteEvidence {
    pub event: FeedbackId,
    pub opportunity: OpportunityId,
    pub title: String,
    pub company: String,
    pub action: FeedbackAction,
    pub at: DateTime<Utc>,
    pub direction: Direction,
    pub weight: f64,
    pub kind: EvidenceKind,
}

impl TasteEvidence {
    pub fn is_reason(&self) -> bool {
        matches!(self.kind, EvidenceKind::Reason { .. })
    }
}

/// How sure JobHunt is of a learned pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Tentative,
    Established,
    Strong,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tentative => "tentative",
            Self::Established => "established",
            Self::Strong => "strong",
        }
    }

    /// How much it moves a ranking.
    pub fn weight(self) -> f64 {
        match self {
            Self::Tentative => 0.5,
            Self::Established => 1.0,
            Self::Strong => 1.5,
        }
    }

    fn weaker(self) -> Self {
        match self {
            Self::Strong => Self::Established,
            Self::Established | Self::Tentative => Self::Tentative,
        }
    }
}

/// Whether a pattern is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TasteStatus {
    Active {
        confidence: Confidence,
    },
    /// Evidence on both sides is comparable: not used.
    Mixed,
    /// Too little evidence to use yet.
    NotEnough,
    /// The person stated a preference about the same thing, which is used
    /// instead.
    Explicit {
        preference: String,
        agrees: bool,
    },
}

/// A pattern inferred from feedback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnedTaste {
    pub key: TasteKey,
    /// The direction the evidence leans.
    pub direction: Direction,
    pub status: TasteStatus,
    /// Positive toward, negative away (sum of weights).
    pub net: f64,
    /// Evidence in the leaning direction, newest first.
    pub support: Vec<TasteEvidence>,
    /// Evidence the other way, newest first.
    pub against: Vec<TasteEvidence>,
    /// Supporting evidence that is the person's words.
    pub reasons: usize,
    /// Distinct opportunities behind the support.
    pub opportunities: usize,
    pub last_reinforced: DateTime<Utc>,
}

impl LearnedTaste {
    /// How this pattern moves a ranking, when it is used.
    pub fn effect(&self) -> Option<(Direction, Confidence)> {
        match &self.status {
            TasteStatus::Active { confidence } => Some((self.direction, *confidence)),
            _ => None,
        }
    }

    pub fn behavior_only(&self) -> bool {
        self.reasons == 0
    }

    /// "3 reasons in your words and 2 actions without a reason", for
    /// display. An action that came with a reason counts as the reason.
    pub fn basis(&self) -> String {
        let with_reason: Vec<FeedbackId> = self
            .support
            .iter()
            .filter(|e| e.is_reason())
            .map(|e| e.event)
            .collect();
        let behaviors = self
            .support
            .iter()
            .filter(|e| !e.is_reason() && !with_reason.contains(&e.event))
            .count();
        let mut parts = Vec::new();
        if self.reasons > 0 {
            parts.push(plural(
                self.reasons,
                "reason in your words",
                "reasons in your words",
            ));
        }
        if behaviors > 0 {
            parts.push(plural(
                behaviors,
                "action without a reason",
                "actions without a reason",
            ));
        }
        parts.join(" and ")
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A reason about one job only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpportunityNote {
    pub opportunity: OpportunityId,
    pub key: TasteKey,
    pub direction: Direction,
    pub phrase: String,
    pub reason: String,
    pub action: FeedbackAction,
    pub at: DateTime<Utc>,
}

/// A reason nothing could be read from, kept as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnreadReason {
    pub event: FeedbackId,
    pub opportunity: OpportunityId,
    pub title: String,
    pub company: String,
    pub action: FeedbackAction,
    pub reason: String,
    pub at: DateTime<Utc>,
}

/// One opportunity the person gave feedback on, with what it is.
#[derive(Debug, Clone)]
pub struct FeedbackOpportunity {
    /// The opportunity now (after any identity merges).
    pub opportunity: OpportunityId,
    pub state: OpportunityState,
    /// `None` when its records are gone: its reasons still count for what
    /// they name, but behavior can't be spread over facets.
    pub facets: Option<JobFacets>,
    /// Content fingerprints of what the facets were read from.
    pub fingerprint: String,
}

/// Everything learned, with its evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TasteModel {
    /// Strongest effect first, then most evidence.
    pub learned: Vec<LearnedTaste>,
    pub notes: Vec<OpportunityNote>,
    pub unread: Vec<UnreadReason>,
    /// Events with feedback (not counting "seen").
    pub events: usize,
    pub opportunities: usize,
    pub reader: String,
    /// `taste_<32 hex>` of every input: part of ranking cache keys.
    pub digest: String,
}

impl TasteModel {
    /// No feedback yet.
    pub fn empty(reader: &str) -> Self {
        let mut parts = [TASTE_VERSION, reader];
        parts.sort_unstable();
        Self {
            learned: Vec::new(),
            notes: Vec::new(),
            unread: Vec::new(),
            events: 0,
            opportunities: 0,
            reader: reader.to_owned(),
            digest: format!("taste_{}", StableId::derive("jobhunt.taste.v1", &parts)),
        }
    }

    /// The pattern about a key, if any evidence exists.
    pub fn get(&self, key: &TasteKey) -> Option<&LearnedTaste> {
        self.learned.iter().find(|l| &l.key == key)
    }

    /// Patterns in use.
    pub fn active(&self) -> impl Iterator<Item = &LearnedTaste> {
        self.learned.iter().filter(|l| l.effect().is_some())
    }

    /// Notes on one opportunity.
    pub fn notes_on(&self, opportunity: OpportunityId) -> Vec<&OpportunityNote> {
        self.notes
            .iter()
            .filter(|n| n.opportunity == opportunity)
            .collect()
    }
}

/// The behavior weight of an opportunity's final state.
fn behavior(state: &OpportunityState, reject_reason_read: bool) -> (f64, String) {
    let (mut weight, mut what) = match state.stage {
        Stage::Saved => (0.25, vec!["saved"]),
        Stage::Applied => (0.5, vec!["applied"]),
        Stage::Interviewing => (0.6, vec!["applied, interviewing"]),
        Stage::Offer => (0.7, vec!["applied, offer"]),
        Stage::Rejected if reject_reason_read => (-0.1, vec!["rejected"]),
        Stage::Rejected => (-0.2, vec!["rejected without a reason"]),
        Stage::Seen | Stage::Unseen => (0.0, vec![]),
    };
    match state.sentiment {
        Some(Sentiment::Liked) => {
            weight += 0.6;
            what.push("liked");
        }
        Some(Sentiment::Disliked) => {
            weight -= 0.6;
            what.push("disliked");
        }
        None => {}
    }
    (weight, what.join(", "))
}

/// The facts a reference to the job points at.
fn resolve(reference: JobReference, facets: &JobFacets) -> Vec<(TasteKey, String)> {
    use crate::key::Dimension;
    match reference {
        JobReference::Domain => facets
            .domains
            .iter()
            .map(|f| (f.key.clone(), f.cite()))
            .collect(),
        JobReference::Stack => facets
            .technologies_at(crate::facets::Requirement::Required)
            .into_iter()
            .take(5)
            .map(|t| {
                (
                    TasteKey::new(Dimension::Technology, t.name.clone()),
                    format!("{} ({})", t.name, t.requirement.as_str()),
                )
            })
            .collect(),
        JobReference::Company => vec![(
            crate::facets::company_key(&facets.company),
            format!("at {}", facets.company),
        )],
        JobReference::Level => facets
            .level
            .iter()
            .map(|(level, words)| {
                (
                    TasteKey::new(Dimension::Seniority, level.as_str()),
                    format!("“{words}” (title)"),
                )
            })
            .collect(),
    }
}

/// Derives taste from feedback. `explicit` are the person's stated
/// preferences about single keys (see
/// [`Person::explicit_keys`](crate::person::Person::explicit_keys)).
pub fn derive(
    opportunities: &[FeedbackOpportunity],
    explicit: &[(TasteKey, Direction, String)],
    reader: &dyn ReasonReader,
) -> TasteModel {
    let mut evidence: BTreeMap<TasteKey, Vec<TasteEvidence>> = BTreeMap::new();
    let mut notes = Vec::new();
    let mut unread = Vec::new();
    let mut events = 0;
    let mut digest_parts: Vec<String> =
        vec![TASTE_VERSION.to_owned(), reader.revision().to_owned()];
    for opp in opportunities {
        let state = &opp.state;
        digest_parts.push(format!("{}|{}", opp.opportunity, opp.fingerprint));
        let mut reject_reason_read = false;
        for event in &state.events {
            digest_parts.push(event.id.to_string());
            if event.action == FeedbackAction::Seen {
                continue;
            }
            events += 1;
            let Some(text) = &event.reason else { continue };
            let reading = reader.read(text, event.action);
            if reading.is_unread() {
                unread.push(UnreadReason {
                    event: event.id,
                    opportunity: opp.opportunity,
                    title: event.title.clone(),
                    company: event.company.clone(),
                    action: event.action,
                    reason: text.clone(),
                    at: event.at,
                });
                continue;
            }
            if event.action == FeedbackAction::Reject {
                reject_reason_read = true;
            }
            // Each key counts once per event.
            let mut seen: Vec<TasteKey> = Vec::new();
            for signal in reading.signals {
                let resolved: Vec<(TasteKey, Option<String>, f64)> = match &signal.target {
                    Target::Key { key } => vec![(key.clone(), None, REASON_WEIGHT)],
                    Target::Job { reference } => opp
                        .facets
                        .as_ref()
                        .map(|f| resolve(*reference, f))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(k, fact)| (k, Some(fact), REFERENCE_WEIGHT))
                        .collect(),
                };
                for (key, fact, weight) in resolved {
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key.clone());
                    if signal.scope == Scope::ThisOpportunity {
                        notes.push(OpportunityNote {
                            opportunity: opp.opportunity,
                            key,
                            direction: signal.direction,
                            phrase: signal.phrase.clone(),
                            reason: text.clone(),
                            action: event.action,
                            at: event.at,
                        });
                        continue;
                    }
                    evidence.entry(key).or_default().push(TasteEvidence {
                        event: event.id,
                        opportunity: opp.opportunity,
                        title: event.title.clone(),
                        company: event.company.clone(),
                        action: event.action,
                        at: event.at,
                        direction: signal.direction,
                        weight: signal.direction.sign() * weight,
                        kind: EvidenceKind::Reason {
                            text: text.clone(),
                            phrase: signal.phrase.clone(),
                            resolved: fact,
                        },
                    });
                }
            }
        }
        let (weight, what) = behavior(state, reject_reason_read);
        let (Some(facets), Some(last), Some(direction)) = (
            &opp.facets,
            state
                .events
                .iter()
                .rev()
                .find(|e| e.action != FeedbackAction::Seen),
            Direction::of(weight),
        ) else {
            continue;
        };
        for (key, fact) in facets.keys() {
            evidence.entry(key).or_default().push(TasteEvidence {
                event: last.id,
                opportunity: opp.opportunity,
                title: last.title.clone(),
                company: last.company.clone(),
                action: last.action,
                at: last.at,
                direction,
                weight,
                kind: EvidenceKind::Behavior {
                    what: what.clone(),
                    fact,
                },
            });
        }
    }

    let mut learned: Vec<LearnedTaste> = evidence
        .into_iter()
        .map(|(key, items)| conclude(key, items, explicit))
        .collect();
    learned.sort_by(|a, b| {
        let strength = |l: &LearnedTaste| l.effect().map(|(_, c)| c.weight()).unwrap_or(0.0);
        strength(b)
            .total_cmp(&strength(a))
            .then(b.net.abs().total_cmp(&a.net.abs()))
            .then(a.key.cmp(&b.key))
    });
    digest_parts.sort();
    let refs: Vec<&str> = digest_parts.iter().map(String::as_str).collect();
    TasteModel {
        learned,
        notes,
        unread,
        events,
        opportunities: opportunities
            .iter()
            .filter(|o| o.state.has_feedback())
            .count(),
        reader: reader.revision().to_owned(),
        digest: format!("taste_{}", StableId::derive("jobhunt.taste.v1", &refs)),
    }
}

fn distinct_opportunities(items: &[TasteEvidence]) -> usize {
    let mut opps: Vec<OpportunityId> = items.iter().map(|e| e.opportunity).collect();
    opps.sort();
    opps.dedup();
    opps.len()
}

/// How sure one side's evidence alone would make a pattern: one reason is
/// tentative, two established, three strong; behavior alone needs three
/// jobs (tentative) or five (established), and is never strong.
fn side_confidence(items: &[TasteEvidence]) -> Option<Confidence> {
    let mut reason_events: Vec<FeedbackId> = items
        .iter()
        .filter(|e| e.is_reason())
        .map(|e| e.event)
        .collect();
    reason_events.sort();
    reason_events.dedup();
    let weight: f64 = items.iter().map(|e| e.weight.abs()).sum();
    let opps = distinct_opportunities(items);
    match reason_events.len() {
        n if n >= 3 => Some(Confidence::Strong),
        2 => Some(Confidence::Established),
        1 => Some(Confidence::Tentative),
        _ if opps >= 5 && weight >= 1.0 => Some(Confidence::Established),
        _ if opps >= 3 && weight >= 0.6 => Some(Confidence::Tentative),
        _ => None,
    }
}

fn conclude(
    key: TasteKey,
    mut items: Vec<TasteEvidence>,
    explicit: &[(TasteKey, Direction, String)],
) -> LearnedTaste {
    items.sort_by(|a, b| b.at.cmp(&a.at).then(a.event.cmp(&b.event)));
    let positive: f64 = items
        .iter()
        .filter(|e| e.weight > 0.0)
        .map(|e| e.weight)
        .sum();
    let negative: f64 = items
        .iter()
        .filter(|e| e.weight < 0.0)
        .map(|e| -e.weight)
        .sum();
    let net = positive - negative;
    let direction = if net >= 0.0 {
        Direction::Prefer
    } else {
        Direction::Avoid
    };
    let (support, against): (Vec<TasteEvidence>, Vec<TasteEvidence>) =
        items.into_iter().partition(|e| e.direction == direction);
    let reasons = support.iter().filter(|e| e.is_reason()).count();
    let opportunities = distinct_opportunities(&support);
    let (big, small) = if positive >= negative {
        (positive, negative)
    } else {
        (negative, positive)
    };
    let confidence = side_confidence(&support).map(|c| if small > 0.0 { c.weaker() } else { c });
    // Contradictions matter once either side would be a pattern on its
    // own; two passive actions against one are just too little evidence.
    let mixed = small > 0.0
        && small / big >= 0.5
        && (confidence.is_some() || side_confidence(&against).is_some());
    let status = if let Some((_, stated, text)) = explicit.iter().find(|(k, ..)| *k == key) {
        TasteStatus::Explicit {
            preference: text.clone(),
            agrees: !mixed && *stated == direction,
        }
    } else if mixed {
        TasteStatus::Mixed
    } else {
        match confidence {
            Some(confidence) => TasteStatus::Active { confidence },
            None => TasteStatus::NotEnough,
        }
    };
    let last_reinforced = support
        .iter()
        .map(|e| e.at)
        .max()
        .or_else(|| against.iter().map(|e| e.at).max())
        .unwrap_or_default();
    LearnedTaste {
        key,
        direction,
        status,
        net,
        reasons,
        opportunities,
        support,
        against,
        last_reinforced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facets::facets;
    use crate::key::Dimension;
    use crate::reason::RuleReader;
    use crate::testing::{event_on, record};

    /// An opportunity with these actions, the job read from `title` and
    /// `description`.
    fn opp(
        n: u32,
        title: &str,
        description: &str,
        actions: &[(FeedbackAction, Option<&str>)],
    ) -> FeedbackOpportunity {
        let record = record(&format!("ashby:c{n}"), title, description);
        let events = actions
            .iter()
            .enumerate()
            .map(|(i, (a, r))| event_on(&record, *a, *r, i64::from(n) * 10 + i as i64))
            .collect::<Vec<_>>();
        FeedbackOpportunity {
            opportunity: record.opportunity_id,
            state: OpportunityState::of(events),
            facets: Some(facets(&record)),
            fingerprint: record.posting.content_fingerprint().to_hex(),
        }
    }

    fn key(d: Dimension, v: &str) -> TasteKey {
        TasteKey::new(d, v)
    }

    #[test]
    fn repeated_reasons_become_strong_with_their_evidence() {
        let opps: Vec<_> = (0..3)
            .map(|n| {
                opp(
                    n,
                    "Site Reliability Engineer",
                    "",
                    &[(FeedbackAction::Reject, Some("pure SRE"))],
                )
            })
            .collect();
        let model = derive(&opps, &[], &RuleReader);
        let sre = model.get(&TasteKey::role("sre")).unwrap();
        assert_eq!(sre.direction, Direction::Avoid);
        assert_eq!(
            sre.status,
            TasteStatus::Active {
                confidence: Confidence::Strong
            }
        );
        assert_eq!(sre.reasons, 3);
        assert_eq!(sre.opportunities, 3);
        assert!(sre.against.is_empty());
        let EvidenceKind::Reason { text, phrase, .. } = &sre.support[0].kind else {
            panic!("reason evidence first");
        };
        assert_eq!((text.as_str(), phrase.as_str()), ("pure SRE", "SRE"));
        assert_eq!(sre.last_reinforced, sre.support[0].at);
        assert_eq!(
            model.learned[0].key,
            TasteKey::role("sre"),
            "strongest first"
        );
    }

    #[test]
    fn a_rejection_without_a_reason_teaches_almost_nothing() {
        let one = [opp(
            0,
            "Site Reliability Engineer",
            "",
            &[(FeedbackAction::Reject, None)],
        )];
        let model = derive(&one, &[], &RuleReader);
        assert!(model.active().next().is_none());
        let sre = model.get(&TasteKey::role("sre")).unwrap();
        assert_eq!(sre.status, TasteStatus::NotEnough);
        // Three of them: a tentative, behavior-only pattern, never strong.
        let three: Vec<_> = (0..3)
            .map(|n| {
                opp(
                    n,
                    "Site Reliability Engineer",
                    "",
                    &[(FeedbackAction::Reject, None)],
                )
            })
            .collect();
        let model = derive(&three, &[], &RuleReader);
        let sre = model.get(&TasteKey::role("sre")).unwrap();
        assert_eq!(
            sre.status,
            TasteStatus::Active {
                confidence: Confidence::Tentative
            }
        );
        assert!(sre.behavior_only());
        let ten: Vec<_> = (0..10)
            .map(|n| {
                opp(
                    n,
                    "Site Reliability Engineer",
                    "",
                    &[(FeedbackAction::Reject, None)],
                )
            })
            .collect();
        let model = derive(&ten, &[], &RuleReader);
        assert_ne!(
            model
                .get(&TasteKey::role("sre"))
                .unwrap()
                .effect()
                .map(|e| e.1),
            Some(Confidence::Strong)
        );
    }

    #[test]
    fn contradictions_are_kept_and_not_used() {
        let opps = vec![
            opp(
                0,
                "Backend Engineer",
                "",
                &[(FeedbackAction::Reject, Some("fintech"))],
            ),
            opp(
                1,
                "Infrastructure Engineer, Fintech",
                "",
                &[
                    (FeedbackAction::Save, None),
                    (FeedbackAction::Applied, None),
                ],
            ),
            opp(
                2,
                "Senior Infrastructure Engineer (Fintech)",
                "",
                &[
                    (FeedbackAction::Save, None),
                    (FeedbackAction::Applied, None),
                ],
            ),
        ];
        let model = derive(&opps, &[], &RuleReader);
        let fintech = model.get(&key(Dimension::Domain, "fintech")).unwrap();
        assert_eq!(fintech.status, TasteStatus::Mixed);
        assert_eq!(fintech.effect(), None);
        assert!(!fintech.support.is_empty() && !fintech.against.is_empty());
        // The role within the domain is its own pattern.
        let infra = model.get(&TasteKey::role("infrastructure")).unwrap();
        assert_eq!(infra.direction, Direction::Prefer);
        assert_eq!(infra.opportunities, 2);
    }

    #[test]
    fn explicit_preferences_win() {
        let opps: Vec<_> = (0..3)
            .map(|n| {
                opp(
                    n,
                    "Backend Engineer",
                    "",
                    &[(FeedbackAction::Reject, Some("too corporate"))],
                )
            })
            .collect();
        let stated = [(
            key(Dimension::CompanyTrait, "large_company"),
            Direction::Prefer,
            "large companies".to_owned(),
        )];
        let model = derive(&opps, &stated, &RuleReader);
        let corporate = model
            .get(&key(Dimension::CompanyTrait, "large_company"))
            .unwrap();
        assert_eq!(
            corporate.status,
            TasteStatus::Explicit {
                preference: "large companies".into(),
                agrees: false
            }
        );
        assert_eq!(
            corporate.effect(),
            None,
            "the stated preference is used instead"
        );
        assert_eq!(corporate.reasons, 3, "the evidence is still there");
    }

    #[test]
    fn references_resolve_to_the_job_and_single_job_reasons_stay_notes() {
        let opps = vec![opp(
            0,
            "Senior Engineer, Payments",
            "",
            &[(
                FeedbackAction::Reject,
                Some("already worked with this domain; boring product"),
            )],
        )];
        let model = derive(&opps, &[], &RuleReader);
        let payments = model.get(&key(Dimension::Domain, "payments")).unwrap();
        assert_eq!(payments.direction, Direction::Avoid);
        let EvidenceKind::Reason { resolved, .. } = &payments.support[0].kind else {
            panic!("reason");
        };
        assert_eq!(resolved.as_deref(), Some("“Payments” (title)"));
        assert_eq!(model.notes.len(), 1);
        assert_eq!(model.notes[0].key, key(Dimension::Product, "interest"));
        assert!(model.get(&key(Dimension::Product, "interest")).is_none());
    }

    #[test]
    fn unread_reasons_are_kept_as_written() {
        let opps = vec![opp(
            0,
            "Backend Engineer",
            "",
            &[(FeedbackAction::Reject, Some("meh vibes"))],
        )];
        let model = derive(&opps, &[], &RuleReader);
        assert_eq!(model.unread.len(), 1);
        assert_eq!(model.unread[0].reason, "meh vibes");
        assert_eq!(model.events, 1);
    }

    #[test]
    fn the_digest_changes_with_any_feedback() {
        let a = vec![opp(
            0,
            "Backend Engineer",
            "",
            &[(FeedbackAction::Save, None)],
        )];
        let b = vec![opp(
            0,
            "Backend Engineer",
            "",
            &[(FeedbackAction::Reject, None)],
        )];
        let da = derive(&a, &[], &RuleReader).digest;
        assert_eq!(da, derive(&a, &[], &RuleReader).digest);
        assert_ne!(da, derive(&b, &[], &RuleReader).digest);
        assert!(da.starts_with("taste_"));
    }

    #[test]
    fn likes_and_applications_outweigh_saves() {
        let liked: Vec<_> = (0..3)
            .map(|n| opp(n, "Backend Engineer", "", &[(FeedbackAction::Like, None)]))
            .collect();
        let saved: Vec<_> = (0..3)
            .map(|n| opp(n, "Backend Engineer", "", &[(FeedbackAction::Save, None)]))
            .collect();
        let net = |opps: &[FeedbackOpportunity]| {
            derive(opps, &[], &RuleReader)
                .get(&TasteKey::role("backend"))
                .unwrap()
                .net
        };
        assert!(net(&liked) > net(&saved));
        let two_saves = derive(&saved[..2], &[], &RuleReader);
        assert_eq!(
            two_saves.get(&TasteKey::role("backend")).unwrap().status,
            TasteStatus::NotEnough,
            "two saves are not a pattern"
        );
        assert_eq!(
            TasteModel::empty(RuleReader.revision()).digest,
            derive(&[], &[], &RuleReader).digest
        );
    }
}
