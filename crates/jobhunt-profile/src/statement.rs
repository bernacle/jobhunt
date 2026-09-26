//! Reading preferences out of the user's own words.
//!
//! [`StatementParser`] is the extension point: the built-in
//! [`RuleParser`] is deterministic and needs nothing but the text; an
//! AI-assisted parser can implement the same trait later. Whatever the
//! parser, the statement itself is always stored verbatim, and everything a
//! parser could not read is kept as `unparsed` text rather than dropped.
//!
//! The rule parser splits a statement into clauses (sentences, then at
//! "but", or at "and"/commas when a new clause starts with its own
//! polarity: "I want X and avoid Y"), decides each clause's polarity
//! (wanted, acceptable, required, unwanted) from cue words, and looks for
//! known values in it: roles, compensation amounts, work modes, regions,
//! time zones, relocation and visa needs, company and team kinds, domains
//! and work-style aspects. Hedged clauses ("maybe", "not sure") and values
//! without a clear polarity are marked uncertain.

use jobhunt_core::text::clean_line;

use crate::infer::{canonical_domain, domains_in};
use crate::preferences::{
    Arrangement, Certainty, CompanyTrait, CompensationBound, Engagement, PayPeriod,
    PreferenceValue, Stance, WorkAspect, WorkMode,
};
use crate::words::{Pattern, Word, span_text, words};

/// One preference read from a statement.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPreference {
    pub value: PreferenceValue,
    pub stance: Stance,
    pub certainty: Certainty,
    /// The clause it was read from, verbatim.
    pub snippet: String,
    /// How an ambiguous part was read ("“$” can mean USD, CAD, …").
    pub note: Option<String>,
}

/// What a parser made of a statement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatementReadout {
    pub preferences: Vec<ReadPreference>,
    /// Clauses that produced nothing.
    pub unparsed: Vec<String>,
}

/// Turns a preference statement into structured preferences.
pub trait StatementParser: Send + Sync {
    /// Recorded with each statement (`rules/1`).
    fn name(&self) -> &str;
    fn read(&self, text: &str) -> StatementReadout;
}

/// The built-in deterministic parser.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleParser;

impl StatementParser for RuleParser {
    fn name(&self) -> &str {
        "rules/1"
    }

    fn read(&self, text: &str) -> StatementReadout {
        let mut out = StatementReadout::default();
        for clause in clauses(text) {
            let found = read_clause(&clause, text);
            if found.is_empty() {
                out.unparsed.push(clause);
                continue;
            }
            // A clause can carry a part that says something else ("at least
            // USD 180k, and something about vibes"): keep that part too.
            out.unparsed.extend(unread_parts(&clause, text));
            for pref in found {
                let key = pref.value.key();
                if !out.preferences.iter().any(|p| p.value.key() == key) {
                    out.preferences.push(pref);
                }
            }
        }
        out
    }
}

/// Words that start a clause of their own.
const CLAUSE_STARTS: [&str; 26] = [
    "i",
    "avoid",
    "no",
    "not",
    "never",
    "don",
    "prefer",
    "want",
    "love",
    "like",
    "open",
    "would",
    "at",
    "minimum",
    "ideally",
    "only",
    "must",
    "nothing",
    "tired",
    "please",
    "looking",
    "interested",
    "also",
    "rather",
    "happy",
    "fine",
];

fn clauses(text: &str) -> Vec<String> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let ends = match c {
            '!' | '?' | ';' | '\n' => true,
            // A period ends a sentence unless it sits inside a word or
            // number ("Node.js", "1.5").
            '.' => next.is_none_or(char::is_whitespace),
            _ => false,
        };
        if *c == '?' {
            current.push('?');
        }
        if ends {
            sentences.push(std::mem::take(&mut current));
        } else {
            current.push(*c);
        }
    }
    sentences.push(current);

    let mut out = Vec::new();
    for sentence in sentences {
        let mut pieces = vec![sentence];
        for connector in [" but ", " however ", " although ", " though ", " except "] {
            pieces = pieces
                .into_iter()
                .flat_map(|p| split_ci(&p, connector))
                .collect();
        }
        for piece in pieces {
            out.extend(split_new_clauses(&piece));
        }
    }
    out.into_iter()
        .filter_map(|c| {
            let trimmed = c.trim_matches(|ch: char| ch.is_whitespace() || ch == ',' || ch == '-');
            clean_line(trimmed)
        })
        .collect()
}

fn split_ci(text: &str, separator: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return vec![text.to_owned()];
    }
    let mut out = Vec::new();
    let mut last = 0;
    for (at, _) in lower.match_indices(separator) {
        out.push(text[last..at].to_owned());
        last = at + separator.len();
    }
    out.push(text[last..].to_owned());
    out
}

/// Words that carry no preference on their own.
const FILLER: [&str; 40] = [
    "a", "an", "the", "and", "or", "of", "in", "on", "at", "to", "for", "with", "about", "from",
    "by", "as", "is", "are", "be", "i", "me", "my", "we", "our", "it", "that", "this", "some",
    "any", "more", "very", "really", "also", "just", "role", "roles", "job", "jobs", "work",
    "team",
];

/// Parts of a clause, split at every ", ", " and " and " or ", that say
/// something (at least two words that aren't filler) and yield no
/// preference when read alone. Only called for clauses that yielded
/// preferences, so a list such as "backend, frontend and devops roles" is
/// not broken apart: its parts are single words.
fn unread_parts(clause: &str, statement: &str) -> Vec<String> {
    let lower = clause.to_lowercase();
    if lower.len() != clause.len() {
        return Vec::new();
    }
    let mut parts = vec![clause.to_owned()];
    for separator in [", ", " and ", " or "] {
        parts = parts
            .into_iter()
            .flat_map(|p| split_ci(&p, separator))
            .collect();
    }
    if parts.len() < 2 {
        return Vec::new();
    }
    parts
        .into_iter()
        .filter_map(|part| {
            let mut part = part.trim_matches(|c: char| c.is_whitespace() || c == ',');
            for connector in ["and ", "or "] {
                if part.len() > connector.len()
                    && part[..connector.len()].eq_ignore_ascii_case(connector)
                {
                    part = &part[connector.len()..];
                }
            }
            let part = part.to_owned();
            let meaningful = words(&part)
                .iter()
                .filter(|w| w.lower.len() > 2 && !FILLER.contains(&w.lower.as_str()))
                .count();
            (meaningful >= 2 && read_clause(&part, statement).is_empty()).then_some(part)
        })
        .collect()
}

