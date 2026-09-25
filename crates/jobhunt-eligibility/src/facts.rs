//! What a job posting says about where and how it can be done, and what it
//! pays, with the evidence for each statement.
//!
//! Facts come from the structured fields sources publish (locations, the
//! workplace type, the remote flag, employment type, pay ranges, Work at a
//! Startup's visa field) and from sentences of the description that state
//! eligibility ("open to candidates based in North America and Europe",
//! "we do sponsor visas", "(in the US - Pacific timezone)"). Every fact
//! keeps its [`Evidence`]: the source, the field, the words, and when the
//! source was last seen listing the job. Nothing is inferred from silence:
//! a remote flag with no place attached says nothing about where.

use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use jobhunt_core::SourceKey;
use jobhunt_jobs::{CompensationKind, EmploymentType, JobRecord, PayInterval, WorkplaceType};
use jobhunt_profile::words::{Pattern, words};

use crate::geo::{Area, Country, Place, lookup_code, parse_places, places_in_text};
use crate::zones::{Offsets, zones_in};

/// Where a fact was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub source: SourceKey,
    /// `locations`, `workplace_type`, `description`, `work_authorization`, …
    pub field: &'static str,
    /// The source's words.
    pub text: String,
    /// When the source was last seen listing the job.
    pub seen_at: DateTime<Utc>,
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: “{}”", self.source, self.field, self.text)
    }
}

/// How the job is done, as its source says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    Remote,
    Hybrid,
    Onsite,
    /// Some options are remote, others are offices.
    Mixed,
    /// Not published.
    Unknown,
}

impl JobMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Hybrid => "hybrid",
            Self::Onsite => "on-site",
            Self::Mixed => "remote or office",
            Self::Unknown => "work arrangement not stated",
        }
    }

    pub fn allows_remote(self) -> bool {
        matches!(self, Self::Remote | Self::Mixed)
    }
}

/// Why an area is part of where the job can be done from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The source names it ("Remote (Canada)", "Europe" on a remote job).
    Stated,
    /// A remote job that lists an office city: remote work is probably
    /// limited to that city's country, but the source does not say so.
    OfficeCountry,
    /// A sentence of the description.
    Description,
}

/// A place tied to a job.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceFact {
    /// `None` when JobHunt did not recognize the place.
    pub area: Option<Area>,
    pub raw: String,
    pub basis: Basis,
    /// A requirement rather than a preference ("must be based in" versus
    /// "particularly interested in").
    pub strict: bool,
    pub evidence: Evidence,
}

/// "Must be authorized to work in X" and similar.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationFact {
    pub countries: Vec<&'static Country>,
    /// A requirement rather than a preference.
    pub strict: bool,
    pub evidence: Evidence,
}

/// A time-zone expectation.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneFact {
    pub label: String,
    pub offsets: Offsets,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SponsorshipFact {
    pub offered: bool,
    /// Offered, "but not for every role".
    pub caveat: bool,
    pub evidence: Evidence,
}

/// Contractors, or hiring through an employer of record.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractorFact {
    /// Where contractors are hired from; empty when not said.
    pub areas: Vec<Area>,
    pub evidence: Evidence,
}

/// How a salary's currency is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrencyBasis {
    /// The source gives an ISO code.
    Stated,
    /// "$" on a job whose places are all in one dollar country.
    FromLocation(&'static Country),
    /// Not known.
    Unknown,
}

/// One published salary range.
#[derive(Debug, Clone, PartialEq)]
pub struct SalaryFact {
    pub currency: Option<String>,
    pub currency_basis: CurrencyBasis,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub interval: Option<PayInterval>,
    /// The source's label ("Canada Annual Pay Range").
    pub label: Option<String>,
    pub evidence: Evidence,
}

