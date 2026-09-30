//! The built-in taste reader: deterministic, offline, and always there.
//!
//! It is what Narrow uses when no model is configured (the open-source
//! default) or when a model fails, and what tests read with. It knows a
//! compact vocabulary ([`super::vocab`]) and reads each clause's polarity
//! from cue words ("I don't want …", "open to …"), the way the preference
//! statement parser does. A model reads more; this reads what it can, and
//! keeps the rest as ambiguities rather than guessing.
//!
//! From the profile it infers one thing only: the level of the latest
//! title, as a medium-confidence starting point when the person's words
//! say nothing about level.

use std::ops::Range;

use async_trait::async_trait;

use crate::words::{Pattern, Word, span_text, words};

use super::reading::{InterpretError, ReadAssertion, TasteInterpreter, TasteReading, TasteRequest};
use super::{
    Polarity, TasteConfidence, TasteDimension, TasteOrigin, TasteSource, normalize_value, vocab,
};

use TasteDimension as D;

/// One vocabulary entry: a word pattern and what it means.
struct Term {
    pattern: &'static str,
    dimension: TasteDimension,
    value: &'static str,
    confidence: TasteConfidence,
}

const fn t(pattern: &'static str, dimension: TasteDimension, value: &'static str) -> Term {
    Term {
        pattern,
        dimension,
        value,
        confidence: TasteConfidence::High,
    }
}

const fn implied(pattern: &'static str, dimension: TasteDimension, value: &'static str) -> Term {
    Term {
        pattern,
        dimension,
        value,
        confidence: TasteConfidence::Medium,
    }
}