/// Splits at " and " / ", " when what follows starts a clause of its own.
fn split_new_clauses(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return vec![text.to_owned()];
    }
    let mut search = 0;
    while search < lower.len() {
        let next = [" and ", ", ", " or "]
            .iter()
            .filter_map(|sep| lower[search..].find(sep).map(|i| (search + i, sep.len())))
            .min_by_key(|(i, _)| *i);
        let Some((at, len)) = next else {
            break;
        };
        let rest = &text[at + len..];
        let first = words(rest).into_iter().next().map(|w| w.lower);
        if first.is_some_and(|w| CLAUSE_STARTS.contains(&w.as_str())) {
            out.push(text[start..at].to_owned());
            start = at + len;
        }
        search = at + len;
    }
    out.push(text[start..].to_owned());
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Unwanted,
    Acceptable,
    Required,
    Wanted,
    /// No cue at all.
    Unknown,
}

// Checked in this order; phrases containing a negation that are not
// negative ("don't mind") come first.
const ACCEPTABLE_CUES: [&str; 14] = [
    "don t mind",
    "dont mind",
    "wouldn t mind",
    "not opposed",
    "no problem",
    "open to",
    "consider*",
    "fine with",
    "ok with",
    "okay with",
    "acceptable",
    "happy to",
    "could do",
    "if needed",
];
const UNWANTED_CUES: [&str; 26] = [
    "avoid*",
    "no",
    "not",
    "don t",
    "dont",
    "do not",
    "never",
    "tired of",
    "sick of",
    "dislike",
    "hate",
    "without",
    "rather not",
    "stay away",
    "won t",
    "wouldn t",
    "exclude",
    "nothing",
    "none",
    "less",
    "fewer",
    "few",
    "minimal",
    "burned out",
    "burnt out",
    "zero",
];
const REQUIRED_CUES: [&str; 9] = [
    "must",
    "only",
    "need",
    "needs",
    "require*",
    "at least",
    "minimum",
    "non negotiable",
    "have to",
];
const WANTED_CUES: [&str; 14] = [
    "want*",
    "prefer*",
    "love",
    "like",
    "looking for",
    "interested",
    "excited",
    "ideally",
    "enjoy",
    "seek*",
    "hope",
    "keen",
    "would like",
    "passionate",
];
const HEDGES: [&str; 9] = [
    "maybe", "perhaps", "probably", "might", "not sure", "kind of", "sort of", "possibly",
    "i guess",
];

fn has_any(ws: &[Word], cues: &[&str]) -> bool {
    cues.iter().any(|c| Pattern::new(c).find(ws).is_some())
}

fn polarity(ws: &[Word]) -> Polarity {
    if has_any(ws, &ACCEPTABLE_CUES) {
        Polarity::Acceptable
    } else if has_any(ws, &UNWANTED_CUES) {
        Polarity::Unwanted
    } else if has_any(ws, &REQUIRED_CUES) {
        Polarity::Required
    } else if has_any(ws, &WANTED_CUES) {
        Polarity::Wanted
    } else {
        Polarity::Unknown
    }
}

fn stance_for(polarity: Polarity) -> Stance {
    match polarity {
        Polarity::Unwanted => Stance::Unwanted,
        Polarity::Acceptable => Stance::Acceptable,
        Polarity::Required => Stance::Required,
        Polarity::Wanted | Polarity::Unknown => Stance::Wanted,
    }
}

/// Role phrases. Ambiguous words (platform, security, data) only count as
/// roles next to a role word; otherwise they are domains.
const ROLE_PHRASES: [(&str, &str); 26] = [
    ("founding engineer*", "founding engineer"),
    ("staff engineer*", "staff engineer"),
    ("staff level", "staff engineer"),
    ("principal engineer*", "principal engineer"),
    ("engineering manager*", "engineering manager"),
    ("tech lead*", "tech lead"),
    ("technical lead*", "tech lead"),
    ("team lead*", "tech lead"),
    ("full stack", "full stack"),
    ("fullstack", "full stack"),
    ("backend", "backend"),
    ("back end", "backend"),
    ("frontend", "frontend"),
    ("front end", "frontend"),
    ("=SRE", "sre"),
    ("sre", "sre"),
    ("site reliability", "sre"),
    ("devops", "devops"),
    ("platform engineer*", "platform"),
    ("platform role*", "platform"),
    ("platform team*", "platform"),
    ("infrastructure role*", "infrastructure"),
    ("infra role*", "infrastructure"),
    ("data engineer*", "data engineering"),
    ("ml engineer*", "machine learning"),
    ("mobile", "mobile"),
];

const COMPANY_PHRASES: [(&str, CompanyTrait); 31] = [
    ("start up*", CompanyTrait::Startup),
    ("startup*", CompanyTrait::Startup),
    ("early stage", CompanyTrait::EarlyStage),
    ("seed", CompanyTrait::EarlyStage),
    ("pre seed", CompanyTrait::EarlyStage),
    ("series a", CompanyTrait::EarlyStage),
    ("scale up*", CompanyTrait::Scaleup),
    ("scaleup*", CompanyTrait::Scaleup),
    ("growth stage", CompanyTrait::Scaleup),
    ("big tech", CompanyTrait::LargeCompany),
    ("large compan*", CompanyTrait::LargeCompany),
    ("big compan*", CompanyTrait::LargeCompany),
    ("enterprise*", CompanyTrait::LargeCompany),
    ("corporate*", CompanyTrait::LargeCompany),
    ("corporation*", CompanyTrait::LargeCompany),
    ("founder led", CompanyTrait::FounderLed),
    ("product compan*", CompanyTrait::ProductCompany),
    ("product team*", CompanyTrait::ProductCompany),
    ("product led", CompanyTrait::ProductCompany),
    ("agenc*", CompanyTrait::Agency),
    ("consult*", CompanyTrait::Consulting),
    ("outsourc*", CompanyTrait::Consulting),
    ("body shop*", CompanyTrait::Consulting),
    ("public compan*", CompanyTrait::PublicCompany),
    ("private compan*", CompanyTrait::PrivateCompany),
    // The company's size, not the team's ("small teams" is the team).
    ("small compan*", CompanyTrait::SmallCompany),
    ("tiny compan*", CompanyTrait::SmallCompany),
    ("small _ team*", CompanyTrait::SmallTeam),
    ("small team*", CompanyTrait::SmallTeam),
    ("large team*", CompanyTrait::LargeTeam),
    ("remote first", CompanyTrait::RemoteFirst),
];

