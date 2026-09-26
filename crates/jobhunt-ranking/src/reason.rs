//! Reading the reasons people give ("too corporate", "pure SRE", "love
//! tiny founder-led teams") into structured taste signals.
//!
//! The reason itself is always kept verbatim on its
//! [`FeedbackEvent`](crate::feedback::FeedbackEvent); a reading only adds
//! structure next to it, and it is recomputed whenever taste is derived, so
//! a better reader improves old feedback too. When nothing in a reason is
//! recognized, the reading is empty and the text is shown as written: the
//! reader never invents an interpretation.
//!
//! [`ReasonReader`] is the seam for other readers (an AI-assisted one
//! could sit behind it); [`RuleReader`] is the deterministic one and needs
//! nothing but the text.
//!
//! How a reason is read:
//!
//! 1. It is split into clauses (punctuation, "but").
//! 2. Each clause is searched for known terms: roles ("SRE", "frontend"),
//!    company kinds ("corporate", "founder-led"), work style ("ownership",
//!    "management", "on-call"), domains and technologies (the profile's
//!    vocabularies), pay, product interest, and references to the job
//!    itself ("this domain", "this stack", "this company").
//! 3. Each term's direction comes from the clause's cues: "too", "boring",
//!    "hate" point away; "love", "great", "interesting" toward; "no",
//!    "not enough", "lack of" point toward the missing thing ("no
//!    ownership" is wanting ownership). Without a cue, the action decides:
//!    a reason given while rejecting describes what was wrong.

use jobhunt_profile::infer::{domains_in, technologies_in};
use jobhunt_profile::words::{Pattern, Word, span_text, words};
use serde::{Deserialize, Serialize};

use crate::feedback::FeedbackAction;
use crate::key::{Dimension, Direction, TasteKey};

/// Something about the job a reason refers to without naming it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobReference {
    /// "this domain", "this industry".
    Domain,
    /// "this stack", "these technologies".
    Stack,
    /// "this company".
    Company,
    /// "too senior", "too junior": the job's level.
    Level,
}

impl JobReference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Domain => "the job's domain",
            Self::Stack => "the job's stack",
            Self::Company => "the employer",
            Self::Level => "the job's level",
        }
    }
}

/// What a signal is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    Key {
        key: TasteKey,
    },
    /// Resolved against the job's facets when taste is derived.
    Job {
        reference: JobReference,
    },
}

/// Whether a signal says something general about the person's taste, or
/// only about this one job ("boring product" doesn't say which products
/// are boring).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    General,
    ThisOpportunity,
}

/// One structured reading of part of a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadSignal {
    pub target: Target,
    pub direction: Direction,
    pub scope: Scope,
    /// The words it was read from, verbatim.
    pub phrase: String,
    /// The direction came from the action, not from words in the reason.
    pub from_action: bool,
}

/// What a reader made of one reason.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReasonReading {
    /// Which reader (`rules/1`).
    pub reader: String,
    pub signals: Vec<ReadSignal>,
}

impl ReasonReading {
    /// Nothing was recognized: the reason stays as written.
    pub fn is_unread(&self) -> bool {
        self.signals.is_empty()
    }
}

/// Reads a reason in the context of the action it was given with.
pub trait ReasonReader: Send + Sync {
    /// Identifies the reader and its rules; part of every ranking's cache
    /// key.
    fn revision(&self) -> &str;

    fn read(&self, reason: &str, action: FeedbackAction) -> ReasonReading;
}

/// The deterministic reader.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleReader;

/// Revision of [`RuleReader`]'s vocabulary and cues. Bump it with any
/// change that can read a reason differently.
pub const RULE_READER_REVISION: &str = "rules/1";

/// Words that point away from what follows.
const AWAY: [&str; 22] = [
    "too",
    "hate*",
    "dislike*",
    "boring",
    "bored",
    "dull",
    "bad",
    "poor",
    "avoid",
    "meh",
    "heavy",
    "much",
    "lots of",
    "tired of",
    "sick of",
    "enough of",
    "overly",
    "annoying",
    "terrible",
    "awful",
    "not interest*",
    "unclear",
];

/// Words that point toward what follows.
const TOWARD: [&str; 18] = [
    "love*",
    "i like",
    "great",
    "interesting",
    "interested in",
    "strong",
    "good",
    "excellent",
    "exciting",
    "excited",
    "cool",
    "want*",
    "enjoy*",
    "nice",
    "awesome",
    "fun",
    "amazing",
    "prefer*",
];

/// Words saying something is missing: the person wants it.
const ABSENT: [&str; 7] = [
    "no",
    "not enough",
    "too little",
    "lack*",
    "without",
    "missing",
    "needs more",
];