const TERMS: &[Term] = &[
    // Level.
    t("early career", D::Seniority, "early_career"),
    t("early in my career", D::Seniority, "early_career"),
    t("early in their career", D::Seniority, "early_career"),
    t("junior*", D::Seniority, "early_career"),
    t("entry level", D::Seniority, "early_career"),
    t("new grad*", D::Seniority, "early_career"),
    t("graduate program*", D::Seniority, "early_career"),
    t("intern", D::Seniority, "early_career"),
    t("internship*", D::Seniority, "early_career"),
    t("mid level", D::Seniority, "mid"),
    t("intermediate", D::Seniority, "mid"),
    t("senior", D::Seniority, "senior"),
    t("seniors", D::Seniority, "senior"),
    t("sr", D::Seniority, "senior"),
    t("staff", D::Seniority, "staff_plus"),
    t("staff plus", D::Seniority, "staff_plus"),
    t("principal", D::Seniority, "staff_plus"),
    t("distinguished", D::Seniority, "staff_plus"),
    // The shape of the work.
    t("backend", D::WorkShape, "backend"),
    t("back end", D::WorkShape, "backend"),
    t("server side", D::WorkShape, "backend"),
    t("platform", D::WorkShape, "platform"),
    t("infrastructure", D::WorkShape, "infrastructure"),
    t("infra", D::WorkShape, "infrastructure"),
    t("product engineer*", D::WorkShape, "product"),
    t("product development", D::WorkShape, "product"),
    t("product work", D::WorkShape, "product"),
    t("full stack", D::WorkShape, "full_stack"),
    t("fullstack", D::WorkShape, "full_stack"),
    t("frontend", D::WorkShape, "frontend"),
    t("front end", D::WorkShape, "frontend"),
    t("mobile", D::WorkShape, "mobile"),
    t("database internals", D::WorkShape, "database_internals"),
    t("db internals", D::WorkShape, "database_internals"),
    t("database engine*", D::WorkShape, "database_internals"),
    t("database kernel*", D::WorkShape, "database_internals"),
    t("query engine*", D::WorkShape, "database_internals"),
    t("query planner*", D::WorkShape, "database_internals"),
    t("storage engine*", D::WorkShape, "database_internals"),
    t("storage system*", D::WorkShape, "database_internals"),
    t("distributed system*", D::WorkShape, "distributed_systems"),
    t("developer tool*", D::WorkShape, "developer_tooling"),
    t("dev tool*", D::WorkShape, "developer_tooling"),
    t("devtools", D::WorkShape, "developer_tooling"),
    t("developer experience", D::WorkShape, "developer_tooling"),
    t("developer productivity", D::WorkShape, "developer_tooling"),
    t("security", D::WorkShape, "security"),
    t("appsec", D::WorkShape, "security"),
    t("ml research", D::WorkShape, "ml_research"),
    t("ai research", D::WorkShape, "ml_research"),
    t("machine learning research", D::WorkShape, "ml_research"),
    t("research scientist*", D::WorkShape, "ml_research"),
    t("research", D::WorkShape, "research"),
    t("applied ml", D::WorkShape, "ml_product"),
    t("applied ai", D::WorkShape, "ml_product"),
    t("ml product*", D::WorkShape, "ml_product"),
    t("ai product*", D::WorkShape, "ml_product"),
    t("llm product*", D::WorkShape, "ml_product"),
    implied("machine learning", D::WorkShape, "ml_product"),
    t("data engineer*", D::WorkShape, "data"),
    t("data pipeline*", D::WorkShape, "data"),
    t("data platform*", D::WorkShape, "data"),
    t("sre", D::WorkShape, "sre"),
    t("site reliability", D::WorkShape, "sre"),
    t("devops", D::WorkShape, "sre"),
    t("embedded", D::WorkShape, "embedded"),
    t("firmware", D::WorkShape, "embedded"),
    // How specialized.
    t("generalist*", D::Specialization, "broad"),
    t("broad", D::Specialization, "broad"),
    t("broadly", D::Specialization, "broad"),
    t("many hats", D::Specialization, "broad"),
    t("across the stack", D::Specialization, "broad"),
    implied("end to end", D::Specialization, "broad"),
    t("specialist*", D::Specialization, "deep"),
    t("specializ*", D::Specialization, "deep"),
    t("specialis*", D::Specialization, "deep"),
    t("deep technical", D::Specialization, "deep"),
    t("technical depth", D::Specialization, "deep"),
    t("depth", D::Specialization, "deep"),
    t("deep expertise", D::Specialization, "deep"),
    t("narrow role*", D::Specialization, "deep"),
    t("narrow specialist*", D::Specialization, "deep"),
    t("moderately speciali*", D::Specialization, "moderate"),
    t("some specializ*", D::Specialization, "moderate"),
    // Ownership.
    t("ownership", D::Ownership, "high"),
    t("own things", D::Ownership, "high"),
    t("own the", D::Ownership, "high"),
    t("own my", D::Ownership, "high"),
    t("owning", D::Ownership, "high"),
    t("autonomy", D::Ownership, "high"),
    t("autonomous", D::Ownership, "high"),
    t("end to end", D::Ownership, "high"),
    // Company.
    t("startup*", D::Company, "startup"),
    t("start up*", D::Company, "startup"),
    t("early stage", D::Company, "early_stage"),
    t("seed stage", D::Company, "early_stage"),
    t("series a", D::Company, "early_stage"),
    t("growth stage", D::Company, "growth"),
    t("growth compan*", D::Company, "growth"),
    t("scale up*", D::Company, "growth"),
    t("scaleup*", D::Company, "growth"),
    t("established compan*", D::Company, "established"),
    t("big tech", D::Company, "large_company"),
    t("faang", D::Company, "large_company"),
    t("enterprise*", D::Company, "large_company"),
    t("corporate", D::Company, "large_company"),
    t("corporation*", D::Company, "large_company"),
    t("founder led", D::Company, "founder_led"),
    t("product compan*", D::Company, "product_company"),
    t("agency", D::Company, "agency"),
    t("agencies", D::Company, "agency"),
    t("consultanc*", D::Company, "consulting"),
    t("consulting", D::Company, "consulting"),
    t("public compan*", D::Company, "public_company"),
    t("open source", D::Company, "open_source"),
    // Team.
    t("distributed team*", D::Team, "distributed"),
    t("remote team*", D::Team, "distributed"),
    // Culture.
    t("process heavy", D::Culture, "process_heavy"),
    t("heavy process*", D::Culture, "process_heavy"),
    t("bureaucra*", D::Culture, "process_heavy"),
    t("red tape", D::Culture, "process_heavy"),
    t("lots of process", D::Culture, "process_heavy"),
    t("engineering culture", D::Culture, "strong_engineering"),
    t("strong engineering", D::Culture, "strong_engineering"),
    t("engineering driven", D::Culture, "strong_engineering"),
    t("engineering led", D::Culture, "strong_engineering"),
    t("technical culture", D::Culture, "strong_engineering"),
    t("fast paced", D::Culture, "fast_paced"),
    t("fast moving", D::Culture, "fast_paced"),
    t("move fast", D::Culture, "fast_paced"),
    t("mentor*", D::Culture, "mentorship"),
    t("learn from", D::Culture, "mentorship"),
    t("learning", D::Culture, "mentorship"),
    t("remote first", D::Culture, "remote_first"),
    // Way of working.
    t(
        "individual contributor*",
        D::WorkStyle,
        "individual_contributor",
    ),
    t("hands on", D::WorkStyle, "individual_contributor"),
    t("people manag*", D::WorkStyle, "management"),
    t("managing people", D::WorkStyle, "management"),
    t("manage people", D::WorkStyle, "management"),
    t("engineering manag*", D::WorkStyle, "management"),
    t("management", D::WorkStyle, "management"),
    t("greenfield", D::WorkStyle, "greenfield"),
    t("zero to one", D::WorkStyle, "greenfield"),
    t("maintenance", D::WorkStyle, "maintenance"),
    t("legacy", D::WorkStyle, "maintenance"),
    t("on call", D::WorkStyle, "on_call"),
    t("oncall", D::WorkStyle, "on_call"),
    t("meetings", D::WorkStyle, "meetings"),
    t("async*", D::WorkStyle, "async_communication"),
    t("asynchronous*", D::WorkStyle, "async_communication"),
    t("close to users", D::WorkStyle, "product_closeness"),
    t("close to the users", D::WorkStyle, "product_closeness"),
    t("close to customers", D::WorkStyle, "product_closeness"),
    t("close to the product", D::WorkStyle, "product_closeness"),
];