/// Everything a job says about eligibility.
#[derive(Debug, Clone, PartialEq)]
pub struct JobFacts {
    pub mode: JobMode,
    pub mode_evidence: Vec<Evidence>,
    /// Offices: on-site or hybrid places.
    pub offices: Vec<PlaceFact>,
    /// Where remote work is allowed, from the location fields.
    pub remote_areas: Vec<PlaceFact>,
    /// A remote option with no place given ("Remote").
    pub remote_unplaced: Vec<Evidence>,
    /// Description statements limiting where people can be ("open to
    /// candidates based in North America and Europe").
    pub restrictions: Vec<PlaceFact>,
    /// Places the description rules out.
    pub exclusions: Vec<PlaceFact>,
    /// The description says people can be anywhere.
    pub worldwide: Option<Evidence>,
    pub authorization: Vec<AuthorizationFact>,
    pub zones: Vec<ZoneFact>,
    pub sponsorship: Option<SponsorshipFact>,
    pub contractors: Option<ContractorFact>,
    pub relocation: Option<Evidence>,
    pub employment: Option<(EmploymentType, Evidence)>,
    pub salaries: Vec<SalaryFact>,
    /// Pay published only as text that could not be read into a range.
    pub pay_text: Option<Evidence>,
}

impl JobFacts {
    /// Every country the job's places are in.
    pub fn countries(&self) -> Vec<&'static Country> {
        let mut out: Vec<&'static Country> = Vec::new();
        for p in self
            .offices
            .iter()
            .chain(&self.remote_areas)
            .chain(&self.restrictions)
        {
            if let Some(c) = p.area.and_then(|a| a.country())
                && !out.iter().any(|x| x.code == c.code)
            {
                out.push(c);
            }
        }
        out
    }
}

fn evidence(record: &JobRecord, field: &'static str, text: &str) -> Evidence {
    Evidence {
        source: record.posting.provenance.source.clone(),
        field,
        text: text.trim().to_owned(),
        seen_at: record.last_seen_at,
    }
}

