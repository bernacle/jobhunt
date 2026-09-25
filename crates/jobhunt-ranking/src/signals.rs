//! Deterministic ranking signals, one group at a time.
//!
//! Each [`Signal`] is independently inspectable: which group it belongs
//! to, what it rests on ([`Basis`]: a stated preference, learned taste,
//! resume evidence, the posting, the person's feedback, verification or
//! eligibility), a one-line summary, the evidence behind it, and a weight.
//! Weights only order jobs; they are never shown as a match percentage.
//!
//! Rough scale: ±3 for what the person explicitly wants or refuses, ±2 for
//! what they want, 1–1.5 for learned taste and pay, 0.5 for experience
//! and verification, 0.25 for freshness. Unknowns weigh nothing: pay that
//! isn't published is unknown, not low.

use chrono::{DateTime, Utc};
use jobhunt_eligibility::{Assessment, Eligibility};
use jobhunt_jobs::PayInterval;
use jobhunt_jobs::verification::{
    CompensationCheck, CompensationStatus, CurrencyEvidence, PayRange, Standing, TrustState,
    VerificationAge, ago,
};
use jobhunt_profile::preferences::group_thousands;
use jobhunt_profile::{
    Arrangement, CompensationBound, EvidenceStrength, PayPeriod, Stance, WorkMode,
};
use serde::{Deserialize, Serialize};

use crate::facets::{JobFacets, JobFunction, Requirement};
use crate::feedback::{OpportunityState, Sentiment, Stage};
use crate::key::{Dimension, Direction, TasteKey};
use crate::person::{Matcher, PayPreference, Person, StatedPreference};
use crate::taste::{EvidenceKind, TasteModel};

/// What a signal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalGroup {
    Eligibility,
    Verification,
    Feedback,
    Role,
    Seniority,
    Stack,
    Domain,
    Compensation,
    Company,
    WorkStyle,
    WorkMode,
    Freshness,
}

impl SignalGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eligibility => "eligibility",
            Self::Verification => "verification",
            Self::Feedback => "feedback",
            Self::Role => "role",
            Self::Seniority => "seniority",
            Self::Stack => "stack",
            Self::Domain => "domain",
            Self::Compensation => "pay",
            Self::Company => "company",
            Self::WorkStyle => "work style",
            Self::WorkMode => "work mode",
            Self::Freshness => "freshness",
        }
    }

    fn of(dimension: Dimension) -> Self {
        match dimension {
            Dimension::Role => Self::Role,
            Dimension::Seniority => Self::Seniority,
            Dimension::WorkStyle => Self::WorkStyle,
            Dimension::Technology => Self::Stack,
            Dimension::Domain => Self::Domain,
            Dimension::CompanyTrait | Dimension::Company => Self::Company,
            Dimension::Compensation => Self::Compensation,
            Dimension::Product | Dimension::Information => Self::Feedback,
        }
    }
}

/// What a signal rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// A preference the person set or stated.
    Stated,
    /// A pattern learned from their feedback.
    Learned,
    /// Their resume.
    Experience,
    /// What the posting says, on its own.
    Posting,
    /// What they did with this job.
    Feedback,
    Verification,
    Eligibility,
}

impl Basis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stated => "your preference",
            Self::Learned => "learned from your feedback",
            Self::Experience => "your resume",
            Self::Posting => "the posting",
            Self::Feedback => "your feedback",
            Self::Verification => "verification",
            Self::Eligibility => "eligibility",
        }
    }

    /// Whether it says something about what the person wants.
    pub fn is_personal(self) -> bool {
        matches!(self, Self::Stated | Self::Learned | Self::Feedback)
    }
}

/// How a signal reads in a brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// A reason it may be worth attention.
    Plus,
    /// A reason it may not be.
    Minus,
    /// It holds only if something is true (relocating, the low end of a
    /// range).
    Condition,
    /// Something that isn't known.
    Unknown,
    /// Worth knowing, neither for nor against.
    Context,
    /// Rules it out of recommendations.
    Blocker,
}

/// One inspectable reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub group: SignalGroup,
    pub basis: Basis,
    pub kind: SignalKind,
    /// For ordering only.
    pub weight: f64,
    /// One line.
    pub summary: String,
    /// What it rests on: the posting's words, the preference, the feedback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