const SMALL: [&str; 4] = ["small", "smaller", "tiny", "little"];
const LARGE: [&str; 7] = [
    "big", "bigger", "large", "larger", "giant", "huge", "massive",
];
const COMPANY_NOUNS: [&str; 7] = [
    "compan*",
    "org",
    "orgs",
    "organi*",
    "corporation*",
    "employer*",
    "enterprise*",
];
const TEAM_NOUNS: [&str; 2] = ["team*", "squad*"];

/// Practical words: noted, never taste.
const CONSTRAINTS: [(&str, &str); 17] = [
    ("remote", "remote"),
    ("fully remote", "remote"),
    ("hybrid", "hybrid"),
    ("on site", "on-site"),
    ("onsite", "on-site"),
    ("in office", "in the office"),
    ("office", "in the office"),
    ("relocat*", "relocation"),
    ("visa*", "visa sponsorship"),
    ("sponsor*", "visa sponsorship"),
    ("salary", "pay"),
    ("pay", "pay"),
    ("compensation", "pay"),
    ("usd", "pay"),
    ("time zone*", "time zone"),
    ("timezone*", "time zone"),
    ("based in", "where you live"),
];

/// Cues that set the polarity of what follows, longest first where they
/// overlap ("don't mind" before "don't").
const OPEN_CUES: [&str; 9] = [
    "open to",
    "fine with",
    "okay with",
    "ok with",
    "happy with",
    "don t mind",
    "wouldn t mind",
    "acceptable",
    "also fine",
];
const AVOID_CUES: [&str; 17] = [
    "don t want",
    "do not want",
    "don t like",
    "do not like",
    "not interested",
    "rather not",
    "tired of",
    "avoid*",
    "no longer",
    "not",
    "no",
    "never",
    "nothing",
    "without",
    "hate",
    "dislike",
    "except",
];
const PREFER_CUES: [&str; 11] = [
    "want",
    "prefer*",
    "ideally",
    "like",
    "love",
    "looking for",
    "interested in",
    "enjoy",
    "plus",
    "wish",
    "would like",
];
const HEDGES: [&str; 6] = [
    "maybe", "perhaps", "might", "possibly", "not sure", "could be",
];

/// A term found in a clause.
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    dimension: TasteDimension,
    value: String,
    range: Range<usize>,
    confidence: TasteConfidence,
    /// A size combination ("small … companies"): overlaps other terms.
    combo: bool,
}

