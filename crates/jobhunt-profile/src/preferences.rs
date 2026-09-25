//! What the user wants next, and what they don't.
//!
//! Preferences are individual [`Preference`] records with a typed
//! [`PreferenceValue`] and a [`Stance`]. They come either from structured
//! input (`jobhunt preferences role backend --want`) or from a
//! [`PreferenceStatement`]: a sentence in the user's own words, which is
//! always stored verbatim, with whatever structured preferences could be
//! read from it linked back to it (and marked [`Certainty::Uncertain`] when
//! the reading is shaky).
//!
//! Each value has a [`PreferenceValue::key`]; a newer preference with the
//! same key supersedes the older one, which is kept (inactive) as history.
//! Nothing here decides whether a job fits: these are the constraints later
//! eligibility and ranking will read.

use std::fmt;

use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use serde::{Deserialize, Serialize};

use crate::ids::{PreferenceId, StatementId};

/// How the user feels about a preference value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    /// A hard constraint ("remote only", "at least $120k").
    Required,
    Wanted,
    Acceptable,
    Unwanted,
}

impl Stance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Wanted => "wanted",
            Self::Acceptable => "acceptable",
            Self::Unwanted => "unwanted",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "required" => Some(Self::Required),
            "wanted" => Some(Self::Wanted),
            "acceptable" => Some(Self::Acceptable),
            "unwanted" => Some(Self::Unwanted),
            _ => None,
        }
    }

    pub fn is_positive(self) -> bool {
        matches!(self, Self::Required | Self::Wanted)
    }
}

/// Whether JobHunt is sure it read a statement correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    Certain,
    /// Kept, shown with a question mark, and worth checking.
    Uncertain,
}

impl Certainty {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Certain => "certain",
            Self::Uncertain => "uncertain",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "certain" => Some(Self::Certain),
            "uncertain" => Some(Self::Uncertain),
            _ => None,
        }
    }
}

/// Pay period of a compensation amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayPeriod {
    Year,
    Month,
    Day,
    Hour,
}

impl PayPeriod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Hour => "hour",
        }
    }
}

/// Whether a compensation figure is a floor or a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationBound {
    Minimum,
    Target,
}

impl CompensationBound {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimum => "minimum",
            Self::Target => "target",
        }
    }
}

/// Employment context a compensation figure applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrangement {
    Employment,
    Contract,
}

impl Arrangement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Employment => "employment",
            Self::Contract => "contract",
        }
    }
}

/// Where the work happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Remote,
    Hybrid,
    Onsite,
}

impl WorkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Hybrid => "hybrid",
            Self::Onsite => "onsite",
        }
    }
}

/// Kinds of company and team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyTrait {
    Startup,
    EarlyStage,
    Scaleup,
    LargeCompany,
    FounderLed,
    ProductCompany,
    Agency,
    Consulting,
    PublicCompany,
    PrivateCompany,
    SmallTeam,
    LargeTeam,
    RemoteFirst,
    OpenSource,
}

impl CompanyTrait {
    pub const ALL: [CompanyTrait; 14] = [
        Self::Startup,
        Self::EarlyStage,
        Self::Scaleup,
        Self::LargeCompany,
        Self::FounderLed,
        Self::ProductCompany,
        Self::Agency,
        Self::Consulting,
        Self::PublicCompany,
        Self::PrivateCompany,
        Self::SmallTeam,
        Self::LargeTeam,
        Self::RemoteFirst,
        Self::OpenSource,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::EarlyStage => "early_stage",
            Self::Scaleup => "scaleup",
            Self::LargeCompany => "large_company",
            Self::FounderLed => "founder_led",
            Self::ProductCompany => "product_company",
            Self::Agency => "agency",
            Self::Consulting => "consulting",
            Self::PublicCompany => "public_company",
            Self::PrivateCompany => "private_company",
            Self::SmallTeam => "small_team",
            Self::LargeTeam => "large_team",
            Self::RemoteFirst => "remote_first",
            Self::OpenSource => "open_source",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        let value = value.replace('-', "_");
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "startups",
            Self::EarlyStage => "early-stage companies",
            Self::Scaleup => "scale-ups",
            Self::LargeCompany => "large companies",
            Self::FounderLed => "founder-led companies",
            Self::ProductCompany => "product companies",
            Self::Agency => "agencies",
            Self::Consulting => "consulting",
            Self::PublicCompany => "public companies",
            Self::PrivateCompany => "private companies",
            Self::SmallTeam => "small teams",
            Self::LargeTeam => "large teams",
            Self::RemoteFirst => "remote-first companies",
            Self::OpenSource => "open-source companies",
        }
    }
}