impl Signal {
    pub fn new(group: SignalGroup, basis: Basis, weight: f64, summary: impl Into<String>) -> Self {
        let kind = if weight > 0.0 {
            SignalKind::Plus
        } else if weight < 0.0 {
            SignalKind::Minus
        } else {
            SignalKind::Context
        };
        Self {
            group,
            basis,
            kind,
            weight,
            summary: summary.into(),
            evidence: Vec::new(),
        }
    }

    pub fn kind(mut self, kind: SignalKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn evidence(mut self, evidence: impl IntoIterator<Item = String>) -> Self {
        for e in evidence {
            if !self.evidence.contains(&e) {
                self.evidence.push(e);
            }
        }
        self
    }
}

/// Everything a group needs.
pub struct Inputs<'a> {
    pub facets: &'a JobFacets,
    pub person: &'a Person,
    pub taste: &'a TasteModel,
    pub state: &'a OpportunityState,
    pub assessment: &'a Assessment,
    pub compensation: &'a PayEvidence,
    pub now: DateTime<Utc>,
}

/// The compensation a ranking reads: the latest successful verification's
/// facts when there is one, else what discovery stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PayEvidence {
    pub check: CompensationCheck,
    /// When it was verified; `None` for what discovery stored.
    pub verified_at: Option<DateTime<Utc>>,
    /// `greenhouse:figma`.
    pub source: String,
}