fn compiled() -> &'static [(Pattern, &'static Term)] {
    static CACHE: std::sync::OnceLock<Vec<(Pattern, &'static Term)>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| TERMS.iter().map(|t| (Pattern::new(t.pattern), t)).collect())
}

fn combos() -> &'static [(Pattern, TasteDimension, &'static str)] {
    static CACHE: std::sync::OnceLock<Vec<(Pattern, TasteDimension, &'static str)>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let mut out = Vec::new();
        for (sizes, company, team) in [
            (&SMALL[..], "small_company", "small_team"),
            (&LARGE[..], "large_company", "large_team"),
        ] {
            for size in sizes {
                for gap in ["", "_ ", "_ _ "] {
                    for noun in COMPANY_NOUNS {
                        out.push((
                            Pattern::new(&format!("{size} {gap}{noun}")),
                            D::Company,
                            company,
                        ));
                    }
                    for noun in TEAM_NOUNS {
                        out.push((Pattern::new(&format!("{size} {gap}{noun}")), D::Team, team));
                    }
                }
            }
        }
        out
    })
}

fn hits(ws: &[Word], text: &str) -> Vec<Hit> {
    let mut found: Vec<Hit> = Vec::new();
    for (pattern, term) in compiled() {
        for range in pattern.find_all(ws) {
            found.push(Hit {
                dimension: term.dimension,
                value: term.value.to_owned(),
                range,
                confidence: term.confidence,
                combo: false,
            });
        }
    }
    for (pattern, dimension, value) in combos() {
        for range in pattern.find_all(ws) {
            found.push(Hit {
                dimension: *dimension,
                value: (*value).to_owned(),
                range,
                confidence: TasteConfidence::High,
                combo: true,
            });
        }
    }
    for hit in crate::infer::domains_in_words(text, ws) {
        if !hit.strong {
            continue;
        }
        let key = jobhunt_core::text::search_key(&hit.phrase);
        let n = key.split(' ').count();
        if let Some(start) = (0..ws.len()).find(|&i| {
            i + n <= ws.len()
                && jobhunt_core::text::search_key(span_text(text, ws, &(i..i + n))) == key
        }) {
            found.push(Hit {
                dimension: D::Domain,
                value: normalize_value(hit.domain),
                range: start..start + n,
                confidence: TasteConfidence::High,
                combo: false,
            });
        }
    }
    // A term inside a longer one is part of it ("research" in "ML
    // research"), unless one of them is a size combination.
    let all = found.clone();
    found.retain(|h| {
        h.combo
            || !all.iter().any(|o| {
                !o.combo
                    && o.range != h.range
                    && o.range.start <= h.range.start
                    && h.range.end <= o.range.end
            })
    });
    // A domain named by the same words as a kind of work ("developer
    // tooling") is that work, once.
    let all = found.clone();
    found.retain(|h| {
        h.dimension != D::Domain
            || !all.iter().any(|o| {
                o.dimension != D::Domain
                    && !o.combo
                    && o.range.start < h.range.end
                    && h.range.start < o.range.end
            })
    });
    found.sort_by_key(|h| (h.range.start, h.range.end));
    found
}

/// Every taste value named in `text`, in order, without polarity: how a
/// structured role ("senior platform") reads.
pub fn read_terms(text: &str) -> Vec<(TasteDimension, String)> {
    let ws = words(text);
    let mut out: Vec<(TasteDimension, String)> = Vec::new();
    for h in hits(&ws, text) {
        if !out.iter().any(|(d, v)| *d == h.dimension && *v == h.value) {
            out.push((h.dimension, h.value));
        }
    }
    out
}

/// Sentences, then clauses at "but", "however", "though".
fn clauses(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for sentence in text.split(['.', '!', '?', ';', '\n']) {
        let mut rest = sentence.to_owned();
        loop {
            let lower = rest.to_lowercase();
            let cut = [" but ", ", but", " however", " though ", " although "]
                .iter()
                .filter_map(|sep| lower.find(sep))
                .filter(|&i| i > 0)
                .min();
            match cut {
                Some(i) => {
                    let (head, tail) = rest.split_at(i);
                    out.push(head.trim().trim_matches(',').trim().to_owned());
                    rest = tail
                        .trim_start_matches([',', ' '])
                        .trim_start_matches(|c: char| c.is_alphabetic())
                        .to_owned();
                }
                None => {
                    out.push(rest.trim().trim_matches(',').trim().to_owned());
                    break;
                }
            }
        }
    }
    out.retain(|c| c.chars().any(char::is_alphanumeric));
    out
}

