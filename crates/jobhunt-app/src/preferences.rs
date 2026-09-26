//! Updating what the person wants: a statement in their own words, precise
//! structured values, or removals. `jobhunt preferences add|set|remove`
//! and the MCP `update_preferences` tool both come here, so a preference
//! means the same thing whichever way it was given.
//!
//! Statements are read by the same parser ([`RuleParser`]) and stored
//! verbatim; parts it could not read are returned, never dropped, and
//! uncertain readings are flagged. Repeating an update that is already in
//! effect changes nothing.

use chrono::{DateTime, Utc};
use jobhunt_profile::{
    Arrangement, Certainty, CompanyTrait, CompensationBound, Engagement, PayPeriod, Preference,
    PreferenceOrigin, PreferenceStatement, PreferenceValue, ProfileData, Removal, RuleParser,
    Stance, StatementOutcome, WorkAspect, WorkMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::views::time;

/// How strongly a preference holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StanceInput {
    /// A hard requirement.
    Require,
    /// Wanted (the default).
    #[default]
    Want,
    /// Acceptable.
    Accept,
    /// Not wanted.
    Avoid,
}

impl From<StanceInput> for Stance {
    fn from(value: StanceInput) -> Self {
        match value {
            StanceInput::Require => Stance::Required,
            StanceInput::Want => Stance::Wanted,
            StanceInput::Accept => Stance::Acceptable,
            StanceInput::Avoid => Stance::Unwanted,
        }
    }
}

/// A pay period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeriodInput {
    #[default]
    Year,
    Month,
    Day,
    Hour,
}

/// Remote, hybrid or on-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkModeInput {
    Remote,
    Hybrid,
    Onsite,
}

/// Hired as an employee or as a contractor (B2B, freelance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EngagementInput {
    Employee,
    Contractor,
}

/// Which arrangement a pay preference applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArrangementInput {
    Employment,
    Contract,
}

/// One precise preference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreferenceInput {
    /// A kind of role: "backend", "full stack", "founding engineer".
    Role {
        role: String,
        #[serde(default)]
        stance: StanceInput,
    },
    /// Pay: a minimum (a hard floor) and/or a target, in an ISO currency.
    Compensation {
        #[serde(default)]
        minimum: Option<u64>,
        #[serde(default)]
        target: Option<u64>,
        /// ISO 4217 code (USD, EUR, BRL). Never assumed.
        currency: String,
        #[serde(default)]
        period: PeriodInput,
        /// Applies to employment only or contracts only (both when absent).
        #[serde(default)]
        applies_to: Option<ArrangementInput>,
    },
    /// Remote, hybrid or on-site.
    WorkMode {
        mode: WorkModeInput,
        #[serde(default)]
        stance: StanceInput,
    },
    /// Where the person lives now ("São Paulo, Brazil").
    Location { place: String },
    /// A region the person can (or can't) work in.
    Region {
        region: String,
        #[serde(default)]
        stance: StanceInput,
    },
    /// A time zone the person can work in ("UTC-3", "US hours").
    Timezone {
        zone: String,
        #[serde(default)]
        stance: StanceInput,
    },
    /// Whether the person would relocate.
    Relocation { willing: bool },
    /// Whether the person needs visa sponsorship to work where they live.
    Sponsorship { needed: bool },
    /// A country (or "the EU") the person may already work in without
    /// sponsorship.
    AuthorizedIn { place: String },
    /// Employee or contractor; `require` for the only one accepted, `avoid`
    /// to rule one out.
    Engagement {
        engagement: EngagementInput,
        #[serde(default = "accept")]
        stance: StanceInput,
    },
    /// A kind of company or team: startup, early-stage, scaleup,
    /// large-company, founder-led, product-company, agency, consulting,
    /// public-company, private-company, small-team, large-team,
    /// remote-first, open-source.
    Company {
        company: String,
        #[serde(default)]
        stance: StanceInput,
    },
    /// A domain: "developer tools", "fintech", "gambling".
    Domain {
        domain: String,
        #[serde(default)]
        stance: StanceInput,
    },
    /// How the person likes to work: ownership, individual-contributor,
    /// management, greenfield, maintenance, async-communication, meetings,
    /// product-closeness, on-call.
    WorkStyle {
        aspect: String,
        #[serde(default)]
        stance: StanceInput,
    },
}