/// Aspects of how the work is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAspect {
    /// Ownership and autonomy.
    Ownership,
    /// Individual-contributor work.
    IndividualContributor,
    /// Managing people.
    Management,
    /// Building new things.
    Greenfield,
    /// Maintaining existing systems.
    Maintenance,
    /// Asynchronous, written communication.
    AsyncCommunication,
    /// Many meetings.
    Meetings,
    /// Working close to product and users.
    ProductCloseness,
    /// On-call duty.
    OnCall,
}

impl WorkAspect {
    pub const ALL: [WorkAspect; 9] = [
        Self::Ownership,
        Self::IndividualContributor,
        Self::Management,
        Self::Greenfield,
        Self::Maintenance,
        Self::AsyncCommunication,
        Self::Meetings,
        Self::ProductCloseness,
        Self::OnCall,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ownership => "ownership",
            Self::IndividualContributor => "individual_contributor",
            Self::Management => "management",
            Self::Greenfield => "greenfield",
            Self::Maintenance => "maintenance",
            Self::AsyncCommunication => "async_communication",
            Self::Meetings => "meetings",
            Self::ProductCloseness => "product_closeness",
            Self::OnCall => "on_call",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        let value = value.replace('-', "_");
        Self::ALL.into_iter().find(|t| t.as_str() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ownership => "ownership and autonomy",
            Self::IndividualContributor => "individual-contributor work",
            Self::Management => "managing people",
            Self::Greenfield => "greenfield work",
            Self::Maintenance => "maintenance work",
            Self::AsyncCommunication => "async communication",
            Self::Meetings => "many meetings",
            Self::ProductCloseness => "working close to product",
            Self::OnCall => "on-call duty",
        }
    }
}

/// One structured preference value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreferenceValue {
    /// A kind of role ("backend", "founding engineer").
    Role {
        role: String,
    },
    Compensation {
        bound: CompensationBound,
        amount: u64,
        /// ISO 4217 code when known. Never assumed from the user's country.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        currency: Option<String>,
        period: PayPeriod,
        /// Employment or contract; `None` applies to both.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arrangement: Option<Arrangement>,
    },
    WorkMode {
        mode: WorkMode,
    },
    /// Where the user lives now, as they wrote it.
    CurrentLocation {
        place: String,
    },
    /// A region the user can or wants to work in (or not).
    Region {
        region: String,
    },
    /// Time zones the user can work in ("UTC-3", "Europe").
    Timezone {
        zone: String,
    },
    Relocation {
        willing: bool,
    },
    Sponsorship {
        needed: bool,
    },
    Company {
        company: CompanyTrait,
    },
    Domain {
        domain: String,
    },
    WorkStyle {
        aspect: WorkAspect,
    },
}

/// Which group of preferences a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PreferenceCategory {
    Role,
    Compensation,
    Location,
    Company,
    Domain,
    WorkStyle,
}

impl PreferenceCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Compensation => "compensation",
            Self::Location => "location",
            Self::Company => "company",
            Self::Domain => "domain",
            Self::WorkStyle => "work_style",
        }
    }
}

impl PreferenceValue {
    pub fn category(&self) -> PreferenceCategory {
        match self {
            Self::Role { .. } => PreferenceCategory::Role,
            Self::Compensation { .. } => PreferenceCategory::Compensation,
            Self::WorkMode { .. }
            | Self::CurrentLocation { .. }
            | Self::Region { .. }
            | Self::Timezone { .. }
            | Self::Relocation { .. }
            | Self::Sponsorship { .. } => PreferenceCategory::Location,
            Self::Company { .. } => PreferenceCategory::Company,
            Self::Domain { .. } => PreferenceCategory::Domain,
            Self::WorkStyle { .. } => PreferenceCategory::WorkStyle,
        }
    }