/// The polarity in effect at each word of a clause, and whether it is
/// hedged there.
fn polarities(ws: &[Word]) -> Vec<(Polarity, bool)> {
    let open: Vec<Pattern> = OPEN_CUES.iter().map(|p| Pattern::new(p)).collect();
    let avoid: Vec<Pattern> = AVOID_CUES.iter().map(|p| Pattern::new(p)).collect();
    let prefer: Vec<Pattern> = PREFER_CUES.iter().map(|p| Pattern::new(p)).collect();
    let hedges: Vec<Pattern> = HEDGES.iter().map(|p| Pattern::new(p)).collect();
    let at = |patterns: &[Pattern], i: usize| -> Option<usize> {
        patterns
            .iter()
            .filter_map(|p| p.find(&ws[i..]).filter(|r| r.start == 0).map(|r| r.end))
            .max()
    };
    let mut out = Vec::with_capacity(ws.len());
    let mut polarity = Polarity::Prefer;
    let mut hedged = false;
    let mut i = 0;
    while i < ws.len() {
        if at(&hedges, i).is_some() {
            hedged = true;
        }
        let cue = at(&open, i)
            .map(|n| (Polarity::Open, n))
            .or_else(|| at(&avoid, i).map(|n| (Polarity::Avoid, n)))
            .or_else(|| at(&prefer, i).map(|n| (Polarity::Prefer, n)));
        if let Some((p, n)) = cue {
            // "not sure" hedges; it doesn't negate.
            if !(p == Polarity::Avoid && at(&hedges, i).is_some()) {
                polarity = p;
            }
            for _ in 0..n {
                out.push((polarity, hedged));
            }
            i += n;
            continue;
        }
        out.push((polarity, hedged));
        i += 1;
    }
    out
}

/// A statement read by the rules, with the clause it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleHit {
    pub dimension: TasteDimension,
    pub value: String,
    pub polarity: Polarity,
    pub confidence: TasteConfidence,
    /// The clause, verbatim.
    pub quote: String,
}

/// What the rules read in some words.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleReading {
    pub hits: Vec<RuleHit>,
    /// Practical constraints the words mentioned.
    pub constraints: Vec<String>,
    /// Clauses nothing was read from.
    pub unread: Vec<String>,
}

/// Reads taste out of the person's words.
pub fn read(text: &str) -> RuleReading {
    let mut out = RuleReading::default();
    let constraint_patterns: Vec<(Pattern, &str)> = CONSTRAINTS
        .iter()
        .map(|(p, label)| (Pattern::new(p), *label))
        .collect();
    for clause in clauses(text) {
        let ws = words(&clause);
        let found = hits(&ws, &clause);
        let polarity = polarities(&ws);
        let mut constraints_here = false;
        for (pattern, label) in &constraint_patterns {
            for range in pattern.find_all(&ws) {
                // "remote-first", "remote teams" are culture and team.
                let next = ws.get(range.end).map(|w| w.lower.as_str());
                if matches!(next, Some("first" | "team" | "teams")) {
                    continue;
                }
                constraints_here = true;
                if !out.constraints.iter().any(|c| c == label) {
                    out.constraints.push((*label).to_owned());
                }
            }
        }
        if found.is_empty() {
            if !constraints_here && ws.len() >= 2 {
                out.unread.push(clause.clone());
            }
            continue;
        }
        for h in found {
            let (p, hedged) = polarity
                .get(h.range.start)
                .copied()
                .unwrap_or((Polarity::Prefer, false));
            let confidence = if hedged {
                TasteConfidence::Medium.min(h.confidence)
            } else {
                h.confidence
            };
            if out
                .hits
                .iter()
                .any(|x| x.dimension == h.dimension && x.value == h.value)
            {
                continue;
            }
            out.hits.push(RuleHit {
                dimension: h.dimension,
                value: h.value,
                polarity: p,
                confidence,
                quote: clause.clone(),
            });
        }
    }
    out
}

/// The built-in interpreter (`rules/1`).
#[derive(Debug, Clone, Copy, Default)]
pub struct RulesInterpreter;

impl RulesInterpreter {
    pub const NAME: &'static str = "rules/1";