fn accept() -> StanceInput {
    StanceInput::Accept
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::InvalidPreference(message.into())
}

impl PreferenceInput {
    /// The stored preference values (a pay input can give two).
    pub fn values(&self) -> Result<Vec<(PreferenceValue, Stance)>, AppError> {
        let text = |value: &str, what: &str| {
            let value = value.trim();
            if value.is_empty() {
                Err(invalid(format!("the {what} is empty")))
            } else {
                Ok(value.to_owned())
            }
        };
        Ok(match self {
            Self::Role { role, stance } => vec![(
                PreferenceValue::Role {
                    role: text(role, "role")?.to_lowercase(),
                },
                (*stance).into(),
            )],
            Self::Compensation {
                minimum,
                target,
                currency,
                period,
                applies_to,
            } => {
                if minimum.is_none() && target.is_none() {
                    return Err(invalid("give a minimum, a target, or both"));
                }
                if minimum == &Some(0) || target == &Some(0) {
                    return Err(invalid("pay amounts must be positive"));
                }
                let currency = currency.trim().to_uppercase();
                if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(invalid(
                        "the currency must be a three-letter ISO code such as USD, EUR or BRL",
                    ));
                }
                let period = match period {
                    PeriodInput::Year => PayPeriod::Year,
                    PeriodInput::Month => PayPeriod::Month,
                    PeriodInput::Day => PayPeriod::Day,
                    PeriodInput::Hour => PayPeriod::Hour,
                };
                let arrangement = applies_to.map(|a| match a {
                    ArrangementInput::Employment => Arrangement::Employment,
                    ArrangementInput::Contract => Arrangement::Contract,
                });
                let value = |bound, amount: u64| PreferenceValue::Compensation {
                    bound,
                    amount,
                    currency: Some(currency.clone()),
                    period,
                    arrangement,
                };
                let mut out = Vec::new();
                if let Some(amount) = minimum {
                    out.push((value(CompensationBound::Minimum, *amount), Stance::Required));
                }
                if let Some(amount) = target {
                    out.push((value(CompensationBound::Target, *amount), Stance::Wanted));
                }
                out
            }
            Self::WorkMode { mode, stance } => vec![(
                PreferenceValue::WorkMode {
                    mode: match mode {
                        WorkModeInput::Remote => WorkMode::Remote,
                        WorkModeInput::Hybrid => WorkMode::Hybrid,
                        WorkModeInput::Onsite => WorkMode::Onsite,
                    },
                },
                (*stance).into(),
            )],
            Self::Location { place } => vec![(
                PreferenceValue::CurrentLocation {
                    place: text(place, "location")?,
                },
                Stance::Required,
            )],
            Self::Region { region, stance } => vec![(
                PreferenceValue::Region {
                    region: text(region, "region")?,
                },
                (*stance).into(),
            )],
            Self::Timezone { zone, stance } => vec![(
                PreferenceValue::Timezone {
                    zone: text(zone, "time zone")?,
                },
                (*stance).into(),
            )],
            Self::Relocation { willing } => vec![(
                PreferenceValue::Relocation { willing: *willing },
                Stance::Required,
            )],
            Self::Sponsorship { needed } => vec![(
                PreferenceValue::Sponsorship { needed: *needed },
                Stance::Required,
            )],
            Self::AuthorizedIn { place } => vec![(
                PreferenceValue::WorkAuthorization {
                    place: text(place, "country")?,
                },
                Stance::Required,
            )],
            Self::Engagement { engagement, stance } => vec![(
                PreferenceValue::Engagement {
                    engagement: match engagement {
                        EngagementInput::Employee => Engagement::Employee,
                        EngagementInput::Contractor => Engagement::Contractor,
                    },
                },
                (*stance).into(),
            )],
            Self::Company { company, stance } => {
                let key = company.trim().to_lowercase().replace(' ', "_");
                let Some(company) = CompanyTrait::from_canonical(&key) else {
                    let valid: Vec<String> = CompanyTrait::ALL
                        .iter()
                        .map(|t| t.as_str().replace('_', "-"))
                        .collect();
                    return Err(invalid(format!(
                        "unknown company kind {company:?}; use one of: {}",
                        valid.join(", ")
                    )));
                };
                vec![(PreferenceValue::Company { company }, (*stance).into())]
            }
            Self::Domain { domain, stance } => {
                let raw = text(domain, "domain")?;
                let domain = jobhunt_profile::infer::canonical_domain(&raw)
                    .map_or_else(|| raw.to_lowercase(), str::to_owned);
                vec![(PreferenceValue::Domain { domain }, (*stance).into())]
            }
            Self::WorkStyle { aspect, stance } => {
                let key = aspect.trim().to_lowercase().replace(' ', "_");
                let Some(aspect) = WorkAspect::from_canonical(&key) else {
                    let valid: Vec<String> = WorkAspect::ALL
                        .iter()
                        .map(|a| a.as_str().replace('_', "-"))
                        .collect();
                    return Err(invalid(format!(
                        "unknown work-style aspect {aspect:?}; use one of: {}",
                        valid.join(", ")
                    )));
                };
                vec![(PreferenceValue::WorkStyle { aspect }, (*stance).into())]
            }
        })
    }
}