/// Words negating a positive one ("don't want", "not interesting").
const NEGATION: [&str; 7] = [
    "not", "don t", "do not", "dont", "never", "isn t", "doesn t",
];

/// Things a reason can name, and the key each is about.
const TERMS: &[(&str, Dimension, &str)] = &[
    // Roles.
    ("site reliability", Dimension::Role, "sre"),
    ("sre", Dimension::Role, "sre"),
    ("devops", Dimension::Role, "sre"),
    ("dev ops", Dimension::Role, "sre"),
    ("ops", Dimension::Role, "sre"),
    ("frontend*", Dimension::Role, "frontend"),
    ("front end*", Dimension::Role, "frontend"),
    ("=UI", Dimension::Role, "frontend"),
    ("css", Dimension::Role, "frontend"),
    ("backend*", Dimension::Role, "backend"),
    ("back end*", Dimension::Role, "backend"),
    ("full stack", Dimension::Role, "full stack"),
    ("fullstack", Dimension::Role, "full stack"),
    ("platform", Dimension::Role, "platform"),
    ("infra role*", Dimension::Role, "infrastructure"),
    ("infrastructure role*", Dimension::Role, "infrastructure"),
    ("data engineering", Dimension::Role, "data"),
    ("mobile", Dimension::Role, "mobile"),
    ("=iOS", Dimension::Role, "mobile"),
    ("android", Dimension::Role, "mobile"),
    ("security engineer*", Dimension::Role, "security"),
    ("embedded", Dimension::Role, "embedded"),
    ("firmware", Dimension::Role, "embedded"),
    ("solutions engineer*", Dimension::Role, "solutions"),
    ("forward deployed", Dimension::Role, "solutions"),
    ("customer facing", Dimension::Role, "solutions"),
    ("pre sales", Dimension::Role, "solutions"),
    ("sales", Dimension::Role, "sales"),
    ("product manag*", Dimension::Role, "product management"),
    // Work style.
    ("management", Dimension::WorkStyle, "management"),
    ("managing people", Dimension::WorkStyle, "management"),
    ("people manag*", Dimension::WorkStyle, "management"),
    ("manager role*", Dimension::WorkStyle, "management"),
    ("direct reports", Dimension::WorkStyle, "management"),
    ("ownership", Dimension::WorkStyle, "ownership"),
    ("autonomy", Dimension::WorkStyle, "ownership"),
    ("autonomous", Dimension::WorkStyle, "ownership"),
    ("end to end", Dimension::WorkStyle, "ownership"),
    ("hands on", Dimension::WorkStyle, "individual_contributor"),
    (
        "individual contributor",
        Dimension::WorkStyle,
        "individual_contributor",
    ),
    ("=IC", Dimension::WorkStyle, "individual_contributor"),
    ("on call", Dimension::WorkStyle, "on_call"),
    ("pager*", Dimension::WorkStyle, "on_call"),
    ("greenfield", Dimension::WorkStyle, "greenfield"),
    ("from scratch", Dimension::WorkStyle, "greenfield"),
    ("zero to one", Dimension::WorkStyle, "greenfield"),
    ("legacy", Dimension::WorkStyle, "maintenance"),
    ("maintenance", Dimension::WorkStyle, "maintenance"),
    ("meeting*", Dimension::WorkStyle, "meetings"),
    ("async*", Dimension::WorkStyle, "async_communication"),
    (
        "close to product",
        Dimension::WorkStyle,
        "product_closeness",
    ),
    ("close to users", Dimension::WorkStyle, "product_closeness"),
    (
        "close to customers",
        Dimension::WorkStyle,
        "product_closeness",
    ),
    ("product minded", Dimension::WorkStyle, "product_closeness"),
    // Company and team.
    ("corporate", Dimension::CompanyTrait, "large_company"),
    ("enterprise*", Dimension::CompanyTrait, "large_company"),
    ("big compan*", Dimension::CompanyTrait, "large_company"),
    ("large compan*", Dimension::CompanyTrait, "large_company"),
    ("big tech", Dimension::CompanyTrait, "large_company"),
    ("=FAANG", Dimension::CompanyTrait, "large_company"),
    ("bureaucra*", Dimension::CompanyTrait, "large_company"),
    ("early stage", Dimension::CompanyTrait, "early_stage"),
    ("pre seed", Dimension::CompanyTrait, "early_stage"),
    ("seed", Dimension::CompanyTrait, "early_stage"),
    ("series a", Dimension::CompanyTrait, "early_stage"),
    ("startup*", Dimension::CompanyTrait, "startup"),
    ("start up*", Dimension::CompanyTrait, "startup"),
    ("scale up*", Dimension::CompanyTrait, "scaleup"),
    ("scaleup*", Dimension::CompanyTrait, "scaleup"),
    ("founder*", Dimension::CompanyTrait, "founder_led"),
    // A company's size is not its teams' size; the explicit company
    // phrases come first, so the generic "tiny" below can't take them.
    ("tiny compan*", Dimension::CompanyTrait, "small_company"),
    ("small compan*", Dimension::CompanyTrait, "small_company"),
    ("tiny team*", Dimension::CompanyTrait, "small_team"),
    ("tiny", Dimension::CompanyTrait, "small_team"),
    ("small team*", Dimension::CompanyTrait, "small_team"),
    ("big team*", Dimension::CompanyTrait, "large_team"),
    ("large team*", Dimension::CompanyTrait, "large_team"),
    ("consulting", Dimension::CompanyTrait, "consulting"),
    ("consultancy", Dimension::CompanyTrait, "consulting"),
    ("client work", Dimension::CompanyTrait, "consulting"),
    ("agency", Dimension::CompanyTrait, "agency"),
    ("agencies", Dimension::CompanyTrait, "agency"),
    ("public compan*", Dimension::CompanyTrait, "public_company"),
    ("open source", Dimension::CompanyTrait, "open_source"),
    ("remote first", Dimension::CompanyTrait, "remote_first"),
    (
        "product compan*",
        Dimension::CompanyTrait,
        "product_company",
    ),
    // Pay.
    ("local salar*", Dimension::Compensation, "local_pay"),
    ("salary too local", Dimension::Compensation, "local_pay"),
    ("local pay", Dimension::Compensation, "local_pay"),
    ("local rate*", Dimension::Compensation, "local_pay"),
    ("salar*", Dimension::Compensation, "pay_level"),
    ("pay", Dimension::Compensation, "pay_level"),
    ("paid", Dimension::Compensation, "pay_level"),
    ("underpa*", Dimension::Compensation, "pay_level"),
    ("compensation", Dimension::Compensation, "pay_level"),
    ("=comp", Dimension::Compensation, "pay_level"),
    ("money", Dimension::Compensation, "pay_level"),
    // Only about this job.
    ("remote policy", Dimension::Information, "remote_policy"),
    ("vague", Dimension::Information, "description"),
    ("product", Dimension::Product, "interest"),
    ("problem*", Dimension::Product, "interest"),
    ("mission", Dimension::Product, "interest"),
    ("team", Dimension::Product, "team"),
];