    /// The reading, synchronously.
    pub fn read(&self, request: &TasteRequest) -> TasteReading {
        let mut reading = TasteReading {
            interpreter: Self::NAME.to_owned(),
            ..TasteReading::default()
        };
        for w in &request.words {
            let r = read(&w.text);
            for hit in r.hits {
                if reading
                    .assertions
                    .iter()
                    .any(|a| a.dimension == hit.dimension && a.value == hit.value)
                {
                    continue;
                }
                reading.assertions.push(ReadAssertion {
                    text: vocab::sentence(hit.dimension, &hit.value, hit.polarity),
                    dimension: hit.dimension,
                    value: hit.value,
                    polarity: hit.polarity,
                    confidence: hit.confidence,
                    explanation: Some("Read from your words.".to_owned()),
                    origin: TasteOrigin::Interpreted,
                    sources: vec![TasteSource::Words {
                        quote: hit.quote,
                        statement: w.statement,
                    }],
                });
            }
            for c in r.constraints {
                if !reading.constraints_noted.contains(&c) {
                    reading.constraints_noted.push(c);
                }
            }
            for u in r.unread {
                reading
                    .ambiguities
                    .push(format!("Narrow couldn't place “{u}”: say it another way?"));
            }
        }
        reading.ambiguities.truncate(3);
        // The latest title's level, when the words don't mention a level
        // they want: a starting point, never the only level they'd take.
        let level_said = reading.assertions.iter().any(|a| {
            a.dimension == D::Seniority && matches!(a.polarity, Polarity::Prefer | Polarity::Open)
        });
        if !level_said
            && request.focus.is_none()
            && let Some(latest) = request.evidence.iter().find(|e| e.latest)
            && let Some(title) = &latest.title
        {
            let levels: Vec<String> = read_terms(title)
                .into_iter()
                .filter(|(d, _)| *d == D::Seniority)
                .map(|(_, v)| v)
                .collect();
            if let Some(level) = levels.first()
                && !reading
                    .assertions
                    .iter()
                    .any(|a| a.dimension == D::Seniority && &a.value == level)
            {
                reading.assertions.push(ReadAssertion {
                    dimension: D::Seniority,
                    value: level.clone(),
                    polarity: Polarity::Prefer,
                    confidence: TasteConfidence::Medium,
                    text: vocab::sentence(D::Seniority, level, Polarity::Prefer),
                    explanation: Some(format!(
                        "Your latest title is “{title}”. Other levels aren't ruled out."
                    )),
                    origin: TasteOrigin::Profile,
                    sources: vec![TasteSource::Evidence {
                        text: format!("latest title “{title}”"),
                        records: latest.records.iter().take(1).cloned().collect(),
                    }],
                });
            }
        }
        reading
    }
}

#[async_trait]
impl TasteInterpreter for RulesInterpreter {
    fn name(&self) -> String {
        Self::NAME.to_owned()
    }

    fn is_remote(&self) -> bool {
        false
    }

    async fn interpret(&self, request: &TasteRequest) -> Result<TasteReading, InterpretError> {
        Ok(self.read(request))
    }
}

#[cfg(test)]
mod tests {
    use super::super::reading::{EvidenceItem, Words};
    use super::*;

    fn keys(r: &RuleReading) -> Vec<(TasteDimension, &str, Polarity)> {
        r.hits
            .iter()
            .map(|h| (h.dimension, h.value.as_str(), h.polarity))
            .collect()
    }

    fn has(r: &RuleReading, d: TasteDimension, v: &str, p: Polarity) -> bool {
        keys(r).contains(&(d, v, p))
    }

    #[test]
    fn senior_startup_generalist() {
        let r = read(
            "I like small technical teams where I can own things end to end. Backend/platform work, \
             startups or small growth companies, remote. I don't want early-career roles or giant \
             process-heavy companies.",
        );
        for (d, v, p) in [
            (D::Team, "small_team", Polarity::Prefer),
            (D::Ownership, "high", Polarity::Prefer),
            (D::Specialization, "broad", Polarity::Prefer),
            (D::WorkShape, "backend", Polarity::Prefer),
            (D::WorkShape, "platform", Polarity::Prefer),
            (D::Company, "startup", Polarity::Prefer),
            (D::Company, "growth", Polarity::Prefer),
            (D::Company, "small_company", Polarity::Prefer),
            (D::Seniority, "early_career", Polarity::Avoid),
            (D::Culture, "process_heavy", Polarity::Avoid),
            (D::Company, "large_company", Polarity::Avoid),
        ] {
            assert!(
                has(&r, d, v, p),
                "{d}:{v} {p:?} missing from {:?}",
                keys(&r)
            );
        }
        assert_eq!(r.constraints, ["remote"], "remote is practical, not taste");
        assert!(r.unread.is_empty(), "{:?}", r.unread);
    }