/// A change to preferences. Every part is optional, but at least one is
/// needed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreferenceUpdate {
    /// What the person wants, in their own words.
    pub statement: Option<String>,
    pub set: Vec<PreferenceInput>,
    /// `pref_…` or `stmt_…` ids (or unique prefixes) to remove.
    pub remove: Vec<String>,
}

/// What an update did.
#[derive(Debug, Clone)]
pub struct PreferenceChanges {
    pub statement: Option<StatementOutcome>,
    /// Each structured value set: the preference in effect, what it
    /// replaced, and whether it was already in effect (nothing changed).
    pub set: Vec<(Preference, Vec<Preference>, bool)>,
    pub removed: Vec<Removal>,
    /// The profile afterwards.
    pub data: ProfileData,
}

impl LocalApp {
    /// Applies a preference update. Every structured value and removal id
    /// is checked before anything is written.
    pub async fn update_preferences(
        &self,
        update: &PreferenceUpdate,
        now: DateTime<Utc>,
    ) -> Result<PreferenceChanges, AppError> {
        let statement = update
            .statement
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if statement.is_none() && update.set.is_empty() && update.remove.is_empty() {
            return Err(invalid(
                "nothing to update: give a statement, preferences to set, or ids to remove",
            ));
        }
        let mut values = Vec::new();
        for input in &update.set {
            values.extend(input.values()?);
        }
        for id in &update.remove {
            let id = id.trim();
            if !(id.starts_with("pref_") || id.starts_with("stmt_")) {
                return Err(AppError::InvalidArguments(format!(
                    "{id:?} is not a preference (pref_…) or statement (stmt_…) id"
                )));
            }
        }
        self.exclusive(async {
            let profiles = self.profiles();
            let mut removed = Vec::new();
            for id in &update.remove {
                removed.push(retry_conflicts(|| profiles.remove(id.trim(), now)).await?);
            }
            let statement = match statement {
                Some(text) => {
                    Some(retry_conflicts(|| profiles.add_statement(text, &RuleParser, now)).await?)
                }
                None => None,
            };
            let mut set = Vec::new();
            for (value, stance) in values {
                let outcome = retry_conflicts(|| async {
                    let data = profiles.load_or_new(now).await?;
                    let existing = data
                        .preferences
                        .iter()
                        .find(|p| p.active && p.value == value && p.stance == stance)
                        .cloned();
                    Ok::<_, jobhunt_profile::ProfileError>(match existing {
                        Some(p) => (p, Vec::new(), true),
                        None => {
                            let (p, replaced) =
                                profiles.set_preference(value.clone(), stance, now).await?;
                            (p, replaced, false)
                        }
                    })
                })
                .await?;
                set.push(outcome);
            }
            let data = profiles.load_or_new(now).await?;
            Ok(PreferenceChanges {
                statement,
                set,
                removed,
                data,
            })
        })
        .await
    }
}

