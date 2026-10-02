//! What a job posting says about where, how and on what terms it can be
//! done, each statement with the evidence it rests on.
//!
//! Structured source fields come first (locations, workplace type, remote
//! flag, employment type, Work at a Startup's visa field); then explicit
//! sentences of the description ("must be based in the US", "open to
//! candidates in LATAM", "we hire contractors in Brazil through Deel",
//! "must overlap Pacific time", "hybrid in London"). Marketing language
//! ("a globally distributed team") is not read as a rule, pay is not
//! hiring scope ("the pay range for applicants based within the United
//! States"), and nothing is read from silence: a remote flag with no place
//! is a remote option whose scope is [`RemoteScope::Unknown`], never
//! "anywhere".
//!
//! Where the structured fields and the description disagree about where
//! the job can be done, both are kept and the disagreement is recorded as
//! a [`Conflict`]; the rules decide what it means for a given person.

use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use jobhunt_core::SourceKey;
use jobhunt_core::text::search_key;
use jobhunt_jobs::{EmploymentType, JobRecord, WorkplaceType};
use jobhunt_profile::words::{Pattern, words};

use crate::geo::{
    Area, Country, Place, Resolution, lookup_code, lookup_code_near, parse_places, places_in_text,
    resolve_name, resolve_within,
};
use crate::zones::{Clock, Zone, zones_in};

/// Where a statement was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub source: SourceKey,
    /// `locations`, `location`, `workplace_type`, `is_remote`,
    /// `employment_type`, `work_authorization`, `description`.
    pub field: &'static str,
    /// The source's own words.
    pub text: String,
    /// When the source was last seen listing the job.
    pub seen_at: DateTime<Utc>,
}

impl Evidence {
    /// A structured field rather than prose.
    pub fn is_structured(&self) -> bool {
        self.field != "description"
    }
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
}

/// Why an area bounds a remote option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeBasis {
    /// A location field names it ("Remote (Canada)", "US-Remote", "Europe"
    /// on a remote job, "US" beside a bare "Remote").
    Stated,
    /// A remote job whose location fields list only places in these
    /// countries, and nothing unscoped beside them: the finite list is the
    /// scope (remote, with Seattle, Austin and San Francisco, is remote in
    /// the United States). An explicit statement of the description
    /// outranks it.
    Listed,
    /// A remote job that lists an office city beside an unscoped "Remote"
    /// or a place Narrow can't read: remote work is probably limited to
    /// that city's country, but the source does not say so. Never enough
    /// for a definite answer.
    OfficeCity,
}

impl ScopeBasis {
    /// Whether the area settles where remote work is allowed.
    pub fn is_decisive(self) -> bool {
        !matches!(self, Self::OfficeCity)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedArea {
    pub area: Area,
    pub raw: String,
    pub basis: ScopeBasis,
    pub evidence: Evidence,
}

/// Where remote work is allowed, from the structured location fields.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteScope {
    /// "Remote - Worldwide", "Anywhere".
    Global(Evidence),
    /// "Remote (US)", "Remote - LATAM", "Remote: Brazil, Argentina".
    Areas(Vec<ScopedArea>),
    /// "Remote" with no place.
    Unknown,
}

impl RemoteScope {
    pub fn label(&self) -> String {
        match self {
            Self::Global(_) => "anywhere (global)".into(),
            Self::Unknown => "scope not published".into(),
            Self::Areas(areas) => {
                let mut names: Vec<String> = Vec::new();
                for a in areas {
                    let name = match a.basis {
                        ScopeBasis::Stated => a.area.to_string(),
                        ScopeBasis::Listed => format!("{} (from {})", a.area, a.raw),
                        ScopeBasis::OfficeCity => format!("{} (inferred from {})", a.area, a.raw),
                    };
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                names.join(", ")
            }
        }
    }
}

/// Being present at an office.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Hybrid,
    Onsite,
    /// An office the source lists without saying how often.
    Office,
}

impl Presence {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::Onsite => "on-site",
            Self::Office => "office-based",
        }
    }
}

/// An engagement mechanism that opens a remote path from some places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mechanism {
    Contractor,
    B2b,
    /// An employer of record (Deel, Oyster, Remote.com, …).
    Eor,
}

impl Mechanism {
    pub fn label(self) -> &'static str {
        match self {
            Self::Contractor => "contractor",
            Self::B2b => "B2B contractor",
            Self::Eor => "employer of record",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MechanismTerm {
    pub mechanism: Mechanism,
    /// Where the mechanism is offered; empty when the source does not say.
    pub areas: Vec<Area>,
    pub evidence: Evidence,
}

/// One way the job can be done.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkOption {
    Remote {
        scope: RemoteScope,
        evidence: Vec<Evidence>,
    },
    Office {
        presence: Presence,
        /// `None` when the office place is not recognized.
        area: Option<Area>,
        raw: String,
        evidence: Evidence,
    },
    /// Remote through a named engagement mechanism in named places ("we
    /// hire contractors in Brazil through Deel").
    Engagement(MechanismTerm),
}

impl WorkOption {
    /// "Remote (the Americas)", "On-site in New York", "Contractor in Brazil".
    pub fn label(&self) -> String {
        match self {
            Self::Remote { scope, .. } => format!("Remote ({})", scope.label()),
            Self::Office {
                presence,
                area,
                raw,
                ..
            } => {
                let place = area.map_or_else(|| raw.clone(), |a| a.to_string());
                let what = match presence {
                    Presence::Hybrid => "Hybrid",
                    Presence::Onsite => "On-site",
                    Presence::Office => "Office",
                };
                format!("{what} in {place}")
            }
            Self::Engagement(m) => {
                let places: Vec<String> = m.areas.iter().map(ToString::to_string).collect();
                let mut label = m.mechanism.label().to_owned();
                if let Some(first) = label.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                if places.is_empty() {
                    label
                } else {
                    format!("{label} in {}", places.join(", "))
                }
            }
        }
    }
}

/// How firmly a statement is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength {
    /// "must", "only", "required", or a plain statement of scope.
    Required,
    /// "preferred", "ideally", "a plus".
    Preferred,
}

