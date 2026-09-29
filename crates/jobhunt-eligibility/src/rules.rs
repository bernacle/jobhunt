//! The eligibility rules. Each is a plain function from one way of doing
//! the job (a [`WorkOption`]), the posting's requirements and the person's
//! facts to the [`Reason`]s it concludes, so each can be tested on its
//! own. [`RULES`] is their order: hard restrictions first.
//!
//! 1. The listing gate (active, verified, authoritative) is applied on top
//!    of the decision in [`crate::evaluate`](mod@crate::evaluate).
//! 2. [`work_mode`] and [`presence`]: on-site and hybrid requirements.
//! 3. and 4. [`geography`]: countries, cities and regions the job allows or
//!    rules out, and remote scope.
//! 5. [`authorization`]: work authorization and sponsorship.
//! 6. [`engagement`]: contractor, B2B and employer-of-record paths.
//! 7. [`timezone`]: time-zone requirements, judged on every day of the
//!    reference year with each place's IANA rules (daylight saving time
//!    included), never from a fixed offset.
//! 8. [`ambiguity`]: what could not be read.
//!
//! A rule that has nothing to say returns no reasons. Nothing is inferred
//! from silence: no scope is "unknown", no time-zone language is "no
//! requirement published", and a missing profile fact is "unknown".

use jobhunt_profile::{Engagement, Stance, WorkMode};

use crate::decision::{ProfileFact, Reason, RuleId, Verdict};
use crate::geo::{Area, Country, Membership};
use crate::job::{
    AreaConstraint, Evidence, JobRequirements, Mechanism, Presence, Relocation, RemoteScope,
    ScopeBasis, Sponsorship, Strength, WorkOption, ZoneKind, ZoneRequirement,
};
use crate::profile::{FactBasis, ProfileFacts, ProfileLocation};
use crate::zones::{Offsets, Zone, day_pairs, days_text, distance_hours, offsets_text};

/// What a rule looks at.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub job: &'a JobRequirements,
    pub option: &'a WorkOption,
    pub profile: &'a ProfileFacts,
}

pub type Rule = fn(&Context<'_>) -> Vec<Reason>;

/// Every rule, in the order it is applied.
pub const RULES: [(&str, Rule); 7] = [
    ("work_mode", work_mode),
    ("presence", presence),
    ("geography", geography),
    ("authorization", authorization),
    ("engagement", engagement),
    ("timezone", timezone),
    ("ambiguity", ambiguity),
];

/// A working day, for overlap arithmetic.
const WORKING_DAY_HOURS: f32 = 8.0;

/// How far from a named zone's hours still counts as working them, when
/// the posting gives no tolerance.
const DEFAULT_ZONE_TOLERANCE_HOURS: f32 = 3.0;

// ---------------------------------------------------------------------------
// Names

/// "the United States", "Brazil", "the Americas", "New York".
pub fn area_name(area: Area) -> String {
    match area {
        Area::Country(c) => country_name(c),
        Area::City { name, .. } | Area::Subdivision { name, .. } => name.to_owned(),
        Area::Region(r) => r.name().to_owned(),
        Area::Worldwide => "anywhere".to_owned(),
    }
}

pub fn country_name(country: &Country) -> String {
    match country.code {
        "US" | "GB" | "NL" | "AE" | "PH" | "DO" => format!("the {}", country.name),
        _ => country.name.to_owned(),
    }
}

/// "Brazil", capitalized for the start of a sentence.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn join(names: &[String], word: &str) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {word} {last}", init.join(", ")),
    }
}

fn area_list(areas: impl IntoIterator<Item = Area>, word: &str) -> String {
    let mut names: Vec<String> = Vec::new();
    for a in areas {
        let n = area_name(a);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    join(&names, word)
}

fn mode_name(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Remote => "remote",
        WorkMode::Hybrid => "hybrid",
        WorkMode::Onsite => "on-site",
    }
}

// ---------------------------------------------------------------------------
// Membership

/// Whether `area` includes where the person lives; `None` when their
/// country is not known.
pub fn membership(area: Area, location: &ProfileLocation) -> Option<Membership> {
    if area == Area::Worldwide {
        return Some(Membership::Yes);
    }
    let home = location.country()?;
    Some(match area {
        Area::Worldwide => Membership::Yes,
        Area::Region(r) => r.contains(home),
        Area::Country(c) => {
            if c.code == home.code {
                Membership::Yes
            } else {
                Membership::No
            }
        }
        Area::Subdivision { country, .. } => match location.area {
            Some(mine) if mine == area || mine.within(&area) => Membership::Yes,
            _ if country.code == home.code => Membership::Maybe,
            _ => Membership::No,
        },
        Area::City { country, .. } => match location.area {
            Some(mine) if mine == area => Membership::Yes,
            _ if country.code == home.code => Membership::Maybe,
            _ => Membership::No,
        },
    })
}

/// The rule an area's constraint belongs to.
fn rule_for(area: Area) -> RuleId {
    match area {
        Area::Region(_) | Area::Worldwide => RuleId::RegionConstraint,
        _ => RuleId::CountryConstraint,
    }
}

/// The best membership among areas, and the areas that reach it.
fn best(
    areas: &[(Area, &Evidence)],
    location: &ProfileLocation,
) -> (Membership, Vec<Area>, Vec<Evidence>) {
    let rank = |m: Membership| match m {
        Membership::Yes => 2,
        Membership::Maybe => 1,
        Membership::No => 0,
    };
    let mut top = Membership::No;
    let mut matched: Vec<Area> = Vec::new();
    let mut evidence: Vec<Evidence> = Vec::new();
    for (area, ev) in areas {
        let m = membership(*area, location).unwrap_or(Membership::No);
        if rank(m) > rank(top) {
            top = m;
            matched.clear();
            evidence.clear();
        }
        if m == top && m != Membership::No {
            matched.push(*area);
            evidence.push((*ev).clone());
        }
    }
    (top, matched, evidence)
}