/// Runs a profile change again when another process changed the profile
/// between reading and writing it (the optimistic revision check failed).
/// Every change re-reads the profile, so a retry applies it to what is
/// stored now; after a few lost races the conflict is reported.
async fn retry_conflicts<T, F, Fut>(mut change: F) -> Result<T, AppError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, jobhunt_profile::ProfileError>>,
{
    const ATTEMPTS: u32 = 5;
    let mut attempt = 0;
    loop {
        match change().await.map_err(AppError::from) {
            Err(AppError::Conflict(_)) if attempt + 1 < ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(25 * u64::from(attempt))).await;
            }
            other => return other,
        }
    }
}

/// One preference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreferenceView {
    /// `pref_…`.
    pub id: String,
    /// `role`, `compensation`, `location`, `company`, `domain`, `work_style`.
    pub category: String,
    /// `required`, `wanted`, `acceptable` or `unwanted`.
    pub stance: String,
    /// The value, readable ("backend roles", "at least USD 120,000 per year").
    pub value: String,
    /// `certain`, or `uncertain` when JobHunt read it with doubts.
    pub certainty: String,
    /// `statement` (read from the person's words) or `user_entered`.
    pub origin: String,
    /// The words it was read from, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// How an ambiguous part was read, for the person to check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `stmt_…` it was read from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_id: Option<String>,
    pub active: bool,
    /// What the person must settle before Narrow relies on it (read from
    /// their words, and ambiguous in a way that matters). Until then it is
    /// unresolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarify: Option<Clarify>,
}

/// A question about a preference read from someone's words, with the
/// reading it would correct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Clarify {
    /// "140,000: at least, or around? In which currency?" A pay without a
    /// currency is never compared with any job's.
    Pay {
        amount: u64,
        /// `year`, `month`, `day` or `hour`, as read.
        period: String,
        /// As read: `minimum` or `target`.
        bound: String,
        /// As read; absent when the words didn't say (never assumed).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        currency: Option<String>,
        /// `employment` or `contract`, when the words said.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        applies_to: Option<String>,
    },
    /// "Small teams: must have, or nice to have? The team you'd join, or
    /// the company's size?" Words like these rarely say which.
    Size {
        /// As read: `small_team` or `small_company`.
        value: String,
    },
}

impl Clarify {
    /// The question a preference raises, if any.
    pub fn of(p: &Preference) -> Option<Self> {
        if p.origin != PreferenceOrigin::Statement || !p.active {
            return None;
        }
        match &p.value {
            PreferenceValue::Compensation {
                bound,
                amount,
                currency,
                period,
                arrangement,
            } if currency.is_none() || p.certainty == Certainty::Uncertain => Some(Self::Pay {
                amount: *amount,
                period: period.as_str().to_owned(),
                bound: bound.as_str().to_owned(),
                currency: currency.clone(),
                applies_to: arrangement.map(|a| a.as_str().to_owned()),
            }),
            PreferenceValue::Company {
                company: company @ (CompanyTrait::SmallTeam | CompanyTrait::SmallCompany),
            } => Some(Self::Size {
                value: company.as_str().to_owned(),
            }),
            _ => None,
        }
    }
}

impl PreferenceView {
    pub fn of(p: &Preference) -> Self {
        Self {
            id: p.id.to_string(),
            category: p.value.category().as_str().to_owned(),
            stance: p.stance.as_str().to_owned(),
            value: p.value.to_string(),
            certainty: match p.certainty {
                Certainty::Certain => "certain",
                Certainty::Uncertain => "uncertain",
            }
            .to_owned(),
            origin: p.origin.as_str().to_owned(),
            snippet: p.snippet.clone(),
            note: p.note.clone(),
            statement_id: p.statement.map(|s| s.to_string()),
            active: p.active,
            clarify: Clarify::of(p),
        }
    }
}

/// A statement in the person's words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StatementView {
    /// `stmt_…`.
    pub id: String,
    /// Verbatim.
    pub text: String,
    /// `understood`, `partial` or `not_understood`.
    pub reading: String,
    /// Parts of the text that produced no preference (kept, never dropped).
    pub not_understood: Vec<String>,
    pub at: String,
}