/// Whom a statement of the description about places is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// This role: "this role requires you to be based within EMEA", "This
    /// is a remote position available anywhere in the world", a
    /// "Location:" or "Countries:" line.
    Role,
    /// Hiring in general, or the company: "we are open to candidates
    /// across the Americas", "work from anywhere: we have no HQ".
    General,
}

/// A place the description allows or rules out.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaConstraint {
    pub area: Area,
    pub strength: Strength,
    pub subject: Subject,
    pub evidence: Evidence,
}

/// "Must be authorized to work in X".
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationRequirement {
    pub areas: Vec<Area>,
    pub strength: Strength,
    pub evidence: Evidence,
}

/// What a posting says about visa sponsorship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sponsorship {
    /// Offered; `caveat` when "not for every role".
    Offered {
        caveat: bool,
    },
    Unavailable,
}

/// What a posting says about relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relocation {
    /// Help is offered.
    Offered,
    /// Moving is required.
    Required,
}

/// How a time-zone statement constrains the work.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoneKind {
    /// Must be located in these zones ("UTC-5 to UTC+1", "based in a
    /// European time zone").
    Within,
    /// Must work these zones' hours, with an explicit tolerance in hours
    /// when given ("EST ±3 hours").
    Hours { tolerance: Option<f32> },
    /// Must overlap these zones' working day by this many hours.
    Overlap { hours: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoneRequirement {
    /// The words that named the zone.
    pub label: String,
    /// The IANA zones (or the fixed offsets) the words mean: "Pacific
    /// time" is America/Los_Angeles, daylight saving included.
    pub zone: Zone,
    pub kind: ZoneKind,
    pub strength: Strength,
    pub evidence: Evidence,
}

/// Two statements about the same thing that disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub summary: String,
    pub evidence: Vec<Evidence>,
}

/// Everything a posting says that bears on who can take it.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRequirements {
    pub source: SourceKey,
    pub mode: JobMode,
    pub mode_evidence: Vec<Evidence>,
    /// Every way the job can be done, from the source's fields and
    /// explicit sentences.
    pub options: Vec<WorkOption>,
    /// Places the description limits the job to.
    pub allow: Vec<AreaConstraint>,
    /// Places the description rules out.
    pub deny: Vec<AreaConstraint>,
    /// The description explicitly says people can be anywhere, and about
    /// whom.
    pub worldwide: Option<(Subject, Evidence)>,
    pub authorization: Vec<AuthorizationRequirement>,
    pub sponsorship: Option<(Sponsorship, Evidence)>,
    pub relocation: Option<(Relocation, Evidence)>,
    pub employment: Option<(EmploymentType, Evidence)>,
    /// Engagement mechanisms the posting names (contractors, EOR).
    pub mechanisms: Vec<MechanismTerm>,
    /// The posting says it hires employees only (no contractors).
    pub employee_only: Option<Evidence>,
    pub zones: Vec<ZoneRequirement>,
    /// "Fully async", "flexible hours": no working-hours requirement.
    pub flexible_hours: Option<Evidence>,
    /// Time-zone wording too vague to read ("overlap with the team").
    pub vague_zone: Option<Evidence>,
    pub conflicts: Vec<Conflict>,
    /// Location text JobHunt does not recognize.
    pub unrecognized: Vec<Evidence>,
    /// Statements that expect presence or travel whatever the job's own
    /// location says ("we expect all staff to be in one of our offices at
    /// least 25% of the time", "Remote-Friendly (Travel-Required)"), with
    /// or without a named place. On a remote job they are a contradiction
    /// to surface, not a conclusion to draw.
    pub presence_policy: Vec<Evidence>,
}