/// Why a membership is "maybe", in words.
fn maybe_reason(area: Area, home: &Country) -> String {
    match area {
        Area::City { .. } | Area::Subdivision { .. } => format!(
            "The listing names {}; it doesn't say whether elsewhere in {} works",
            area_name(area),
            country_name(home)
        ),
        _ => format!(
            "Usage disagrees on whether {} includes {}",
            area_name(area),
            country_name(home)
        ),
    }
}

// ---------------------------------------------------------------------------
// 2. Work mode and presence

/// A work mode the person requires, against this option's.
pub fn work_mode(ctx: &Context<'_>) -> Vec<Reason> {
    let required = ctx.profile.required_modes();
    if required.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = required.iter().map(|m| mode_name(*m).to_owned()).collect();
    let fact = ProfileFact::new(
        "work mode",
        format!("requires {}", join(&names, "or")),
        FactBasis::Preference,
    );
    let (mode, evidence): (Option<WorkMode>, Vec<&Evidence>) = match ctx.option {
        WorkOption::Remote { evidence, .. } => (Some(WorkMode::Remote), evidence.iter().collect()),
        WorkOption::Engagement(m) => (Some(WorkMode::Remote), vec![&m.evidence]),
        WorkOption::Office {
            presence, evidence, ..
        } => (
            match presence {
                Presence::Hybrid => Some(WorkMode::Hybrid),
                Presence::Onsite => Some(WorkMode::Onsite),
                Presence::Office => None,
            },
            vec![evidence],
        ),
    };
    // A remote listing whose own words also expect office presence or
    // travel ("Remote-Friendly" and "in one of our offices at least 25% of
    // the time"): for someone who requires remote work only, whether that
    // applies to this role is unknown. Never read as fine, never as a
    // conflict the posting didn't state.
    let policy = &ctx.job.presence_policy;
    let remote_only =
        !required.contains(&WorkMode::Hybrid) && !required.contains(&WorkMode::Onsite);
    let reason = match mode {
        Some(WorkMode::Remote) if remote_only && !policy.is_empty() => Reason::new(
            RuleId::WorkMode,
            Verdict::Unknown,
            format!(
                "The listing says remote, but it also expects office presence or travel (“{}”); Narrow can't tell whether that applies to this role",
                clip(&policy[0].text, 140)
            ),
        )
        .evidence(policy),
        Some(m) if required.contains(&m) => Reason::new(
            RuleId::WorkMode,
            Verdict::Pass,
            format!("The position is {}, as you require", mode_name(m)),
        ),
        Some(m) => Reason::new(
            RuleId::WorkMode,
            Verdict::Fail,
            format!(
                "This option is {}; you require {} work",
                mode_name(m),
                join(&names, "or")
            ),
        ),
        None if required == [WorkMode::Remote] => Reason::new(
            RuleId::WorkMode,
            Verdict::Fail,
            "This option is office-based; you require remote work",
        ),
        None => Reason::new(
            RuleId::WorkMode,
            Verdict::Unknown,
            "The posting doesn't say whether the office role is hybrid or on-site",
        ),
    };
    vec![reason.evidence(evidence).fact(fact)]
}

/// At most `max` characters of `text`, cut at a word, with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':']))
}

/// On-site and hybrid options: being at the office's place.
pub fn presence(ctx: &Context<'_>) -> Vec<Reason> {
    let WorkOption::Office {
        presence,
        area,
        raw,
        evidence,
    } = ctx.option
    else {
        return Vec::new();
    };
    let how = presence.label();
    let reason =
        |verdict, text: String| Reason::new(RuleId::Presence, verdict, text).evidence([evidence]);
    let Some(office) = *area else {
        return vec![reason(
            Verdict::Unknown,
            format!("The {how} location “{raw}” isn't one Narrow recognizes"),
        )];
    };
    let Some(me) = &ctx.profile.location else {
        return vec![
            reason(
                Verdict::Unknown,
                format!(
                    "Requires {how} presence in {}; your location isn't known",
                    area_name(office)
                ),
            )
            .fact(ProfileFact::missing("location")),
        ];
    };
    let Some(home) = me.country() else {
        return vec![reason(Verdict::Unknown, me.unresolved()).fact(ProfileFact::location(me))];
    };
    let place = area_name(office);
    let office_country = office.country();
    let relocation_offered = matches!(ctx.job.relocation, Some((Relocation::Offered, _)));
    let only_to = ctx.profile.relocation_places();
    let relocation_fact = match ctx.profile.relocation {
        Some(true) if !only_to.is_empty() => ProfileFact::new(
            "relocation",
            format!("willing to relocate only to {}", join(&only_to, "or")),
            FactBasis::Preference,
        ),
        Some(true) => ProfileFact::new("relocation", "willing to relocate", FactBasis::Preference),
        Some(false) => ProfileFact::new(
            "relocation",
            "not willing to relocate",
            FactBasis::Preference,
        ),
        None => ProfileFact::missing("relocation"),
    };
    let mut out = Vec::new();
    if office_country.is_some_and(|c| c.code == home.code) {
        let same_city = matches!(office, Area::City { .. }) && me.area == Some(office);
        let r = if same_city {
            reason(
                Verdict::Pass,
                match presence {
                    Presence::Office => format!("The office is in {place}, where you live"),
                    _ => format!("The {how} office is in {place}, where you live"),
                },
            )
        } else if !matches!(office, Area::City { .. }) {
            reason(
                Verdict::Unknown,
                format!(
                    "Requires {how} presence somewhere in {place}; the office city isn't published"
                ),
            )
        } else if me.city().is_none() {
            reason(
                Verdict::Unknown,
                format!(
                    "Requires {how} presence in {place}; your profile gives only your country ({})",
                    country_name(home)
                ),
            )
        } else if ctx.profile.would_relocate_to(office) == Some(Membership::Yes) {
            reason(
                Verdict::Conditional,
                format!(
                    "Requires {how} presence in {place}; you'd move within {}",
                    country_name(home)
                ),
            )
            .fact(relocation_fact)
        } else {
            reason(
                Verdict::Unknown,
                format!(
                    "Requires {how} presence in {place}; you live in {} (commuting distance is unknown)",
                    me.raw
                ),
            )
            .fact(relocation_fact)
        };
        out.push(r.fact(ProfileFact::location(me)));
        return out;
    }
    let abroad = match (office, office_country) {
        (Area::City { .. } | Area::Subdivision { .. }, Some(c)) => {
            format!("{place}, in {}", country_name(c))
        }
        _ => place.clone(),
    };
    let r = match ctx.profile.would_relocate_to(office) {
        Some(Membership::No) if ctx.profile.relocation == Some(false) => reason(
            Verdict::Fail,
            format!(
                "Requires {how} presence in {abroad}; you live in {} and are not willing to relocate",
                country_name(home)
            ),
        ),
        Some(Membership::No) => reason(
            Verdict::Fail,
            format!(
                "Requires {how} presence in {abroad}; you'd only relocate to {}",
                join(&only_to, "or")
            ),
        ),
        // A destination Narrow can't place: unknown, never a conflict.
        Some(Membership::Maybe) => reason(
            Verdict::Unknown,
            format!(
                "Requires {how} presence in {abroad}; Narrow can't tell whether it is among the places you'd relocate to ({})",
                join(&only_to, "or")
            ),
        ),
        Some(Membership::Yes) => {
            let mut r = reason(
                Verdict::Conditional,
                format!("Requires relocating to {abroad} for {how} work"),
            );
            if relocation_offered && let Some((_, ev)) = &ctx.job.relocation {
                r = r.evidence([ev]);
                r.conclusion
                    .push_str("; the posting offers relocation help");
            }
            r
        }
        None => reason(
            Verdict::Unknown,
            format!(
                "Requires {how} presence in {abroad}; your profile doesn't say whether you'd relocate from {}",
                country_name(home)
            ),
        ),
    };
    out.push(r.fact(ProfileFact::location(me)).fact(relocation_fact));
    out
}