/// References to the job itself.
const REFERENCES: &[(&str, JobReference)] = &[
    ("this domain", JobReference::Domain),
    ("domain", JobReference::Domain),
    ("industry", JobReference::Domain),
    ("this industry", JobReference::Domain),
    ("this space", JobReference::Domain),
    ("this sector", JobReference::Domain),
    ("same domain", JobReference::Domain),
    ("that domain", JobReference::Domain),
    ("this stack", JobReference::Stack),
    ("the stack", JobReference::Stack),
    ("stack", JobReference::Stack),
    ("tech stack", JobReference::Stack),
    ("these technologies", JobReference::Stack),
    ("this company", JobReference::Company),
    ("the company", JobReference::Company),
    ("this employer", JobReference::Company),
    ("too senior", JobReference::Level),
    ("too junior", JobReference::Level),
    ("wrong level", JobReference::Level),
];

/// Levels a reason can name outright ("staff roles").
const LEVELS: &[(&str, &str)] = &[
    ("junior", "junior"),
    ("entry level", "junior"),
    ("intern*", "intern"),
    ("senior", "senior"),
    ("staff", "staff"),
    ("principal", "principal"),
];

/// Terms whose key the dimension alone doesn't make general.
fn scope_of(dimension: Dimension) -> Scope {
    match dimension {
        Dimension::Product | Dimension::Information => Scope::ThisOpportunity,
        _ => Scope::General,
    }
}

fn clauses(reason: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for part in reason.split(['.', ',', ';', '!', '?', '\n', '(', ')']) {
        // "great product but too corporate": two clauses.
        let mut rest = part;
        while let Some(at) = rest.to_lowercase().find(" but ") {
            out.push(&rest[..at]);
            rest = &rest[at + 5..];
        }
        out.push(rest);
    }
    out.into_iter().filter(|c| !c.trim().is_empty()).collect()
}

