//! Checks a job's facts against the user's constraints.
//!
//! Each dimension (where the user can work from, work authorization, work
//! mode, time zone, pay, role) gets a [`Check`] with a [`Fit`], a one-line
//! summary and the evidence behind it. A remote flag alone is never read as
//! "you can work from anywhere": remote work with no place attached is
//! [`Fit::Unknown`]. The overall fit is the location's, lowered by any hard
//! check (a required work mode, authorization, the time zone, a pay
//! minimum) that fits worse.

use std::fmt;

use jobhunt_jobs::{EmploymentType, JobRecord, PayInterval};
use jobhunt_profile::{Arrangement, Stance, WorkMode};

use crate::facts::{
    Basis, CurrencyBasis, Evidence, JobFacts, JobMode, PlaceFact, SalaryFact, job_facts,
};
use crate::geo::{Area, Country, Membership, format_offset};
use crate::user::{MoneyPreference, UserConstraints};
use crate::zones::{Offsets, distance_hours, span};

/// How well a job fits, from worst to best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fit {
    /// The posting rules the user out.
    No,
    /// Probably not, on indirect or partial evidence.
    Unlikely,
    /// The posting does not say, or JobHunt cannot tell.
    Unknown,
    /// Probably, on indirect or partial evidence.
    Likely,
    /// The posting says so.
    Yes,
}

impl Fit {
    pub fn label(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::Unlikely => "unlikely",
            Self::Unknown => "unknown",
            Self::Likely => "likely",
            Self::Yes => "yes",
        }
    }
}

impl fmt::Display for Fit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a check is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Location,
    Authorization,
    WorkMode,
    Timezone,
    Compensation,
    Role,
}

impl Dimension {
    pub fn label(self) -> &'static str {
        match self {
            Self::Location => "location",
            Self::Authorization => "work authorization",
            Self::WorkMode => "work mode",
            Self::Timezone => "time zone",
            Self::Compensation => "compensation",
            Self::Role => "role",
        }
    }

    /// Whether a poor fit here lowers the overall fit.
    fn is_hard(self) -> bool {
        !matches!(self, Self::Role)
    }
}

/// One dimension's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub dimension: Dimension,
    pub fit: Fit,
    /// One line: "Remote, but United States only".
    pub summary: String,
    /// More detail, and what would settle an unknown.
    pub notes: Vec<String>,
    /// The posting's words behind the answer.
    pub evidence: Vec<Evidence>,
}

impl Check {
    fn new(dimension: Dimension, fit: Fit, summary: impl Into<String>) -> Self {
        Self {
            dimension,
            fit,
            summary: summary.into(),
            notes: Vec::new(),
            evidence: Vec::new(),
        }
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    fn with_evidence<'a>(mut self, evidence: impl IntoIterator<Item = &'a Evidence>) -> Self {
        for e in evidence {
            if !self.evidence.contains(e) {
                self.evidence.push(e.clone());
            }
        }
        self
    }

    /// Whether this check lowers the overall fit (and by how much).
    fn cap(&self, required_mode: bool) -> Fit {
        match (self.dimension, self.fit) {
            (Dimension::Location, fit) => fit,
            (d, _) if !d.is_hard() => Fit::Yes,
            // A work-mode preference that is not a requirement is reported,
            // not enforced.
            (Dimension::WorkMode, _) if !required_mode => Fit::Yes,
            // Most postings publish no pay; that stays an open question
            // rather than lowering every job.
            (Dimension::Compensation, Fit::Unknown) => Fit::Yes,
            (_, Fit::Unknown) => Fit::Likely,
            (_, fit) => fit,
        }
    }
}

/// A job checked against the user's constraints.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub fit: Fit,
    /// Location first, then the other dimensions that apply.
    pub checks: Vec<Check>,
    /// The check that set the overall fit, when it is not the location.
    decisive: Option<usize>,
}

impl Assessment {
    pub fn check(&self, dimension: Dimension) -> Option<&Check> {
        self.checks.iter().find(|c| c.dimension == dimension)
    }

    /// One line: the location answer, plus what lowered the overall fit.
    pub fn headline(&self) -> String {
        let location = &self.checks[0].summary;
        match self.decisive.map(|i| &self.checks[i]) {
            Some(c) if c.fit == Fit::No => c.summary.clone(),
            Some(c) => format!("{location}; {}", lower_first(&c.summary)),
            None => location.clone(),
        }
    }