// ---------------------------------------------------------------------------
// 3. and 4. Countries, regions and remote scope

/// Places the job allows or rules out, for remote and engagement options
/// (and exclusions for every option).
pub fn geography(ctx: &Context<'_>) -> Vec<Reason> {
    let job = ctx.job;
    let mut out = deny(job, ctx.profile);
    let (scope, apply_allow): (Option<&RemoteScope>, bool) = match ctx.option {
        WorkOption::Remote { scope, .. } => (Some(scope), true),
        WorkOption::Engagement(_) => (None, false),
        WorkOption::Office { .. } => return out,
    };
    let Some(me) = &ctx.profile.location else {
        let open_anywhere = matches!(scope, Some(RemoteScope::Global(_)))
            || (job.allow.is_empty() && job.worldwide.is_some())
            || matches!(ctx.option, WorkOption::Engagement(m) if m.areas.contains(&Area::Worldwide));
        out.push(if open_anywhere {
            Reason::new(
                RuleId::RegionConstraint,
                Verdict::Pass,
                "The listing is open to people anywhere",
            )
        } else {
            Reason::new(
                RuleId::RemoteScope,
                Verdict::Unknown,
                "Your location isn't known, so Narrow can't tell whether this includes you",
            )
            .fact(ProfileFact::missing("location"))
        });
        return out;
    };
    if me.country().is_none() {
        out.push(
            Reason::new(RuleId::RemoteScope, Verdict::Unknown, me.unresolved())
                .fact(ProfileFact::location(me)),
        );
        return out;
    }

    // A scope only inferred from an office city gives way to an explicit
    // statement of the description.
    let inferred_only = matches!(scope, Some(RemoteScope::Areas(areas))
        if areas.iter().all(|a| a.basis == ScopeBasis::OfficeCity));
    let described_scope =
        job.allow.iter().any(|c| c.strength == Strength::Required) || job.worldwide.is_some();
    let stated = match ctx.option {
        WorkOption::Engagement(m) => engagement_scope(m, me),
        _ if inferred_only && described_scope => None,
        _ => scope.and_then(|s| scope_reason(s, me)),
    };
    let described = if apply_allow {
        allow_reason(job, me)
    } else {
        None
    };
    match (stated, described) {
        (None, None) => {
            if let Some(r) = preferred_reason(job, me) {
                out.push(r);
            } else {
                let evidence = match ctx.option {
                    WorkOption::Remote { evidence, .. } => evidence.clone(),
                    _ => Vec::new(),
                };
                out.push(
                    Reason::new(
                        RuleId::RemoteScope,
                        Verdict::Unknown,
                        "The listing says “Remote” but publishes no geographic scope",
                    )
                    .evidence(&evidence),
                );
            }
        }
        (Some(s), None) => out.push(s),
        (None, Some(a)) => out.push(a),
        (Some(mut s), Some(a)) => match (s.verdict, a.verdict) {
            (Verdict::Pass, Verdict::Fail) => {
                // The description is narrower: its restriction applies.
                s.verdict = Verdict::NotApplicable;
                s.conclusion = format!("{} (but see the description)", s.conclusion);
                let mut conflict = Reason::new(
                    RuleId::Ambiguity,
                    Verdict::NotApplicable,
                    "The location fields and the description disagree; the description's narrower restriction applies",
                );
                conflict.evidence.extend(s.evidence.iter().cloned());
                conflict.evidence.extend(a.evidence.iter().cloned());
                out.extend([s, a, conflict]);
            }
            (Verdict::Fail, Verdict::Pass) => {
                // The description is broader: it may describe the company,
                // not this role. That can't be settled safely.
                let mut conflict = Reason::new(
                    RuleId::Ambiguity,
                    Verdict::Unknown,
                    format!(
                        "The location fields exclude {} but the description includes it; Narrow can't tell which applies to this role",
                        me.country().map(country_name).unwrap_or_default()
                    ),
                );
                conflict.evidence.extend(s.evidence.iter().cloned());
                conflict.evidence.extend(a.evidence.iter().cloned());
                conflict.profile.push(ProfileFact::location(me));
                s.verdict = Verdict::NotApplicable;
                let mut a = a;
                a.verdict = Verdict::NotApplicable;
                out.extend([s, a, conflict]);
            }
            _ => out.extend([s, a]),
        },
    }
    out
}

