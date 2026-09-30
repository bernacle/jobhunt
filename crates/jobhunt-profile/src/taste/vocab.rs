//! The canonical values Narrow knows for each dimension, how they read,
//! and how the structured preferences set before the taste profile map
//! onto it.
//!
//! Values are open: anything the person says that isn't here is kept as
//! its normalized words. The tokens here are what the built-in rules
//! produce, what a model is asked to use, and what the older structured
//! preferences map to, so the same idea lands on the same key whoever read
//! it.

use crate::preferences::{
    CompanyTrait, Preference, PreferenceOrigin, PreferenceValue, Stance, WorkAspect,
};

use super::{Polarity, TasteConfidence, TasteDimension, TasteSource, normalize_value};

/// Known values of a dimension with the phrase each reads as.
pub fn known(dimension: TasteDimension) -> &'static [(&'static str, &'static str)] {
    match dimension {
        TasteDimension::Seniority => &[
            ("early_career", "Early-career roles"),
            ("mid", "Mid-level roles"),
            ("senior", "Senior roles"),
            ("staff_plus", "Staff-plus roles"),
        ],
        TasteDimension::WorkShape => &[
            ("backend", "Backend engineering"),
            ("platform", "Platform engineering"),
            ("infrastructure", "Infrastructure engineering"),
            ("product", "Product engineering"),
            ("full_stack", "Full-stack work"),
            ("frontend", "Frontend work"),
            ("mobile", "Mobile work"),
            (
                "database_internals",
                "Database internals and storage engines",
            ),
            ("distributed_systems", "Distributed systems"),
            ("developer_tooling", "Developer tooling"),
            ("security", "Security engineering"),
            ("ml_product", "ML product engineering"),
            ("ml_research", "ML research"),
            ("research", "Research roles"),
            ("data", "Data engineering"),
            ("sre", "SRE and operations"),
            ("embedded", "Embedded systems"),
        ],
        TasteDimension::Specialization => &[
            ("broad", "Broad, generalist roles"),
            ("moderate", "Moderately specialized roles"),
            ("deep", "Deep specialist roles"),
        ],
        TasteDimension::Ownership => &[("high", "High ownership and autonomy")],
        TasteDimension::Company => &[
            ("startup", "Startups"),
            ("early_stage", "Early-stage companies"),
            ("growth", "Growth-stage companies"),
            ("established", "Established companies"),
            ("small_company", "Small companies"),
            ("large_company", "Large companies"),
            ("founder_led", "Founder-led companies"),
            ("product_company", "Product companies"),
            ("agency", "Agencies"),
            ("consulting", "Consulting"),
            ("public_company", "Public companies"),
            ("private_company", "Private companies"),
            ("open_source", "Open-source companies"),
        ],
        TasteDimension::Team => &[
            ("small_team", "Small technical teams"),
            ("large_team", "Larger engineering teams"),
            ("distributed", "Distributed remote teams"),
        ],
        TasteDimension::Culture => &[
            ("strong_engineering", "A strong engineering culture"),
            ("process_heavy", "Process-heavy organizations"),
            ("fast_paced", "A fast pace"),
            ("mentorship", "Mentorship and learning"),
            ("remote_first", "Remote-first culture"),
        ],
        TasteDimension::WorkStyle => &[
            ("individual_contributor", "Individual-contributor work"),
            ("management", "Managing people"),
            ("greenfield", "Building new things"),
            ("maintenance", "Maintaining existing systems"),
            ("async_communication", "Async, written communication"),
            ("meetings", "Many meetings"),
            ("product_closeness", "Working close to product and users"),
            ("on_call", "On-call duty"),
        ],
        TasteDimension::Domain | TasteDimension::Technology | TasteDimension::Other => &[],
    }
}