const WORK_PHRASES: [(&str, WorkAspect); 27] = [
    ("ownership", WorkAspect::Ownership),
    ("autonomy", WorkAspect::Ownership),
    ("autonomous", WorkAspect::Ownership),
    ("end to end", WorkAspect::Ownership),
    ("=IC", WorkAspect::IndividualContributor),
    ("individual contributor", WorkAspect::IndividualContributor),
    ("hands on", WorkAspect::IndividualContributor),
    ("manage people", WorkAspect::Management),
    ("managing people", WorkAspect::Management),
    ("people management", WorkAspect::Management),
    ("management", WorkAspect::Management),
    ("managing a team", WorkAspect::Management),
    ("greenfield", WorkAspect::Greenfield),
    ("0 to 1", WorkAspect::Greenfield),
    ("zero to one", WorkAspect::Greenfield),
    ("from scratch", WorkAspect::Greenfield),
    ("new product*", WorkAspect::Greenfield),
    ("maintenance", WorkAspect::Maintenance),
    ("legacy", WorkAspect::Maintenance),
    ("async*", WorkAspect::AsyncCommunication),
    ("written communication", WorkAspect::AsyncCommunication),
    ("meeting*", WorkAspect::Meetings),
    ("close to product", WorkAspect::ProductCloseness),
    ("close to _ user*", WorkAspect::ProductCloseness),
    ("product minded", WorkAspect::ProductCloseness),
    ("talk* to users", WorkAspect::ProductCloseness),
    ("on call", WorkAspect::OnCall),
];

const REGIONS: [(&str, &str); 26] = [
    ("europe*", "Europe"),
    ("=EU", "Europe"),
    ("emea", "EMEA"),
    ("north america*", "North America"),
    ("latin america*", "Latin America"),
    ("latam", "Latin America"),
    ("south america*", "South America"),
    ("americas", "Americas"),
    ("apac", "APAC"),
    ("asia", "Asia"),
    ("=US", "United States"),
    ("usa", "United States"),
    ("united states", "United States"),
    ("=UK", "United Kingdom"),
    ("united kingdom", "United Kingdom"),
    ("canada", "Canada"),
    ("brazil", "Brazil"),
    ("portugal", "Portugal"),
    ("spain", "Spain"),
    ("germany", "Germany"),
    ("netherlands", "Netherlands"),
    ("mexico", "Mexico"),
    ("argentina", "Argentina"),
    ("worldwide", "Worldwide"),
    ("anywhere", "Worldwide"),
    ("global*", "Worldwide"),
];

const TIMEZONE_ABBREVIATIONS: [&str; 16] = [
    "EST", "EDT", "CST", "CDT", "MST", "PST", "PDT", "ET", "PT", "CET", "CEST", "BRT", "GMT",
    "UTC", "IST", "JST",
];

fn read_clause(clause: &str, statement: &str) -> Vec<ReadPreference> {
    let ws = words(clause);
    let polarity = polarity(&ws);
    let hedged = has_any(&ws, &HEDGES) || clause.contains('?');
    let certainty = if hedged || polarity == Polarity::Unknown {
        Certainty::Uncertain
    } else {
        Certainty::Certain
    };
    let stance = stance_for(polarity);
    let mut out = Vec::new();
    let mut push_noted = |value, stance, certainty, note: Option<String>| {
        out.push(ReadPreference {
            value,
            stance,
            certainty,
            snippet: clause.to_owned(),
            note,
        })
    };

    // Compensation has its own bounds; polarity words like "no less than"
    // do not make it unwanted.
    let money = amounts(clause);
    let comp_context = has_any(
        &ws,
        &[
            "salary",
            "pay",
            "comp",
            "compensation",
            "rate",
            "earn*",
            "base",
            "package",
            "ote",
            "income",
        ],
    );
    let money: Vec<Amount> = money
        .into_iter()
        .filter(|a| a.currency.is_some() || a.symbol.is_some() || a.thousands || comp_context)
        .collect();
    for (value, stance, certainty, note) in compensation(&ws, &money, hedged, statement) {
        push_noted(value, stance, certainty, note);
    }
    let mut push = |value, stance, certainty| push_noted(value, stance, certainty, None);

    // Location and logistics.
    let remote_only = has_any(
        &ws,
        &[
            "remote only",
            "only remote",
            "fully remote",
            "100 remote",
            "remote first only",
            "full remote",
        ],
    );
    if remote_only {
        push(
            PreferenceValue::WorkMode {
                mode: WorkMode::Remote,
            },
            Stance::Required,
            if hedged {
                Certainty::Uncertain
            } else {
                Certainty::Certain
            },
        );
    } else if Pattern::new("remote").find(&ws).is_some()
        && Pattern::new("remote first").find(&ws).is_none()
    {
        push(
            PreferenceValue::WorkMode {
                mode: WorkMode::Remote,
            },
            stance,
            certainty,
        );
    }
    if has_any(&ws, &["hybrid"]) {
        push(
            PreferenceValue::WorkMode {
                mode: WorkMode::Hybrid,
            },
            stance,
            certainty,
        );
    }
    if has_any(
        &ws,
        &[
            "on site",
            "onsite",
            "in office",
            "in the office",
            "office based",
        ],
    ) {
        push(
            PreferenceValue::WorkMode {
                mode: WorkMode::Onsite,
            },
            stance,
            certainty,
        );
    }
    if has_any(&ws, &["relocat*", "move abroad", "move to"]) {
        let willing = polarity != Polarity::Unwanted;
        push(
            PreferenceValue::Relocation { willing },
            Stance::Required,
            if hedged {
                Certainty::Uncertain
            } else {
                Certainty::Certain
            },
        );
    }
    if has_any(&ws, &["sponsor*", "visa*"]) {
        let needed = polarity != Polarity::Unwanted;
        push(
            PreferenceValue::Sponsorship { needed },
            Stance::Required,
            if hedged {
                Certainty::Uncertain
            } else {
                Certainty::Certain
            },
        );
    }
    let authorized_in = place_after(
        clause,
        &ws,
        &[
            "authorized to work in",
            "authorised to work in",
            "eligible to work in",
            "right to work in",
            "work permit for",
            "work permit in",
        ],
    );
    let names_authorization = has_any(
        &ws,
        &[
            "authorized to work",
            "authorised to work",
            "eligible to work",
            "work permit",
            "right to work",
        ],
    );
    match authorized_in {
        Some(place) if polarity != Polarity::Unwanted => push(
            PreferenceValue::WorkAuthorization { place },
            Stance::Required,
            if hedged {
                Certainty::Uncertain
            } else {
                Certainty::Certain
            },
        ),
        None if names_authorization && !has_any(&ws, &["sponsor*", "visa*"]) => push(
            PreferenceValue::Sponsorship { needed: false },
            Stance::Required,
            Certainty::Uncertain,
        ),
        _ => {}
    }
    // "open to B2B contracts", "as a contractor", "no freelance".
    if has_any(
        &ws,
        &[
            "contractor*",
            "b2b",
            "freelanc*",
            "contract work",
            "contract roles",
            "contract basis",
            "independent contract*",
        ],
    ) {
        push(
            PreferenceValue::Engagement {
                engagement: Engagement::Contractor,
            },
            stance,
            certainty,
        );
    }
    if has_any(
        &ws,
        &[
            "as an employee",
            "full time employment",
            "permanent employment",
            "employee only",
        ],
    ) {
        push(
            PreferenceValue::Engagement {
                engagement: Engagement::Employee,
            },
            stance,
            certainty,
        );
    }
    let place = current_place(clause, &ws);
    if let Some(place) = &place {
        push(
            PreferenceValue::CurrentLocation {
                place: place.clone(),
            },
            Stance::Required,
            Certainty::Certain,
        );
    }
    for zone in timezones(clause, &ws) {
        push(PreferenceValue::Timezone { zone }, stance, certainty);
    }
    let place_lower = place.as_deref().map(str::to_lowercase);
    let mut regions: Vec<&str> = Vec::new();
    for (pattern, region) in REGIONS {
        if let Some(range) = Pattern::new(pattern).find(&ws) {
            // "European time zones" is a time zone, not a region wish.
            let next = ws.get(range.end).map(|w| w.lower.as_str());
            if matches!(next, Some("time" | "timezone" | "timezones" | "hours")) {
                continue;
            }
            let matched = span_text(clause, &ws, &range).to_lowercase();
            // "based in Brazil" is where the user is, not a wish.
            if place_lower.as_deref().is_some_and(|p| p.contains(&matched)) {
                continue;
            }
            if !regions.contains(&region) {
                regions.push(region);
                push(
                    PreferenceValue::Region {
                        region: region.to_owned(),
                    },
                    stance,
                    certainty,
                );
            }
        }
    }

    // Roles.
    let role_context = has_any(&ws, &["role*", "job*", "position*", "work", "engineer*"]);
    let mut roles: Vec<&str> = Vec::new();
    for (pattern, role) in ROLE_PHRASES {
        if roles.contains(&role) {
            continue;
        }
        if Pattern::new(pattern).find(&ws).is_some() {
            // "mobile" alone might describe a product; require role words.
            if role == "mobile" && !role_context {
                continue;
            }
            roles.push(role);
            push(
                PreferenceValue::Role {
                    role: role.to_owned(),
                },
                stance,
                certainty,
            );
        }
    }

    // Company and team kinds.
    let mut traits: Vec<CompanyTrait> = Vec::new();
    for (pattern, company) in COMPANY_PHRASES {
        if traits.contains(&company) {
            continue;
        }
        // "enterprise customers" describes a market, not an employer.
        if company == CompanyTrait::LargeCompany
            && has_any(
                &ws,
                &[
                    "enterprise customer*",
                    "enterprise software",
                    "enterprise sales",
                ],
            )
        {
            continue;
        }
        if Pattern::new(pattern).find(&ws).is_some() {
            traits.push(company);
            push(PreferenceValue::Company { company }, stance, certainty);
        }
    }

    // Work style.
    let mut aspects: Vec<WorkAspect> = Vec::new();
    for (pattern, aspect) in WORK_PHRASES {
        if aspects.contains(&aspect) {
            continue;
        }
        if aspect == WorkAspect::Management && roles.contains(&"engineering manager") {
            continue;
        }
        if Pattern::new(pattern).find(&ws).is_some() {
            aspects.push(aspect);
            push(PreferenceValue::WorkStyle { aspect }, stance, certainty);
        }
    }

    // Domains, when named outright.
    let mut domains: Vec<&str> = Vec::new();
    for hit in domains_in(clause).into_iter().filter(|h| h.strong) {
        // A role or a location word is not a domain here.
        if (roles.contains(&"sre") || roles.contains(&"platform")) && hit.domain == "infrastructure"
        {
            continue;
        }
        if hit.domain == "b2b saas" && traits.contains(&CompanyTrait::ProductCompany) {
            continue;
        }
        let Some(domain) = canonical_domain(&hit.phrase) else {
            continue;
        };
        if !domains.contains(&domain) {
            domains.push(domain);
            push(
                PreferenceValue::Domain {
                    domain: domain.to_owned(),
                },
                stance,
                certainty,
            );
        }
    }
    out
}