/// Exclusions: "we can't hire in X", "excluding Y".
fn deny(job: &JobRequirements, profile: &ProfileFacts) -> Vec<Reason> {
    let Some(me) = &profile.location else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in &job.deny {
        match membership(c.area, me) {
            Some(Membership::Yes) => out.push(
                Reason::new(
                    rule_for(c.area),
                    Verdict::Fail,
                    format!("The posting rules out people in {}", area_name(c.area)),
                )
                .evidence([&c.evidence])
                .fact(ProfileFact::location(me)),
            ),
            Some(Membership::Maybe) => out.push(
                Reason::new(
                    rule_for(c.area),
                    Verdict::Unknown,
                    format!(
                        "The posting rules out {}; whether that includes you is unclear",
                        area_name(c.area)
                    ),
                )
                .evidence([&c.evidence])
                .fact(ProfileFact::location(me)),
            ),
            _ => {}
        }
    }
    out
}

/// The structured remote scope, for this person.
fn scope_reason(scope: &RemoteScope, me: &ProfileLocation) -> Option<Reason> {
    let home = me.country()?;
    match scope {
        RemoteScope::Unknown => None,
        RemoteScope::Global(ev) => Some(
            Reason::new(
                RuleId::RegionConstraint,
                Verdict::Pass,
                "The listing is remote from anywhere",
            )
            .evidence([ev]),
        ),
        RemoteScope::Areas(areas) => {
            let stated: Vec<(Area, &Evidence)> = areas
                .iter()
                .filter(|a| a.basis == ScopeBasis::Stated)
                .map(|a| (a.area, &a.evidence))
                .collect();
            if stated.is_empty() {
                // Only an office city on a remote job.
                let inferred: Vec<Area> = areas.iter().map(|a| a.area).collect();
                let raw: Vec<String> = areas.iter().map(|a| a.raw.clone()).collect();
                let same = inferred
                    .iter()
                    .any(|a| membership(*a, me) == Some(Membership::Yes));
                let mut text = format!(
                    "The listing is remote but names only {}; it doesn't say where remote work is allowed",
                    join(&raw, "and")
                );
                if same {
                    text.push_str(&format!(
                        " (probably within {})",
                        area_list(inferred.iter().copied(), "or")
                    ));
                }
                return Some(
                    Reason::new(RuleId::RemoteScope, Verdict::Unknown, text)
                        .evidence(areas.iter().map(|a| &a.evidence))
                        .fact(ProfileFact::location(me)),
                );
            }
            let (m, matched, evidence) = best(&stated, me);
            let all: Vec<Area> = stated.iter().map(|(a, _)| *a).collect();
            let listed = area_list(all.iter().copied(), "or");
            Some(match m {
                Membership::Yes => {
                    let area = matched[0];
                    let text = match area {
                        Area::Country(_) => {
                            format!("The listing allows remote work from {}", area_name(area))
                        }
                        Area::City { .. } | Area::Subdivision { .. } => {
                            format!(
                                "The listing allows remote work from {}, where you live",
                                area_name(area)
                            )
                        }
                        _ => format!(
                            "{} is within the listed {} region",
                            capitalized(&country_name(home)),
                            area_name(area).trim_start_matches("the ")
                        ),
                    };
                    Reason::new(rule_for(area), Verdict::Pass, text)
                        .evidence(&evidence)
                        .fact(ProfileFact::location(me))
                }
                Membership::Maybe => Reason::new(
                    rule_for(matched[0]),
                    Verdict::Unknown,
                    maybe_reason(matched[0], home),
                )
                .evidence(&evidence)
                .fact(ProfileFact::location(me)),
                Membership::No => Reason::new(
                    all.first()
                        .copied()
                        .map_or(RuleId::RegionConstraint, rule_for),
                    Verdict::Fail,
                    format!(
                        "The listing limits remote work to {listed}; you live in {}",
                        country_name(home)
                    ),
                )
                .evidence(stated.iter().map(|(_, e)| *e))
                .fact(ProfileFact::location(me)),
            })
        }
    }
}

/// An engagement mechanism's places, for this person.
fn engagement_scope(m: &crate::job::MechanismTerm, me: &ProfileLocation) -> Option<Reason> {
    let home = me.country()?;
    let areas: Vec<(Area, &Evidence)> = m.areas.iter().map(|a| (*a, &m.evidence)).collect();
    let (best_m, matched, _) = best(&areas, me);
    let what = m.mechanism.label();
    Some(
        match best_m {
            Membership::Yes => Reason::new(
                rule_for(matched[0]),
                Verdict::Pass,
                if matched[0] == Area::Worldwide {
                    format!("The posting hires {what}s from anywhere")
                } else {
                    format!(
                        "The posting hires {what}s in {}, including {}",
                        area_list(m.areas.iter().copied(), "and"),
                        country_name(home)
                    )
                },
            ),
            Membership::Maybe => Reason::new(
                rule_for(matched[0]),
                Verdict::Unknown,
                format!("{} ({what} path)", maybe_reason(matched[0], home)),
            ),
            Membership::No => Reason::new(
                RuleId::CountryConstraint,
                Verdict::Fail,
                format!(
                    "The posting hires {what}s only in {}",
                    area_list(m.areas.iter().copied(), "and")
                ),
            ),
        }
        .evidence([&m.evidence])
        .fact(ProfileFact::location(me)),
    )
}