/// Reads a job's eligibility facts.
pub fn job_facts(record: &JobRecord) -> JobFacts {
    let posting = &record.posting;
    let mut facts = JobFacts {
        mode: JobMode::Unknown,
        mode_evidence: Vec::new(),
        offices: Vec::new(),
        remote_areas: Vec::new(),
        remote_unplaced: Vec::new(),
        restrictions: Vec::new(),
        exclusions: Vec::new(),
        worldwide: None,
        authorization: Vec::new(),
        zones: Vec::new(),
        sponsorship: None,
        contractors: None,
        relocation: None,
        employment: None,
        salaries: Vec::new(),
        pay_text: None,
    };

    // Places, from each listed location (or the primary location text).
    let mut places: Vec<(Place, Evidence)> = Vec::new();
    for location in &posting.locations {
        let Some(name) = location.name.clone().or_else(|| {
            let parts: Vec<&str> = [&location.locality, &location.region, &location.country]
                .into_iter()
                .filter_map(|p| p.as_deref())
                .collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }) else {
            continue;
        };
        let country = location
            .country
            .as_deref()
            .and_then(|c| lookup_code(&c.to_uppercase(), false))
            .and_then(|a| a.country());
        for mut place in parse_places(&name) {
            // The source's structured country fills in what the name leaves out.
            if place.area.is_none()
                && let Some(c) = country
                && !place.raw.is_empty()
                && !place.remote
            {
                place.area = Some(Area::Country(c));
            }
            places.push((place, evidence(record, "locations", &name)));
        }
    }
    // The primary location text often lists more than the structured
    // locations do (Greenhouse offices).
    if let Some(location) = &posting.location {
        for place in parse_places(location) {
            let known = places.iter().any(|(p, _)| {
                p.area == place.area
                    && p.remote == place.remote
                    && (p.area.is_some() || p.raw == place.raw)
            });
            if !known {
                places.push((place, evidence(record, "location", location)));
            }
        }
    }
    let mut seen: Vec<(Option<Area>, bool, String)> = Vec::new();
    places.retain(|(p, _)| {
        let key = (
            p.area,
            p.remote,
            if p.area.is_some() {
                String::new()
            } else {
                p.raw.clone()
            },
        );
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });

    // Mode.
    let any_remote = places.iter().any(|(p, _)| p.remote);
    let all_remote = !places.is_empty() && places.iter().all(|(p, _)| p.remote);
    facts.mode = match &posting.workplace_type {
        Some(WorkplaceType::Remote) => JobMode::Remote,
        Some(WorkplaceType::Hybrid) if any_remote => JobMode::Mixed,
        Some(WorkplaceType::Hybrid) => JobMode::Hybrid,
        Some(WorkplaceType::OnSite) if any_remote => JobMode::Mixed,
        Some(WorkplaceType::OnSite) => JobMode::Onsite,
        _ if all_remote => JobMode::Remote,
        _ if any_remote => JobMode::Mixed,
        _ if posting.is_remote == Some(true) && places.is_empty() => JobMode::Remote,
        _ => JobMode::Unknown,
    };
    if let Some(w) = &posting.workplace_type {
        facts
            .mode_evidence
            .push(evidence(record, "workplace_type", w.as_str()));
    }
    if let Some(remote) = posting.is_remote {
        facts.mode_evidence.push(evidence(
            record,
            "is_remote",
            if remote { "true" } else { "false" },
        ));
    }

    // Place roles.
    for (place, ev) in places {
        let fact = |basis| PlaceFact {
            area: place.area,
            raw: place.raw.clone(),
            basis,
            strict: true,
            evidence: ev.clone(),
        };
        if place.remote {
            match place.area {
                Some(_) => facts.remote_areas.push(fact(Basis::Stated)),
                None => facts.remote_unplaced.push(ev.clone()),
            }
        } else if facts.mode == JobMode::Remote {
            match place.area {
                Some(Area::City { country, .. }) | Some(Area::Subdivision { country, .. }) => {
                    let area = Some(Area::Country(country));
                    if !facts.remote_areas.iter().any(|p| p.area == area) {
                        facts.remote_areas.push(PlaceFact {
                            area,
                            raw: place.raw.clone(),
                            basis: Basis::OfficeCountry,
                            strict: false,
                            evidence: ev.clone(),
                        });
                    }
                }
                Some(_) => facts.remote_areas.push(fact(Basis::Stated)),
                None => facts.remote_unplaced.push(ev.clone()),
            }
        } else {
            facts.offices.push(fact(Basis::Stated));
        }
    }
    if facts.mode == JobMode::Remote
        && facts.remote_areas.is_empty()
        && facts.remote_unplaced.is_empty()
        && let Some(e) = facts.mode_evidence.first()
    {
        facts.remote_unplaced.push(e.clone());
    }

    // Work at a Startup's visa field.
    if let Some(text) = &posting.work_authorization {
        read_authorization_field(record, text, &mut facts);
    }
    if let Some(kind) = &posting.employment_type {
        facts.employment = Some((
            kind.clone(),
            evidence(record, "employment_type", kind.as_str()),
        ));
        if *kind == EmploymentType::Contract && facts.contractors.is_none() {
            facts.contractors = Some(ContractorFact {
                areas: Vec::new(),
                evidence: evidence(record, "employment_type", kind.as_str()),
            });
        }
    }
    if let Some(text) = &posting.description_text {
        for sentence in sentences(text) {
            read_sentence(record, &sentence, &mut facts);
        }
    }
    read_pay(record, &mut facts);
    facts
}

fn read_authorization_field(record: &JobRecord, text: &str, facts: &mut JobFacts) {
    let ev = evidence(record, "work_authorization", text);
    let ws = words(text);
    let has = |p: &str| Pattern::new(p).find(&ws).is_some();
    if has("not required") || has("no requirement*") {
        return;
    }
    if has("sponsor*") && !has("not") && !has("no") {
        facts.sponsorship = Some(SponsorshipFact {
            offered: true,
            caveat: false,
            evidence: ev,
        });
        return;
    }
    // Work at a Startup is a US platform: "US citizen/visa only".
    let mut countries: Vec<&'static Country> = places_in_text(text)
        .into_iter()
        .filter_map(|a| a.country())
        .collect();
    if countries.is_empty() && has("citizen*") {
        countries.extend(crate::geo::country("US"));
    }
    if !countries.is_empty() {
        facts.authorization.push(AuthorizationFact {
            countries,
            strict: true,
            evidence: ev.clone(),
        });
        if has("visa only") || has("citizen*") {
            facts.sponsorship.get_or_insert(SponsorshipFact {
                offered: false,
                caveat: false,
                evidence: ev,
            });
        }
    }
}