impl StatementView {
    pub fn of(s: &PreferenceStatement) -> Self {
        Self {
            id: s.id.to_string(),
            text: s.text.clone(),
            reading: s.reading.as_str().to_owned(),
            not_understood: s.unparsed.clone(),
            at: time(s.created_at),
        }
    }
}

/// The answer of `update_preferences`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreferenceUpdateResult {
    /// The statement, as stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<StatementView>,
    /// What was understood, from the statement and structured values.
    pub interpreted: Vec<PreferenceView>,
    /// The interpretations JobHunt is unsure about (also in
    /// `interpreted`), for the person to confirm or correct.
    pub uncertain: Vec<PreferenceView>,
    /// Parts of the statement that were not understood, verbatim.
    pub not_understood: Vec<String>,
    /// Earlier preferences these replaced.
    pub replaced: Vec<PreferenceView>,
    /// Ids removed (or, for a resume-derived record, rejected).
    pub removed: Vec<String>,
    /// Everything asked was already in effect; nothing changed.
    pub unchanged: bool,
    /// Every preference in effect now.
    pub active: Vec<PreferenceView>,
}

impl PreferenceUpdateResult {
    pub fn of(c: &PreferenceChanges) -> Self {
        let mut interpreted: Vec<PreferenceView> = Vec::new();
        let mut replaced = Vec::new();
        let mut unchanged = c.removed.is_empty();
        if let Some(s) = &c.statement {
            interpreted.extend(s.preferences.iter().map(PreferenceView::of));
            replaced.extend(s.replaced.iter().map(PreferenceView::of));
            unchanged &= s.repeated;
        }
        for (p, r, repeated) in &c.set {
            interpreted.push(PreferenceView::of(p));
            replaced.extend(r.iter().map(PreferenceView::of));
            unchanged &= *repeated;
        }
        Self {
            statement: c
                .statement
                .as_ref()
                .map(|s| StatementView::of(&s.statement)),
            uncertain: interpreted
                .iter()
                .filter(|p| p.certainty == "uncertain")
                .cloned()
                .collect(),
            not_understood: c
                .statement
                .as_ref()
                .map(|s| s.statement.unparsed.clone())
                .unwrap_or_default(),
            interpreted,
            replaced,
            removed: c
                .removed
                .iter()
                .map(|r| match r {
                    Removal::Deleted(id) | Removal::Rejected(id) => id.clone(),
                })
                .collect(),
            unchanged,
            active: c
                .data
                .preferences
                .iter()
                .filter(|p| p.active)
                .map(PreferenceView::of)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_values() {
        let comp = PreferenceInput::Compensation {
            minimum: Some(120_000),
            target: Some(150_000),
            currency: "usd".into(),
            period: PeriodInput::Year,
            applies_to: None,
        }
        .values()
        .unwrap();
        assert_eq!(comp.len(), 2);
        assert_eq!(comp[0].1, Stance::Required);
        assert!(matches!(
            &comp[0].0,
            PreferenceValue::Compensation { currency: Some(c), amount: 120_000, .. } if c == "USD"
        ));
        let bad = PreferenceInput::Company {
            company: "unicorn".into(),
            stance: StanceInput::Want,
        };
        assert!(matches!(bad.values(), Err(AppError::InvalidPreference(_))));
        let company = PreferenceInput::Company {
            company: "founder-led".into(),
            stance: StanceInput::Want,
        }
        .values()
        .unwrap();
        assert_eq!(
            company[0].0,
            PreferenceValue::Company {
                company: CompanyTrait::FounderLed
            }
        );
        let engagement: PreferenceInput =
            serde_json::from_str(r#"{"kind": "engagement", "engagement": "contractor"}"#).unwrap();
        assert_eq!(
            engagement.values().unwrap()[0].1,
            Stance::Acceptable,
            "engagement defaults to accept, like the CLI"
        );
        assert!(
            serde_json::from_str::<PreferenceInput>(r#"{"kind": "role", "role": "x", "extra": 1}"#)
                .is_err()
        );
        assert!(
            PreferenceInput::AuthorizedIn { place: " ".into() }
                .values()
                .is_err()
        );
    }
}