/// The description's required limits (and "anywhere"), for this person.
fn allow_reason(job: &JobRequirements, me: &ProfileLocation) -> Option<Reason> {
    let home = me.country()?;
    let limits: Vec<&AreaConstraint> = job
        .allow
        .iter()
        .filter(|c| c.strength == Strength::Required)
        .collect();
    if limits.is_empty() {
        return job.worldwide.as_ref().map(|ev| {
            Reason::new(
                RuleId::RegionConstraint,
                Verdict::Pass,
                "The description says people can work from anywhere",
            )
            .evidence([ev])
        });
    }
    let areas: Vec<(Area, &Evidence)> = limits.iter().map(|c| (c.area, &c.evidence)).collect();
    let (m, matched, evidence) = best(&areas, me);
    let all = area_list(limits.iter().map(|c| c.area), "or");
    Some(match m {
        Membership::Yes => Reason::new(
            rule_for(matched[0]),
            Verdict::Pass,
            match matched[0] {
                Area::Region(_) => format!(
                    "{} is within {}, which the description allows",
                    capitalized(&country_name(home)),
                    area_name(matched[0])
                ),
                other => format!("The description allows people in {}", area_name(other)),
            },
        )
        .evidence(&evidence)
        .fact(ProfileFact::location(me)),
        Membership::Maybe => Reason::new(
            rule_for(matched[0]),
            Verdict::Unknown,
            maybe_reason(matched[0], home),
        )
        .evidence(&evidence)
        .fact(ProfileFact::location(me)),
        Membership::No => Reason::new(
            rule_for(limits[0].area),
            Verdict::Fail,
            format!(
                "The description limits the job to people in {all}; you live in {}",
                country_name(home)
            ),
        )
        .evidence(limits.iter().map(|c| &c.evidence))
        .fact(ProfileFact::location(me)),
    })
}

/// Preferred places, when nothing else bounds a remote option.
fn preferred_reason(job: &JobRequirements, me: &ProfileLocation) -> Option<Reason> {
    let preferred: Vec<&AreaConstraint> = job
        .allow
        .iter()
        .filter(|c| c.strength == Strength::Preferred)
        .collect();
    if preferred.is_empty() {
        return None;
    }
    let areas: Vec<(Area, &Evidence)> = preferred.iter().map(|c| (c.area, &c.evidence)).collect();
    let (m, matched, evidence) = best(&areas, me);
    let all = area_list(preferred.iter().map(|c| c.area), "or");
    Some(if m == Membership::Yes {
        Reason::new(
            rule_for(matched[0]),
            Verdict::Pass,
            format!(
                "The description prefers people in {}, where you are",
                area_name(matched[0])
            ),
        )
        .evidence(&evidence)
        .fact(ProfileFact::location(me))
    } else {
        Reason::new(
            RuleId::RemoteScope,
            Verdict::Unknown,
            format!("The description prefers people in {all}; it doesn't say others are excluded"),
        )
        .evidence(preferred.iter().map(|c| &c.evidence))
        .fact(ProfileFact::location(me))
    })
}

// ---------------------------------------------------------------------------
// 5. Work authorization and sponsorship

/// Explicit authorization requirements, and the authorization an office
/// abroad implies, against what the person stated.
pub fn authorization(ctx: &Context<'_>) -> Vec<Reason> {
    let job = ctx.job;
    let profile = ctx.profile;
    let home = profile.country();
    let mut requirements: Vec<(Vec<Area>, Vec<&Evidence>, bool)> = job
        .authorization
        .iter()
        .filter(|a| a.strength == Strength::Required)
        .map(|a| {
            (
                a.areas.clone(),
                vec![&a.evidence],
                a.evidence.is_structured(),
            )
        })
        .collect();
    // Working at an office abroad needs the right to work there.
    if let WorkOption::Office {
        area: Some(office),
        evidence,
        ..
    } = ctx.option
        && let Some(country) = office.country()
        && home.is_some_and(|h| h.code != country.code)
        && !requirements
            .iter()
            .any(|(areas, _, _)| areas.contains(&Area::Country(country)))
    {
        requirements.push((vec![Area::Country(country)], vec![evidence], false));
    }
    let sponsorship = job.sponsorship.as_ref();
    if requirements.is_empty() {
        // "No sponsorship" says nothing about remote work from abroad.
        return match (sponsorship, ctx.option, home) {
            (
                Some((Sponsorship::Unavailable, ev)),
                WorkOption::Remote { .. } | WorkOption::Engagement(_),
                Some(home),
            ) => {
                vec![Reason::new(
                    RuleId::Authorization,
                    Verdict::NotApplicable,
                    format!(
                        "The posting doesn't sponsor visas; that doesn't restrict remote work from {} by itself, only roles that require authorization in another country",
                        country_name(home)
                    ),
                )
                .evidence([ev])]
            }
            _ => Vec::new(),
        };
    }
    let mut out = Vec::new();
    for (areas, evidence, _) in requirements {
        let names = area_list(areas.iter().copied(), "or");
        let countries: Vec<&'static Country> = areas.iter().filter_map(|a| a.country()).collect();
        let stated = areas.iter().find_map(|a| match a {
            Area::Country(c) => profile.authorized_for(c).map(|raw| (raw, *a)),
            Area::Region(_) => profile
                .authorized_in
                .iter()
                .find(|(mine, _)| *mine == Some(*a))
                .map(|(_, raw)| (raw.clone(), *a)),
            _ => None,
        });
        let lives_there = home.is_some_and(|h| {
            areas.iter().any(|a| {
                matches!(a, Area::Country(c) if c.code == h.code)
                    || matches!(a, Area::Region(r) if r.contains(h) == Membership::Yes)
            })
        });
        let base = |verdict, text: String| {
            Reason::new(RuleId::Authorization, verdict, text).evidence(evidence.iter().copied())
        };
        let sponsorship_evidence: Vec<&Evidence> =
            sponsorship.map(|(_, e)| e).into_iter().collect();
        let reason = if let Some((raw, _)) = stated {
            base(
                Verdict::Pass,
                format!("Requires authorization to work in {names}; your profile says you're authorized to work in {raw}"),
            )
            .fact(ProfileFact::new("work authorization", raw, FactBasis::Preference))
        } else if lives_there && profile.needs_sponsorship == Some(false) {
            base(
                Verdict::Pass,
                format!("Requires authorization to work in {names}; you live there and your profile says you don't need sponsorship"),
            )
            .fact(ProfileFact::new("sponsorship", "not needed", FactBasis::Preference))
        } else if profile.needs_sponsorship == Some(true) {
            let fact = ProfileFact::new("sponsorship", "needed", FactBasis::Preference);
            match sponsorship {
                Some((Sponsorship::Offered { caveat: false }, _)) => base(
                    Verdict::Conditional,
                    format!("Requires authorization to work in {names}; the posting offers visa sponsorship"),
                ),
                Some((Sponsorship::Offered { caveat: true }, _)) => base(
                    Verdict::Unknown,
                    format!("Requires authorization to work in {names}; the posting sponsors visas, but not for every role"),
                ),
                Some((Sponsorship::Unavailable, _)) => base(
                    Verdict::Fail,
                    format!("Requires authorization to work in {names} and doesn't sponsor visas; you need sponsorship"),
                ),
                None => base(
                    Verdict::Unknown,
                    format!("Requires authorization to work in {names}; the posting doesn't say whether it sponsors visas"),
                ),
            }
            .evidence(sponsorship_evidence)
            .fact(fact)
        } else {
            let mut r = base(
                Verdict::Unknown,
                format!(
                    "Requires authorization to work in {names}; your profile doesn't say whether you hold it"
                ),
            )
            .evidence(sponsorship_evidence)
            .fact(ProfileFact::missing("work authorization"));
            if countries.len() == 1 {
                r.conclusion.push_str(&format!(
                    " (`narrow preferences set authorized-in \"{}\"`)",
                    countries[0].name
                ));
            }
            r
        };
        out.push(reason);
    }
    out
}