/// A money amount in text.
#[derive(Debug, Clone, PartialEq)]
struct Amount {
    value: u64,
    /// ISO code, when the text says which currency it is.
    currency: Option<&'static str>,
    /// A symbol shared by several currencies ("$", "¥"), when that is all
    /// the amount says.
    symbol: Option<&'static str>,
    period: Option<PayPeriod>,
    /// Written with a thousands suffix ("120k").
    thousands: bool,
    start: usize,
}

/// Symbols that name exactly one currency. "$" and "¥" are not here: they
/// are shared by several currencies (see [`AMBIGUOUS_SYMBOLS`]).
const CURRENCY_PREFIXES: [(&str, &str); 15] = [
    ("US$", "USD"),
    ("R$", "BRL"),
    ("CA$", "CAD"),
    ("C$", "CAD"),
    ("A$", "AUD"),
    ("AU$", "AUD"),
    ("NZ$", "NZD"),
    ("MX$", "MXN"),
    ("S$", "SGD"),
    ("HK$", "HKD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("₹", "INR"),
    ("CHF", "CHF"),
    ("CN¥", "CNY"),
];

const CURRENCY_CODES: [&str; 18] = [
    "USD", "EUR", "GBP", "BRL", "CAD", "AUD", "CHF", "JPY", "CNY", "INR", "MXN", "ARS", "SEK",
    "PLN", "NZD", "SGD", "HKD", "COP",
];

/// Symbols several currencies use, with the words that can tell them apart
/// when the statement uses them. Without such evidence the currency stays
/// unknown: "$120k" alone could be USD, CAD, AUD, …
struct AmbiguousSymbol {
    symbol: &'static str,
    /// For the note: what it can mean.
    meanings: &'static str,
    /// Currency code, and words in a statement that point to it.
    contexts: &'static [(&'static str, &'static [&'static str])],
}

const AMBIGUOUS_SYMBOLS: [AmbiguousSymbol; 2] = [
    AmbiguousSymbol {
        symbol: "$",
        meanings: "USD, CAD, AUD, NZD, SGD, MXN and other dollars",
        contexts: &[
            (
                "USD",
                &[
                    "=US",
                    "=USA",
                    "=U.S",
                    "united states",
                    "american",
                    "san francisco",
                    "bay area",
                    "new york",
                    "seattle",
                    "austin",
                    "boston",
                    "los angeles",
                    "chicago",
                ],
            ),
            (
                "CAD",
                &["canada", "canadian", "toronto", "vancouver", "montreal"],
            ),
            ("AUD", &["australia", "australian", "sydney", "melbourne"]),
            ("NZD", &["new zealand", "auckland", "wellington"]),
            ("SGD", &["singapore"]),
            ("HKD", &["hong kong"]),
            ("MXN", &["mexico", "mexican"]),
        ],
    },
    AmbiguousSymbol {
        symbol: "¥",
        meanings: "JPY and CNY",
        contexts: &[
            ("JPY", &["japan", "japanese", "tokyo"]),
            ("CNY", &["china", "chinese", "shanghai", "beijing"]),
        ],
    },
];