/// How a value reads: "Small technical teams", "Fintech".
pub fn phrase(dimension: TasteDimension, value: &str) -> String {
    if let Some((_, phrase)) = known(dimension).iter().find(|(v, _)| *v == value) {
        return (*phrase).to_owned();
    }
    let words = value.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The phrase with its polarity, as shown in a list: "Startups", "Open to
/// startups". Avoided values read as the plain phrase (they are listed
/// under "You tend to avoid"), and neutral ones as "… doesn't matter".
pub fn sentence(dimension: TasteDimension, value: &str, polarity: Polarity) -> String {
    let phrase = phrase(dimension, value);
    match polarity {
        Polarity::Prefer | Polarity::Avoid => phrase,
        Polarity::Open => format!("Open to {}", lower_first(&phrase)),
        Polarity::Neutral => format!("{phrase}: doesn't matter"),
    }
}

fn lower_first(text: &str) -> String {
    // Keep acronyms ("ML research", "SRE …") as written.
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase() => text.to_owned(),
        (Some(a), _) => a.to_lowercase().chain(text.chars().skip(1)).collect(),
        _ => String::new(),
    }
}

/// A company or team kind of the structured model, on the taste model.
pub fn company_trait(t: CompanyTrait) -> (TasteDimension, &'static str) {
    match t {
        CompanyTrait::SmallTeam => (TasteDimension::Team, "small_team"),
        CompanyTrait::LargeTeam => (TasteDimension::Team, "large_team"),
        CompanyTrait::RemoteFirst => (TasteDimension::Culture, "remote_first"),
        CompanyTrait::Scaleup => (TasteDimension::Company, "growth"),
        CompanyTrait::Startup => (TasteDimension::Company, "startup"),
        CompanyTrait::EarlyStage => (TasteDimension::Company, "early_stage"),
        CompanyTrait::LargeCompany => (TasteDimension::Company, "large_company"),
        CompanyTrait::FounderLed => (TasteDimension::Company, "founder_led"),
        CompanyTrait::ProductCompany => (TasteDimension::Company, "product_company"),
        CompanyTrait::Agency => (TasteDimension::Company, "agency"),
        CompanyTrait::Consulting => (TasteDimension::Company, "consulting"),
        CompanyTrait::PublicCompany => (TasteDimension::Company, "public_company"),
        CompanyTrait::PrivateCompany => (TasteDimension::Company, "private_company"),
        CompanyTrait::SmallCompany => (TasteDimension::Company, "small_company"),
        CompanyTrait::OpenSource => (TasteDimension::Company, "open_source"),
    }
}

/// A work-style aspect of the structured model, on the taste model.
pub fn work_aspect(a: WorkAspect) -> (TasteDimension, &'static str) {
    match a {
        WorkAspect::Ownership => (TasteDimension::Ownership, "high"),
        other => (TasteDimension::WorkStyle, other.as_str()),
    }
}

/// A role of the structured model ("backend", "senior platform",
/// "storage engine") on the taste model: the shapes and levels it names,
/// or the role as written when Narrow knows neither.
pub fn role(role: &str) -> Vec<(TasteDimension, String)> {
    let read = super::rules::read_terms(role);
    let found: Vec<(TasteDimension, String)> = read
        .into_iter()
        .filter(|(d, _)| matches!(d, TasteDimension::WorkShape | TasteDimension::Seniority))
        .collect();
    if found.is_empty() {
        let value = normalize_value(role);
        if value.is_empty() {
            Vec::new()
        } else {
            vec![(TasteDimension::WorkShape, value)]
        }
    } else {
        found
    }
}

/// How a stance reads as a polarity.
pub fn polarity_of(stance: Stance) -> Polarity {
    match stance {
        Stance::Required | Stance::Wanted => Polarity::Prefer,
        Stance::Acceptable => Polarity::Open,
        Stance::Unwanted => Polarity::Avoid,
    }
}