    /// Checks whose answer is unknown.
    pub fn unknowns(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.fit == Fit::Unknown)
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        // Keep acronyms and names ("US", "Brazil") as written.
        Some(c)
            if chars.clone().next().is_some_and(char::is_lowercase) && !starts_with_name(text) =>
        {
            c.to_lowercase().chain(chars).collect()
        }
        _ => text.to_owned(),
    }
}

fn starts_with_name(text: &str) -> bool {
    let first = text.split_whitespace().next().unwrap_or_default();
    crate::geo::lookup_name(first).is_some()
}

/// Checks one stored job.
pub fn assess_record(record: &JobRecord, user: &UserConstraints) -> Assessment {
    assess(&job_facts(record), &record.posting.title, user)
}

/// Checks a job's facts; `title` is matched against the user's roles.
pub fn assess(facts: &JobFacts, title: &str, user: &UserConstraints) -> Assessment {
    let (location, path, scope) = location(facts, user);
    let mut checks = vec![location];
    checks.extend(authorization(facts, user, path));
    checks.extend(work_mode(facts, user));
    if path == Path::Remote && checks[0].fit > Fit::No {
        checks.extend(timezone(facts, user, &scope));
    }
    checks.extend(compensation(facts, user));
    checks.extend(role(title, user));

    let required_mode = user.work_modes.iter().any(|(_, s)| *s == Stance::Required);
    let mut fit = checks[0].fit;
    let mut decisive = None;
    for (i, check) in checks.iter().enumerate().skip(1) {
        let cap = check.cap(required_mode);
        if cap < fit {
            fit = cap;
            decisive = Some(i);
        }
    }
    Assessment {
        fit,
        checks,
        decisive,
    }
}

/// Which way of working the location answer is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    Remote,
    Office,
    Contractor,
    None,
}

/// The location answer, the way of working it is about, and the areas the
/// user qualifies through (empty when none or anywhere).
fn location(facts: &JobFacts, user: &UserConstraints) -> (Check, Path, Vec<Area>) {
    let unknown = |summary: String| Check::new(Dimension::Location, Fit::Unknown, summary);
    let Some(me) = &user.location else {
        return (
            unknown("Your location isn't set".into())
                .note("Set it with `jobhunt preferences set location <place>`."),
            Path::None,
            Vec::new(),
        );
    };
    let Some(home) = me.country() else {
        return (
            unknown(format!("JobHunt doesn't recognize your location “{}”", me.raw))
                .note("Use a country or a major city (`jobhunt preferences set location \"Lisbon, Portugal\"`)."),
            Path::None,
            Vec::new(),
        );
    };
    for x in &facts.exclusions {
        if x.area.is_some_and(|a| a.contains(home) == Membership::Yes) {
            let check = Check::new(
                Dimension::Location,
                Fit::No,
                format!("Not open to people in {}", country_name(home)),
            )
            .with_evidence([&x.evidence]);
            return (check, Path::None, Vec::new());
        }
    }

    let mut paths: Vec<(Check, Path, Vec<Area>)> = Vec::new();
    let remote = facts.mode.allows_remote()
        || !facts.remote_areas.is_empty()
        || !facts.remote_unplaced.is_empty()
        || facts.worldwide.is_some();
    if remote {
        let (check, scope) = remote_path(facts, home, me.area);
        paths.push((check, Path::Remote, scope));
    }
    // Someone who requires remote work is not weighing an office.
    let remote_only = user
        .work_modes
        .iter()
        .filter(|(_, s)| *s == Stance::Required)
        .all(|(m, _)| *m == WorkMode::Remote)
        && user.stance_on(WorkMode::Remote) == Some(Stance::Required);
    let weighs_offices = !(remote_only && remote);
    if weighs_offices && !facts.offices.is_empty() {
        paths
            .extend(office_path(facts, home, me.area, user).map(|c| (c, Path::Office, Vec::new())));
    }
    if let Some(check) = contractor_path(facts, home) {
        paths.push((check, Path::Contractor, Vec::new()));
    }
    let mut best: Option<(Check, Path, Vec<Area>)> = None;
    let mut others: Vec<String> = Vec::new();
    for candidate in paths {
        match &best {
            Some(b) if candidate.0.fit <= b.0.fit => others.push(candidate.0.summary),
            _ => {
                if let Some(b) = best.take() {
                    others.push(b.0.summary);
                }
                best = Some(candidate);
            }
        }
    }
    let (mut check, path, scope) = best.unwrap_or_else(|| {
        (
            unknown("The posting doesn't say where the job is".into()),
            Path::None,
            Vec::new(),
        )
    });
    for other in others {
        check.notes.push(format!("Otherwise: {other}."));
    }
    if me.basis == crate::user::LocationBasis::Resume {
        check
            .notes
            .push(format!("Your location (“{}”) comes from your resume; set it with `jobhunt preferences set location`.", me.raw));
    }
    (check, path, scope)
}