/// Finds money amounts: `$120k`, `120,000 USD`, `€90k/year`, `R$ 25.000 por mês`,
/// `$75/hour`, `120-150k`.
fn amounts(text: &str) -> Vec<Amount> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if !c.is_ascii_digit() || (i > 0 && chars[i - 1].1.is_alphanumeric()) {
            i += 1;
            continue;
        }
        // Digits with thousands separators and an optional decimal part.
        let mut j = i;
        let mut raw = String::new();
        while j < chars.len() {
            let ch = chars[j].1;
            let next_is_digit = chars.get(j + 1).is_some_and(|(_, n)| n.is_ascii_digit());
            if ch.is_ascii_digit() || ((ch == ',' || ch == '.') && next_is_digit) {
                raw.push(ch);
                j += 1;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
        let before = text[..at].trim_end();
        let after = &text[end..];
        let mut currency = CURRENCY_PREFIXES
            .iter()
            .find(|(prefix, _)| before.ends_with(prefix))
            .map(|(_, code)| *code)
            .or_else(|| {
                CURRENCY_CODES
                    .iter()
                    .find(|code| before.to_uppercase().ends_with(*code))
                    .copied()
            });
        let symbol = AMBIGUOUS_SYMBOLS
            .iter()
            .map(|a| a.symbol)
            .find(|symbol| currency.is_none() && before.ends_with(symbol));
        let (mut value, decimals) = number_value(&raw, currency == Some("BRL"));
        let after_trim = after.trim_start();
        let lower_after = after_trim.to_lowercase();
        let mut thousands = false;
        let mut rest = after_trim;
        if lower_after.starts_with('k')
            && !lower_after[1..].starts_with(|c: char| c.is_alphabetic())
        {
            value *= 1_000.0;
            thousands = true;
            rest = &after_trim[1..];
        } else if (lower_after.starts_with('m')
            && !lower_after[1..].starts_with(|c: char| c.is_alphabetic()))
            || lower_after.starts_with("mil ")
        {
            // "1.2M" means millions; Portuguese "mil" means thousand.
            if lower_after.starts_with("mil") {
                value *= 1_000.0;
                rest = &after_trim[3..];
            } else {
                value *= 1_000_000.0;
                rest = &after_trim[1..];
            }
            thousands = true;
        } else if lower_after.starts_with("thousand") {
            value *= 1_000.0;
            thousands = true;
            rest = &after_trim["thousand".len()..];
        } else if decimals && !raw.contains(',') {
            // plain decimal like 1.5 with no suffix: keep as is
        }
        let rest_trim = rest.trim_start();
        if currency.is_none() {
            currency = CURRENCY_CODES
                .iter()
                .find(|code| rest_trim.to_uppercase().starts_with(*code))
                .copied();
        }
        let period = period_after(rest_trim);
        if value >= 1.0 {
            out.push(Amount {
                value: value.round() as u64,
                currency,
                // "$120k USD": the code settles it.
                symbol: if currency.is_some() { None } else { symbol },
                period,
                thousands,
                start: at,
            });
        }
        i = j.max(i + 1);
    }
    // "120-150k": the first number shares the second's suffix and currency.
    for k in 1..out.len() {
        let (a, b) = (out[k - 1].clone(), out[k].clone());
        let between = &text[a.start..b.start];
        let joined = between.contains('-') || between.contains('–') || between.contains(" to ");
        if joined && b.thousands && !a.thousands && a.value < 1_000 {
            out[k - 1].value = a.value * 1_000;
            out[k - 1].thousands = true;
        }
        if joined {
            if out[k - 1].currency.is_none() {
                out[k - 1].currency = b.currency;
            }
            if out[k].currency.is_none() {
                out[k].currency = out[k - 1].currency;
            }
            if out[k - 1].currency.is_some() {
                out[k - 1].symbol = None;
            }
            if out[k].currency.is_some() {
                out[k].symbol = None;
            }
            if out[k - 1].symbol.is_none() && out[k - 1].currency.is_none() {
                out[k - 1].symbol = out[k].symbol;
            }
            if out[k].symbol.is_none() && out[k].currency.is_none() {
                out[k].symbol = out[k - 1].symbol;
            }
            if out[k - 1].period.is_none() {
                out[k - 1].period = b.period;
            }
        }
    }
    out
}

/// Parses "120,000", "120.000" (Brazilian thousands) or "1.5".
fn number_value(raw: &str, dot_thousands: bool) -> (f64, bool) {
    let groups_of_three = |sep: char| {
        let parts: Vec<&str> = raw.split(sep).collect();
        parts.len() > 1 && parts[1..].iter().all(|p| p.len() == 3)
    };
    let cleaned = if groups_of_three(',') {
        raw.replace(',', "")
    } else if groups_of_three('.')
        && (dot_thousands || raw.matches('.').count() > 1 || raw.len() >= 5)
    {
        raw.replace('.', "")
    } else {
        raw.replace(',', ".")
    };
    let decimals = cleaned.contains('.');
    (cleaned.parse().unwrap_or(0.0), decimals)
}

fn period_after(rest: &str) -> Option<PayPeriod> {
    let lower = rest.to_lowercase();
    let lower = lower.trim_start_matches(|c: char| c == '/' || c.is_whitespace());
    let starts = |options: &[&str]| options.iter().any(|o| lower.starts_with(o));
    if starts(&[
        "year",
        "yr",
        "y ",
        "annual",
        "per year",
        "per annum",
        "a year",
        "pa",
        "p.a",
        "anual",
        "ao ano",
        "por ano",
    ]) || lower == "y"
    {
        Some(PayPeriod::Year)
    } else if starts(&[
        "month",
        "mo",
        "per month",
        "a month",
        "monthly",
        "mês",
        "mes",
        "por mês",
        "por mes",
    ]) {
        Some(PayPeriod::Month)
    } else if starts(&["hour", "hr", "h ", "per hour", "an hour", "hourly", "hora"]) || lower == "h"
    {
        Some(PayPeriod::Hour)
    } else if starts(&["day", "per day", "a day", "daily", "dia"]) {
        Some(PayPeriod::Day)
    } else {
        None
    }
}

type CompValue = (PreferenceValue, Stance, Certainty, Option<String>);