/// The stance a polarity is stored as in the structured model (`None`:
/// neutral, which the structured model has no stance for).
pub fn stance_of(polarity: Polarity) -> Option<Stance> {
    match polarity {
        Polarity::Prefer => Some(Stance::Wanted),
        Polarity::Open => Some(Stance::Acceptable),
        Polarity::Avoid => Some(Stance::Unwanted),
        Polarity::Neutral => None,
    }
}

/// One taste statement a structured preference amounts to, before it is
/// given an id and times.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyReading {
    pub dimension: TasteDimension,
    pub value: String,
    pub polarity: Polarity,
    pub confidence: TasteConfidence,
    pub text: String,
    pub sources: Vec<TasteSource>,
}

/// Whether a structured preference is a practical constraint (where the
/// person can work, pay, the policies for what isn't known) rather than
/// taste. Practical constraints never become taste.
pub fn is_practical(value: &PreferenceValue) -> bool {
    !matches!(
        value,
        PreferenceValue::Role { .. }
            | PreferenceValue::Company { .. }
            | PreferenceValue::Domain { .. }
            | PreferenceValue::WorkStyle { .. }
    )
}

/// The taste a structured preference amounts to: company and team kinds,
/// roles, domains and work style. Practical constraints give nothing.
pub fn from_preference(p: &Preference) -> Vec<LegacyReading> {
    let targets: Vec<(TasteDimension, String)> = match &p.value {
        PreferenceValue::Company { company } => {
            let (d, v) = company_trait(*company);
            vec![(d, v.to_owned())]
        }
        PreferenceValue::WorkStyle { aspect } => {
            let (d, v) = work_aspect(*aspect);
            vec![(d, v.to_owned())]
        }
        PreferenceValue::Domain { domain } => {
            vec![(TasteDimension::Domain, normalize_value(domain))]
        }
        PreferenceValue::Role { role: r } => role(r),
        _ => Vec::new(),
    };
    let polarity = polarity_of(p.stance);
    let confidence = if p.certainty == crate::Certainty::Uncertain {
        TasteConfidence::Medium
    } else {
        TasteConfidence::High
    };
    let mut sources = vec![TasteSource::Preference {
        preference: p.id,
        text: format!("{} {}", stance_label(p.stance), p.value),
    }];
    if p.origin == PreferenceOrigin::Statement
        && let Some(snippet) = &p.snippet
    {
        sources.push(TasteSource::Words {
            quote: snippet.clone(),
            statement: p.statement,
        });
    }
    targets
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(dimension, value)| {
            let mut text = sentence(dimension, &value, polarity);
            if p.stance == Stance::Required {
                text.push_str(" (a must)");
            }
            LegacyReading {
                dimension,
                text,
                value,
                polarity,
                confidence,
                sources: sources.clone(),
            }
        })
        .collect()
}

fn stance_label(stance: Stance) -> &'static str {
    match stance {
        Stance::Required => "must have:",
        Stance::Wanted => "want:",
        Stance::Acceptable => "fine with:",
        Stance::Unwanted => "avoid:",
    }
}