/// Splits description text into sentences, keeping "U.S." together.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let end = match c {
            '\n' | '•' => true,
            '.' | '!' | '?' => {
                let prev = i.checked_sub(1).map(|p| chars[p]);
                let before_prev = i.checked_sub(2).map(|p| chars[p]);
                // "U.S." / "e.g." abbreviations
                let abbreviation = prev.is_some_and(char::is_uppercase)
                    && before_prev.is_none_or(|b| b == '.' || b == ' ');
                next.is_none_or(char::is_whitespace) && !abbreviation
            }
            _ => false,
        };
        if end {
            if *c != '\n' && *c != '•' {
                current.push(*c);
            }
            if !current.trim().is_empty() {
                out.push(current.trim().to_owned());
            }
            current.clear();
        } else {
            current.push(*c);
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

const LIMIT_CUES: [&str; 36] = [
    "based in",
    "based out of",
    "located in",
    "reside in",
    "residing in",
    "resident* of",
    "residents",
    "live in",
    "living in",
    "remote within",
    "remote in",
    "remote from",
    "remotely from",
    "work from",
    "within the",
    "open to candidates",
    "open to applicants",
    "open to remote",
    "candidates in",
    "candidates from",
    "candidates based",
    "applicants in",
    "applicants from",
    "only hire",
    "only hiring",
    "can only",
    "must be in",
    "must be located",
    "must be based",
    "hiring in",
    "hire in",
    "people in",
    "people based",
    "you can be",
    "can be held",
    "can be located",
];

/// Wording that makes a location statement a preference, not a rule.
const SOFT_CUES: [&str; 7] = [
    "prefer*",
    "ideally",
    "particularly interested",
    "encourage*",
    "nice to have",
    "bonus",
    "a plus",
];

/// Wording that places an office ("based in our Toronto office").
const OFFICE_CUES: [&str; 9] = [
    "office*",
    "hub*",
    "based in",
    "based out of",
    "located in",
    "headquarter*",
    "hybrid",
    "on site",
    "in person",
];

const AUTHORIZATION_CUES: [&str; 10] = [
    "authorized to work",
    "authorised to work",
    "eligible to work",
    "right to work",
    "work authorization",
    "work authorisation",
    "citizen*",
    "green card",
    "work permit",
    "security clearance",
];

const EXCLUSION_CUES: [&str; 8] = [
    "not open to",
    "unable to hire",
    "cannot hire",
    "can t hire",
    "do not hire",
    "don t hire",
    "except",
    "excluding",
];

const WORLDWIDE_CUES: [&str; 10] = [
    "work from anywhere",
    "anywhere in the world",
    "remote anywhere",
    "fully distributed",
    "globally distributed",
    "any country",
    "hire anywhere",
    "hire globally",
    "from any location",
    "location independent",
];

const HIRING_CONTEXT: [&str; 12] = [
    "you",
    "candidates",
    "applicants",
    "remote",
    "remotely",
    "hire",
    "hiring",
    "role",
    "position",
    "based",
    "we",
    "team members",
];

/// Sponsorship is only about immigration when the sentence says so.
const VISA_CONTEXT: [&str; 5] = [
    "visa*",
    "immigration",
    "work authori*",
    "work permit*",
    "h 1b",
];

const NEGATIONS: [&str; 12] = [
    "not", "unable", "cannot", "can t", "aren t", "isn t", "won t", "don t", "do not", "no",
    "without", "never",
];

const CONTRACTOR_CUES: [&str; 9] = [
    "contractor*",
    "contract basis",
    "as a contractor",
    "=Deel",
    "=Oyster",
    "employer of record",
    "=EOR",
    "b2b contract*",
    "independent contract*",
];

const ZONE_CONTEXT: [&str; 11] = [
    "time zone*",
    "timezone*",
    "hours",
    "overlap",
    "=PT",
    "=PST",
    "=ET",
    "=EST",
    "=CET",
    "=UTC",
    "=GMT",
];

/// Compiled cue lists, by the list's address and length.
type PatternCache = HashMap<(usize, usize), &'static [Pattern]>;

/// A cue list's patterns, compiled once.
fn compiled(cues: &'static [&'static str]) -> &'static [Pattern] {
    static CACHE: LazyLock<Mutex<PatternCache>> = LazyLock::new(Default::default);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .entry((cues.as_ptr() as usize, cues.len()))
        .or_insert_with(|| Vec::leak(cues.iter().map(|c| Pattern::new(c)).collect()))
}