    /// Identity of the preference: a newer value with the same key replaces
    /// the older one ("at least $140k" replaces "at least $120k").
    pub fn key(&self) -> String {
        match self {
            Self::Role { role } => format!("role:{}", search_key(role)),
            Self::Compensation {
                bound, arrangement, ..
            } => format!(
                "compensation:{}:{}",
                bound.as_str(),
                arrangement.map_or("any", Arrangement::as_str)
            ),
            Self::WorkMode { mode } => format!("work_mode:{}", mode.as_str()),
            Self::CurrentLocation { .. } => "current_location".to_owned(),
            Self::Region { region } => format!("region:{}", search_key(region)),
            Self::Timezone { zone } => format!("timezone:{}", search_key(zone)),
            Self::Relocation { .. } => "relocation".to_owned(),
            Self::Sponsorship { .. } => "sponsorship".to_owned(),
            Self::Company { company } => format!("company:{}", company.as_str()),
            Self::Domain { domain } => format!("domain:{}", search_key(domain)),
            Self::WorkStyle { aspect } => format!("work_style:{}", aspect.as_str()),
        }
    }
}

impl fmt::Display for PreferenceValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Role { role } => write!(f, "{role} roles"),
            Self::Compensation {
                bound,
                amount,
                currency,
                period,
                arrangement,
            } => {
                let bound = match bound {
                    CompensationBound::Minimum => "at least",
                    CompensationBound::Target => "target",
                };
                let currency = currency.as_deref().unwrap_or("(currency unknown)");
                write!(
                    f,
                    "{bound} {currency} {} per {}",
                    group_thousands(*amount),
                    period.as_str()
                )?;
                if let Some(arrangement) = arrangement {
                    write!(f, " ({})", arrangement.as_str())?;
                }
                Ok(())
            }
            Self::WorkMode { mode } => write!(f, "{} work", mode.as_str()),
            Self::CurrentLocation { place } => write!(f, "based in {place}"),
            Self::Region { region } => write!(f, "work in {region}"),
            Self::Timezone { zone } => write!(f, "time zone {zone}"),
            Self::Relocation { willing: true } => write!(f, "willing to relocate"),
            Self::Relocation { willing: false } => write!(f, "not willing to relocate"),
            Self::Sponsorship { needed: true } => write!(f, "needs visa sponsorship"),
            Self::Sponsorship { needed: false } => write!(f, "does not need visa sponsorship"),
            Self::Company { company } => write!(f, "{}", company.label()),
            Self::Domain { domain } => write!(f, "{domain}"),
            Self::WorkStyle { aspect } => write!(f, "{}", aspect.label()),
        }
    }
}

/// `120000` → `120,000`.
pub fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Where a preference came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceOrigin {
    /// Set with a structured command.
    UserEntered,
    /// Read from a statement in the user's words.
    Statement,
}

impl PreferenceOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserEntered => "user_entered",
            Self::Statement => "statement",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "user_entered" => Some(Self::UserEntered),
            "statement" => Some(Self::Statement),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preference {
    pub id: PreferenceId,
    pub value: PreferenceValue,
    pub stance: Stance,
    pub origin: PreferenceOrigin,
    /// The statement it was read from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<StatementId>,
    /// The part of the statement it was read from, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub certainty: Certainty,
    /// False once a newer preference with the same key replaced it, or the
    /// user removed it.
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<PreferenceId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// How much of a statement JobHunt could read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementReading {
    /// Every part was understood.
    Understood,
    /// Some parts were understood, or understood with doubts.
    Partial,
    /// Nothing could be read with confidence; the text is kept as is.
    NotUnderstood,
}

impl StatementReading {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Understood => "understood",
            Self::Partial => "partial",
            Self::NotUnderstood => "not_understood",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "understood" => Some(Self::Understood),
            "partial" => Some(Self::Partial),
            "not_understood" => Some(Self::NotUnderstood),
            _ => None,
        }
    }
}

/// A preference in the user's own words. Never discarded or rewritten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceStatement {
    pub id: StatementId,
    pub text: String,
    /// Which parser read it (`rules/1`, ...).
    pub parser: String,
    pub reading: StatementReading,
    /// Parts of the text that produced no structured preference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unparsed: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Compensation preferences, one per bound and arrangement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompensationView<'a> {
    pub minimum: Vec<&'a Preference>,
    pub target: Vec<&'a Preference>,
}

/// Location constraints, as the user stated them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationView<'a> {
    pub current: Option<&'a Preference>,
    pub work_modes: Vec<&'a Preference>,
    pub regions: Vec<&'a Preference>,
    pub timezones: Vec<&'a Preference>,
    pub relocation: Option<&'a Preference>,
    pub sponsorship: Option<&'a Preference>,
}

/// Typed read access over the active preferences.
#[derive(Debug, Clone, Copy)]
pub struct PreferencesView<'a> {
    items: &'a [Preference],
}

impl<'a> PreferencesView<'a> {
    pub fn new(items: &'a [Preference]) -> Self {
        Self { items }
    }