impl JobRequirements {
    pub fn remote_option(&self) -> Option<&RemoteScope> {
        self.options.iter().find_map(|o| match o {
            WorkOption::Remote { scope, .. } => Some(scope),
            _ => None,
        })
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

/// Reads a stored job's requirements.
pub fn requirements(record: &JobRecord) -> JobRequirements {
    let posting = &record.posting;
    let mut job = JobRequirements {
        source: posting.provenance.source.clone(),
        mode: JobMode::Unknown,
        mode_evidence: Vec::new(),
        options: Vec::new(),
        allow: Vec::new(),
        deny: Vec::new(),
        worldwide: None,
        authorization: Vec::new(),
        sponsorship: None,
        relocation: None,
        employment: None,
        mechanisms: Vec::new(),
        employee_only: None,
        zones: Vec::new(),
        flexible_hours: None,
        vague_zone: None,
        conflicts: Vec::new(),
        unrecognized: Vec::new(),
        presence_policy: Vec::new(),
    };

    let mut places = structured_places(record);
    // A bare "Remote" beside only countries or regions ("Remote" with the
    // location "US", as Stripe lists it): those places are where remote
    // work is allowed, not offices.
    let unscoped_remote = places.iter().any(|(p, _, _)| p.remote && p.area.is_none());
    let others: Vec<&(Place, Evidence, bool)> =
        places.iter().filter(|(p, _, _)| !p.remote).collect();
    if unscoped_remote
        && matches!(posting.workplace_type, None | Some(WorkplaceType::Remote))
        && !others.is_empty()
        && others.iter().all(|(p, _, filled)| {
            !filled && matches!(p.area, Some(Area::Country(_) | Area::Region(_)))
        })
    {
        for (p, _, _) in &mut places {
            p.remote |= p.area.is_some();
        }
    }
    // "Remote-Friendly (Travel-Required)": travel stated in a location field.
    for (field, text) in posting
        .location
        .iter()
        .map(|l| ("location", l.as_str()))
        .chain(
            posting
                .locations
                .iter()
                .filter_map(|l| l.name.as_deref())
                .map(|n| ("locations", n)),
        )
    {
        let ws = words(text);
        if compiled(&TRAVEL_CUES).iter().any(|p| p.find(&ws).is_some())
            && !job.presence_policy.iter().any(|e| e.text == text.trim())
        {
            job.presence_policy.push(evidence(record, field, text));
        }
    }

    // Mode, from the workplace type first, then the places.
    let any_remote = places.iter().any(|(p, _, _)| p.remote);
    let all_remote = !places.is_empty() && places.iter().all(|(p, _, _)| p.remote);
    job.mode = match &posting.workplace_type {
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
        job.mode_evidence
            .push(evidence(record, "workplace_type", w.as_str()));
    }
    if let Some(remote) = posting.is_remote {
        job.mode_evidence.push(evidence(
            record,
            "is_remote",
            if remote { "true" } else { "false" },
        ));
    }

    // Places into options.
    let presence = match job.mode {
        JobMode::Hybrid => Presence::Hybrid,
        JobMode::Onsite => Presence::Onsite,
        _ => Presence::Office,
    };
    let mut scope: Vec<ScopedArea> = Vec::new();
    let mut global: Option<Evidence> = None;
    // A remote place without a readable scope: an unscoped "Remote", or a
    // place Narrow doesn't recognize.
    let mut unscoped = false;
    let mut remote_evidence: Vec<Evidence> = job
        .mode_evidence
        .iter()
        .filter(|e| job.mode == JobMode::Remote || e.field == "is_remote")
        .cloned()
        .collect();
    for (place, ev, filled) in places {
        let remote_place = place.remote || job.mode == JobMode::Remote;
        // A place that is itself remote ("Remote (US)", "Remote, Global") is
        // also evidence that the job is remote.
        if place.remote && place.area.is_some() {
            remote_evidence.push(ev.clone());
        }
        match (remote_place, place.area) {
            (true, Some(Area::Worldwide)) => global = Some(ev),
            (
                true,
                Some(area @ (Area::City { country, .. } | Area::Subdivision { country, .. })),
            ) if !place.remote => {
                // A remote job that lists an office city.
                let _ = area;
                let country_area = Area::Country(country);
                if !scope.iter().any(|s| s.area == country_area) {
                    scope.push(ScopedArea {
                        area: country_area,
                        raw: place.raw.clone(),
                        basis: ScopeBasis::OfficeCity,
                        evidence: ev,
                    });
                }
            }
            // An office town named with only the source's country: that
            // country is where the office is, not a stated remote scope.
            (true, Some(area @ Area::Country(_))) if filled && !place.remote => {
                if !scope.iter().any(|s| s.area == area) {
                    scope.push(ScopedArea {
                        area,
                        raw: place.raw.clone(),
                        basis: ScopeBasis::OfficeCity,
                        evidence: ev,
                    });
                }
            }
            (true, Some(area)) => {
                let stated = ScopedArea {
                    area,
                    raw: place.raw.clone(),
                    basis: ScopeBasis::Stated,
                    evidence: ev,
                };
                match scope.iter_mut().find(|s| s.area == area) {
                    // "Remote (US)" after "New York, NY": the posting states
                    // what the office only suggested, and the statement wins.
                    Some(existing) if existing.basis == ScopeBasis::OfficeCity => {
                        *existing = stated;
                    }
                    Some(_) => {}
                    None => scope.push(stated),
                }
            }
            (true, None) => {
                unscoped = true;
                if !place.remote {
                    job.unrecognized.push(ev.clone());
                }
                remote_evidence.push(ev);
            }
            (false, area) => {
                if area.is_none() {
                    job.unrecognized.push(ev.clone());
                }
                if area == Some(Area::Worldwide) {
                    global = Some(ev);
                    continue;
                }
                job.options.push(WorkOption::Office {
                    presence,
                    area,
                    raw: place.raw.clone(),
                    evidence: ev,
                });
            }
        }
    }
    // A stated place wins over one only inferred from an office, and a
    // stated "anywhere" over offices.
    if scope.iter().any(|s| s.basis == ScopeBasis::Stated) {
        scope.retain(|s| s.basis == ScopeBasis::Stated);
    } else if global.is_some() {
        scope.clear();
    } else if !unscoped {
        // Only places, and nothing unscoped beside them: a finite list.
        for s in &mut scope {
            s.basis = ScopeBasis::Listed;
        }
    }
    // An explicit hybrid or on-site workplace type outranks a remote flag
    // (Ashby sets `isRemote` on some hybrid jobs); the disagreement is kept.
    let explicit_office = matches!(
        posting.workplace_type,
        Some(WorkplaceType::Hybrid | WorkplaceType::OnSite)
    ) && !any_remote;
    if explicit_office && posting.is_remote == Some(true) {
        job.conflicts.push(Conflict {
            summary: format!(
                "The workplace type says {}, but the remote flag is set; the workplace type is used",
                job.mode.label()
            ),
            evidence: job.mode_evidence.clone(),
        });
    }
    let remote_allowed = matches!(job.mode, JobMode::Remote | JobMode::Mixed)
        || !scope.is_empty()
        || global.is_some()
        || (posting.is_remote == Some(true) && !explicit_office);
    if remote_allowed {
        let scope = match (global, scope.is_empty()) {
            (Some(ev), true) => RemoteScope::Global(ev),
            (_, false) => RemoteScope::Areas(scope),
            (None, true) => RemoteScope::Unknown,
        };
        job.options.insert(
            0,
            WorkOption::Remote {
                scope,
                evidence: dedup(remote_evidence),
            },
        );
    }

    // A country listed next to cities of that country is where the
    // offices are, not an office of its own.
    let office_countries: Vec<&'static Country> = job
        .options
        .iter()
        .filter_map(|o| match o {
            WorkOption::Office {
                area: Some(a @ (Area::City { .. } | Area::Subdivision { .. })),
                ..
            } => a.country(),
            _ => None,
        })
        .collect();
    job.options.retain(|o| {
        !matches!(o, WorkOption::Office { area: Some(Area::Country(c)), .. }
            if office_countries.iter().any(|x| x.code == c.code))
    });
    if let Some(text) = &posting.work_authorization {
        read_authorization_field(record, text, &mut job);
    }
    if let Some(kind) = &posting.employment_type {
        let ev = evidence(record, "employment_type", kind.as_str());
        if *kind == EmploymentType::Contract {
            job.mechanisms.push(MechanismTerm {
                mechanism: Mechanism::Contractor,
                areas: Vec::new(),
                evidence: ev.clone(),
            });
        }
        job.employment = Some((kind.clone(), ev));
    }
    if let Some(text) = &posting.description_text {
        for sentence in sentences(text) {
            if presence_policy(&sentence) {
                job.presence_policy
                    .push(evidence(record, "description", &sentence));
            }
            for clause in label_clauses(&sentence) {
                read_sentence(record, &clause, &mut job);
            }
        }
    }
    // Mechanisms with named places are paths of their own.
    for m in job.mechanisms.clone() {
        if !m.areas.is_empty() {
            job.options.push(WorkOption::Engagement(m));
        }
    }
    find_conflicts(&mut job);
    job
}

fn dedup(mut items: Vec<Evidence>) -> Vec<Evidence> {
    let mut seen: Vec<Evidence> = Vec::new();
    items.retain(|e| {
        if seen.contains(e) {
            false
        } else {
            seen.push(e.clone());
            true
        }
    });
    items
}

/// Every place of the structured location fields, deduplicated, with the
/// field it came from, and whether its area is only the source's country
/// field standing in for a place the name doesn't settle (an office town
/// Narrow doesn't list, or one of several places of its name).
fn structured_places(record: &JobRecord) -> Vec<(Place, Evidence, bool)> {
    let posting = &record.posting;
    let mut places: Vec<(Place, Evidence, bool)> = Vec::new();
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
        // The field names a country: an ISO code is that country ("GA" is
        // Gabon here, not the state).
        let country = location
            .country
            .as_deref()
            .and_then(|c| {
                crate::geo::country(c.trim())
                    .map(Area::Country)
                    .or_else(|| lookup_code(&c.to_uppercase(), false))
                    .or_else(|| crate::geo::lookup_name(c))
            })
            .and_then(|a| a.country());
        for mut place in parse_places(&name) {
            // A bare code in a structured location uses that location's
            // country: "GA"/GA is Gabon, "SC"/BR Santa Catarina. Keep
            // Narrow's city nicknames such as "LA"/US as cities.
            let code = name.trim();
            if let Some(c) = country
                && !place.remote
                && (2..=3).contains(&code.len())
                && code.chars().all(|ch| ch.is_ascii_uppercase())
                && matches!(
                    place.area,
                    Some(Area::Country(_) | Area::Subdivision { .. })
                )
            {
                place.area = Some(
                    lookup_code_near(code, false, Some(c))
                        .filter(|a| a.country().is_some_and(|x| x.code == c.code))
                        .unwrap_or(Area::Country(c)),
                );
                place.ambiguous.clear();
            }
            // A bare name read as a place in another country: the source's
            // country is stated, a name's most populous reading only
            // likely ("Alexandria" with country "US" is not Egypt's). Read
            // the name inside that country; with no single reading there,
            // the country fills in below.
            if let Some(c) = country
                && !place.remote
                && let Some(area @ (Area::City { .. } | Area::Subdivision { .. })) = place.area
                && area.country().is_some_and(|x| x.code != c.code)
                && resolve_name(&place.raw) == Some(Resolution::Place(area))
            {
                match resolve_within(&place.raw, &Area::Country(c)) {
                    Some(Resolution::Place(inside)) => place.area = Some(inside),
                    other => {
                        place.area = None;
                        place.ambiguous = match other {
                            Some(Resolution::Ambiguous(options)) => options,
                            _ => Vec::new(),
                        };
                    }
                }
            }
            // The source's structured country chooses among the places an
            // ambiguous name could be ("Cambridge" with country "US"), or
            // fills in what the name leaves out.
            let mut filled = false;
            if place.area.is_none()
                && let Some(c) = country
                && !place.raw.is_empty()
                && !place.remote
            {
                let mut inside = place
                    .ambiguous
                    .iter()
                    .filter(|a| a.country().is_some_and(|x| x.code == c.code));
                place.area = match (inside.next(), inside.next()) {
                    (Some(one), None) => Some(*one),
                    _ => {
                        filled = true;
                        Some(Area::Country(c))
                    }
                };
                place.ambiguous.clear();
            }
            places.push((place, evidence(record, "locations", &name), filled));
        }
    }
    // The primary location text often lists more than the structured
    // locations do (Greenhouse offices).
    if let Some(location) = &posting.location {
        for place in parse_places(location) {
            // The same place, or the same words a structured location
            // already read (with its country: "Cambridge" or "Alexandria"
            // in the US).
            let known = places.iter().any(|(p, _, _)| {
                p.remote == place.remote
                    && (p.raw == place.raw || (p.area.is_some() && p.area == place.area))
            });
            if !known {
                places.push((place, evidence(record, "location", location), false));
            }
        }
    }
    let mut seen: Vec<(Option<Area>, bool, String)> = Vec::new();
    places.retain(|(p, _, _)| {
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
    places
}

/// Work at a Startup's first-party visa field ("US citizen/visa only",
/// "Will sponsor", "US citizenship/visa not required").
fn read_authorization_field(record: &JobRecord, text: &str, job: &mut JobRequirements) {
    let ev = evidence(record, "work_authorization", text);
    let ws = words(text);
    let has = |p: &str| Pattern::new(p).find(&ws).is_some();
    if has("not required") || has("no requirement*") {
        return;
    }
    if has("sponsor*") && !has("not") && !has("no") {
        job.sponsorship = Some((Sponsorship::Offered { caveat: false }, ev));
        return;
    }
    // Work at a Startup is a US platform: "US citizen/visa only".
    let mut areas: Vec<Area> = places_in_text(text)
        .into_iter()
        .filter(|a| a.country().is_some() || matches!(a, Area::Region(_)))
        .collect();
    if areas.is_empty() && has("citizen*") {
        areas.extend(crate::geo::country("US").map(Area::Country));
    }
    if !areas.is_empty() {
        job.authorization.push(AuthorizationRequirement {
            areas,
            strength: Strength::Required,
            evidence: ev.clone(),
        });
        if has("visa only") || has("citizen*") {
            job.sponsorship
                .get_or_insert((Sponsorship::Unavailable, ev));
        }
    }
}

/// Splits description text into sentences, keeping "U.S." together.
pub fn sentences(text: &str) -> Vec<String> {
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

const LIMIT_CUES: [&str; 51] = [
    "based in",
    "based out of",
    "based within",
    "located in",
    "located within",
    "reside within",
    "residing within",
    "live within",
    "living within",
    "available to applicants",
    "available to candidates",
    "eligible countries",
    "eligible locations",
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
    "_ only",
    "only in",
    // "open to US-based candidates", "considering US-based applicants".
    "_ based candidates",
    "_ based applicants",
    "_ based employees",
];

/// Pay, not hiring: "the anticipated annual pay range … for applicants
/// based within the United States is …" says what is paid where, never
/// where people may be.
const COMPENSATION_CUES: [&str; 16] = [
    "pay range*",
    "pay band*",
    "pay scale*",
    "pay transparency",
    "annual pay",
    "base pay",
    "salary",
    "salaries",
    "compensation",
    "base range",
    "annual range",
    "salary range*",
    "on target earnings",
    "=OTE",
    "total rewards",
    "benefits",
];

/// Terms for some of the people hired ("For US-based applicants: this
/// position is part of a bargaining unit"), which presuppose others.
const SUBSET_CUES: [&str; 12] = [
    "for _ based applicants",
    "for _ based candidates",
    "for _ based employees",
    "for applicants based",
    "for candidates based",
    "for employees based",
    "for applicants located",
    "for candidates located",
    "for employees located",
    "for applicants in",
    "for candidates in",
    "for employees in",
];

/// A statement about this role, not the company or hiring in general.
const ROLE_CUES: [&str; 12] = [
    "this role",
    "this position",
    "this job",
    "this opening",
    "this opportunity",
    "this _ role",
    "this _ position",
    "the role",
    "the position",
    "a remote role",
    "a remote position",
    "a fully remote",
];

/// Labels that introduce where the job is open: "Location: Americas -
/// North, Central and South America, EMEA, APAC", "Countries: Brazil,
/// Canada, Colombia, …".
const SCOPE_LABELS: [&str; 10] = [
    "location",
    "locations",
    "location s",
    "countries",
    "eligible countries",
    "hiring countries",
    "remote locations",
    "remote location",
    "region",
    "regions",
];

/// Whether a line is a scope label ("Location: …"), read as a statement
/// of where this role is open.
fn scope_label(sentence: &str) -> bool {
    let line = sentence.trim_start_matches(|c: char| !c.is_alphanumeric());
    line.split_once(':').is_some_and(|(label, rest)| {
        !rest.trim().is_empty() && SCOPE_LABELS.contains(&search_key(label).as_str())
    })
}

/// A scope label's clauses, each read on its own ("Location: San Francisco
/// (strongly preferred); remote (US) considered": the preference is the
/// city's, not the remote scope's); any other sentence whole.
fn label_clauses(sentence: &str) -> Vec<String> {
    match sentence.split_once(':') {
        Some((label, value)) if scope_label(sentence) && value.contains(';') => value
            .split(';')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| format!("{}: {c}", label.trim()))
            .collect(),
        _ => vec![sentence.to_owned()],
    }
}

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
const OFFICE_CUES: [&str; 11] = [
    "office*",
    "hub*",
    "based in",
    "based out of",
    "located in",
    "headquarter*",
    "hybrid",
    "on site",
    "onsite",
    "in person",
    "in office",
];

const AUTHORIZATION_CUES: [&str; 11] = [
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
    "legally work",
];

const EXCLUSION_CUES: [&str; 10] = [
    "not open to",
    "unable to hire",
    "cannot hire",
    "can t hire",
    "do not hire",
    "don t hire",
    "except",
    "excluding",
    "not able to hire",
    "cannot employ",
];

/// Explicit statements that people can be anywhere. Descriptions of the
/// team ("globally distributed") are not rules and are not here.
const WORLDWIDE_CUES: [&str; 9] = [
    "work from anywhere",
    "anywhere in the world",
    "remote anywhere",
    "from any country",
    "hire anywhere",
    "hire globally",
    "hire from anywhere",
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

const CONTRACTOR_CUES: [&str; 6] = [
    "contractor*",
    "contract basis",
    "as a contractor",
    "b2b",
    "independent contract*",
    "freelanc*",
];

const EOR_CUES: [&str; 6] = [
    "=Deel",
    "=Oyster",
    "=Remote.com",
    "employer of record",
    "=EOR",
    "=Rippling",
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

const FLEXIBLE_CUES: [&str; 7] = [
    "fully async*",
    "asynchronous first",
    "async first",
    "flexible hours",
    "flexible working hours",
    "work any hours",
    "set your own hours",
];

const RELOCATION_OFFERED: [&str; 6] = [
    "relocation assistance",
    "relocation support",
    "relocation package",
    "help you relocate",
    "relocation help",
    "visa and relocation",
];

const RELOCATION_REQUIRED: [&str; 5] = [
    "must relocate",
    "relocation required",
    "required to relocate",
    "willing to relocate to",
    "must be willing to relocate",
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

/// Where presence happens, for a presence policy.
const PRESENCE_PLACES: [&str; 7] = [
    "office",
    "offices",
    "on site",
    "onsite",
    "in person",
    "headquarters",
    "in office",
];

/// How often, or how firmly, presence is expected.
const PRESENCE_CADENCE: [&str; 19] = [
    "of the time",
    "percent",
    "days a week",
    "days per week",
    "day a week",
    "day per week",
    "times a week",
    "times per week",
    "per week",
    "each week",
    "every week",
    "per month",
    "a month",
    "quarterly",
    "regularly",
    "periodic*",
    "expect*",
    "required",
    "mandatory",
];

/// A hybrid arrangement named as the job's ("a hybrid role").
const HYBRID_ROLE: [&str; 6] = [
    "hybrid role",
    "hybrid position",
    "hybrid schedule",
    "hybrid model",
    "hybrid policy",
    "hybrid work",
];

/// Travel stated as a requirement of the job.
const TRAVEL_CUES: [&str; 8] = [
    "travel required",
    "required to travel",
    "travel requirement*",
    "travel is required",
    "will need to travel",
    "must travel",
    "must be able to travel",
    "travel up to",
];

/// Offices as a perk or an option, not an expectation.
const PRESENCE_NOT_REQUIRED: [&str; 23] = [
    "budget",
    "perk*",
    "allowance",
    "stipend",
    "equipment",
    "membership",
    "co working",
    "coworking",
    "wework",
    "reimburs*",
    "no offices",
    "no office",
    "not required",
    "optional",
    "if you prefer",
    "if you d like",
    "home office",
    // The software, not a place.
    "microsoft office",
    "ms office",
    "office 365",
    "office suite",
    "google workspace",
    "office hours",
];

/// Whether a sentence states an expectation of office presence or travel
/// ("we expect all staff to be in one of our offices at least 25% of the
/// time", "this is a hybrid role", "travel required"), named place or not.
fn presence_policy(sentence: &str) -> bool {
    let ws = words(sentence);
    let has = |cues: &'static [&'static str]| compiled(cues).iter().any(|p| p.find(&ws).is_some());
    if has(&PRESENCE_NOT_REQUIRED) {
        return false;
    }
    has(&TRAVEL_CUES)
        || has(&HYBRID_ROLE)
        || (has(&PRESENCE_PLACES) && has(&PRESENCE_CADENCE))
        || (has(&["hybrid"]) && has(&PRESENCE_CADENCE))
}

/// Substrings (lowercase) at least one of which every eligibility sentence
/// contains; sentences without any are skipped cheaply.
const KEYWORDS: [&str; 40] = [
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
    "within",
    "you can be",
    "can be held",
    "people",
    "office",
    "only",
    "countries",
    "region",
    "hybrid",
    "on-site",
    "onsite",
    "freelanc",
];

const ZONE_ABBREVIATIONS: [&str; 13] = [
    "PT", "PST", "ET", "EST", "CT", "CST", "MT", "CET", "UTC", "GMT", "BST", "EOR", "B2B",
];

fn worth_reading(sentence: &str) -> bool {
    let lower = sentence.to_lowercase();
    KEYWORDS.iter().any(|k| lower.contains(k))
        || sentence
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| ZONE_ABBREVIATIONS.contains(&w))
}

fn read_sentence(record: &JobRecord, sentence: &str, job: &mut JobRequirements) {
    if !worth_reading(sentence) {
        return;
    }
    let ws = words(sentence);
    // Long sentences are usually about the company, not the job's terms.
    if ws.len() > 60 {
        return;
    }
    let has_any =
        |cues: &'static [&'static str]| compiled(cues).iter().any(|p| p.find(&ws).is_some());
    let ev = || evidence(record, "description", sentence);
    let areas = places_in_text(sentence);
    let (cities, broad): (Vec<Area>, Vec<Area>) = areas
        .iter()
        .partition(|a| matches!(a, Area::City { .. } | Area::Subdivision { .. }));

    // Pay for people in a place, and terms for some of the people hired,
    // are not where the job can be done.
    let hiring_scope = !has_any(&COMPENSATION_CUES) && !has_any(&SUBSET_CUES);
    let label = scope_label(sentence);
    let subject = if label || has_any(&ROLE_CUES) {
        Subject::Role
    } else {
        Subject::General
    };
    let excluded = has_any(&EXCLUSION_CUES);
    let limited = has_any(&LIMIT_CUES) || label;
    let authorization = has_any(&AUTHORIZATION_CUES);
    let strength = if has_any(&SOFT_CUES) {
        Strength::Preferred
    } else {
        Strength::Required
    };
    let office_cue = has_any(&OFFICE_CUES);
    let remote_words = has_any(&["remote*"]);
    let contractor = has_any(&CONTRACTOR_CUES);
    // An employer of record's own postings name it everywhere ("Work from
    // anywhere: Oyster has no borders"); that is the company, not a way
    // this job is offered.
    let company = search_key(&record.posting.company);
    let eor = compiled(&EOR_CUES)
        .iter()
        .zip(EOR_CUES)
        .any(|(p, cue)| search_key(cue) != company && p.find(&ws).is_some());
    let mechanism_sentence = contractor || eor;

    if hiring_scope && !broad.is_empty() && (limited || authorization) && !mechanism_sentence {
        let target = if excluded {
            &mut job.deny
        } else {
            &mut job.allow
        };
        if authorization && !excluded && !limited {
            // "Must be authorized to work in the US": an authorization
            // requirement, not a statement of where people live.
        } else {
            for area in &broad {
                if !target.iter().any(|c| c.area == *area) {
                    target.push(AreaConstraint {
                        area: *area,
                        strength,
                        subject,
                        evidence: ev(),
                    });
                }
            }
        }
        if authorization && !excluded {
            job.authorization.push(AuthorizationRequirement {
                areas: broad.clone(),
                strength,
                evidence: ev(),
            });
        }
        // "can be held … remotely in the United States": a remote option.
        if remote_words && !excluded {
            add_remote_option(job, ev());
        }
    }
    // "Location: Fully remote.": the label says how the job is done, even
    // where the location fields list only places.
    if label && remote_words && hiring_scope && !has_any(&NEGATIONS) {
        add_remote_option(job, ev());
    }
    // "Applicants must live in NYC": a city limit on where people live.
    if hiring_scope
        && !label
        && !cities.is_empty()
        && limited
        && !excluded
        && !office_cue
        && !mechanism_sentence
        && strength == Strength::Required
    {
        for area in cities.iter().filter(|a| matches!(a, Area::City { .. })) {
            if !job.allow.iter().any(|c| c.area == *area) {
                job.allow.push(AreaConstraint {
                    area: *area,
                    strength,
                    subject,
                    evidence: ev(),
                });
            }
        }
    }
    // "hybrid in London", "onsite in New York", "based in our Toronto
    // office".
    if hiring_scope && office_cue && !excluded && !mechanism_sentence && job.mode != JobMode::Remote
    {
        let presence = if has_any(&["hybrid"]) {
            Presence::Hybrid
        } else if has_any(&["on site", "onsite", "in person", "in office"]) {
            Presence::Onsite
        } else {
            Presence::Office
        };
        for area in cities
            .iter()
            .copied()
            .filter(|a| matches!(a, Area::City { .. }))
        {
            let known = job
                .options
                .iter()
                .any(|o| matches!(o, WorkOption::Office { area: Some(a), .. } if *a == area));
            if !known {
                job.options.push(WorkOption::Office {
                    presence,
                    area: Some(area),
                    raw: area.to_string(),
                    evidence: ev(),
                });
                if job.mode == JobMode::Unknown {
                    job.mode = match presence {
                        Presence::Hybrid => JobMode::Hybrid,
                        _ => JobMode::Onsite,
                    };
                }
            }
        }
    }
    // "Work from anywhere in the US or Europe" limits; it isn't global. A
    // statement about this role outranks one about the company.
    let stated_worldwide = job.worldwide.as_ref().map(|(s, _)| *s);
    if hiring_scope
        && stated_worldwide.is_none_or(|s| s == Subject::General && subject == Subject::Role)
        && broad.is_empty()
        && has_any(&WORLDWIDE_CUES)
        && has_any(&HIRING_CONTEXT)
    {
        job.worldwide = Some((subject, ev()));
    }
    if has_any(&FLEXIBLE_CUES) && job.flexible_hours.is_none() {
        job.flexible_hours = Some(ev());
    }
    if has_any(&ZONE_CONTEXT) {
        let found = zones_in(sentence);
        if found.is_empty() {
            // Only a requirement without a zone is vague; "across multiple
            // time zones" or "from most timezones" is not a requirement.
            let requirement = has_any(&["overlap"])
                || (has_any(&["time zone*", "timezone*", "working hours"])
                    && has_any(&["must", "required", "need to", "expected to", "should"]));
            let permissive = has_any(&["across", "multiple", "any", "most", "flexible"]);
            if requirement && !permissive && job.vague_zone.is_none() {
                job.vague_zone = Some(ev());
            }
        } else {
            read_zones(sentence, &ws, found, strength, ev(), job);
        }
    }
    if has_any(&["sponsor*"]) && has_any(&VISA_CONTEXT) {
        let negative = has_any(&NEGATIONS);
        // "We do sponsor visas, but not for every role."
        let partial = negative
            && has_any(&[
                "do sponsor",
                "we sponsor",
                "sponsorship is available",
                "can sponsor",
            ])
            && has_any(&[
                "every role",
                "all roles",
                "every position",
                "all positions",
                "some roles",
            ]);
        match job.sponsorship.as_mut() {
            // A statement from a structured field outranks prose.
            Some((_, existing)) if existing.is_structured() => {}
            // "We do sponsor visas! However, we aren't able to … for every role."
            Some((Sponsorship::Offered { caveat }, _)) if negative => *caveat = true,
            Some((Sponsorship::Offered { .. }, _)) => {}
            _ if partial => {
                job.sponsorship = Some((Sponsorship::Offered { caveat: true }, ev()));
            }
            _ => {
                job.sponsorship = Some((
                    if negative {
                        Sponsorship::Unavailable
                    } else {
                        Sponsorship::Offered { caveat: false }
                    },
                    ev(),
                ))
            }
        }
    }
    if mechanism_sentence {
        let negative = has_any(&NEGATIONS);
        if negative && contractor && !eor {
            // "We don't work with contractors", "not open to contractors".
            job.employee_only.get_or_insert_with(ev);
        } else {
            let mechanism = if eor {
                Mechanism::Eor
            } else if has_any(&["b2b"]) {
                Mechanism::B2b
            } else {
                Mechanism::Contractor
            };
            let mut areas = broad.clone();
            if has_any(&WORLDWIDE_CUES) || has_any(&["worldwide", "globally", "any country"]) {
                areas = vec![Area::Worldwide];
            }
            job.mechanisms.push(MechanismTerm {
                mechanism,
                areas,
                evidence: ev(),
            });
        }
    }
    if job.relocation.is_none() {
        if has_any(&RELOCATION_REQUIRED) {
            job.relocation = Some((Relocation::Required, ev()));
        } else if has_any(&RELOCATION_OFFERED) {
            job.relocation = Some((Relocation::Offered, ev()));
        }
    }
}

/// A remote option the description states, when the fields gave none.
fn add_remote_option(job: &mut JobRequirements, evidence: Evidence) {
    if job.remote_option().is_some() {
        return;
    }
    job.options.insert(
        0,
        WorkOption::Remote {
            scope: RemoteScope::Unknown,
            evidence: vec![evidence],
        },
    );
    job.mode = match job.mode {
        JobMode::Unknown | JobMode::Remote => JobMode::Remote,
        _ => JobMode::Mixed,
    };
}

/// Time zones in a sentence, with how they constrain the work.
fn read_zones(
    sentence: &str,
    ws: &[jobhunt_profile::words::Word],
    found: Vec<(String, Zone)>,
    strength: Strength,
    ev: Evidence,
    job: &mut JobRequirements,
) {
    let has = |p: &str| Pattern::new(p).find(ws).is_some();
    let lower = sentence.to_lowercase();
    // "EST ±3 hours", "+/- 2 hours".
    let tolerance = ["±", "+/-", "+-", "plus or minus"]
        .iter()
        .find_map(|marker| {
            let at = lower.find(marker)? + marker.len();
            let digits: String = lower[at..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            digits.parse::<f32>().ok()
        });
    // "4 hours of overlap", "overlap … by at least 3 hours".
    let overlap = has("overlap").then(|| {
        ws.windows(2).find_map(|w| {
            let n: f32 = w[0].lower.parse().ok()?;
            (w[1].lower.starts_with("hour") && (1.0..=12.0).contains(&n)).then_some(n)
        })
    });
    let within = has("based in")
        || has("located in")
        || has("located within")
        || has("reside*")
        || has("live in")
        || has("be in");
    // "UTC-5 to UTC+1", "between UTC-3 and UTC+3": one range.
    let ranged = found.len() >= 2
        && (lower.contains(" to ") || lower.contains("between") || lower.contains(" and "))
        && found
            .iter()
            .all(|(l, _)| l.starts_with("UTC") || l.starts_with("GMT"));
    let entries: Vec<(String, Zone)> = if ranged {
        let (lo, hi) = found.iter().fold((i16::MAX, i16::MIN), |(lo, hi), (_, z)| {
            let (a, b) = z.year_range();
            (lo.min(a), hi.max(b))
        });
        let label = found
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join(" to ");
        Zone::new([Clock::Fixed(lo), Clock::Fixed(hi)])
            .map(|zone| vec![(label, zone)])
            .unwrap_or_default()
    } else {
        found
    };
    for (label, zone) in entries {
        let kind = match overlap {
            Some(Some(hours)) => ZoneKind::Overlap { hours },
            _ if within || ranged => ZoneKind::Within,
            _ => ZoneKind::Hours { tolerance },
        };
        if !job.zones.iter().any(|z| z.zone == zone && z.kind == kind) {
            job.zones.push(ZoneRequirement {
                label,
                zone,
                kind,
                strength,
                evidence: ev.clone(),
            });
        }
    }
}

/// Records where structured fields and the description disagree about
/// where the job can be done.
fn find_conflicts(job: &mut JobRequirements) {
    let limits: Vec<&AreaConstraint> = job
        .allow
        .iter()
        .filter(|c| c.strength == Strength::Required)
        .collect();
    let Some(scope) = job.remote_option() else {
        return;
    };
    let mut conflicts = Vec::new();
    match scope {
        RemoteScope::Global(ev) if !limits.is_empty() => conflicts.push(Conflict {
            summary: format!(
                "The location fields say remote anywhere, but the description limits it to {}",
                names(limits.iter().map(|c| c.area))
            ),
            evidence: std::iter::once(ev.clone())
                .chain(limits.iter().map(|c| c.evidence.clone()))
                .collect(),
        }),
        RemoteScope::Areas(areas) => {
            let stated: Vec<&ScopedArea> = areas
                .iter()
                .filter(|a| a.basis == ScopeBasis::Stated)
                .collect();
            let differs = !limits.is_empty()
                && !stated.is_empty()
                && limits
                    .iter()
                    .any(|c| !stated.iter().any(|s| s.area == c.area));
            if differs {
                conflicts.push(Conflict {
                    summary: format!(
                        "The location fields say remote in {}, but the description says {}",
                        names(stated.iter().map(|s| s.area)),
                        names(limits.iter().map(|c| c.area))
                    ),
                    evidence: stated
                        .iter()
                        .map(|s| s.evidence.clone())
                        .chain(limits.iter().map(|c| c.evidence.clone()))
                        .collect(),
                });
            }
        }
        RemoteScope::Unknown => {
            // "Remote", but the description ties the job to a city.
            let cities: Vec<&&AreaConstraint> = limits
                .iter()
                .filter(|c| matches!(c.area, Area::City { .. }))
                .collect();
            if !cities.is_empty() {
                let listed = job
                    .options
                    .iter()
                    .find_map(|o| match o {
                        WorkOption::Remote { evidence, .. } => Some(evidence.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                conflicts.push(Conflict {
                    summary: format!(
                        "Listed as remote, but the description requires living in {}",
                        names(cities.iter().map(|c| c.area))
                    ),
                    evidence: listed
                        .into_iter()
                        .chain(cities.iter().map(|c| c.evidence.clone()))
                        .collect(),
                });
            }
        }
        RemoteScope::Global(_) => {}
    }
    if let Some((_, ev)) = &job.worldwide
        && !limits.is_empty()
    {
        conflicts.push(Conflict {
            summary: format!(
                "The description says people can work from anywhere, but also limits the job to {}",
                names(limits.iter().map(|c| c.area))
            ),
            evidence: std::iter::once(ev.clone())
                .chain(limits.iter().map(|c| c.evidence.clone()))
                .collect(),
        });
    }
    job.conflicts.extend(conflicts);
}

fn names(areas: impl Iterator<Item = Area>) -> String {
    let mut out: Vec<String> = Vec::new();
    for a in areas {
        let n = a.to_string();
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out.join(", ")
}

/// The job's countries (offices and stated remote places), for reading an
/// ambiguous currency symbol as context, never as the currency.
pub fn countries(job: &JobRequirements) -> Vec<&'static Country> {
    let mut out: Vec<&'static Country> = Vec::new();
    let mut add = |c: &'static Country| {
        if !out.iter().any(|x| x.code == c.code) {
            out.push(c);
        }
    };
    for o in &job.options {
        match o {
            WorkOption::Office { area: Some(a), .. } => {
                if let Some(c) = a.country() {
                    add(c);
                }
            }
            WorkOption::Remote {
                scope: RemoteScope::Areas(areas),
                ..
            } => {
                for s in areas {
                    if let Some(c) = s.area.country() {
                        add(c);
                    }
                }
            }
            _ => {}
        }
    }
    out
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
