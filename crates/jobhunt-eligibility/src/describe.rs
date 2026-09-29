//! A job's requirements as short facts ("Remote: the Americas",
//! "Contractor: offered in Brazil", "Visa sponsorship: not stated"), for
//! display next to a decision. Facts about the job only; what they mean
//! for the person is the decision's job.

use crate::job::{
    JobMode, JobRequirements, Mechanism, Relocation, RemoteScope, Sponsorship, Strength,
    WorkOption, ZoneKind,
};
use crate::rules::area_name;

/// One labelled fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: &'static str,
    pub value: String,
}

fn fact(label: &'static str, value: impl Into<String>) -> Fact {
    Fact {
        label,
        value: value.into(),
    }
}

/// Where and how the job is done.
pub fn location(job: &JobRequirements) -> Vec<Fact> {
    let mut out = Vec::new();
    for option in &job.options {
        match option {
            WorkOption::Remote { scope, .. } => out.push(fact(
                "Remote",
                match scope {
                    RemoteScope::Global(_) => "anywhere (explicitly global)".to_owned(),
                    RemoteScope::Unknown => "yes, but no geographic scope is published".to_owned(),
                    other => other.label(),
                },
            )),
            WorkOption::Office {
                presence,
                area,
                raw,
                ..
            } => out.push(fact(
                match presence {
                    crate::job::Presence::Hybrid => "Hybrid",
                    crate::job::Presence::Onsite => "On-site",
                    crate::job::Presence::Office => "Office",
                },
                area.map_or_else(|| format!("{raw} (not recognized)"), |a| a.to_string()),
            )),
            WorkOption::Engagement(_) => {}
        }
    }
    if out.is_empty() && job.mode == JobMode::Unknown {
        out.push(fact("Where", "not published"));
    }
    let required: Vec<String> = job
        .allow
        .iter()
        .filter(|c| c.strength == Strength::Required)
        .map(|c| area_name(c.area))
        .collect();
    if !required.is_empty() {
        out.push(fact("Limited to", dedup(required).join(", ")));
    }
    let preferred: Vec<String> = job
        .allow
        .iter()
        .filter(|c| c.strength == Strength::Preferred)
        .map(|c| area_name(c.area))
        .collect();
    if !preferred.is_empty() {
        out.push(fact("Prefers", dedup(preferred).join(", ")));
    }
    if !job.deny.is_empty() {
        out.push(fact(
            "Excludes",
            dedup(job.deny.iter().map(|c| area_name(c.area)).collect()).join(", "),
        ));
    }
    if job.worldwide.is_some() && !matches!(job.remote_option(), Some(RemoteScope::Global(_))) {
        out.push(fact(
            "Anywhere",
            "the description says people can work from anywhere",
        ));
    }
    out
}

/// How people are engaged, and on what terms.
pub fn employment(job: &JobRequirements) -> Vec<Fact> {
    let mut out = Vec::new();
    if let Some((kind, _)) = &job.employment {
        out.push(fact("Employment", kind.as_str().replace('_', " ")));
    }
    let contractors: Vec<String> = job
        .mechanisms
        .iter()
        .map(|m| {
            let places: Vec<String> = m.areas.iter().map(|a| area_name(*a)).collect();
            let what = match m.mechanism {
                Mechanism::Contractor => "contractors",
                Mechanism::B2b => "B2B contractors",
                Mechanism::Eor => "an employer of record",
            };
            if places.is_empty() {
                format!("{what} (places not stated)")
            } else {
                format!("{what} in {}", places.join(", "))
            }
        })
        .collect();
    out.push(fact(
        "Contractor / EOR",
        match (&job.employee_only, contractors.is_empty()) {
            (Some(_), _) => "employees only".to_owned(),
            (None, true) => "not stated".to_owned(),
            (None, false) => format!("offered: {}", dedup(contractors).join("; ")),
        },
    ));
    out.push(fact(
        "Visa sponsorship",
        match &job.sponsorship {
            Some((Sponsorship::Offered { caveat: false }, _)) => "offered".to_owned(),
            Some((Sponsorship::Offered { caveat: true }, _)) => {
                "offered, but not for every role".to_owned()
            }
            Some((Sponsorship::Unavailable, e)) if e.is_structured() => {
                format!("not offered (“{}”)", e.text)
            }
            Some((Sponsorship::Unavailable, _)) => "not offered".to_owned(),
            None => "not stated".to_owned(),
        },
    ));
    for a in &job.authorization {
        let places: Vec<String> = a.areas.iter().map(|x| area_name(*x)).collect();
        out.push(fact(
            "Authorization",
            format!(
                "must be authorized to work in {}{}",
                places.join(" or "),
                if a.strength == Strength::Preferred {
                    " (preferred)"
                } else {
                    ""
                }
            ),
        ));
    }
    if let Some((relocation, _)) = &job.relocation {
        out.push(fact(
            "Relocation",
            match relocation {
                Relocation::Offered => "help offered",
                Relocation::Required => "required",
            },
        ));
    }
    out
}

/// Time-zone requirements.
pub fn timezone(job: &JobRequirements) -> Vec<Fact> {
    if job.zones.is_empty() {
        return vec![fact(
            "Time zone",
            if job.flexible_hours.is_some() {
                "flexible hours"
            } else if job.vague_zone.is_some() {
                "overlap mentioned, zones not stated"
            } else {
                "no requirement published"
            },
        )];
    }
    job.zones
        .iter()
        .map(|z| {
            fact(
                "Time zone",
                match z.kind {
                    ZoneKind::Within => {
                        format!("within {} ({})", z.label, z.zone.label())
                    }
                    ZoneKind::Hours { tolerance: Some(t) } => {
                        format!("{} hours ±{t}h", z.label)
                    }
                    ZoneKind::Hours { tolerance: None } => format!("{} hours", z.label),
                    ZoneKind::Overlap { hours } => {
                        format!("{hours}h overlap with {}", z.label)
                    }
                },
            )
        })
        .collect()
}

fn dedup(mut items: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
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