    pub fn active(&self) -> impl Iterator<Item = &'a Preference> + 'a {
        self.items.iter().filter(|p| p.active)
    }

    pub fn in_category(&self, category: PreferenceCategory) -> Vec<&'a Preference> {
        self.active()
            .filter(|p| p.value.category() == category)
            .collect()
    }

    /// Roles with their stance.
    pub fn roles(&self) -> Vec<(&'a str, Stance)> {
        self.active()
            .filter_map(|p| match &p.value {
                PreferenceValue::Role { role } => Some((role.as_str(), p.stance)),
                _ => None,
            })
            .collect()
    }

    /// Roles the user wants (required or wanted).
    pub fn preferred_roles(&self) -> Vec<&'a str> {
        self.roles()
            .into_iter()
            .filter(|(_, s)| s.is_positive())
            .map(|(r, _)| r)
            .collect()
    }

    pub fn unwanted_roles(&self) -> Vec<&'a str> {
        self.roles()
            .into_iter()
            .filter(|(_, s)| *s == Stance::Unwanted)
            .map(|(r, _)| r)
            .collect()
    }

    pub fn compensation(&self) -> CompensationView<'a> {
        let mut view = CompensationView::default();
        for p in self.active() {
            if let PreferenceValue::Compensation { bound, .. } = &p.value {
                match bound {
                    CompensationBound::Minimum => view.minimum.push(p),
                    CompensationBound::Target => view.target.push(p),
                }
            }
        }
        view
    }

    pub fn location(&self) -> LocationView<'a> {
        let mut view = LocationView::default();
        for p in self.active() {
            match &p.value {
                PreferenceValue::CurrentLocation { .. } => view.current = Some(p),
                PreferenceValue::WorkMode { .. } => view.work_modes.push(p),
                PreferenceValue::Region { .. } => view.regions.push(p),
                PreferenceValue::Timezone { .. } => view.timezones.push(p),
                PreferenceValue::Relocation { .. } => view.relocation = Some(p),
                PreferenceValue::Sponsorship { .. } => view.sponsorship = Some(p),
                _ => {}
            }
        }
        view
    }

    pub fn company_traits(&self, stance: Stance) -> Vec<CompanyTrait> {
        self.active()
            .filter(|p| p.stance == stance)
            .filter_map(|p| match p.value {
                PreferenceValue::Company { company } => Some(company),
                _ => None,
            })
            .collect()
    }

    /// Company and team kinds the user does not want.
    pub fn disliked_company_traits(&self) -> Vec<CompanyTrait> {
        self.company_traits(Stance::Unwanted)
    }

    pub fn domains(&self, stance: Stance) -> Vec<&'a str> {
        self.active()
            .filter(|p| p.stance == stance)
            .filter_map(|p| match &p.value {
                PreferenceValue::Domain { domain } => Some(domain.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn work_style(&self) -> Vec<(WorkAspect, Stance)> {
        self.active()
            .filter_map(|p| match p.value {
                PreferenceValue::WorkStyle { aspect } => Some((aspect, p.stance)),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_identify_what_a_newer_value_replaces() {
        let min = |amount, arrangement| PreferenceValue::Compensation {
            bound: CompensationBound::Minimum,
            amount,
            currency: Some("USD".into()),
            period: PayPeriod::Year,
            arrangement,
        };
        assert_eq!(min(120_000, None).key(), min(140_000, None).key());
        assert_ne!(
            min(120_000, None).key(),
            min(120_000, Some(Arrangement::Contract)).key()
        );
        assert_eq!(
            PreferenceValue::Role {
                role: "Full Stack".into()
            }
            .key(),
            "role:full stack"
        );
    }

    #[test]
    fn values_serialize_with_a_type_tag() {
        let value = PreferenceValue::Company {
            company: CompanyTrait::FounderLed,
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#"{"type":"company","company":"founder_led"}"#);
        assert_eq!(
            serde_json::from_str::<PreferenceValue>(&json).unwrap(),
            value
        );
        assert!(
            serde_json::from_str::<PreferenceValue>(r#"{"type":"company","company":"x"}"#).is_err()
        );
    }

    #[test]
    fn displays_compensation() {
        let value = PreferenceValue::Compensation {
            bound: CompensationBound::Minimum,
            amount: 120_000,
            currency: Some("USD".into()),
            period: PayPeriod::Year,
            arrangement: None,
        };
        assert_eq!(value.to_string(), "at least USD 120,000 per year");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }
}