/// The clause's own direction, and whether it is about something missing.
fn cue(ws: &[Word]) -> Option<(Direction, bool)> {
    let has = |list: &[&str]| list.iter().any(|p| Pattern::new(p).find(ws).is_some());
    // "not interesting", "don't want": a positive word negated.
    let negated = has(&NEGATION) && has(&TOWARD);
    let away = has(&AWAY) || negated;
    let toward = has(&TOWARD) && !negated;
    let absent = has(&ABSENT);
    match (away, toward, absent) {
        (_, _, true) if !toward => Some((Direction::Prefer, true)),
        (true, false, _) => Some((Direction::Avoid, false)),
        (false, true, _) => Some((Direction::Prefer, false)),
        _ => None,
    }
}

impl RuleReader {
    fn clause(&self, clause: &str, action: FeedbackAction, out: &mut Vec<ReadSignal>) {
        let ws = words(clause);
        let (direction, from_action) = match cue(&ws) {
            Some((d, _)) => (Some(d), false),
            None => (action.polarity(), true),
        };
        let Some(direction) = direction else {
            return;
        };
        let mut taken = vec![false; ws.len()];
        let push = |target: Target, scope: Scope, phrase: &str, out: &mut Vec<ReadSignal>| {
            if !out.iter().any(|s| s.target == target) {
                out.push(ReadSignal {
                    target,
                    direction,
                    scope,
                    phrase: phrase.to_owned(),
                    from_action,
                });
            }
        };
        let claim = |range: &std::ops::Range<usize>, taken: &mut Vec<bool>| -> bool {
            if taken[range.clone()].iter().any(|t| *t) {
                return false;
            }
            taken[range.clone()].iter_mut().for_each(|t| *t = true);
            true
        };
        for mention in technologies_in(clause, false) {
            let key = TasteKey::new(Dimension::Technology, mention.technology);
            if let Some(range) = Pattern::new(&mention.matched).find(&ws)
                && claim(&range, &mut taken)
            {
                push(Target::Key { key }, Scope::General, &mention.matched, out);
            }
        }
        for (pattern, dimension, value) in TERMS {
            for range in Pattern::new(pattern).find_all(&ws) {
                if claim(&range, &mut taken) {
                    let key = TasteKey::new(*dimension, *value);
                    let phrase = span_text(clause, &ws, &range);
                    push(Target::Key { key }, scope_of(*dimension), phrase, out);
                }
            }
        }
        for hit in domains_in(clause).into_iter().filter(|h| h.strong) {
            if let Some(range) = Pattern::new(&hit.phrase).find(&ws)
                && claim(&range, &mut taken)
            {
                let key = TasteKey::new(Dimension::Domain, hit.domain);
                push(Target::Key { key }, Scope::General, &hit.phrase, out);
            }
        }
        for (pattern, level) in LEVELS {
            for range in Pattern::new(pattern).find_all(&ws) {
                if claim(&range, &mut taken) {
                    let key = TasteKey::new(Dimension::Seniority, *level);
                    push(
                        Target::Key { key },
                        Scope::General,
                        span_text(clause, &ws, &range),
                        out,
                    );
                }
            }
        }
        for (pattern, reference) in REFERENCES {
            for range in Pattern::new(pattern).find_all(&ws) {
                if claim(&range, &mut taken) {
                    let target = Target::Job {
                        reference: *reference,
                    };
                    push(target, Scope::General, span_text(clause, &ws, &range), out);
                }
            }
        }
    }
}

impl ReasonReader for RuleReader {
    fn revision(&self) -> &str {
        RULE_READER_REVISION
    }