/// How the currency of an amount was settled.
enum CurrencyReading {
    /// The text names it (a code, or a symbol only one currency uses).
    Stated(&'static str),
    /// An ambiguous symbol, read from other words in the statement.
    FromContext(&'static str, String),
    /// Nothing says which currency it is.
    Unknown(Option<String>),
}

fn read_currency(amount: &Amount, clause: &[Word], statement: &str) -> CurrencyReading {
    if let Some(code) = amount.currency {
        return CurrencyReading::Stated(code);
    }
    // A code elsewhere in the same clause ("$120k, paid in USD").
    let codes: Vec<&'static str> = CURRENCY_CODES
        .iter()
        .copied()
        .filter(|code| clause.iter().any(|w| w.original == *code))
        .collect();
    if let [code] = codes[..] {
        return CurrencyReading::Stated(code);
    }
    let Some(AmbiguousSymbol {
        symbol,
        meanings,
        contexts,
    }) = amount
        .symbol
        .and_then(|s| AMBIGUOUS_SYMBOLS.iter().find(|a| a.symbol == s))
    else {
        return CurrencyReading::Unknown(None);
    };
    let words = words(statement);
    let mut found: Vec<(&'static str, String)> = Vec::new();
    for (code, cues) in contexts.iter() {
        for cue in cues.iter() {
            if let Some(range) = Pattern::new(cue).find(&words) {
                if !found.iter().any(|(c, _)| c == code) {
                    found.push((code, span_text(statement, &words, &range).to_owned()));
                }
                break;
            }
        }
    }
    let set_it = "set it with `jobhunt preferences set compensation --currency …`";
    match &found[..] {
        [(code, cue)] => CurrencyReading::FromContext(
            code,
            format!(
                "“{symbol}” read as {code} because your statement mentions “{cue}”; \
                 if that is wrong, {set_it}"
            ),
        ),
        _ => CurrencyReading::Unknown(Some(format!(
            "“{symbol}” can mean {meanings}, so the currency is unknown; {set_it}"
        ))),
    }
}

fn compensation(ws: &[Word], money: &[Amount], hedged: bool, statement: &str) -> Vec<CompValue> {
    if money.is_empty() {
        return Vec::new();
    }
    let clause_period = if has_any(ws, &["monthly", "per month", "a month"]) {
        Some(PayPeriod::Month)
    } else if has_any(ws, &["hourly", "per hour", "an hour"]) {
        Some(PayPeriod::Hour)
    } else if has_any(ws, &["daily", "day rate", "per day"]) {
        Some(PayPeriod::Day)
    } else if has_any(ws, &["annual*", "yearly", "per year", "a year"]) {
        Some(PayPeriod::Year)
    } else {
        None
    };
    let arrangement = if has_any(
        ws,
        &[
            "contract*",
            "freelanc*",
            "=B2B",
            "=PJ",
            "day rate",
            "consulting rate",
            "invoice*",
        ],
    ) {
        Some(Arrangement::Contract)
    } else if has_any(
        ws,
        &[
            "full time",
            "employee",
            "salary",
            "salaried",
            "=CLT",
            "permanent",
        ],
    ) {
        Some(Arrangement::Employment)
    } else {
        None
    };
    let minimum_cue = has_any(
        ws,
        &[
            "at least",
            "minimum",
            "min",
            "no less than",
            "not less than",
            "not below",
            "not under",
            "floor",
            "above",
            "more than",
            "over",
            "starting at",
            "must",
            "need",
        ],
    );
    let target_cue = has_any(
        ws,
        &[
            "target*",
            "ideally",
            "ideal",
            "aim*",
            "around",
            "hoping",
            "expect*",
            "looking for",
            "want*",
        ],
    );
    let value = |amount: &Amount, bound: CompensationBound| -> CompValue {
        let (period, period_certain) = match amount.period.or(clause_period) {
            Some(p) => (p, true),
            None if amount.thousands || amount.value >= 10_000 => (PayPeriod::Year, true),
            None if amount.value < 1_000 => (PayPeriod::Hour, false),
            None => (PayPeriod::Month, false),
        };
        let (currency, currency_certain, note) = match read_currency(amount, ws, statement) {
            CurrencyReading::Stated(code) => (Some(code), true, None),
            CurrencyReading::FromContext(code, note) => (Some(code), false, Some(note)),
            CurrencyReading::Unknown(note) => (None, false, note),
        };
        let certain = !hedged && period_certain && currency_certain;
        let stance = match bound {
            CompensationBound::Minimum => Stance::Required,
            CompensationBound::Target => Stance::Wanted,
        };
        (
            PreferenceValue::Compensation {
                bound,
                amount: amount.value,
                currency: currency.map(str::to_owned),
                period,
                arrangement,
            },
            stance,
            if certain {
                Certainty::Certain
            } else {
                Certainty::Uncertain
            },
            note,
        )
    };
    if money.len() >= 2 {
        // A range: the low end is the floor, the high end the target.
        let (low, high) = if money[0].value <= money[1].value {
            (&money[0], &money[1])
        } else {
            (&money[1], &money[0])
        };
        return vec![
            value(low, CompensationBound::Minimum),
            value(high, CompensationBound::Target),
        ];
    }
    let bound = if minimum_cue {
        CompensationBound::Minimum
    } else {
        CompensationBound::Target
    };
    let mut single = value(&money[0], bound);
    if !minimum_cue && !target_cue {
        single.2 = Certainty::Uncertain;
    }
    vec![single]
}

/// "based in Lisbon, Portugal" → "Lisbon, Portugal".
fn current_place(clause: &str, ws: &[Word]) -> Option<String> {
    place_after(
        clause,
        ws,
        &[
            "based in",
            "live in",
            "living in",
            "located in",
            "i m in",
            "i am in",
            "reside in",
        ],
    )
}

/// The place named right after the first of `cues` in the clause.
fn place_after(clause: &str, ws: &[Word], cues: &[&str]) -> Option<String> {
    for cue in cues.iter().copied() {
        if let Some(range) = Pattern::new(cue).find(ws) {
            let start = ws[range.end - 1].span.end;
            let rest = &clause[start..];
            let end = rest
                .find([';', '(', '.', '!', '?'])
                .or_else(|| rest.find(" and "))
                .or_else(|| rest.find(" but "))
                .unwrap_or(rest.len());
            let place = rest[..end].trim().trim_end_matches(',').trim();
            if !place.is_empty() && place.split_whitespace().count() <= 5 {
                return Some(place.to_owned());
            }
        }
    }
    None
}

fn timezones(clause: &str, ws: &[Word]) -> Vec<String> {
    let mut out = Vec::new();
    // UTC-3, GMT+1, UTC+05:30
    for w in ws {
        if w.original == "UTC" || w.original == "GMT" {
            let after = &clause[w.span.end..];
            let offset: String = after
                .chars()
                .take_while(|c| {
                    *c == '+' || *c == '-' || *c == '−' || c.is_ascii_digit() || *c == ':'
                })
                .collect();
            if offset.len() > 1 {
                out.push(format!("{}{}", w.original, offset.replace('−', "-")));
                continue;
            }
        }
        if TIMEZONE_ABBREVIATIONS.contains(&w.original.as_str())
            && w.original != "UTC"
            && w.original != "GMT"
        {
            out.push(w.original.clone());
        }
    }
    // "European time zones", "US hours"
    for pattern in ["_ time zone*", "_ timezone*", "_ hours"] {
        if let Some(range) = Pattern::new(pattern).find(ws) {
            let region = &ws[range.start];
            let known = REGIONS
                .iter()
                .find(|(p, _)| Pattern::new(p).find(std::slice::from_ref(region)).is_some())
                .map(|(_, r)| *r)
                .or(match region.lower.as_str() {
                    "european" => Some("Europe"),
                    "american" => Some("Americas"),
                    _ => None,
                });
            if let Some(region) = known {
                out.push(format!("{region} hours"));
            }
        }
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> StatementReadout {
        RuleParser.read(text)
    }

    fn find<'a>(out: &'a StatementReadout, key: &str) -> &'a ReadPreference {
        out.preferences
            .iter()
            .find(|p| p.value.key() == key)
            .unwrap_or_else(|| {
                panic!(
                    "no {key} in {:?}",
                    out.preferences
                        .iter()
                        .map(|p| p.value.key())
                        .collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn reads_the_example_statement() {
        let out = read("I want small product teams and at least $120k. Avoid pure SRE roles.");
        assert!(out.unparsed.is_empty(), "{:?}", out.unparsed);
        let small = find(&out, "company:small_team");
        assert_eq!(small.stance, Stance::Wanted);
        assert_eq!(small.certainty, Certainty::Certain);
        assert_eq!(small.snippet, "I want small product teams");
        assert_eq!(find(&out, "company:product_company").stance, Stance::Wanted);
        let comp = find(&out, "compensation:minimum:any");
        assert_eq!(
            comp.value,
            PreferenceValue::Compensation {
                bound: CompensationBound::Minimum,
                amount: 120_000,
                currency: None,
                period: PayPeriod::Year,
                arrangement: None,
            },
            "“$” alone does not say which dollar"
        );
        assert_eq!(comp.stance, Stance::Required);
        assert_eq!(comp.certainty, Certainty::Uncertain);
        assert!(comp.note.as_deref().unwrap().contains("USD, CAD, AUD"));
        let sre = find(&out, "role:sre");
        assert_eq!(sre.stance, Stance::Unwanted);
        assert_eq!(sre.snippet, "Avoid pure SRE roles");
        assert_eq!(out.preferences.len(), 4);
    }

    /// The phrase a real person typed at onboarding: two things read,
    /// both marked for confirmation, neither turned into a requirement.
    #[test]
    fn reads_a_terse_onboarding_answer_as_two_unconfirmed_wants() {
        let out = read("Small teams and the sallary of 140k");
        let small = find(&out, "company:small_team");
        assert_eq!(small.stance, Stance::Wanted);
        let comp = out
            .preferences
            .iter()
            .find(|p| matches!(p.value, PreferenceValue::Compensation { .. }))
            .expect("the pay is read");
        assert_eq!(
            comp.value,
            PreferenceValue::Compensation {
                bound: CompensationBound::Target,
                amount: 140_000,
                currency: None,
                period: PayPeriod::Year,
                arrangement: None,
            },
            "no “at least”: a target, and no currency is assumed"
        );
        assert_eq!(comp.stance, Stance::Wanted);
        assert_eq!(comp.certainty, Certainty::Uncertain);
    }

    #[test]
    fn small_companies_and_small_teams_are_different_things() {
        let out = read("I want small companies");
        find(&out, "company:small_company");
        assert!(
            !out.preferences
                .iter()
                .any(|p| p.value.key() == "company:small_team"),
            "a company's size is not a team's"
        );
        let out = read("I want small teams");
        find(&out, "company:small_team");
        for companies in ["I like tiny companies", "Only small companies, please"] {
            let out = read(companies);
            find(&out, "company:small_company");
            assert!(
                !out.preferences
                    .iter()
                    .any(|p| p.value.key() == "company:small_team"),
                "{companies}"
            );
        }
        let out = read("I want tiny teams");
        assert!(
            !out.preferences
                .iter()
                .any(|p| p.value.key() == "company:small_company")
        );
    }

    fn currency(text: &str) -> (Option<String>, Certainty, Option<String>) {
        let out = read(text);
        let comp = out
            .preferences
            .iter()
            .find(|p| matches!(p.value, PreferenceValue::Compensation { .. }))
            .unwrap_or_else(|| panic!("no compensation in {text:?}"));
        let PreferenceValue::Compensation { currency, .. } = &comp.value else {
            unreachable!()
        };
        (currency.clone(), comp.certainty, comp.note.clone())
    }

    #[test]
    fn dollar_signs_stay_ambiguous_without_evidence() {
        // A code or a symbol only one currency uses settles it.
        for (text, code) in [
            ("at least USD 120k", "USD"),
            ("at least $120k USD", "USD"),
            ("at least US$120k", "USD"),
            ("at least CA$150k", "CAD"),
            ("at least A$150k", "AUD"),
            ("at least R$ 25.000 per month", "BRL"),
            ("at least $120k, paid in CAD", "CAD"),
        ] {
            let (currency, certainty, note) = currency(text);
            assert_eq!(currency.as_deref(), Some(code), "{text}");
            assert_eq!(certainty, Certainty::Certain, "{text}");
            assert_eq!(note, None, "{text}");
        }

        // "$" alone: unknown, flagged, explained.
        let (currency_read, certainty, note) = currency("at least $120k");
        assert_eq!((currency_read, certainty), (None, Certainty::Uncertain));
        assert!(note.unwrap().contains("so the currency is unknown"));

        // Context elsewhere in the statement suggests one currency, but
        // only as an uncertain reading with the reason.
        let (currency_read, certainty, note) = currency("I live in Toronto. At least $150k.");
        assert_eq!(currency_read.as_deref(), Some("CAD"));
        assert_eq!(certainty, Certainty::Uncertain);
        assert!(note.unwrap().contains("mentions “Toronto”"));
        let (currency_read, certainty, _) = currency("US-based roles only, at least $120k");
        assert_eq!(
            (currency_read.as_deref(), certainty),
            (Some("USD"), Certainty::Uncertain)
        );

        // Conflicting context stays unknown.
        let (currency_read, _, note) = currency("Remote in the US or Canada, at least $120k");
        assert_eq!(currency_read, None);
        assert!(note.unwrap().contains("unknown"));

        // The same for the yen sign.
        assert_eq!(currency("at least ¥8m").0, None);
        assert_eq!(
            currency("Based in Tokyo, at least ¥8m").0.as_deref(),
            Some("JPY")
        );
    }

    #[test]
    fn reads_work_authorization_and_engagement() {
        let out = read(
            "I'm based in São Paulo and authorized to work in Portugal. Open to B2B contracts.",
        );
        assert_eq!(
            find(&out, "current_location").value,
            PreferenceValue::CurrentLocation {
                place: "São Paulo".into()
            }
        );
        let auth = find(&out, "work_authorization:portugal");
        assert_eq!(auth.stance, Stance::Required);
        assert_eq!(auth.certainty, Certainty::Certain);
        let contractor = find(&out, "engagement:contractor");
        assert_eq!(contractor.stance, Stance::Acceptable);

        // Authorization without a place is only a hint that no sponsorship
        // is needed, and it is marked uncertain.
        let out = read("I have the right to work.");
        let sponsorship = find(&out, "sponsorship");
        assert_eq!(
            sponsorship.value,
            PreferenceValue::Sponsorship { needed: false }
        );
        assert_eq!(sponsorship.certainty, Certainty::Uncertain);

        let out = read("No freelance or contractor roles, please.");
        assert_eq!(find(&out, "engagement:contractor").stance, Stance::Unwanted);
    }

    #[test]
    fn reads_domains_and_mixed_polarity() {
        let out = read("I love developer tools but I'm tired of fintech; no gambling or adtech.");
        assert_eq!(find(&out, "domain:developer tools").stance, Stance::Wanted);
        assert_eq!(find(&out, "domain:fintech").stance, Stance::Unwanted);
        assert_eq!(find(&out, "domain:gambling").stance, Stance::Unwanted);
        assert_eq!(find(&out, "domain:adtech").stance, Stance::Unwanted);
    }

    #[test]
    fn reads_location_constraints() {
        let out = read(
            "I'm based in São Paulo, Brazil and want remote only, ideally with European time zones. \
             Not willing to relocate. I don't need visa sponsorship.",
        );
        assert_eq!(
            find(&out, "current_location").value,
            PreferenceValue::CurrentLocation {
                place: "São Paulo, Brazil".into()
            }
        );
        assert_eq!(find(&out, "work_mode:remote").stance, Stance::Required);
        assert_eq!(
            find(&out, "timezone:europe hours").value,
            PreferenceValue::Timezone {
                zone: "Europe hours".into()
            }
        );
        assert_eq!(
            find(&out, "relocation").value,
            PreferenceValue::Relocation { willing: false }
        );
        assert_eq!(
            find(&out, "sponsorship").value,
            PreferenceValue::Sponsorship { needed: false }
        );
        assert!(
            !out.preferences
                .iter()
                .any(|p| p.value.key() == "region:brazil"),
            "where the user lives is not a wish"
        );
    }

    #[test]
    fn reads_compensation_variants() {
        let out = read("Contract rate of at least €600/day");
        assert_eq!(
            find(&out, "compensation:minimum:contract").value,
            PreferenceValue::Compensation {
                bound: CompensationBound::Minimum,
                amount: 600,
                currency: Some("EUR".into()),
                period: PayPeriod::Day,
                arrangement: Some(Arrangement::Contract),
            }
        );
        let out = read("Looking for 150-180k USD");
        let min = find(&out, "compensation:minimum:any");
        let target = find(&out, "compensation:target:any");
        assert!(matches!(
            min.value,
            PreferenceValue::Compensation {
                amount: 150_000,
                ..
            }
        ));
        assert!(matches!(
            target.value,
            PreferenceValue::Compensation {
                amount: 180_000,
                ..
            }
        ));
        let out = read("Salary at least R$ 25.000 por mês");
        assert!(matches!(
            find(&out, "compensation:minimum:employment").value,
            PreferenceValue::Compensation {
                amount: 25_000,
                period: PayPeriod::Month,
                ..
            }
        ));
        // No currency: kept, but uncertain.
        let out = read("at least 120k");
        let comp = find(&out, "compensation:minimum:any");
        assert_eq!(comp.certainty, Certainty::Uncertain);
        assert!(matches!(
            comp.value,
            PreferenceValue::Compensation { currency: None, .. }
        ));
        // Numbers that are not money are ignored.
        assert!(
            read("teams of 5 to 10 people")
                .preferences
                .iter()
                .all(|p| !matches!(p.value, PreferenceValue::Compensation { .. }))
        );
    }

    #[test]
    fn reads_work_style_and_company_kinds() {
        let out = read(
            "I want ownership and greenfield work, open to early-stage startups, \
             no agencies or consulting, fewer meetings.",
        );
        assert_eq!(find(&out, "work_style:ownership").stance, Stance::Wanted);
        assert_eq!(find(&out, "work_style:greenfield").stance, Stance::Wanted);
        assert_eq!(find(&out, "company:early_stage").stance, Stance::Acceptable);
        assert_eq!(find(&out, "company:startup").stance, Stance::Acceptable);
        assert_eq!(find(&out, "company:agency").stance, Stance::Unwanted);
        assert_eq!(find(&out, "company:consulting").stance, Stance::Unwanted);
        assert_eq!(find(&out, "work_style:meetings").stance, Stance::Unwanted);
    }

    #[test]
    fn roles_with_stances() {
        let out = read(
            "Backend or founding engineer roles; I'd consider full stack. Don't want management.",
        );
        assert_eq!(find(&out, "role:backend").stance, Stance::Wanted);
        assert_eq!(find(&out, "role:founding engineer").stance, Stance::Wanted);
        assert_eq!(find(&out, "role:full stack").stance, Stance::Acceptable);
        assert_eq!(find(&out, "work_style:management").stance, Stance::Unwanted);
    }

    #[test]
    fn keeps_what_it_cannot_read() {
        let out = read("Something with a good vibe. Maybe backend?");
        assert_eq!(out.unparsed, ["Something with a good vibe"]);
        let backend = find(&out, "role:backend");
        assert_eq!(backend.certainty, Certainty::Uncertain, "hedged");
        let out = read("purple elephants");
        assert!(out.preferences.is_empty());
        assert_eq!(out.unparsed, ["purple elephants"]);
    }

    #[test]
    fn keeps_unread_parts_of_read_clauses() {
        let out = read("I want backend roles, at least USD 180k, and something about vibes");
        assert!(
            out.preferences
                .iter()
                .any(|p| p.value.key() == "role:backend")
        );
        assert_eq!(out.unparsed, ["something about vibes"]);
        let out = read("I want backend, frontend and devops roles");
        assert!(out.unparsed.is_empty(), "{:?}", out.unparsed);
    }

    #[test]
    fn splits_clauses_sensibly() {
        assert_eq!(
            clauses("I want Node.js work and avoid PHP. Budget: 1.5k?"),
            ["I want Node.js work", "avoid PHP", "Budget: 1.5k?"]
        );
    }
}