/// How well one place fits the user.
fn place_fit(place: &PlaceFact, home: &Country, mine: Option<Area>) -> Fit {
    let Some(area) = place.area else {
        return Fit::Unknown;
    };
    match (area.contains(home), area) {
        (Membership::Yes, _) if place.basis == Basis::OfficeCountry || !place.strict => Fit::Likely,
        (Membership::Yes, _) => Fit::Yes,
        (Membership::Maybe, Area::City { .. } | Area::Subdivision { .. }) => {
            if mine == Some(area) {
                Fit::Yes
            } else {
                Fit::Unknown
            }
        }
        (Membership::Maybe, _) => Fit::Likely,
        (Membership::No, _) => Fit::No,
    }
}

fn best_place<'a>(
    places: &[&'a PlaceFact],
    home: &Country,
    mine: Option<Area>,
) -> (Fit, Vec<&'a PlaceFact>) {
    let mut fit = Fit::No;
    let mut matched: Vec<&PlaceFact> = Vec::new();
    for p in places {
        let f = place_fit(p, home, mine);
        if f > fit {
            fit = f;
            matched.clear();
        }
        if f == fit && f > Fit::No {
            matched.push(p);
        }
    }
    (fit, matched)
}

/// "the United States", "North America or Europe", "New York or Miami".
fn area_list(places: &[&PlaceFact]) -> String {
    let mut names: Vec<String> = Vec::new();
    for p in places {
        let name = p.area.map_or_else(|| p.raw.clone(), area_name);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    const SHOWN: usize = 4;
    if names.len() > SHOWN + 1 {
        let more = names.len() - SHOWN;
        names.truncate(SHOWN);
        names.push(format!("{more} more places"));
    }
    join_or(&names)
}

/// A short name: cities and states without their country.
fn area_name(area: Area) -> String {
    match area {
        Area::Country(c) => country_name(c),
        Area::City { name, .. } | Area::Subdivision { name, .. } => name.to_owned(),
        other => other.to_string(),
    }
}

/// "the United States", "Brazil".
fn country_name(country: &Country) -> String {
    match country.code {
        "US" | "GB" | "NL" | "AE" | "PH" | "DO" => format!("the {}", country.name),
        _ => country.name.to_owned(),
    }
}

fn join_or(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// The remote answer, and the areas the user qualifies through.
fn remote_path(facts: &JobFacts, home: &'static Country, mine: Option<Area>) -> (Check, Vec<Area>) {
    let listed: Vec<&PlaceFact> = facts
        .remote_areas
        .iter()
        .filter(|p| p.area.is_some())
        .collect();
    let limits: Vec<&PlaceFact> = facts
        .restrictions
        .iter()
        .filter(|p| p.strict && p.area.is_some())
        .collect();
    let soft: Vec<&PlaceFact> = facts
        .restrictions
        .iter()
        .filter(|p| !p.strict && p.area.is_some())
        .collect();
    let check = |fit, summary: String| Check::new(Dimension::Location, fit, summary);

    let scope = if listed.is_empty() { &limits } else { &listed };
    if !scope.is_empty() {
        let (fit, matched) = best_place(scope, home, mine);
        let mut out = match fit {
            Fit::No => check(Fit::No, format!("Remote, but {} only", area_list(scope))).with_evidence(scope.iter().map(|p| &p.evidence)),
            Fit::Yes => check(Fit::Yes, format!("{} eligible: remote in {}", home.name, area_list(&matched))),
            Fit::Likely if matched.iter().all(|p| p.basis == Basis::OfficeCountry) => check(
                Fit::Likely,
                format!("Remote, probably within {} (the posting lists {})", area_list(&matched), matched[0].raw),
            )
            .note("The posting doesn't say where remote work is allowed; the office's country is the usual answer."),
            Fit::Likely => {
                let mut c = check(Fit::Likely, format!("{} likely eligible: remote in {}", home.name, area_list(&matched)));
                if matched.iter().any(|p| p.area.is_some_and(|a| a.contains(home) == Membership::Maybe)) {
                    c = c.note(format!("Postings disagree on whether {} includes {}.", area_list(&matched), country_name(home)));
                }
                c
            }
            _ => check(
                Fit::Unknown,
                format!("Remote near {}; unclear whether elsewhere in {} works", area_list(&matched), country_name(home)),
            ),
        };
        if fit > Fit::No {
            out = out.with_evidence(matched.iter().map(|p| &p.evidence));
        }
        let scope_areas: Vec<Area> = matched.iter().filter_map(|p| p.area).collect();
        // The location field and the description disagree.
        if !listed.is_empty() && !limits.is_empty() && fit > Fit::Unknown {
            let (limit_fit, _) = best_place(&limits, home, mine);
            if limit_fit == Fit::No {
                out.fit = Fit::Unknown;
                out.summary = format!(
                    "Remote in {}, but the description limits it to {}",
                    area_list(&listed),
                    area_list(&limits)
                );
                out = out.with_evidence(limits.iter().map(|p| &p.evidence));
            }
        }
        if fit == Fit::No && !listed.is_empty() {
            let (limit_fit, limit_matched) = best_place(&limits, home, mine);
            if limit_fit > Fit::No {
                out = out
                    .note(format!(
                        "The description also mentions {}; another posting may cover it.",
                        area_list(&limit_matched)
                    ))
                    .with_evidence(limit_matched.iter().map(|p| &p.evidence));
            }
        }
        return (out, scope_areas);
    }
    if let Some(ev) = &facts.worldwide {
        return (
            check(
                Fit::Likely,
                "Remote from anywhere, per the description".into(),
            )
            .with_evidence([ev]),
            Vec::new(),
        );
    }
    if !soft.is_empty() {
        let (fit, matched) = best_place(&soft, home, mine);
        let summary = format!("Remote; the posting prefers people in {}", area_list(&soft));
        let fit = if fit > Fit::No {
            Fit::Likely
        } else {
            Fit::Unknown
        };
        return (
            check(fit, summary).with_evidence(soft.iter().map(|p| &p.evidence)),
            matched.iter().filter_map(|p| p.area).collect(),
        );
    }
    (
        check(
            Fit::Unknown,
            "Remote, but the posting doesn't say where you can work from".into(),
        )
        .note("A remote flag is not a promise that every country works; ask the company.")
        .with_evidence(facts.remote_unplaced.iter().chain(&facts.mode_evidence)),
        Vec::new(),
    )
}

fn office_path(
    facts: &JobFacts,
    home: &'static Country,
    mine: Option<Area>,
    user: &UserConstraints,
) -> Option<Check> {
    let how = match facts.mode {
        JobMode::Hybrid => "Hybrid",
        JobMode::Mixed => "Office",
        _ => "On-site",
    };
    let offices: Vec<&PlaceFact> = facts.offices.iter().filter(|p| p.area.is_some()).collect();
    if offices.is_empty() {
        return None;
    }
    let names = area_list(&offices);
    let same_country: Vec<&PlaceFact> = offices
        .iter()
        .copied()
        .filter(|p| {
            p.area
                .and_then(|a| a.country())
                .is_some_and(|c| c.code == home.code)
        })
        .collect();
    let check = |fit, summary: String| Check::new(Dimension::Location, fit, summary);
    if !same_country.is_empty() {
        let here = same_country.iter().find(|p| {
            p.area == mine
                || matches!(
                    (p.area, mine),
                    (Some(Area::City { name: a, .. }), Some(Area::City { name: b, .. })) if a == b
                )
        });
        let out = match (here, mine) {
            (Some(p), _) => check(
                Fit::Yes,
                format!("{how} in {}, where you are", area_list(&[p])),
            ),
            // Only the country is known.
            (None, Some(Area::Country(_)) | None) => check(
                Fit::Likely,
                format!("{how} in {}, in your country", area_list(&same_country)),
            ),
            (None, _) if user.relocation == Some(false) => check(
                Fit::Unlikely,
                format!(
                    "{how} in {}; you're elsewhere in {} and not open to relocating",
                    area_list(&same_country),
                    country_name(home)
                ),
            ),
            (None, _) => check(
                Fit::Likely,
                format!(
                    "{how} in {}, in your country but not your city",
                    area_list(&same_country)
                ),
            ),
        };
        return Some(out.with_evidence(same_country.iter().map(|p| &p.evidence)));
    }

    // Abroad: moving, and probably a visa.
    let evidence: Vec<&Evidence> = offices.iter().map(|p| &p.evidence).collect();
    let mut fit = match user.relocation {
        Some(true) => Fit::Likely,
        None => Fit::Unknown,
        Some(false) => {
            return Some(
                check(
                    Fit::No,
                    format!("{how} in {names}; you're not open to relocating"),
                )
                .with_evidence(evidence),
            );
        }
    };
    let mut notes: Vec<String> = Vec::new();
    if user.relocation.is_none() {
        notes.push(
            "Say whether you'd relocate: `jobhunt preferences set relocation yes|no`.".into(),
        );
    }
    let mut sponsorship_evidence = None;
    if user.needs_sponsorship != Some(false) {
        match &facts.sponsorship {
            Some(s) if s.offered && !s.caveat => {
                notes.push("The posting says it sponsors visas.".into());
                sponsorship_evidence = Some(&s.evidence);
            }
            Some(s) if s.offered => {
                fit = fit.min(Fit::Unknown);
                notes.push("The posting sponsors visas, but not for every role.".into());
                sponsorship_evidence = Some(&s.evidence);
            }
            Some(s) => {
                fit = if user.needs_sponsorship == Some(true) {
                    Fit::No
                } else {
                    fit.min(Fit::Unlikely)
                };
                notes.push("The posting says it doesn't sponsor visas.".into());
                sponsorship_evidence = Some(&s.evidence);
            }
            None => {
                fit = fit.min(Fit::Unknown);
                notes.push("The posting doesn't say whether it sponsors visas.".into());
            }
        }
    }
    if let Some(ev) = &facts.relocation {
        notes.push("The posting mentions relocation help.".into());
        sponsorship_evidence.get_or_insert(ev);
    }
    let summary = match fit {
        Fit::Likely | Fit::Yes => format!("{how} in {names}; possible with relocation"),
        Fit::No | Fit::Unlikely => format!("{how} in {names}, not in {}", country_name(home)),
        _ => format!(
            "{how} in {names}; would mean relocating from {}",
            country_name(home)
        ),
    };
    let mut out = check(fit, summary)
        .with_evidence(evidence)
        .with_evidence(sponsorship_evidence);
    out.notes = notes;
    Some(out)
}

fn contractor_path(facts: &JobFacts, home: &'static Country) -> Option<Check> {
    let c = facts.contractors.as_ref()?;
    let supported = c
        .areas
        .iter()
        .any(|a| matches!(a.contains(home), Membership::Yes | Membership::Maybe));
    supported.then(|| {
        Check::new(
            Dimension::Location,
            Fit::Likely,
            format!("Contractors from {} appear supported", home.name),
        )
        .with_evidence([&c.evidence])
    })
}

fn authorization(facts: &JobFacts, user: &UserConstraints, path: Path) -> Option<Check> {
    let home = user.location.as_ref().and_then(|l| l.country());
    let strict: Vec<_> = facts.authorization.iter().filter(|a| a.strict).collect();
    let check = |fit, summary: String| Check::new(Dimension::Authorization, fit, summary);
    if strict.is_empty() {
        // "We don't sponsor visas" matters to someone who needs one, for an
        // office abroad (the location check covers that) or at home.
        let s = facts.sponsorship.as_ref()?;
        return match (user.needs_sponsorship, s.offered) {
            (Some(true), false) if path != Path::Office => Some(
                check(
                    Fit::Unlikely,
                    "The posting says it doesn't sponsor visas, and you need sponsorship".into(),
                )
                .with_evidence([&s.evidence]),
            ),
            (Some(true), true) => Some(
                check(
                    if s.caveat { Fit::Unknown } else { Fit::Yes },
                    if s.caveat {
                        "Sponsors visas, but not for every role".into()
                    } else {
                        "Sponsors visas".into()
                    },
                )
                .with_evidence([&s.evidence]),
            ),
            _ => None,
        };
    }
    let mut countries: Vec<&'static Country> = Vec::new();
    for a in &strict {
        for c in &a.countries {
            if !countries.iter().any(|x| x.code == c.code) {
                countries.push(c);
            }
        }
    }
    let names = join_or(
        &countries
            .iter()
            .map(|c| country_name(c))
            .collect::<Vec<_>>(),
    );
    let evidence = strict.iter().map(|a| &a.evidence);
    let at_home = home.is_some_and(|h| countries.iter().any(|c| c.code == h.code));
    let sponsors = facts.sponsorship.as_ref().map(|s| s.offered && !s.caveat);
    let out = match (at_home, user.needs_sponsorship, sponsors) {
        (_, Some(true), Some(true)) => check(
            Fit::Likely,
            format!("Requires authorization to work in {names}; the posting sponsors visas"),
        ),
        (_, Some(true), Some(false)) => check(
            Fit::No,
            format!(
                "Requires authorization to work in {names}, with no sponsorship; you need sponsorship"
            ),
        ),
        (_, Some(true), None) => check(
            Fit::Unknown,
            format!(
                "Requires authorization to work in {names}; sponsorship not stated and you need it"
            ),
        ),
        (true, Some(false), _) => check(
            Fit::Yes,
            format!("Requires authorization to work in {names}; you don't need sponsorship"),
        ),
        (true, None, _) => check(
            Fit::Likely,
            format!("Requires authorization to work in {names}, where you live"),
        )
        .note("Say whether you need sponsorship: `jobhunt preferences set sponsorship yes|no`."),
        (false, Some(false), _) => check(
            Fit::Likely,
            format!(
                "Requires authorization to work in {names}; your profile says you don't need sponsorship"
            ),
        ),
        (false, None, _) => check(
            Fit::Unlikely,
            format!("Requires authorization to work in {names}"),
        )
        .note("If you hold it, say so: `jobhunt preferences set sponsorship no`."),
    };
    Some(
        out.with_evidence(evidence)
            .with_evidence(facts.sponsorship.as_ref().map(|s| &s.evidence)),
    )
}

fn mode_name(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Remote => "remote",
        WorkMode::Hybrid => "hybrid",
        WorkMode::Onsite => "on-site",
    }
}

fn work_mode(facts: &JobFacts, user: &UserConstraints) -> Option<Check> {
    if user.work_modes.is_empty() {
        return None;
    }
    let offered: &[(WorkMode, Fit)] = match facts.mode {
        JobMode::Remote => &[(WorkMode::Remote, Fit::Yes)],
        JobMode::Hybrid => &[(WorkMode::Hybrid, Fit::Yes)],
        JobMode::Onsite => &[(WorkMode::Onsite, Fit::Yes)],
        JobMode::Mixed => &[
            (WorkMode::Remote, Fit::Yes),
            (WorkMode::Hybrid, Fit::Likely),
            (WorkMode::Onsite, Fit::Likely),
        ],
        JobMode::Unknown => &[],
    };
    let check = |fit, summary: String| {
        Check::new(Dimension::WorkMode, fit, summary).with_evidence(&facts.mode_evidence)
    };
    if offered.is_empty() {
        return Some(check(Fit::Unknown, "Work arrangement not published".into()));
    }
    let job = facts.mode.label();
    let with = |stances: &[Stance]| -> Vec<WorkMode> {
        user.work_modes
            .iter()
            .filter(|(_, s)| stances.contains(s))
            .map(|(m, _)| *m)
            .collect()
    };
    let best_of = |modes: &[WorkMode]| {
        offered
            .iter()
            .filter(|(m, _)| modes.contains(m))
            .map(|(_, f)| *f)
            .max()
    };
    let required = with(&[Stance::Required]);
    let names = |modes: &[WorkMode]| {
        join_or(
            &modes
                .iter()
                .map(|m| mode_name(*m).to_owned())
                .collect::<Vec<_>>(),
        )
    };
    if !required.is_empty() {
        return Some(match best_of(&required) {
            Some(fit) => check(fit, format!("Work mode: {job}, as you require")),
            None => check(
                Fit::No,
                format!("Work mode: {job}, but you require {}", names(&required)),
            ),
        });
    }
    let liked = with(&[Stance::Wanted, Stance::Acceptable]);
    if let Some(fit) = best_of(&liked) {
        return Some(check(fit, format!("Work mode: {job}, as you want")));
    }
    let unwanted = with(&[Stance::Unwanted]);
    if offered.iter().all(|(m, _)| unwanted.contains(m)) {
        return Some(check(
            Fit::Unlikely,
            format!("Work mode: {job}, which you'd rather avoid"),
        ));
    }
    if !liked.is_empty() {
        return Some(check(
            Fit::Unlikely,
            format!("Work mode: {job}; you want {}", names(&liked)),
        ));
    }
    None
}

fn timezone(facts: &JobFacts, user: &UserConstraints, scope: &[Area]) -> Option<Check> {
    let mine = user.zone_offsets();
    if !facts.zones.is_empty() {
        let job: Vec<Offsets> = facts.zones.iter().map(|z| z.offsets).collect();
        let range = span(&job)?;
        let labels = join_or(
            &facts
                .zones
                .iter()
                .map(|z| z.label.clone())
                .collect::<Vec<_>>(),
        );
        let evidence = facts.zones.iter().map(|z| &z.evidence);
        let Some((me, stated)) = mine else {
            return Some(
                Check::new(
                    Dimension::Timezone,
                    Fit::Unknown,
                    format!("Expects {labels} hours; your time zone isn't known"),
                )
                .note("Set it with `jobhunt preferences set timezone <zone>`.")
                .with_evidence(evidence),
            );
        };
        let hours = distance_hours(me, range);
        let (fit, summary) = if hours <= 2.0 {
            (
                Fit::Yes,
                format!("Expects {labels} hours, which fits your time zone"),
            )
        } else if hours <= 4.0 {
            (
                Fit::Likely,
                format!("Expects {labels} hours, {hours:.0}h from your time zone"),
            )
        } else {
            (
                Fit::Unlikely,
                format!("Expects {labels} hours, {hours:.0}h from your time zone"),
            )
        };
        let mut check = Check::new(Dimension::Timezone, fit, summary).with_evidence(evidence);
        if !stated {
            check = check.note(format!(
                "Your time zone ({}) is your country's; set yours with `jobhunt preferences set timezone`.",
                offsets_label(me)
            ));
        }
        return Some(check);
    }
    // No zone published: a remote scope within a few hours implies one.
    let width = span(
        &scope
            .iter()
            .filter_map(|a| a.utc_offsets())
            .collect::<Vec<_>>(),
    )
    .filter(|_| !scope.contains(&Area::Worldwide))
    .map(|(lo, hi)| hi - lo);
    match width {
        Some(w) if w <= 360 => None,
        _ => Some(
            Check::new(
                Dimension::Timezone,
                Fit::Unknown,
                "Eligibility unclear because the time zone isn't published",
            )
            .note(match width {
                Some(w) => format!("Remote work spans {} hours of time zones.", w / 60),
                None => "Remote work with no stated place or hours.".to_owned(),
            }),
        ),
    }
}

fn offsets_label((lo, hi): Offsets) -> String {
    if lo == hi {
        format_offset(lo)
    } else {
        format!("{} to {}", format_offset(lo), format_offset(hi))
    }
}

/// A salary per year, and whether the interval was assumed.
fn annual(salary: &SalaryFact, value: f64) -> Option<(f64, bool)> {
    let per_year = match salary.interval {
        Some(PayInterval::Year) => value,
        Some(PayInterval::Month) => value * 12.0,
        Some(PayInterval::Week) => value * 52.0,
        Some(PayInterval::Day) => value * 260.0,
        Some(PayInterval::Hour) => value * 2_080.0,
        Some(PayInterval::OneTime) => return None,
        // Unstated: a five-figure amount or more is a yearly salary.
        None if value >= 20_000.0 => return Some((value, true)),
        None => return None,
    };
    Some((per_year, false))
}

fn money(amount: f64, currency: &str) -> String {
    let rounded = amount.round() as u64;
    let digits = rounded.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{currency} {out}")
}

fn applies(pref: &MoneyPreference, employment: Option<&EmploymentType>) -> bool {
    !matches!(
        (pref.arrangement, employment),
        (
            Some(Arrangement::Contract),
            Some(EmploymentType::FullTime | EmploymentType::PartTime)
        ) | (
            Some(Arrangement::Employment),
            Some(EmploymentType::Contract)
        )
    )
}

fn compensation(facts: &JobFacts, user: &UserConstraints) -> Option<Check> {
    let employment = facts.employment.as_ref().map(|(e, _)| e);
    let prefs: Vec<&MoneyPreference> = user
        .minimums
        .iter()
        .filter(|p| applies(p, employment))
        .collect();
    let pref = *prefs.first()?;
    let check = |fit, summary: String| Check::new(Dimension::Compensation, fit, summary);
    if facts.salaries.is_empty() {
        return Some(match &facts.pay_text {
            Some(ev) => check(
                Fit::Unknown,
                format!("Compensation unknown: published only as “{}”", ev.text),
            )
            .with_evidence([ev]),
            None => check(Fit::Unknown, "Compensation unknown".into()),
        });
    }
    let Some(currency) = pref.currency.as_deref() else {
        return Some(
            check(Fit::Unknown, format!("Your minimum ({}) has no currency", pref.label))
                .note("Set one: `jobhunt preferences set compensation --minimum <amount> --currency <code>`.")
                .with_evidence(facts.salaries.iter().map(|s| &s.evidence)),
        );
    };
    let floor = pref.annual();
    let matching: Vec<&SalaryFact> = facts
        .salaries
        .iter()
        .filter(|s| s.currency.as_deref() == Some(currency))
        .collect();
    if matching.is_empty() {
        let published: Vec<String> = facts
            .salaries
            .iter()
            .filter_map(|s| s.currency.clone())
            .collect();
        let summary = if published.is_empty() {
            format!(
                "Pay is published without a currency ({})",
                facts.salaries[0].evidence.text
            )
        } else {
            format!(
                "Pay is in {}; your minimum is in {currency}",
                join_or(&dedup(published))
            )
        };
        return Some(
            check(Fit::Unknown, summary)
                .note("JobHunt doesn't convert currencies or guess which dollar “$” means.")
                .with_evidence(facts.salaries.iter().map(|s| &s.evidence)),
        );
    }

    let mut best: Option<Check> = None;
    for salary in matching {
        let low = salary.min.and_then(|v| annual(salary, v));
        let high = salary.max.and_then(|v| annual(salary, v));
        let assumed = low.is_some_and(|(_, a)| a) || high.is_some_and(|(_, a)| a);
        let (low, high) = (low.map(|(v, _)| v), high.map(|(v, _)| v));
        let range = match (low, high) {
            (Some(l), Some(h)) if (l - h).abs() > f64::EPSILON => format!(
                "{} – {}",
                money(l, currency),
                money(h, currency).trim_start_matches(currency).trim()
            ),
            (Some(v), _) | (None, Some(v)) => money(v, currency),
            (None, None) => continue,
        };
        let floor_text = money(floor, currency);
        let (mut fit, summary) = match (low, high) {
            (_, Some(h)) if h < floor => (
                Fit::No,
                format!(
                    "Pay tops out at {} a year, below your minimum of {floor_text}",
                    money(h, currency)
                ),
            ),
            (Some(l), _) if l >= floor => (
                Fit::Yes,
                format!("Salary satisfies your minimum ({range} a year vs {floor_text})"),
            ),
            (Some(l), None) if l < floor => (
                Fit::Unknown,
                format!(
                    "Pay starts at {range} a year, below your minimum of {floor_text}; no top of range"
                ),
            ),
            _ => (
                Fit::Likely,
                format!("Pay range {range} a year spans your minimum of {floor_text}"),
            ),
        };
        let mut notes = Vec::new();
        if let CurrencyBasis::FromLocation(country) = salary.currency_basis {
            notes.push(format!(
                "“$” read as {currency} because the job is in {}.",
                country.name
            ));
        }
        if assumed {
            notes.push("The posting doesn't say per what; read as a yearly salary.".into());
        }
        if !notes.is_empty() {
            // Inferred values never give a definite answer.
            fit = match fit {
                Fit::Yes => Fit::Likely,
                Fit::No => Fit::Unlikely,
                f => f,
            };
        }
        let mut c = check(fit, summary).with_evidence([&salary.evidence]);
        c.notes = notes;
        if best.as_ref().is_none_or(|b| c.fit > b.fit) {
            best = Some(c);
        }
    }
    best
}

fn dedup(mut items: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    items.retain(|i| {
        if seen.contains(i) {
            false
        } else {
            seen.push(i.clone());
            true
        }
    });
    items
}

fn matches_role(title: &str, role: &str) -> bool {
    let title = jobhunt_core::text::search_key(title);
    let words: Vec<&str> = title.split(' ').collect();
    let role = jobhunt_core::text::search_key(role);
    !role.is_empty()
        && role.split(' ').all(|w| {
            words
                .iter()
                .any(|t| *t == w || t.strip_suffix('s') == Some(w))
        })
}

fn role(title: &str, user: &UserConstraints) -> Option<Check> {
    if let Some(r) = user.unwanted_roles.iter().find(|r| matches_role(title, r)) {
        return Some(Check::new(
            Dimension::Role,
            Fit::Unlikely,
            format!("Title matches a role you don't want (“{r}”)"),
        ));
    }
    if user.wanted_roles.is_empty() {
        return None;
    }
    Some(
        match user.wanted_roles.iter().find(|r| matches_role(title, r)) {
            Some(r) => Check::new(Dimension::Role, Fit::Yes, format!("Title matches “{r}”")),
            None => Check::new(
                Dimension::Role,
                Fit::Unknown,
                format!(
                    "Title doesn't name your roles ({})",
                    user.wanted_roles.join(", ")
                ),
            ),
        },
    )
}