    fn read(&self, reason: &str, action: FeedbackAction) -> ReasonReading {
        let mut signals = Vec::new();
        for clause in clauses(reason) {
            self.clause(clause, action, &mut signals);
        }
        // "salary too local" also names the pay: keep the specific reading.
        if signals
            .iter()
            .any(|s| matches!(&s.target, Target::Key { key } if key.value == "local_pay"))
        {
            signals
                .retain(|s| !matches!(&s.target, Target::Key { key } if key.value == "pay_level"));
        }
        ReasonReading {
            reader: RULE_READER_REVISION.to_owned(),
            signals,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(reason: &str, action: FeedbackAction) -> Vec<String> {
        RuleReader
            .read(reason, action)
            .signals
            .iter()
            .map(|s| {
                let target = match &s.target {
                    Target::Key { key } => key.to_string(),
                    Target::Job { reference } => format!("job:{reference:?}"),
                };
                format!("{} {}", s.direction.as_str(), target)
            })
            .collect()
    }

    fn reject(reason: &str) -> Vec<String> {
        read(reason, FeedbackAction::Reject)
    }

    #[test]
    fn reads_common_rejection_reasons() {
        assert_eq!(
            reject("too corporate"),
            ["avoid company_trait:large_company"]
        );
        assert_eq!(reject("pure SRE"), ["avoid role:sre"]);
        assert_eq!(reject("too frontend-heavy"), ["avoid role:frontend"]);
        assert_eq!(
            reject("stack is too frontend-heavy"),
            ["avoid role:frontend", "avoid job:Stack"]
        );
        assert_eq!(reject("full stack"), ["avoid role:full stack"]);
        assert_eq!(
            reject("feels like consulting"),
            ["avoid company_trait:consulting"]
        );
        assert_eq!(
            reject("don't want management"),
            ["avoid work_style:management"]
        );
        assert_eq!(
            reject("a little too corporate"),
            ["avoid company_trait:large_company"]
        );
        assert_eq!(
            reject("too little ownership"),
            ["prefer work_style:ownership"]
        );
        assert_eq!(
            reject("too much consulting"),
            ["avoid company_trait:consulting"]
        );
        assert_eq!(
            reject("too much management"),
            ["avoid work_style:management"]
        );
        assert_eq!(reject("salary too local"), ["avoid compensation:local_pay"]);
        assert_eq!(
            reject("compensation too low"),
            ["avoid compensation:pay_level"]
        );
        assert_eq!(reject("fintech"), ["avoid domain:fintech"]);
        assert_eq!(
            reject("already worked with this domain"),
            ["avoid job:Domain"]
        );
        assert_eq!(
            reject("unclear remote policy"),
            ["avoid information:remote_policy"]
        );
        assert_eq!(reject("boring product"), ["avoid product:interest"]);
    }

    #[test]
    fn reads_positive_reasons() {
        let save = |r| read(r, FeedbackAction::Save);
        assert_eq!(
            save("love tiny founder-led teams"),
            [
                "prefer company_trait:founder_led",
                "prefer company_trait:small_team"
            ]
        );
        assert_eq!(
            save("interesting infra problem"),
            ["prefer product:interest", "prefer domain:infrastructure"]
        );
        assert_eq!(save("great product"), ["prefer product:interest"]);
        // A company's size, not a team's; a team's, not a company's.
        assert_eq!(
            save("I like small companies"),
            ["prefer company_trait:small_company"]
        );
        assert_eq!(
            save("I like tiny companies"),
            ["prefer company_trait:small_company"]
        );
        assert_eq!(save("tiny team"), ["prefer company_trait:small_team"]);
        assert_eq!(
            save("love how tiny it is"),
            ["prefer company_trait:small_team"]
        );
        assert_eq!(save("strong ownership"), ["prefer work_style:ownership"]);
        assert_eq!(
            save("Rust and Postgres"),
            ["prefer technology:Rust", "prefer technology:PostgreSQL"]
        );
    }

    #[test]
    fn cues_beat_the_action() {
        // Something missing is something wanted.
        assert_eq!(reject("no ownership"), ["prefer work_style:ownership"]);
        assert_eq!(
            reject("not enough autonomy"),
            ["prefer work_style:ownership"]
        );
        // A positive clause inside a rejection.
        assert_eq!(
            reject("great product but too corporate"),
            [
                "prefer product:interest",
                "avoid company_trait:large_company"
            ]
        );
        // A negative clause inside a save.
        assert_eq!(
            read(
                "on-call is annoying, but interesting domain",
                FeedbackAction::Save
            ),
            ["avoid work_style:on_call", "prefer job:Domain"]
        );
        assert_eq!(reject("not interesting"), Vec::<String>::new());
    }

    #[test]
    fn unrecognized_reasons_stay_unread() {
        let reading = RuleReader.read("meh vibes, idk", FeedbackAction::Reject);
        assert!(reading.is_unread());
        assert_eq!(reading.reader, RULE_READER_REVISION);
        // Interviews and offers carry no direction of their own.
        assert!(
            RuleReader
                .read("fintech", FeedbackAction::Interview)
                .is_unread()
        );
        assert_eq!(
            read("love fintech", FeedbackAction::Interview),
            ["prefer domain:fintech"]
        );
    }

    #[test]
    fn scope_says_what_generalizes() {
        let reading = RuleReader.read("boring product, too corporate", FeedbackAction::Reject);
        let scopes: Vec<Scope> = reading.signals.iter().map(|s| s.scope).collect();
        assert_eq!(scopes, [Scope::ThisOpportunity, Scope::General]);
        assert!(reading.signals.iter().all(|s| !s.from_action));
        let reading = RuleReader.read("fintech", FeedbackAction::Reject);
        assert!(reading.signals[0].from_action);
    }
}