    #[test]
    fn database_internals_specialist() {
        let r = read(
            "I want to work on database internals, query engines and storage systems. Deep \
             specialization is a plus.",
        );
        assert!(has(
            &r,
            D::WorkShape,
            "database_internals",
            Polarity::Prefer
        ));
        assert!(has(&r, D::Specialization, "deep", Polarity::Prefer));
        assert!(!keys(&r).iter().any(|(_, _, p)| *p == Polarity::Avoid));
    }

    #[test]
    fn early_career_generalist() {
        let r = read(
            "I'm early in my career and want a broad backend role where I can learn from a larger \
             engineering team.",
        );
        for (d, v) in [
            (D::Seniority, "early_career"),
            (D::Specialization, "broad"),
            (D::WorkShape, "backend"),
            (D::Culture, "mentorship"),
            (D::Team, "large_team"),
        ] {
            assert!(has(&r, d, v, Polarity::Prefer), "{d}:{v} in {:?}", keys(&r));
        }
    }

    #[test]
    fn dislikes_after_but_are_avoided() {
        let r = read(
            "I like infrastructure-heavy product engineering, but I don't want to work on database \
             internals or ML research.",
        );
        assert!(has(&r, D::WorkShape, "infrastructure", Polarity::Prefer));
        assert!(has(&r, D::WorkShape, "product", Polarity::Prefer));
        assert!(has(&r, D::WorkShape, "database_internals", Polarity::Avoid));
        assert!(has(&r, D::WorkShape, "ml_research", Polarity::Avoid));
        assert!(
            !keys(&r).iter().any(|(_, v, _)| *v == "research"),
            "research is part of ML research"
        );
    }

    #[test]
    fn open_and_hedged_readings() {
        let r = read("Open to startups or small established companies. Maybe staff roles.");
        assert!(has(&r, D::Company, "startup", Polarity::Open));
        assert!(has(&r, D::Company, "small_company", Polarity::Open));
        assert!(has(&r, D::Company, "established", Polarity::Open));
        let staff = r.hits.iter().find(|h| h.value == "staff_plus").unwrap();
        assert_eq!(staff.confidence, TasteConfidence::Medium);
    }

    #[test]
    fn senior_generalist_with_preferably() {
        let r = read(
            "Senior generalist role, broad ownership, preferably a startup or small growth company.",
        );
        for (d, v) in [
            (D::Seniority, "senior"),
            (D::Specialization, "broad"),
            (D::Ownership, "high"),
            (D::Company, "startup"),
            (D::Company, "growth"),
            (D::Company, "small_company"),
        ] {
            assert!(has(&r, d, v, Polarity::Prefer), "{d}:{v} in {:?}", keys(&r));
        }
    }

    #[test]
    fn a_kind_of_work_is_not_also_a_domain() {
        let r = read("I'd love developer tooling.");
        assert_eq!(
            keys(&r),
            [(D::WorkShape, "developer_tooling", Polarity::Prefer)]
        );
        let r = read("Fintech, not adtech.");
        assert!(
            r.hits.iter().all(|h| h.dimension == D::Domain),
            "{:?}",
            keys(&r)
        );
    }

    #[test]
    fn nothing_is_read_from_nothing() {
        let r = read("Something that feels right, with good vibes.");
        assert!(r.hits.is_empty());
        assert_eq!(r.unread.len(), 1);
    }

    #[test]
    fn the_latest_title_seeds_a_level_only_when_the_words_do_not() {
        let request = |text: &str| TasteRequest {
            words: vec![Words {
                text: text.into(),
                statement: None,
            }],
            evidence: vec![EvidenceItem {
                id: "e1".into(),
                text: "Senior Software Engineer".into(),
                title: Some("Senior Software Engineer".into()),
                latest: true,
                records: vec!["exp_1".into()],
            }],
            feedback: Vec::new(),
            settled: Vec::new(),
            focus: None,
        };
        let r = RulesInterpreter.read(&request("Small teams. No early-career roles."));
        let senior = r
            .assertions
            .iter()
            .find(|a| a.dimension == D::Seniority && a.value == "senior")
            .expect("inferred");
        assert_eq!(senior.origin, TasteOrigin::Profile);
        assert_eq!(senior.confidence, TasteConfidence::Medium);
        let r = RulesInterpreter.read(&request("Staff-plus roles only."));
        assert!(
            !r.assertions
                .iter()
                .any(|a| a.value == "senior" && a.origin == TasteOrigin::Profile),
            "the person's words win"
        );
    }
}
