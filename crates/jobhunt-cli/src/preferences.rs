//! `jobhunt preferences`: what the user wants next.

use std::io::Write;
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::bail;
use chrono::Utc;
use clap::Subcommand;
use jobhunt_profile::{
    Arrangement, CompanyTrait, CompensationBound, Engagement, PayPeriod, PreferenceValue,
    ProfileService, RuleParser, Stance, WorkAspect, WorkMode,
};

use crate::config::LoadedConfig;
use crate::profile_args::{StanceArg, finish, open_store};
use crate::profile_render::{self, short};

#[derive(Debug, clap::Args)]
pub struct PreferencesArgs {
    #[command(subcommand)]
    pub command: Option<PreferencesCommand>,
}

#[derive(Debug, Subcommand)]
pub enum PreferencesCommand {
    /// Show your preferences and statements (the default).
    Show,
    /// Say what you want in your own words. The statement is kept as
    /// written; what JobHunt understands from it is saved as preferences.
    Add {
        #[arg(required = true, value_name = "STATEMENT")]
        words: Vec<String>,
    },
    /// Set one preference precisely.
    #[command(subcommand)]
    Set(SetCommand),
    /// Remove a preference (pref_…) or a statement and what was read from it (stmt_…).
    Remove { id: String },
}

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// A kind of role: backend, full stack, founding engineer, ...
    Role {
        role: String,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// Pay: a minimum (a hard floor) and/or a target.
    Compensation {
        /// Minimum, e.g. 120k or 120000.
        #[arg(long, value_name = "AMOUNT")]
        minimum: Option<Amount>,
        /// Target, e.g. 150k.
        #[arg(long, value_name = "AMOUNT")]
        target: Option<Amount>,
        /// ISO currency code (USD, EUR, BRL, ...). Never assumed.
        #[arg(long)]
        currency: String,
        #[arg(long, value_enum, default_value = "year")]
        per: PeriodArg,
        /// Applies to contract work only.
        #[arg(long, conflicts_with = "employment")]
        contract: bool,
        /// Applies to employment only.
        #[arg(long)]
        employment: bool,
    },
    /// Remote, hybrid or on-site.
    WorkMode {
        #[arg(value_enum)]
        mode: ModeArg,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// Where you live now.
    Location { place: Vec<String> },
    /// A region you can (or can't) work in.
    Region {
        region: String,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// A time zone you can work in (UTC-3, CET, "US hours").
    Timezone {
        zone: String,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// Whether you would relocate.
    Relocation {
        #[arg(value_enum)]
        answer: YesNo,
    },
    /// Whether you need visa sponsorship. "no" is read as: you may work
    /// where you live without it (use `authorized-in` for other places).
    Sponsorship {
        #[arg(value_enum)]
        answer: YesNo,
    },
    /// A country (or "the EU") you may already work in without
    /// sponsorship. Repeat for each.
    AuthorizedIn { place: Vec<String> },
    /// Being hired as an employee or as a contractor (B2B, freelance):
    /// `--stance require` for the only one you accept, `avoid` to rule one out.
    Engagement {
        #[arg(value_enum)]
        kind: EngagementArg,
        #[arg(long, value_enum, default_value = "accept")]
        stance: StanceArg,
    },
    /// A kind of company or team: startup, early-stage, scaleup,
    /// large-company, founder-led, product-company, agency, consulting,
    /// public-company, private-company, small-team, large-team,
    /// remote-first, open-source.
    Company {
        kind: String,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// A domain you like or avoid (developer tools, fintech, gambling, ...).
    Domain {
        domain: Vec<String>,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
    /// How you like to work: ownership, individual-contributor,
    /// management, greenfield, maintenance, async-communication, meetings,
    /// product-closeness, on-call.
    WorkStyle {
        aspect: String,
        #[arg(long, value_enum, default_value = "want")]
        stance: StanceArg,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PeriodArg {
    Year,
    Month,
    Day,
    Hour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ModeArg {
    Remote,
    Hybrid,
    #[value(alias = "on-site")]
    Onsite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum EngagementArg {
    Employee,
    #[value(alias = "b2b", alias = "freelance")]
    Contractor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum YesNo {
    Yes,
    No,
}

/// `120k`, `120,000`, `1.5m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(pub u64);

impl FromStr for Amount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim().to_lowercase().replace([',', '_', ' '], "");
        let (number, factor) = if let Some(n) = t.strip_suffix('k') {
            (n, 1_000.0)
        } else if let Some(n) = t.strip_suffix('m') {
            (n, 1_000_000.0)
        } else {
            (t.as_str(), 1.0)
        };
        let value: f64 = number
            .parse()
            .map_err(|_| format!("{s:?} is not an amount (for example 120k or 120000)"))?;
        if value.is_nan() || value <= 0.0 {
            return Err(format!("{s:?} must be positive"));
        }
        Ok(Self((value * factor).round() as u64))
    }
}

pub async fn run(args: PreferencesArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let store = open_store(loaded).await?;
    let result = execute(args, &store).await;
    store.close().await;
    result
}

async fn execute(
    args: PreferencesArgs,
    store: &jobhunt_storage::SqliteJobStore,
) -> anyhow::Result<ExitCode> {
    let service = ProfileService::new(store);
    let now = Utc::now();
    let mut out = anstream::stdout().lock();
    match args.command.unwrap_or(PreferencesCommand::Show) {
        PreferencesCommand::Show => {
            let data = service.load_or_new(now).await?;
            finish(profile_render::preferences(&mut out, &data))
        }
        PreferencesCommand::Add { words } => {
            let outcome = service
                .add_statement(&words.join(" "), &RuleParser, now)
                .await?;
            finish(profile_render::statement_outcome(&mut out, &outcome))
        }
        PreferencesCommand::Remove { id } => {
            service.remove(&id, now).await?;
            finish(writeln!(out, "Removed {}.", short(&id)))
        }
        PreferencesCommand::Set(set) => {
            let values = values(set)?;
            let mut result = Ok(());
            for (value, stance) in values {
                let (pref, replaced) = service.set_preference(value, stance, now).await?;
                if result.is_ok() {
                    result = writeln!(
                        out,
                        "Set: {} {} ({})",
                        stance.as_str(),
                        pref.value,
                        short(pref.id)
                    );
                }
                for old in replaced {
                    if result.is_ok() {
                        result = writeln!(out, "  replaces: {} {}", old.stance.as_str(), old.value);
                    }
                }
            }
            finish(result)
        }
    }
}

fn values(set: SetCommand) -> anyhow::Result<Vec<(PreferenceValue, Stance)>> {
    Ok(match set {
        SetCommand::Role { role, stance } => {
            vec![(
                PreferenceValue::Role {
                    role: role.trim().to_lowercase(),
                },
                stance.into(),
            )]
        }
        SetCommand::Compensation {
            minimum,
            target,
            currency,
            per,
            contract,
            employment,
        } => {
            if minimum.is_none() && target.is_none() {
                bail!("give --minimum, --target, or both");
            }
            let currency = currency.trim().to_uppercase();
            if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
                bail!("--currency must be a three-letter ISO code such as USD, EUR or BRL");
            }
            let period = match per {
                PeriodArg::Year => PayPeriod::Year,
                PeriodArg::Month => PayPeriod::Month,
                PeriodArg::Day => PayPeriod::Day,
                PeriodArg::Hour => PayPeriod::Hour,
            };
            let arrangement = if contract {
                Some(Arrangement::Contract)
            } else if employment {
                Some(Arrangement::Employment)
            } else {
                None
            };
            let value = |bound, amount: Amount| PreferenceValue::Compensation {
                bound,
                amount: amount.0,
                currency: Some(currency.clone()),
                period,
                arrangement,
            };
            let mut out = Vec::new();
            if let Some(amount) = minimum {
                out.push((value(CompensationBound::Minimum, amount), Stance::Required));
            }
            if let Some(amount) = target {
                out.push((value(CompensationBound::Target, amount), Stance::Wanted));
            }
            out
        }
        SetCommand::WorkMode { mode, stance } => {
            let mode = match mode {
                ModeArg::Remote => WorkMode::Remote,
                ModeArg::Hybrid => WorkMode::Hybrid,
                ModeArg::Onsite => WorkMode::Onsite,
            };
            vec![(PreferenceValue::WorkMode { mode }, stance.into())]
        }
        SetCommand::Location { place } => {
            let place = place.join(" ").trim().to_owned();
            if place.is_empty() {
                bail!(
                    "say where you live, e.g. jobhunt preferences set location \"Lisbon, Portugal\""
                );
            }
            vec![(PreferenceValue::CurrentLocation { place }, Stance::Required)]
        }
        SetCommand::Region { region, stance } => {
            vec![(
                PreferenceValue::Region {
                    region: region.trim().to_owned(),
                },
                stance.into(),
            )]
        }
        SetCommand::Timezone { zone, stance } => {
            vec![(
                PreferenceValue::Timezone {
                    zone: zone.trim().to_owned(),
                },
                stance.into(),
            )]
        }
        SetCommand::Relocation { answer } => vec![(
            PreferenceValue::Relocation {
                willing: answer == YesNo::Yes,
            },
            Stance::Required,
        )],
        SetCommand::Sponsorship { answer } => vec![(
            PreferenceValue::Sponsorship {
                needed: answer == YesNo::Yes,
            },
            Stance::Required,
        )],
        SetCommand::AuthorizedIn { place } => {
            let place = place.join(" ").trim().to_owned();
            if place.is_empty() {
                bail!("name a country, e.g. jobhunt preferences set authorized-in Portugal");
            }
            vec![(
                PreferenceValue::WorkAuthorization { place },
                Stance::Required,
            )]
        }
        SetCommand::Engagement { kind, stance } => vec![(
            PreferenceValue::Engagement {
                engagement: match kind {
                    EngagementArg::Employee => Engagement::Employee,
                    EngagementArg::Contractor => Engagement::Contractor,
                },
            },
            stance.into(),
        )],
        SetCommand::Company { kind, stance } => {
            let Some(company) = CompanyTrait::from_canonical(&kind.trim().to_lowercase()) else {
                let valid: Vec<String> = CompanyTrait::ALL
                    .iter()
                    .map(|t| t.as_str().replace('_', "-"))
                    .collect();
                bail!(
                    "unknown company kind {kind:?}; use one of: {}",
                    valid.join(", ")
                );
            };
            vec![(PreferenceValue::Company { company }, stance.into())]
        }
        SetCommand::Domain { domain, stance } => {
            let raw = domain.join(" ");
            let domain = jobhunt_profile::infer::canonical_domain(&raw)
                .map_or_else(|| raw.trim().to_lowercase(), str::to_owned);
            if domain.is_empty() {
                bail!("name a domain, e.g. jobhunt preferences set domain \"developer tools\"");
            }
            vec![(PreferenceValue::Domain { domain }, stance.into())]
        }
        SetCommand::WorkStyle { aspect, stance } => {
            let Some(aspect) = WorkAspect::from_canonical(&aspect.trim().to_lowercase()) else {
                let valid: Vec<String> = WorkAspect::ALL
                    .iter()
                    .map(|a| a.as_str().replace('_', "-"))
                    .collect();
                bail!(
                    "unknown work-style aspect {aspect:?}; use one of: {}",
                    valid.join(", ")
                );
            };
            vec![(PreferenceValue::WorkStyle { aspect }, stance.into())]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_amounts() {
        assert_eq!("120k".parse::<Amount>().unwrap(), Amount(120_000));
        assert_eq!("120,000".parse::<Amount>().unwrap(), Amount(120_000));
        assert_eq!("1.5m".parse::<Amount>().unwrap(), Amount(1_500_000));
        assert!("lots".parse::<Amount>().is_err());
        assert!("-5".parse::<Amount>().is_err());
    }

    #[test]
    fn structured_values() {
        let comp = values(SetCommand::Compensation {
            minimum: Some(Amount(120_000)),
            target: Some(Amount(150_000)),
            currency: "usd".into(),
            per: PeriodArg::Year,
            contract: false,
            employment: false,
        })
        .unwrap();
        assert_eq!(comp.len(), 2);
        assert_eq!(comp[0].1, Stance::Required);
        assert!(matches!(
            &comp[0].0,
            PreferenceValue::Compensation { currency: Some(c), amount: 120_000, .. } if c == "USD"
        ));
        assert!(
            values(SetCommand::Company {
                kind: "unicorn".into(),
                stance: StanceArg::Want
            })
            .is_err()
        );
        let auth = values(SetCommand::AuthorizedIn {
            place: vec!["Portugal".into()],
        })
        .unwrap();
        assert_eq!(
            auth[0],
            (
                PreferenceValue::WorkAuthorization {
                    place: "Portugal".into()
                },
                Stance::Required
            )
        );
        assert!(values(SetCommand::AuthorizedIn { place: vec![] }).is_err());
        let engagement = values(SetCommand::Engagement {
            kind: EngagementArg::Contractor,
            stance: StanceArg::Avoid,
        })
        .unwrap();
        assert_eq!(
            engagement[0],
            (
                PreferenceValue::Engagement {
                    engagement: Engagement::Contractor
                },
                Stance::Unwanted
            )
        );
        let company = values(SetCommand::Company {
            kind: "founder-led".into(),
            stance: StanceArg::Want,
        })
        .unwrap();
        assert_eq!(
            company[0].0,
            PreferenceValue::Company {
                company: CompanyTrait::FounderLed
            }
        );
    }
}