impl PayEvidence {
    fn provenance(&self, now: DateTime<Utc>) -> String {
        match self.verified_at {
            Some(at) => format!("verified at {} {}", self.source, ago(now - at)),
            None => format!(
                "as {} published it at discovery (not verified)",
                self.source
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Eligibility and verification.

pub fn eligibility(a: &Assessment) -> Signal {
    let d = &a.decision;
    let group = SignalGroup::Eligibility;
    let basis = Basis::Eligibility;
    let evidence: Vec<String> = d
        .reasons
        .iter()
        .filter(|r| r.verdict != jobhunt_eligibility::Verdict::NotApplicable)
        .take(2)
        .map(|r| r.conclusion.clone())
        .collect();
    match d.status {
        Eligibility::Eligible => {
            Signal::new(group, basis, 0.5, format!("Eligible: {}", d.headline))
        }
        Eligibility::Conditional => {
            Signal::new(group, basis, 0.0, format!("Conditional: {}", d.headline))
                .kind(SignalKind::Condition)
        }
        Eligibility::Uncertain => Signal::new(
            group,
            basis,
            0.0,
            format!("Eligibility unclear: {}", d.headline),
        )
        .kind(SignalKind::Unknown),
        Eligibility::Ineligible => Signal::new(
            group,
            basis,
            0.0,
            format!("You can't take it: {}", d.headline),
        )
        .kind(SignalKind::Blocker),
    }
    .evidence(evidence)
}

pub fn verification(a: &Assessment, now: DateTime<Utc>) -> Signal {
    let group = SignalGroup::Verification;
    let basis = Basis::Verification;
    let best = a.trust.best();
    let when = best
        .and_then(|b| b.last_success.as_ref())
        .map(|v| ago(now - v.attempted_at))
        .unwrap_or_default();
    let who = best.map(|b| b.authority.label()).unwrap_or("its source");
    match &a.trust.standing {
        Standing::Trusted => match best.and_then(|b| b.age) {
            Some(VerificationAge::Aging) => Signal::new(
                group,
                basis,
                0.25,
                format!("Verified active {when} on {who}; verify again before applying"),
            ),
            _ => Signal::new(
                group,
                basis,
                0.5,
                format!("Verified active {when} on {who}"),
            ),
        },
        Standing::NotTrusted(why) if a.trust.state.is_closed() => {
            Signal::new(group, basis, 0.0, format!("Closed: {why}")).kind(SignalKind::Blocker)
        }
        Standing::NotTrusted(why) => {
            let summary = if a.trust.state == TrustState::NotVerified {
                "Not verified yet: it may be closed or changed".to_owned()
            } else {
                format!("Not trusted enough to recommend yet: {why}")
            };
            Signal::new(group, basis, 0.0, summary).kind(SignalKind::Unknown)
        }
    }
}

// ---------------------------------------------------------------------------
// The person's own history with this job.

pub fn feedback(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::Feedback;
    let basis = Basis::Feedback;
    let mut out = Vec::new();
    let date = |at: DateTime<Utc>| at.format("%Y-%m-%d").to_string();
    let quoted = |e: Option<&crate::feedback::FeedbackEvent>| {
        e.and_then(|e| e.reason.as_ref())
            .map(|r| format!(": “{r}”"))
            .unwrap_or_default()
    };
    match i.state.stage {
        Stage::Saved => {
            let at = i.state.since().map(date).unwrap_or_default();
            let why = quoted(i.state.last_reason(crate::feedback::FeedbackAction::Save));
            out.push(Signal::new(
                group,
                basis,
                0.5,
                format!("You saved it on {at}{why}"),
            ));
        }
        Stage::Rejected => {
            let why = quoted(i.state.last_reason(crate::feedback::FeedbackAction::Reject));
            out.push(
                Signal::new(group, basis, 0.0, format!("You rejected it{why}"))
                    .kind(SignalKind::Blocker),
            );
        }
        stage if stage.in_progress() => {
            out.push(
                Signal::new(
                    group,
                    basis,
                    0.0,
                    format!("Already in your pipeline ({})", stage.as_str()),
                )
                .kind(SignalKind::Blocker),
            );
        }
        _ => {}
    }
    match i.state.sentiment {
        Some(Sentiment::Liked) => {
            let why = quoted(i.state.last_reason(crate::feedback::FeedbackAction::Like));
            out.push(Signal::new(group, basis, 1.0, format!("You liked it{why}")));
        }
        Some(Sentiment::Disliked) => {
            let why = quoted(
                i.state
                    .last_reason(crate::feedback::FeedbackAction::Dislike),
            );
            out.push(Signal::new(
                group,
                basis,
                -2.0,
                format!("You disliked it{why}"),
            ));
        }
        None => {}
    }
    out
}

// ---------------------------------------------------------------------------
// Stated preferences and learned taste.

/// Whether a job has what a stated preference is about, with the job's
/// words.
fn matches(p: &StatedPreference, facets: &JobFacets) -> Option<Vec<String>> {
    match &p.matcher {
        Matcher::Keys(keys) => keys
            .iter()
            .map(|k| facets.evidence_for(k))
            .collect::<Option<Vec<String>>>(),
        Matcher::TitleWords(words) => {
            let title = jobhunt_core::text::search_key(&facets.title);
            (format!(" {title} ").contains(&format!(" {words} ")))
                .then(|| vec![format!("“{}” (title)", facets.title)])
        }
    }
}

fn stated_weight(p: &StatedPreference) -> f64 {
    match (p.dimension, p.stance) {
        (_, Stance::Required | Stance::Wanted) if p.dimension == Dimension::Role => 2.0,
        (Dimension::Domain, Stance::Required | Stance::Wanted) => 2.0,
        (_, Stance::Required | Stance::Wanted) => 1.5,
        (_, Stance::Acceptable) => 0.5,
        (Dimension::Role | Dimension::Domain, Stance::Unwanted) => -3.0,
        (_, Stance::Unwanted) => -2.0,
    }
}

fn stated_summary(p: &StatedPreference) -> String {
    let what = match p.dimension {
        Dimension::Role => "role",
        Dimension::Domain => "domain",
        Dimension::WorkStyle => "way of working",
        _ => "kind of company or team",
    };
    let doubt = if p.uncertain {
        " (read with doubts)"
    } else {
        ""
    };
    match p.stance {
        Stance::Required => format!("{}: a {what} you require{doubt}", capitalize(&p.text)),
        Stance::Wanted => format!("{}: a {what} you want{doubt}", capitalize(&p.text)),
        Stance::Acceptable => format!("{}: a {what} you'd accept{doubt}", capitalize(&p.text)),
        Stance::Unwanted => format!("{}: a {what} you don't want{doubt}", capitalize(&p.text)),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Stated preferences of these dimensions, matched against the job.
fn stated(i: &Inputs<'_>, dimensions: &[Dimension]) -> Vec<Signal> {
    let mut out = Vec::new();
    let relevant: Vec<&StatedPreference> = i
        .person
        .stated
        .iter()
        .filter(|p| dimensions.contains(&p.dimension))
        .collect();
    for p in &relevant {
        if let Some(words) = matches(p, i.facets) {
            let group = SignalGroup::of(p.dimension);
            out.push(
                Signal::new(group, Basis::Stated, stated_weight(p), stated_summary(p))
                    .evidence(words),
            );
        }
    }
    out
}

/// Learned taste about the job's facets in these dimensions.
fn learned(i: &Inputs<'_>, dimensions: &[Dimension]) -> Vec<Signal> {
    let mut out = Vec::new();
    for (key, fact) in i.facets.keys() {
        if !dimensions.contains(&key.dimension) {
            continue;
        }
        let Some(taste) = i.taste.get(&key) else {
            continue;
        };
        let Some((direction, confidence)) = taste.effect() else {
            continue;
        };
        let verb = match direction {
            Direction::Prefer => "you've favored",
            Direction::Avoid => "you've turned down",
        };
        let summary = format!(
            "{}: {verb} jobs like this ({}; {})",
            capitalize(&key.label()),
            taste.basis(),
            confidence.as_str()
        );
        let evidence = taste.support.iter().take(3).map(describe_evidence);
        out.push(
            Signal::new(
                SignalGroup::of(key.dimension),
                Basis::Learned,
                direction.sign() * confidence.weight(),
                summary,
            )
            .evidence(std::iter::once(format!("this job: {fact}")).chain(evidence)),
        );
    }
    out
}

/// "“pure SRE” — rejected Site Reliability Engineer at Acme, 2026-09-20",
/// "applied, liked: Staff Engineer at Modal, 2026-09-25".
pub fn describe_evidence(e: &crate::taste::TasteEvidence) -> String {
    let when = e.at.format("%Y-%m-%d");
    match &e.kind {
        EvidenceKind::Reason { text, resolved, .. } => {
            let resolved = resolved
                .as_ref()
                .map(|f| format!(" → {}", clip(f, 80)))
                .unwrap_or_default();
            format!(
                "“{text}”{resolved} — {} {} at {}, {when}",
                e.action.past(),
                e.title,
                e.company
            )
        }
        EvidenceKind::Behavior { what, fact } => format!(
            "{what}: {} at {}, {when} — {}",
            e.title,
            e.company,
            clip(fact, 80)
        ),
    }
}

/// At most `max` characters, cut at a word, with an ellipsis.
pub fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > max / 2 => &cut[..at],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', ' ']))
}

// ---------------------------------------------------------------------------
// Role and seniority.

pub fn role(i: &Inputs<'_>) -> Vec<Signal> {
    let f = i.facets;
    let group = SignalGroup::Role;
    let mut out = stated(i, &[Dimension::Role]);
    let wants_role = |value: &str| {
        i.person.stated.iter().any(|p| {
            p.dimension == Dimension::Role
                && p.stance != Stance::Unwanted
                && matches!(&p.matcher, Matcher::Keys(k) if k.contains(&TasteKey::role(value)))
        })
    };
    // Work of another kind altogether.
    if i.person.engineer && !f.function.is_engineering() {
        out.push(
            Signal::new(
                group,
                Basis::Experience,
                -3.0,
                format!(
                    "{}: {}, while your experience is in engineering",
                    f.title,
                    f.function.label()
                ),
            )
            .evidence([format!("“{}” (title)", f.title)]),
        );
    } else if i.person.engineer
        && f.function == JobFunction::CustomerEngineering
        && !wants_role("solutions")
    {
        out.push(
            Signal::new(
                group,
                Basis::Posting,
                -0.5,
                "Customer-facing engineering (solutions / forward-deployed), not product engineering",
            )
            .evidence([format!("“{}” (title)", f.title)]),
        );
    }
    // Roles the person listed that this isn't.
    let role_prefs: Vec<&StatedPreference> = i
        .person
        .stated
        .iter()
        .filter(|p| p.dimension == Dimension::Role && p.stance != Stance::Unwanted)
        .collect();
    let matched = out
        .iter()
        .any(|s| s.basis == Basis::Stated && s.weight > 0.0);
    if !matched && f.function.is_engineering() {
        let required: Vec<&str> = role_prefs
            .iter()
            .filter(|p| p.stance == Stance::Required)
            .map(|p| p.text.as_str())
            .collect();
        let wanted: Vec<&str> = role_prefs
            .iter()
            .filter(|p| p.stance == Stance::Wanted)
            .map(|p| p.text.as_str())
            .collect();
        if !required.is_empty() {
            out.push(Signal::new(
                group,
                Basis::Stated,
                -2.5,
                format!("Not one of the roles you require ({})", required.join(", ")),
            ));
        } else if !wanted.is_empty() {
            out.push(Signal::new(
                group,
                Basis::Stated,
                -1.0,
                format!("Not one of the roles you listed ({})", wanted.join(", ")),
            ));
        }
    }
    // Experience: the person has done this kind of work.
    if let Some((fact, exp)) = f
        .roles
        .iter()
        .find_map(|r| i.person.role_experience(&r.key.value).map(|e| (r, e)))
    {
        let places = if exp.places.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                exp.places
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        out.push(
            Signal::new(
                group,
                Basis::Experience,
                0.5,
                format!("You have {} experience{places}", fact.key.value_label()),
            )
            .evidence([fact.cite()]),
        );
    }
    out.extend(learned(i, &[Dimension::Role]));
    out
}

pub fn seniority(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::Seniority;
    let mut out = Vec::new();
    if let (Some((job, words)), Some((mine, title))) = (&i.facets.level, &i.person.level)
        && let (Some(j), Some(m)) = (job.ic_rank(), mine.ic_rank())
    {
        let evidence = [
            format!("“{words}” (title)"),
            format!("your latest title: {title}"),
        ];
        if j + 2 <= m {
            out.push(
                Signal::new(
                    group,
                    Basis::Experience,
                    -1.5,
                    format!("{} level, well below your latest title", job.as_str()),
                )
                .evidence(evidence),
            );
        } else if j < m {
            out.push(
                Signal::new(
                    group,
                    Basis::Experience,
                    -0.5,
                    format!("{} level, a step below your latest title", job.as_str()),
                )
                .evidence(evidence),
            );
        } else if j > m + 1 {
            out.push(
                Signal::new(
                    group,
                    Basis::Experience,
                    0.0,
                    format!("{} level, a stretch from your latest title", job.as_str()),
                )
                .evidence(evidence),
            );
        }
    }
    out.extend(learned(i, &[Dimension::Seniority]));
    out
}

// ---------------------------------------------------------------------------
// Stack.

pub fn stack(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::Stack;
    let mut out = Vec::new();
    let required = i.facets.technologies_at(Requirement::Required);
    if !required.is_empty() && !i.person.technologies.is_empty() {
        let used: Vec<&str> = required
            .iter()
            .filter(|t| {
                matches!(
                    i.person.technology(&t.name),
                    Some(EvidenceStrength::Demonstrated | EvidenceStrength::UserStated)
                )
            })
            .map(|t| t.name.as_str())
            .collect();
        let listed: Vec<&str> = required
            .iter()
            .filter(|t| i.person.technology(&t.name) == Some(EvidenceStrength::Listed))
            .map(|t| t.name.as_str())
            .collect();
        let missing: Vec<&str> = required
            .iter()
            .filter(|t| i.person.technology(&t.name).is_none())
            .map(|t| t.name.as_str())
            .collect();
        let evidence = required
            .iter()
            .filter(|t| used.contains(&t.name.as_str()))
            .take(3)
            .map(|t| format!("{}: “{}”", t.name, t.evidence));
        if !used.is_empty() {
            let weight = (0.5 * used.len() as f64).min(1.5);
            out.push(
                Signal::new(
                    group,
                    Basis::Experience,
                    weight,
                    format!("Asks for {}, which you've used", list(&used)),
                )
                .evidence(evidence),
            );
        }
        if !listed.is_empty() {
            out.push(Signal::new(
                group,
                Basis::Experience,
                0.0,
                format!(
                    "Asks for {}, which your resume only lists as a skill",
                    list(&listed)
                ),
            ));
        }
        if !missing.is_empty() {
            // One missing technology is not a reason to skip a job.
            let (weight, summary) = if used.is_empty() && listed.is_empty() && missing.len() >= 2 {
                (
                    -0.75,
                    format!(
                        "None of what it asks for ({}) is in your profile",
                        list(&missing)
                    ),
                )
            } else {
                (
                    0.0,
                    format!("Also asks for {} (not in your profile)", list(&missing)),
                )
            };
            out.push(Signal::new(group, Basis::Experience, weight, summary));
        }
    }
    out.extend(learned(i, &[Dimension::Technology]));
    out
}

fn list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

// ---------------------------------------------------------------------------
// Domain.

pub fn domain(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::Domain;
    let mut out = stated(i, &[Dimension::Domain]);
    let required: Vec<&StatedPreference> = i
        .person
        .stated
        .iter()
        .filter(|p| p.dimension == Dimension::Domain && p.stance == Stance::Required)
        .collect();
    if !required.is_empty() && !required.iter().any(|p| matches(p, i.facets).is_some()) {
        let names: Vec<&str> = required.iter().map(|p| p.text.as_str()).collect();
        if i.facets.domains.is_empty() {
            out.push(
                Signal::new(
                    group,
                    Basis::Stated,
                    0.0,
                    format!(
                        "The posting doesn't say its domain; you require {}",
                        list(&names)
                    ),
                )
                .kind(SignalKind::Unknown),
            );
        } else {
            out.push(Signal::new(
                group,
                Basis::Stated,
                -1.5,
                format!("Not in the domains you require ({})", list(&names)),
            ));
        }
    }
    // Having worked in a domain is not wanting to: context, lightly.
    for fact in &i.facets.domains {
        if let Some(exp) = i.person.domain_experience(&fact.key.value) {
            let places = exp.places.iter().take(3).cloned().collect::<Vec<_>>();
            let at = if places.is_empty() {
                String::new()
            } else {
                format!(" ({})", places.join(", "))
            };
            out.push(
                Signal::new(
                    group,
                    Basis::Experience,
                    0.25,
                    format!("You've worked in {}{at}", fact.key.value),
                )
                .evidence([fact.cite()]),
            );
            break;
        }
    }
    out.extend(learned(i, &[Dimension::Domain]));
    out
}

// ---------------------------------------------------------------------------
// Company and work style.

pub fn company(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::Company;
    let mut out = stated(i, &[Dimension::CompanyTrait]);
    let unstated: Vec<&str> = i
        .person
        .stated
        .iter()
        .filter(|p| {
            p.dimension == Dimension::CompanyTrait
                && p.stance.is_positive()
                && matches(p, i.facets).is_none()
        })
        .map(|p| p.text.as_str())
        .collect();
    if !unstated.is_empty() {
        out.push(
            Signal::new(
                group,
                Basis::Stated,
                0.0,
                format!(
                    "The posting doesn't say whether it's {}",
                    unstated.join(" or ")
                ),
            )
            .kind(SignalKind::Unknown),
        );
    }
    out.extend(learned(i, &[Dimension::CompanyTrait, Dimension::Company]));
    out
}

pub fn work_style(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::WorkStyle;
    let mut out = stated(i, &[Dimension::WorkStyle]);
    let wants_ic = i.person.stated.iter().any(|p| {
        p.dimension == Dimension::WorkStyle
            && p.stance.is_positive()
            && p.key().is_some_and(|k| k.value == "individual_contributor")
    });
    if wants_ic && i.facets.manages_people() {
        out.push(
            Signal::new(
                group,
                Basis::Stated,
                -2.0,
                "People management, while you want individual-contributor work",
            )
            .evidence(
                i.facets
                    .work_style
                    .iter()
                    .filter(|f| f.key.value == "management")
                    .map(crate::facets::Fact::cite),
            ),
        );
    }
    out.extend(learned(i, &[Dimension::WorkStyle]));
    out
}

pub fn work_mode(i: &Inputs<'_>) -> Vec<Signal> {
    let group = SignalGroup::WorkMode;
    let mut out = Vec::new();
    let modes = &i.facets.work_modes;
    for (mode, stance) in &i.person.work_modes {
        let name = match mode {
            WorkMode::Remote => "remote",
            WorkMode::Hybrid => "hybrid",
            WorkMode::Onsite => "on-site",
        };
        match stance {
            // A required work mode is an eligibility rule, not a taste.
            Stance::Required => {}
            Stance::Wanted | Stance::Acceptable if modes.contains(mode) => {
                let weight = if *stance == Stance::Wanted { 1.0 } else { 0.25 };
                out.push(Signal::new(
                    group,
                    Basis::Stated,
                    weight,
                    format!("Can be done {name}, as you prefer"),
                ));
            }
            Stance::Unwanted if !modes.is_empty() && modes.iter().all(|m| m == mode) => {
                out.push(Signal::new(
                    group,
                    Basis::Stated,
                    -1.5,
                    format!("Only {name}, which you'd rather avoid"),
                ));
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Compensation.

/// Whether a pay preference applies to the job's terms.
fn applies(p: &PayPreference, contract: Option<bool>) -> bool {
    match (p.arrangement, contract) {
        (None, _) | (_, None) => true,
        (Some(Arrangement::Contract), Some(c)) => c,
        (Some(Arrangement::Employment), Some(c)) => !c,
    }
}

fn same_period(period: PayPeriod, interval: Option<PayInterval>) -> bool {
    matches!(
        (period, interval),
        (PayPeriod::Year, Some(PayInterval::Year))
            | (PayPeriod::Month, Some(PayInterval::Month))
            | (PayPeriod::Day, Some(PayInterval::Day))
            | (PayPeriod::Hour, Some(PayInterval::Hour))
    )
}

fn money(currency: &str, amount: f64, period: PayPeriod) -> String {
    format!(
        "{currency} {} per {}",
        group_thousands(amount.round().max(0.0) as u64),
        period.as_str()
    )
}

/// The pay signals, and whether the job is ruled out (verified pay below a
/// required minimum).
pub fn compensation(i: &Inputs<'_>) -> (Vec<Signal>, Option<String>) {
    let group = SignalGroup::Compensation;
    let pay = i.compensation;
    let provenance = pay.provenance(i.now);
    let salaries: Vec<&PayRange> = pay
        .check
        .ranges
        .iter()
        .filter(|r| r.kind == "salary")
        .collect();
    let mut out = Vec::new();
    let mut blocked = None;
    let published = pay.check.status == CompensationStatus::Published;
    if !published || salaries.is_empty() {
        let summary = match (&pay.check.summary, published) {
            (Some(text), true) => format!("Pay isn't stated as a salary range: “{text}”"),
            _ => "Pay isn't published: unknown, not low".to_owned(),
        };
        out.push(
            Signal::new(group, Basis::Posting, 0.0, summary)
                .kind(SignalKind::Unknown)
                .evidence([provenance.clone()]),
        );
    } else if i.person.pay.is_empty() {
        let ranges: Vec<String> = salaries.iter().map(|r| r.describe()).collect();
        out.push(
            Signal::new(
                group,
                Basis::Posting,
                0.0,
                format!("Pays {}", ranges.join("; ")),
            )
            .evidence([provenance.clone()]),
        );
    }
    for p in i
        .person
        .pay
        .iter()
        .filter(|p| applies(p, i.facets.contract))
    {
        if !published || salaries.is_empty() {
            break;
        }
        let bound = p.bound.as_str();
        let Some(currency) = &p.currency else {
            out.push(
                Signal::new(
                    group,
                    Basis::Stated,
                    0.0,
                    format!(
                        "Your {bound} ({}) has no currency, so pay can't be compared; set one with \
                         `jobhunt preferences set compensation --currency …`",
                        p.text
                    ),
                )
                .kind(SignalKind::Unknown),
            );
            continue;
        };
        let comparable: Vec<&&PayRange> = salaries
            .iter()
            .filter(|r| r.currency.code() == Some(currency.as_str()))
            .filter(|r| same_period(p.period, r.interval))
            .collect();
        if comparable.is_empty() {
            let ambiguous = salaries.iter().find_map(|r| match &r.currency {
                CurrencyEvidence::Ambiguous { symbol } => Some(symbol),
                _ => None,
            });
            let why = if let Some(symbol) = ambiguous {
                format!(
                    "Pay is in “{symbol}”, which several currencies use; not compared with your {bound} in {currency}"
                )
            } else if let Some(code) = salaries
                .iter()
                .filter_map(|r| r.currency.code())
                .find(|c| *c != currency)
            {
                format!(
                    "Pay is in {code}; your {bound} is in {currency} (currencies are not converted)"
                )
            } else if salaries
                .iter()
                .all(|r| r.currency == CurrencyEvidence::Unknown)
            {
                format!("Pay doesn't state its currency; not compared with your {bound}")
            } else {
                format!(
                    "Pay is stated for another period; your {bound} is per {} (not converted)",
                    p.period.as_str()
                )
            };
            out.push(
                Signal::new(group, Basis::Stated, 0.0, why)
                    .kind(SignalKind::Unknown)
                    .evidence(
                        salaries
                            .iter()
                            .map(|r| r.describe())
                            .chain([provenance.clone()]),
                    ),
            );
            continue;
        }
        let top = comparable
            .iter()
            .filter_map(|r| r.max.or(r.min))
            .fold(f64::MIN, f64::max);
        let bottom = comparable
            .iter()
            .filter_map(|r| r.min.or(r.max))
            .fold(f64::MAX, f64::min);
        let amount = p.amount as f64;
        let want = money(currency, amount, p.period);
        let evidence: Vec<String> = comparable
            .iter()
            .map(|r| r.describe())
            .chain([provenance.clone(), format!("your {bound}: {}", p.text)])
            .collect();
        let signal = match p.bound {
            CompensationBound::Minimum if top < amount => {
                let summary = format!(
                    "Pays at most {}, below your minimum of {want}",
                    money(currency, top, p.period)
                );
                if p.stance == Stance::Required && pay.verified_at.is_some() {
                    blocked = Some(summary.clone());
                    Signal::new(group, Basis::Stated, 0.0, summary).kind(SignalKind::Blocker)
                } else {
                    Signal::new(group, Basis::Stated, -3.0, summary)
                }
            }
            CompensationBound::Minimum if bottom < amount => Signal::new(
                group,
                Basis::Stated,
                -0.25,
                format!("The range starts below your minimum of {want}; the top meets it"),
            )
            .kind(SignalKind::Condition),
            CompensationBound::Minimum => Signal::new(
                group,
                Basis::Stated,
                1.0,
                format!("Meets your minimum of {want} across the range"),
            ),
            CompensationBound::Target if bottom >= amount => Signal::new(
                group,
                Basis::Stated,
                1.5,
                format!("Reaches your target of {want} across the range"),
            ),
            CompensationBound::Target if top >= amount => Signal::new(
                group,
                Basis::Stated,
                0.75,
                format!("Reaches your target of {want} at the top of the range"),
            ),
            CompensationBound::Target => Signal::new(
                group,
                Basis::Stated,
                -0.75,
                format!(
                    "Below your target of {want} (tops out at {})",
                    money(currency, top, p.period)
                ),
            ),
        };
        out.push(signal.evidence(evidence));
    }
    // Pay the person objected to before: worth a look when it is unknown or
    // below what they want.
    let objected = [
        TasteKey::new(Dimension::Compensation, "pay_level"),
        TasteKey::new(Dimension::Compensation, "local_pay"),
    ]
    .iter()
    .filter_map(|k| i.taste.get(k))
    .find(|t| t.effect().is_some_and(|(d, _)| d == Direction::Avoid));
    let weak = out
        .iter()
        .any(|s| s.kind == SignalKind::Unknown || s.weight < 0.0);
    if let Some(t) = objected
        && weak
    {
        out.push(Signal::new(
            group,
            Basis::Learned,
            -0.5,
            format!(
                "You've turned jobs down over {} before ({}); check the pay early",
                t.key.value_label(),
                t.basis()
            ),
        ));
    }
    (out, blocked)
}

// ---------------------------------------------------------------------------
// Freshness.

pub fn freshness(i: &Inputs<'_>) -> Option<Signal> {
    let days = (i.now - i.facets.listed_at).num_days();
    let what = if i.facets.posted {
        "Posted"
    } else {
        "First seen"
    };
    let evidence = format!("{what} {}", i.facets.listed_at.format("%Y-%m-%d"));
    if days <= 7 {
        Some(
            Signal::new(
                SignalGroup::Freshness,
                Basis::Posting,
                0.25,
                format!("{what} {}", ago(i.now - i.facets.listed_at)),
            )
            .evidence([evidence]),
        )
    } else if days > 60 && i.assessment.trust.standing == Standing::Trusted {
        // Verified live: an old posting that is still listed is not stale.
        Some(
            Signal::new(
                SignalGroup::Freshness,
                Basis::Posting,
                0.0,
                format!("{what} {days} days ago, and still listed when verified"),
            )
            .evidence([evidence]),
        )
    } else if days > 60 {
        Some(
            Signal::new(
                SignalGroup::Freshness,
                Basis::Posting,
                -0.5,
                format!("{what} {days} days ago; it may be filled"),
            )
            .evidence([evidence]),
        )
    } else {
        None
    }
}

/// Notes the person left about this job only ("great product").
pub fn notes(i: &Inputs<'_>, opportunity: jobhunt_jobs::OpportunityId) -> Vec<Signal> {
    i.taste
        .notes_on(opportunity)
        .into_iter()
        .map(|n| {
            Signal::new(
                SignalGroup::Feedback,
                Basis::Feedback,
                0.0,
                format!(
                    "You said “{}” when you {} it ({})",
                    n.reason,
                    n.action.past(),
                    n.key.value_label()
                ),
            )
        })
        .collect()
}