const RELOCATION_CUES: [&str; 6] = [
    "relocation assistance",
    "relocation support",
    "relocation package",
    "help you relocate",
    "relocation help",
    "visa and relocation",
];

/// Substrings (lowercase) at least one of which every eligibility sentence
/// contains; sentences without any are skipped cheaply.
const KEYWORDS: [&str; 34] = [
    "based",
    "locat",
    "resid",
    "live in",
    "living in",
    "remote",
    "candidat",
    "applicant",
    "hire",
    "hiring",
    "citizen",
    "authori",
    "eligib",
    "right to work",
    "permit",
    "clearance",
    "sponsor",
    "visa",
    "time zone",
    "timezone",
    "hours",
    "overlap",
    "contract",
    "deel",
    "oyster",
    "employer of record",
    "relocat",
    "anywhere",
    "distributed",
    "within",
    "you can be",
    "can be held",
    "people",
    "office",
];

const ZONE_ABBREVIATIONS: [&str; 12] = [
    "PT", "PST", "ET", "EST", "CT", "CST", "MT", "CET", "UTC", "GMT", "BST", "EOR",
];

fn worth_reading(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    KEYWORDS.iter().any(|k| lower.contains(k))
        || sentence
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| ZONE_ABBREVIATIONS.contains(&w))
}

fn read_sentence(record: &JobRecord, sentence: &str, facts: &mut JobFacts) {
    if !worth_reading(sentence) {
        return;
    }
    let ws = words(sentence);
    let has_any =
        |cues: &'static [&'static str]| compiled(cues).iter().any(|p| p.find(&ws).is_some());
    let ev = || evidence(record, "description", sentence);
    // Long sentences are usually about the company, not the job's terms.
    if ws.len() > 60 {
        return;
    }
    let areas = places_in_text(sentence);
    let (cities, broad): (Vec<Area>, Vec<Area>) = areas
        .iter()
        .partition(|a| matches!(a, Area::City { .. } | Area::Subdivision { .. }));

    let excluded = has_any(&EXCLUSION_CUES);
    let limited = has_any(&LIMIT_CUES);
    let authorization = has_any(&AUTHORIZATION_CUES);
    let strict = !has_any(&SOFT_CUES);

    if !broad.is_empty() && (limited || authorization) {
        let target = if excluded {
            &mut facts.exclusions
        } else {
            &mut facts.restrictions
        };
        for area in &broad {
            if !target.iter().any(|p| p.area == Some(*area)) {
                target.push(PlaceFact {
                    area: Some(*area),
                    raw: area.to_string(),
                    basis: Basis::Description,
                    strict,
                    evidence: ev(),
                });
            }
        }
        // "can be held … remotely in the United States": a remote option.
        let remote_words = has_any(&["remote*"]);
        if remote_words && !excluded && strict {
            for area in &broad {
                if !facts.remote_areas.iter().any(|p| p.area == Some(*area)) {
                    facts.remote_areas.push(PlaceFact {
                        area: Some(*area),
                        raw: area.to_string(),
                        basis: Basis::Description,
                        strict: true,
                        evidence: ev(),
                    });
                }
            }
            if matches!(
                facts.mode,
                JobMode::Unknown | JobMode::Onsite | JobMode::Hybrid
            ) && !facts.offices.is_empty()
            {
                facts.mode = JobMode::Mixed;
            } else if facts.mode == JobMode::Unknown {
                facts.mode = JobMode::Remote;
            }
        }
        if authorization && !excluded {
            let countries: Vec<&'static Country> =
                broad.iter().filter_map(|a| a.country()).collect();
            if !countries.is_empty() {
                facts.authorization.push(AuthorizationFact {
                    countries,
                    strict,
                    evidence: ev(),
                });
            }
        }
    }
    // "based in our Toronto office", "hybrid at our Seattle office"
    let office_cue = has_any(&OFFICE_CUES);
    if office_cue && !excluded && facts.mode != JobMode::Remote {
        for area in cities
            .into_iter()
            .filter(|a| matches!(a, Area::City { .. }))
        {
            if !facts.offices.iter().any(|p| p.area == Some(area)) {
                facts.offices.push(PlaceFact {
                    area: Some(area),
                    raw: area.to_string(),
                    basis: Basis::Description,
                    strict: true,
                    evidence: ev(),
                });
            }
        }
    }
    if facts.worldwide.is_none() && has_any(&WORLDWIDE_CUES) && has_any(&HIRING_CONTEXT) {
        facts.worldwide = Some(ev());
    }
    if has_any(&ZONE_CONTEXT) {
        for (label, offsets) in zones_in(sentence) {
            if !facts.zones.iter().any(|z| z.offsets == offsets) {
                facts.zones.push(ZoneFact {
                    label,
                    offsets,
                    evidence: ev(),
                });
            }
        }
    }
    if has_any(&["sponsor*"]) && has_any(&VISA_CONTEXT) {
        let negative = has_any(&NEGATIONS);
        match facts.sponsorship.as_mut() {
            // A statement from a structured field outranks prose.
            Some(existing) if existing.evidence.field != "description" => {}
            // "We do sponsor visas! However, we aren't able to … for every role."
            Some(existing) if existing.offered && negative => existing.caveat = true,
            Some(existing) if existing.offered => {}
            _ => {
                facts.sponsorship = Some(SponsorshipFact {
                    offered: !negative,
                    caveat: false,
                    evidence: ev(),
                })
            }
        }
    }
    if has_any(&CONTRACTOR_CUES) {
        let keep = facts
            .contractors
            .as_ref()
            .is_some_and(|c| c.evidence.field == "description");
        if !keep {
            facts.contractors = Some(ContractorFact {
                areas: broad.clone(),
                evidence: ev(),
            });
        }
    }
    if facts.relocation.is_none() && has_any(&RELOCATION_CUES) {
        facts.relocation = Some(ev());
    }
}