// ---------------------------------------------------------------------------
// 6. Engagement

/// Contractor, B2B and employer-of-record paths, against the person's
/// stated engagement preferences.
pub fn engagement(ctx: &Context<'_>) -> Vec<Reason> {
    let job = ctx.job;
    let profile = ctx.profile;
    let contractor_stance = profile.engagement(Engagement::Contractor);
    let employee_stance = profile.engagement(Engagement::Employee);
    match ctx.option {
        WorkOption::Engagement(m) => {
            let (kind, stance) = match m.mechanism {
                Mechanism::Contractor | Mechanism::B2b => {
                    (Engagement::Contractor, contractor_stance)
                }
                Mechanism::Eor => (Engagement::Employee, employee_stance),
            };
            let what = m.mechanism.label();
            let r = match stance {
                Some(Stance::Unwanted) => Reason::new(
                    RuleId::Engagement,
                    Verdict::Fail,
                    format!("This path is as a {what}; your profile rules that out"),
                )
                .fact(ProfileFact::new(
                    "engagement",
                    format!("not as {}", kind.as_str()),
                    FactBasis::Preference,
                )),
                Some(_) => Reason::new(
                    RuleId::Engagement,
                    Verdict::Pass,
                    format!(
                        "Engagement as a {what} is explicitly offered, and your profile accepts it"
                    ),
                )
                .fact(ProfileFact::new(
                    "engagement",
                    kind.as_str(),
                    FactBasis::Preference,
                )),
                None => Reason::new(
                    RuleId::Engagement,
                    Verdict::Pass,
                    format!("Engagement as a {what} is explicitly offered"),
                ),
            };
            vec![r.evidence([&m.evidence])]
        }
        WorkOption::Remote { scope, .. } => {
            let mut out = Vec::new();
            let contract_job = matches!(
                job.employment,
                Some((jobhunt_jobs::EmploymentType::Contract, _))
            );
            let global_contractors = job.mechanisms.iter().find(|m| {
                m.areas.contains(&Area::Worldwide)
                    || (m.areas.is_empty() && matches!(scope, RemoteScope::Global(_)))
            });
            if contractor_stance == Some(Stance::Required) {
                out.push(if contract_job || !job.mechanisms.is_empty() {
                    Reason::new(RuleId::Engagement, Verdict::Pass, "The posting offers contractor engagement, which you require")
                        .evidence(job.mechanisms.iter().map(|m| &m.evidence))
                } else if let Some(ev) = &job.employee_only {
                    Reason::new(RuleId::Engagement, Verdict::Fail, "The posting hires employees only; you require contractor engagement")
                        .evidence([ev])
                } else {
                    Reason::new(RuleId::Engagement, Verdict::Unknown, "You require contractor engagement; the posting doesn't say whether contractors are hired")
                }
                .fact(ProfileFact::new("engagement", "contractor only", FactBasis::Preference)));
            } else if employee_stance == Some(Stance::Required) && contract_job {
                if let Some((_, ev)) = &job.employment {
                    out.push(
                        Reason::new(
                            RuleId::Engagement,
                            Verdict::Fail,
                            "The position is a contract; you require employment",
                        )
                        .evidence([ev])
                        .fact(ProfileFact::new(
                            "engagement",
                            "employee only",
                            FactBasis::Preference,
                        )),
                    );
                }
            } else if contractor_stance == Some(Stance::Unwanted)
                && contract_job
                && let Some((_, ev)) = &job.employment
            {
                out.push(
                    Reason::new(
                        RuleId::Engagement,
                        Verdict::Fail,
                        "The position is a contract; your profile rules out contractor work",
                    )
                    .evidence([ev])
                    .fact(ProfileFact::new(
                        "engagement",
                        "not as contractor",
                        FactBasis::Preference,
                    )),
                );
            }
            if let Some(m) = global_contractors {
                out.push(
                    Reason::new(
                        RuleId::Engagement,
                        Verdict::NotApplicable,
                        format!(
                            "International {} engagement is explicitly supported",
                            m.mechanism.label()
                        ),
                    )
                    .evidence([&m.evidence]),
                );
            } else if matches!(scope, RemoteScope::Unknown)
                && job.allow.is_empty()
                && job.worldwide.is_none()
                && job.mechanisms.is_empty()
                && profile.country().is_some()
            {
                out.push(Reason::new(
                    RuleId::Engagement,
                    Verdict::Unknown,
                    format!(
                        "Contractor or employer-of-record hiring from {} isn't stated",
                        profile.country().map(country_name).unwrap_or_default()
                    ),
                ));
            }
            out
        }
        WorkOption::Office { .. } => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// 7. Time zones

/// Time-zone requirements of remote options, against the person's zones.
pub fn timezone(ctx: &Context<'_>) -> Vec<Reason> {
    if matches!(ctx.option, WorkOption::Office { .. }) {
        return Vec::new();
    }
    let job = ctx.job;
    if job.zones.is_empty() {
        return vec![match (&job.flexible_hours, &job.vague_zone) {
            (Some(ev), _) => Reason::new(
                RuleId::Timezone,
                Verdict::NotApplicable,
                "The posting says working hours are flexible",
            )
            .evidence([ev]),
            (None, Some(ev)) => Reason::new(
                RuleId::Timezone,
                Verdict::Unknown,
                "The posting mentions time-zone overlap without saying which zones",
            )
            .evidence([ev]),
            (None, None) => Reason::new(
                RuleId::Timezone,
                Verdict::NotApplicable,
                "No time-zone requirement is published",
            ),
        }];
    }
    let Some((mine, basis)) = ctx.profile.zone() else {
        let labels: Vec<String> = job.zones.iter().map(|z| z.label.clone()).collect();
        return vec![
            Reason::new(
                RuleId::Timezone,
                Verdict::Unknown,
                format!(
                    "Requires {} hours; your time zone isn't known",
                    join(&labels, "or")
                ),
            )
            .evidence(job.zones.iter().map(|z| &z.evidence))
            .fact(ProfileFact::missing("time zone")),
        ];
    };
    let fact = ProfileFact::new("time zone", mine.label(), basis);
    // Zones named in one sentence are alternatives ("EST or PST"); every
    // sentence is a requirement of its own.
    let mut groups: Vec<Vec<&ZoneRequirement>> = Vec::new();
    for z in &job.zones {
        match groups.iter_mut().find(|g| g[0].evidence == z.evidence) {
            Some(g) => g.push(z),
            None => groups.push(vec![z]),
        }
    }
    groups
        .into_iter()
        .map(|group| {
            group
                .iter()
                .map(|z| zone_reason(z, &mine))
                .max_by_key(|r| verdict_rank(r.verdict))
                .unwrap_or_else(|| Reason::new(RuleId::Timezone, Verdict::Unknown, "unreadable"))
                .evidence(group.iter().map(|z| &z.evidence))
                .fact(fact.clone())
        })
        .collect()
}

fn verdict_rank(v: Verdict) -> u8 {
    match v {
        Verdict::Pass | Verdict::NotApplicable => 3,
        Verdict::Conditional => 2,
        Verdict::Unknown => 1,
        Verdict::Fail => 0,
    }
}

/// How one day's offsets meet a requirement.
struct Judgement {
    verdict: Verdict,
    text: String,
    /// What changes with the seasons: "4h away", "5h of overlap".
    measure: String,
    /// How far apart the zones are that day, in hours (worst case).
    far: f32,
}

/// A requirement against the person's zone on every day of the reference
/// year. When the answer is the same all year it is that answer; when
/// daylight saving time changes it (one side moves its clocks, the other
/// doesn't, or not on the same dates), it holds only for part of the year
/// and is uncertain, with the dates; when it holds on no day, it is the
/// worst day's answer.
fn zone_reason(z: &ZoneRequirement, mine: &Zone) -> Reason {
    let spread = mine.is_spread();
    let judged: Vec<(Judgement, Vec<usize>)> = day_pairs(mine, &z.zone)
        .into_iter()
        .map(|((m, t), days)| (judge(z, m, t, spread), days))
        .collect();
    let same = judged.windows(2).all(|w| w[0].0.verdict == w[1].0.verdict);
    if same {
        // A pass states what holds on its worst day; anything else what
        // holds most of the year.
        let chosen = if judged
            .first()
            .is_some_and(|(j, _)| j.verdict == Verdict::Pass)
        {
            judged.iter().max_by(|a, b| a.0.far.total_cmp(&b.0.far))
        } else {
            judged.iter().max_by_key(|(_, days)| days.len())
        };
        return match chosen {
            Some((j, _)) => Reason::new(RuleId::Timezone, j.verdict, j.text.clone()),
            None => Reason::new(RuleId::Timezone, Verdict::Unknown, "unreadable"),
        };
    }
    // The seasons change the answer: say how, and when.
    let mut measures: Vec<(String, Vec<usize>)> = Vec::new();
    for (j, days) in &judged {
        match measures.iter_mut().find(|(m, _)| *m == j.measure) {
            Some((_, all)) => all.extend(days),
            None => measures.push((j.measure.clone(), days.clone())),
        }
    }
    for (_, days) in &mut measures {
        days.sort_unstable();
    }
    measures.sort_by_key(|(_, days)| std::cmp::Reverse(days.len()));
    let when: Vec<String> = measures
        .iter()
        .map(|(m, days)| format!("{m} {}", days_text(days)))
        .collect();
    // Met on no day of the year: the season changes how far, not the
    // answer, which is the worst day's ("8h away" in winter fails a
    // required schedule even if "7h away" in summer is only uncertain).
    if !judged.iter().any(|(j, _)| j.verdict == Verdict::Pass) {
        let worst = judged
            .iter()
            .map(|(j, _)| verdict_rank(j.verdict))
            .min()
            .unwrap_or_default();
        return match judged
            .iter()
            .filter(|(j, _)| verdict_rank(j.verdict) == worst)
            .max_by_key(|(_, days)| days.len())
        {
            Some((j, _)) => Reason::new(
                RuleId::Timezone,
                j.verdict,
                format!(
                    "{} ({}, with daylight saving time)",
                    j.text,
                    join(&when, "and")
                ),
            ),
            None => Reason::new(RuleId::Timezone, Verdict::Unknown, "unreadable"),
        };
    }
    let label = &z.label;
    let what = match z.kind {
        ZoneKind::Within => format!("Requires being within {label}"),
        ZoneKind::Hours { .. } => format!("Expects {label} hours"),
        ZoneKind::Overlap { hours } => format!("Requires {hours:.0}h of overlap with {label}"),
    };
    Reason::new(
        RuleId::Timezone,
        Verdict::Unknown,
        format!(
            "{what}, which your time zone meets for only part of the year: {} (daylight saving time)",
            join(&when, "and")
        ),
    )
}

/// One day: the person's offsets `mine` against the requirement's
/// `theirs`.
fn judge(z: &ZoneRequirement, mine: Offsets, theirs: Offsets, spread: bool) -> Judgement {
    let hard = z.strength == Strength::Required;
    let (near, far) = distances(mine, theirs);
    let label = &z.label;
    let (verdict, text, measure) = match z.kind {
        ZoneKind::Within => {
            let inside = mine.0 >= theirs.0 && mine.1 <= theirs.1;
            if inside {
                (
                    Verdict::Pass,
                    format!(
                        "Requires being within {label}; your time zone is ({})",
                        offsets_text(mine)
                    ),
                    "inside it".to_owned(),
                )
            } else if near > 0.0 {
                (
                    if hard {
                        Verdict::Fail
                    } else {
                        Verdict::Unknown
                    },
                    format!(
                        "Requires being within {label}; your time zone is {near:.0}h outside it"
                    ),
                    format!("{near:.0}h outside it"),
                )
            } else {
                (
                    Verdict::Unknown,
                    format!(
                        "Requires being within {label}; only part of your country's time zones are"
                    ),
                    "partly inside it".to_owned(),
                )
            }
        }
        ZoneKind::Hours { tolerance } => {
            let allowed = tolerance.unwrap_or(DEFAULT_ZONE_TOLERANCE_HOURS);
            let measure = format!("{near:.0}h away");
            if far <= allowed {
                (
                    Verdict::Pass,
                    format!("Expects {label} hours, which fit your time zone"),
                    measure,
                )
            } else if near <= allowed && spread {
                (
                    Verdict::Unknown,
                    format!("Expects {label} hours; that depends on where in your country you are"),
                    measure,
                )
            } else if hard && (tolerance.is_some() || near >= WORKING_DAY_HOURS) {
                (
                    Verdict::Fail,
                    format!("Expects {label} hours; your time zone is {near:.0}h away"),
                    measure,
                )
            } else {
                (
                    Verdict::Unknown,
                    format!(
                        "Expects {label} hours, {near:.0}h from your time zone; the posting doesn't say how much shift is acceptable"
                    ),
                    measure,
                )
            }
        }
        ZoneKind::Overlap { hours } => {
            let overlap_worst = (WORKING_DAY_HOURS - far).max(0.0);
            let overlap_best = (WORKING_DAY_HOURS - near).max(0.0);
            let measure = format!("{overlap_best:.0}h of overlap");
            if overlap_worst >= hours {
                (
                    Verdict::Pass,
                    format!(
                        "Requires {hours:.0}h of overlap with {label}; you'd have {overlap_worst:.0}h"
                    ),
                    measure,
                )
            } else if overlap_best >= hours {
                (
                    Verdict::Unknown,
                    format!(
                        "Requires {hours:.0}h of overlap with {label}; that depends on where in your country you are"
                    ),
                    measure,
                )
            } else {
                (
                    if hard {
                        Verdict::Fail
                    } else {
                        Verdict::Unknown
                    },
                    format!(
                        "Requires {hours:.0}h of overlap with {label}; you'd have about {overlap_best:.0}h"
                    ),
                    measure,
                )
            }
        }
    };
    Judgement {
        verdict,
        text,
        measure,
        far,
    }
}

/// Nearest and farthest distance in hours from any of `mine` to the zone.
fn distances(mine: Offsets, zone: Offsets) -> (f32, f32) {
    let near = distance_hours(mine, zone);
    let far = [mine.0, mine.1]
        .iter()
        .map(|m| distance_hours((*m, *m), zone))
        .fold(0.0_f32, f32::max);
    (near, far)
}

// ---------------------------------------------------------------------------
// 8. Ambiguity

/// What could not be read.
pub fn ambiguity(ctx: &Context<'_>) -> Vec<Reason> {
    let job = ctx.job;
    let mut out = Vec::new();
    if let WorkOption::Remote {
        scope: RemoteScope::Unknown,
        ..
    } = ctx.option
        && !job.unrecognized.is_empty()
    {
        let texts: Vec<String> = job
            .unrecognized
            .iter()
            .map(|e| format!("“{}”", e.text))
            .collect();
        out.push(
            Reason::new(
                RuleId::Ambiguity,
                Verdict::NotApplicable,
                format!(
                    "The listing also names {}, which Narrow doesn't recognize",
                    join(&texts, "and")
                ),
            )
            .evidence(&job.unrecognized),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_naturally() {
        let us = crate::geo::country("US").unwrap();
        assert_eq!(country_name(us), "the United States");
        assert_eq!(
            join(&["a".into(), "b".into(), "c".into()], "or"),
            "a, b or c"
        );
        assert_eq!(distances((-300, -120), (-480, -480)), (3.0, 6.0));
    }
}
