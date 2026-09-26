//! `jobhunt preferences`: what the user wants next.

use std::io::Write;
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::bail;
use chrono::Utc;
use clap::Subcommand;
use jobhunt_app::preferences::{
    ArrangementInput, EngagementInput, PeriodInput, PreferenceInput, PreferenceUpdate, StanceInput,
    WorkModeInput, WorkSetupInput,
};

use crate::config::LoadedConfig;
use crate::profile_args::{StanceArg, finish};
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
    /// Whether you would relocate (separate from your work setup).
    Relocation {
        #[arg(value_enum)]
        answer: YesNo,
        /// Only to this country or region; repeat for each (with `yes`).
        #[arg(long = "only-to", value_name = "PLACE")]
        only_to: Vec<String>,
    },
    /// How you want to work, as one answer: remote-only, prefer-remote,
    /// hybrid-okay (remote or hybrid, not on-site), onsite-okay, or
    /// no-preference. Replaces your remote/hybrid/on-site preferences.
    WorkSetup {
        #[arg(value_enum)]
        setup: WorkSetupArg,
    },
    /// Jobs that don't publish pay: `show` them marked unresolved (the
    /// default), or `hide` them. Unknown pay never meets a minimum.
    UnknownPay {
        #[arg(value_enum)]
        policy: ShowHide,
    },
    /// Jobs whose eligibility Narrow can't confirm ("Remote" with no
    /// geographic scope): `show` them marked unresolved (the default), or
    /// `hide` them until it is confirmed.
    UnclearEligibility {
        #[arg(value_enum)]
        policy: ShowHide,
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
pub enum WorkSetupArg {
    RemoteOnly,
    PreferRemote,
    HybridOkay,
    #[value(alias = "on-site-okay")]
    OnsiteOkay,
    NoPreference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShowHide {
    Show,
    Hide,
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
    let app = crate::local::open(loaded).await?;
    let result = execute(args, &app).await;
    app.close().await;
    result
}

async fn execute(args: PreferencesArgs, app: &jobhunt_app::LocalApp) -> anyhow::Result<ExitCode> {
    let now = Utc::now();
    let mut out = anstream::stdout().lock();
    match args.command.unwrap_or(PreferencesCommand::Show) {
        PreferencesCommand::Show => {
            let data = app.profiles().load_or_new(now).await?;
            finish(profile_render::preferences(&mut out, &data))
        }
        PreferencesCommand::Add { words } => {
            let update = PreferenceUpdate {
                statement: Some(words.join(" ")),
                ..PreferenceUpdate::default()
            };
            let changes = app.update_preferences(&update, now).await?;
            match &changes.statement {
                Some(outcome) => finish(profile_render::statement_outcome(&mut out, outcome)),
                None => bail!("the statement is empty"),
            }
        }
        PreferencesCommand::Remove { id } => {
            let update = PreferenceUpdate {
                remove: vec![id.clone()],
                ..PreferenceUpdate::default()
            };
            app.update_preferences(&update, now).await?;
            finish(writeln!(out, "Removed {}.", short(&id)))
        }
        PreferencesCommand::Set(set) => {
            let update = PreferenceUpdate {
                set: vec![input(set)],
                ..PreferenceUpdate::default()
            };
            let changes = app.update_preferences(&update, now).await?;
            let mut result = Ok(());
            for (pref, replaced, repeated) in &changes.set {
                if result.is_ok() {
                    result = writeln!(
                        out,
                        "{}: {} {} ({})",
                        if *repeated { "Already set" } else { "Set" },
                        pref.stance.as_str(),
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

/// The structured preference a `set` command gives (validated by the
/// application, the same way as the MCP update_preferences tool).
fn input(set: SetCommand) -> PreferenceInput {
    let stance = |s: StanceArg| match s {
        StanceArg::Require => StanceInput::Require,
        StanceArg::Want => StanceInput::Want,
        StanceArg::Accept => StanceInput::Accept,
        StanceArg::Avoid => StanceInput::Avoid,
    };
    let yes = |a: YesNo| a == YesNo::Yes;
    match set {
        SetCommand::Role { role, stance: s } => PreferenceInput::Role {
            role,
            stance: stance(s),
        },
        SetCommand::Compensation {
            minimum,
            target,
            currency,
            per,
            contract,
            employment,
        } => PreferenceInput::Compensation {
            minimum: minimum.map(|a| a.0),
            target: target.map(|a| a.0),
            currency,
            period: match per {
                PeriodArg::Year => PeriodInput::Year,
                PeriodArg::Month => PeriodInput::Month,
                PeriodArg::Day => PeriodInput::Day,
                PeriodArg::Hour => PeriodInput::Hour,
            },
            applies_to: if contract {
                Some(ArrangementInput::Contract)
            } else if employment {
                Some(ArrangementInput::Employment)
            } else {
                None
            },
        },
        SetCommand::WorkMode { mode, stance: s } => PreferenceInput::WorkMode {
            mode: match mode {
                ModeArg::Remote => WorkModeInput::Remote,
                ModeArg::Hybrid => WorkModeInput::Hybrid,
                ModeArg::Onsite => WorkModeInput::Onsite,
            },
            stance: stance(s),
        },
        SetCommand::Location { place } => PreferenceInput::Location {
            place: place.join(" "),
        },
        SetCommand::Region { region, stance: s } => PreferenceInput::Region {
            region,
            stance: stance(s),
        },
        SetCommand::Timezone { zone, stance: s } => PreferenceInput::Timezone {
            zone,
            stance: stance(s),
        },
        SetCommand::Relocation { answer, only_to } => PreferenceInput::Relocation {
            willing: yes(answer),
            only_to,
        },
        SetCommand::WorkSetup { setup } => PreferenceInput::WorkSetup {
            setup: match setup {
                WorkSetupArg::RemoteOnly => WorkSetupInput::RemoteOnly,
                WorkSetupArg::PreferRemote => WorkSetupInput::PreferRemote,
                WorkSetupArg::HybridOkay => WorkSetupInput::HybridOkay,
                WorkSetupArg::OnsiteOkay => WorkSetupInput::OnsiteOkay,
                WorkSetupArg::NoPreference => WorkSetupInput::NoPreference,
            },
        },
        SetCommand::UnknownPay { policy } => PreferenceInput::UnknownPay {
            show: policy == ShowHide::Show,
        },
        SetCommand::UnclearEligibility { policy } => PreferenceInput::UnclearEligibility {
            show: policy == ShowHide::Show,
        },
        SetCommand::Sponsorship { answer } => PreferenceInput::Sponsorship {
            needed: yes(answer),
        },
        SetCommand::AuthorizedIn { place } => PreferenceInput::AuthorizedIn {
            place: place.join(" "),
        },
        SetCommand::Engagement { kind, stance: s } => PreferenceInput::Engagement {
            engagement: match kind {
                EngagementArg::Employee => EngagementInput::Employee,
                EngagementArg::Contractor => EngagementInput::Contractor,
            },
            stance: stance(s),
        },
        SetCommand::Company { kind, stance: s } => PreferenceInput::Company {
            company: kind,
            stance: stance(s),
        },
        SetCommand::Domain { domain, stance: s } => PreferenceInput::Domain {
            domain: domain.join(" "),
            stance: stance(s),
        },
        SetCommand::WorkStyle { aspect, stance: s } => PreferenceInput::WorkStyle {
            aspect,
            stance: stance(s),
        },
    }
}

#[cfg(test)]
mod tests {
    use jobhunt_profile::{CompanyTrait, Engagement, PreferenceValue, Stance};

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
    fn set_commands_become_the_shared_inputs() {
        let comp = input(SetCommand::Compensation {
            minimum: Some(Amount(120_000)),
            target: Some(Amount(150_000)),
            currency: "usd".into(),
            per: PeriodArg::Year,
            contract: false,
            employment: false,
        })
        .values()
        .unwrap();
        assert_eq!(comp.len(), 2);
        assert_eq!(comp[0].1, Stance::Required);
        assert!(matches!(
            &comp[0].0,
            PreferenceValue::Compensation { currency: Some(c), amount: 120_000, .. } if c == "USD"
        ));
        assert!(
            input(SetCommand::Company {
                kind: "unicorn".into(),
                stance: StanceArg::Want
            })
            .values()
            .is_err()
        );
        let auth = input(SetCommand::AuthorizedIn {
            place: vec!["Portugal".into()],
        })
        .values()
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
        assert!(
            input(SetCommand::AuthorizedIn { place: vec![] })
                .values()
                .is_err()
        );
        let engagement = input(SetCommand::Engagement {
            kind: EngagementArg::Contractor,
            stance: StanceArg::Avoid,
        })
        .values()
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
        let setup = input(SetCommand::WorkSetup {
            setup: WorkSetupArg::HybridOkay,
        })
        .values()
        .unwrap();
        assert_eq!(setup.len(), 2, "remote or hybrid, both required");
        assert!(setup.iter().all(|(_, s)| *s == Stance::Required));
        let relocation = input(SetCommand::Relocation {
            answer: YesNo::Yes,
            only_to: vec!["Portugal".into()],
        })
        .values()
        .unwrap();
        assert_eq!(
            relocation[0].0,
            PreferenceValue::Relocation {
                willing: true,
                only_to: vec!["Portugal".into()]
            }
        );
        assert!(
            input(SetCommand::Relocation {
                answer: YesNo::No,
                only_to: vec!["Portugal".into()],
            })
            .values()
            .is_err()
        );
        assert_eq!(
            input(SetCommand::UnknownPay {
                policy: ShowHide::Hide
            })
            .values()
            .unwrap()[0]
                .0,
            PreferenceValue::UnknownPay { show: false }
        );
        let company = input(SetCommand::Company {
            kind: "founder-led".into(),
            stance: StanceArg::Want,
        })
        .values()
        .unwrap();
        assert_eq!(
            company[0].0,
            PreferenceValue::Company {
                company: CompanyTrait::FounderLed
            }
        );
    }
}