fn read_pay(record: &JobRecord, facts: &mut JobFacts) {
    let Some(comp) = &record.posting.compensation else {
        return;
    };
    let summary = comp.summary.clone().unwrap_or_default();
    // "$" on a job whose places are all in one dollar country.
    let dollar_country = {
        let countries = facts.countries();
        let dollars: Vec<&'static Country> = countries
            .iter()
            .copied()
            .filter(|c| c.dollar.is_some())
            .collect();
        (dollars.len() == 1).then(|| dollars[0])
    };
    for c in &comp.components {
        if c.kind != CompensationKind::Salary || (c.min.is_none() && c.max.is_none()) {
            continue;
        }
        let (currency, basis) = match (&c.currency, dollar_country) {
            (Some(code), _) => (Some(code.clone()), CurrencyBasis::Stated),
            (None, Some(country)) if summary.contains('$') => (
                country.dollar.map(str::to_owned),
                CurrencyBasis::FromLocation(country),
            ),
            (None, _) => (None, CurrencyBasis::Unknown),
        };
        let text = match (&c.label, comp.summary.as_deref()) {
            (Some(label), _) => format!(
                "{label}: {} – {} {}",
                c.min.map_or_else(|| "?".into(), |v| v.to_string()),
                c.max.map_or_else(|| "?".into(), |v| v.to_string()),
                c.currency.as_deref().unwrap_or("")
            ),
            (None, Some(summary)) => summary.to_owned(),
            (None, None) => format!(
                "{} – {} {}",
                c.min.map_or_else(|| "?".into(), |v| v.to_string()),
                c.max.map_or_else(|| "?".into(), |v| v.to_string()),
                c.currency.as_deref().unwrap_or("")
            ),
        };
        facts.salaries.push(SalaryFact {
            currency,
            currency_basis: basis,
            min: c.min,
            max: c.max,
            interval: c.interval,
            label: c.label.clone(),
            evidence: evidence(record, "compensation", &text),
        });
    }
    if facts.salaries.is_empty()
        && let Some(summary) = &comp.summary
    {
        facts.pay_text = Some(evidence(record, "compensation", summary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences_but_not_abbreviations() {
        assert_eq!(
            sentences("Remote in the U.S. only. We do sponsor visas! Apply now"),
            [
                "Remote in the U.S. only.",
                "We do sponsor visas!",
                "Apply now"
            ]
        );
    }
}