/// Where a pattern learned from feedback lands on the taste model, from
/// ranking's `dimension` and `value` (`company_trait`, `small_team`).
/// Pay, particular employers and remarks about one posting are not taste.
pub fn from_learned(dimension: &str, value: &str) -> Option<(TasteDimension, String)> {
    match dimension {
        "role" => Some((TasteDimension::WorkShape, normalize_value(value))),
        "seniority" => {
            let level = match value {
                "intern" | "junior" | "entry" | "new grad" | "early career" => "early_career",
                "mid" | "intermediate" => "mid",
                "senior" | "lead" => "senior",
                "staff" | "principal" | "distinguished" => "staff_plus",
                _ => return None,
            };
            Some((TasteDimension::Seniority, level.to_owned()))
        }
        "work_style" => WorkAspect::from_canonical(value)
            .map(work_aspect)
            .map(|(d, v)| (d, v.to_owned())),
        "company_trait" => CompanyTrait::from_canonical(value)
            .map(company_trait)
            .map(|(d, v)| (d, v.to_owned())),
        "technology" => Some((TasteDimension::Technology, normalize_value(value))),
        "domain" => Some((TasteDimension::Domain, normalize_value(value))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::{Certainty, PreferenceId};

    fn pref(value: PreferenceValue, stance: Stance) -> Preference {
        let at = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        Preference {
            id: PreferenceId::derive(&[&value.key()]),
            value,
            stance,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn structured_preferences_map_to_taste_and_constraints_to_nothing() {
        let team = from_preference(&pref(
            PreferenceValue::Company {
                company: CompanyTrait::SmallTeam,
            },
            Stance::Wanted,
        ));
        assert_eq!(team.len(), 1);
        assert_eq!(
            (team[0].dimension, team[0].value.as_str(), team[0].polarity),
            (TasteDimension::Team, "small_team", Polarity::Prefer)
        );
        let company = from_preference(&pref(
            PreferenceValue::Company {
                company: CompanyTrait::SmallCompany,
            },
            Stance::Wanted,
        ));
        assert_eq!(
            company[0].dimension,
            TasteDimension::Company,
            "a company is not a team"
        );
        let early = from_preference(&pref(
            PreferenceValue::Company {
                company: CompanyTrait::EarlyStage,
            },
            Stance::Required,
        ));
        assert_eq!(early[0].value, "early_stage");
        assert!(early[0].text.contains("a must"));
        let ownership = from_preference(&pref(
            PreferenceValue::WorkStyle {
                aspect: WorkAspect::Ownership,
            },
            Stance::Wanted,
        ));
        assert_eq!(ownership[0].dimension, TasteDimension::Ownership);
        for practical in [
            PreferenceValue::WorkMode {
                mode: crate::WorkMode::Remote,
            },
            PreferenceValue::Relocation {
                willing: false,
                only_to: Vec::new(),
            },
            PreferenceValue::WorkAuthorization {
                place: "Brazil".into(),
            },
            PreferenceValue::Compensation {
                bound: crate::CompensationBound::Minimum,
                amount: 120_000,
                currency: Some("USD".into()),
                period: crate::PayPeriod::Year,
                arrangement: None,
            },
        ] {
            assert!(is_practical(&practical));
            assert!(from_preference(&pref(practical, Stance::Required)).is_empty());
        }
    }

    #[test]
    fn roles_read_as_work_shape_and_level() {
        assert_eq!(
            role("senior platform"),
            vec![
                (TasteDimension::Seniority, "senior".to_owned()),
                (TasteDimension::WorkShape, "platform".to_owned())
            ]
        );
        assert_eq!(
            role("storage engine"),
            vec![(TasteDimension::WorkShape, "database_internals".to_owned())]
        );
        assert_eq!(
            role("founding engineer"),
            vec![(TasteDimension::WorkShape, "founding_engineer".to_owned())]
        );
    }

    #[test]
    fn learned_patterns_skip_pay_and_employers() {
        assert_eq!(
            from_learned("company_trait", "large_company"),
            Some((TasteDimension::Company, "large_company".to_owned()))
        );
        assert_eq!(from_learned("compensation", "local_pay"), None);
        assert_eq!(from_learned("company", "Acme"), None);
        assert_eq!(
            from_learned("seniority", "staff"),
            Some((TasteDimension::Seniority, "staff_plus".to_owned()))
        );
    }

    #[test]
    fn sentences_read_naturally() {
        assert_eq!(
            sentence(TasteDimension::Company, "startup", Polarity::Open),
            "Open to startups"
        );
        assert_eq!(
            sentence(TasteDimension::WorkShape, "ml_research", Polarity::Open),
            "Open to ML research"
        );
        assert_eq!(
            sentence(TasteDimension::Domain, "developer_tools", Polarity::Prefer),
            "Developer tools"
        );
    }
}
