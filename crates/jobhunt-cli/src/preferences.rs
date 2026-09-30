//! `narrow preferences`: what the user wants next.
//!
//! `show` (the default) leads with the taste profile: what Narrow
//! understands about the kind of role and company the person wants, and
//! their practical constraints, kept apart. `describe` is the one question
//! that fills it ("What kind of job are you looking for?"); `confirm`,
//! `correct` and `remove` are the person's decisions about it. `add`,
//! `set` and `remove` keep working on the structured preferences as
//! before.

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
use jobhunt_app::taste_profile::{
    PolarityInput, TasteAction, TasteItemView, TasteProfileView, TasteUpdateResult,
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
    /// What Narrow understands you want, and your practical constraints
    /// (the default).
    Show {
        /// Also every structured preference and statement, with ids.
        #[arg(long)]
        all: bool,
    },
    /// Say what kind of job you're looking for, in a few words. Replaces
    /// your previous description; Narrow reads it into a short summary for
    /// you to confirm or correct.
    Describe {
        #[arg(required = true, value_name = "WORDS")]
        words: Vec<String>,
    },
    /// Confirm the summary ("looks right"), or the given statements
    /// (taste_…).
    Confirm { ids: Vec<String> },
    /// Correct one statement of the summary: new words, and/or
    /// `--polarity avoid` (or prefer, open, neutral for "doesn't matter").
    Correct {
        id: String,
        #[arg(value_name = "WORDS")]
        words: Vec<String>,
        #[arg(long, value_enum)]
        polarity: Option<PolarityArg>,
    },
    /// Read your description again (after configuring a model, say),
    /// keeping every correction.
    Reinterpret,
    /// Say what you want in your own words. The statement is kept as
    /// written; what Narrow understands from it is saved as preferences.
    Add {
        #[arg(required = true, value_name = "STATEMENT")]
        words: Vec<String>,
    },
    /// Set one preference precisely.
    #[command(subcommand)]
    Set(SetCommand),
    /// Remove a preference (pref_…), a statement and what was read from it
    /// (stmt_…), or a statement of the summary (taste_…).
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
pub enum PolarityArg {
    Prefer,
    Open,
    Avoid,
    /// It doesn't matter to you.
    Neutral,
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
    match args
        .command
        .unwrap_or(PreferencesCommand::Show { all: false })
    {
        PreferencesCommand::Show { all } => {
            let data = app.profiles().load_or_new(now).await?;
            let result = match app.taste_profile().await {
                Ok(view) => taste(&mut out, &view, !all),
                Err(jobhunt_app::AppError::NoProfile) => Ok(()),
                Err(e) => return Err(e.into()),
            };
            finish(result.and_then(|()| {
                if all {
                    let title = crate::render::TITLE;
                    writeln!(out, "{title}Details{title:#}")?;
                    profile_render::preferences(&mut out, &data)
                } else {
                    Ok(())
                }
            }))
        }
        PreferencesCommand::Describe { words } => {
            let result = app
                .review_taste(
                    &TasteAction::Describe {
                        text: words.join(" "),
                    },
                    now,
                )
                .await?;
            finish(update(&mut out, &result))
        }
        PreferencesCommand::Confirm { ids } => {
            let result = app.review_taste(&TasteAction::Confirm { ids }, now).await?;
            finish(update(&mut out, &result))
        }
        PreferencesCommand::Correct {
            id,
            words,
            polarity,
        } => {
            if words.is_empty() && polarity.is_none() {
                bail!("give new words, --polarity, or both");
            }
            let action = TasteAction::Correct {
                id,
                text: Some(words.join(" ")).filter(|w| !w.trim().is_empty()),
                polarity: polarity.map(|p| match p {
                    PolarityArg::Prefer => PolarityInput::Prefer,
                    PolarityArg::Open => PolarityInput::Open,
                    PolarityArg::Avoid => PolarityInput::Avoid,
                    PolarityArg::Neutral => PolarityInput::Neutral,
                }),
            };
            let result = app.review_taste(&action, now).await?;
            finish(update(&mut out, &result))
        }
        PreferencesCommand::Reinterpret => {
            let result = app.review_taste(&TasteAction::Reinterpret, now).await?;
            finish(update(&mut out, &result))
        }
        PreferencesCommand::Remove { id } if id.trim().starts_with("taste_") => {
            let result = app
                .review_taste(&TasteAction::Remove { id: id.clone() }, now)
                .await?;
            finish(
                writeln!(out, "Removed {}; Narrow won't read it again.", short(&id))
                    .and_then(|()| taste(&mut out, &result.profile, true)),
            )
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

/// The answer of a change: what happened, then the profile.
fn update(out: &mut impl Write, result: &TasteUpdateResult) -> std::io::Result<()> {
    // Doubts about practical constraints matter (must have, or nice to
    // have?); the rest is the summary's to settle.
    let doubts: Vec<&str> = result
        .preferences
        .iter()
        .flat_map(|p| &p.uncertain)
        .filter(|u| matches!(u.category.as_str(), "location" | "compensation"))
        .map(|u| u.value.as_str())
        .collect();
    if !doubts.is_empty() {
        writeln!(
            out,
            "Check these constraints (must have, or nice to have?): {}. See narrow preferences show --all\n",
            doubts.join("; ")
        )?;
    }
    if !result.changed {
        writeln!(out, "Nothing changed.")?;
    }
    taste(out, &result.profile, true)
}

fn item(out: &mut impl Write, i: &TasteItemView) -> std::io::Result<()> {
    use crate::render::DIM;
    writeln!(
        out,
        "  {}  {DIM}{} · {}{DIM:#}",
        i.text,
        short(&i.id),
        i.basis
    )
}

/// The taste profile: what Narrow understands, what the person avoids,
/// their practical constraints, what was learned. `hint` adds what to do
/// next.
fn taste(out: &mut impl Write, v: &TasteProfileView, hint: bool) -> std::io::Result<()> {
    use crate::render::{DIM, TITLE};
    match (&v.looking_for, v.looking_for_source.as_deref()) {
        (Some(words), Some("description")) => {
            writeln!(out, "{TITLE}What you're looking for{TITLE:#}")?;
            writeln!(out, "  “{words}”\n")?;
        }
        (Some(words), _) => {
            writeln!(out, "{TITLE}What you told Narrow before{TITLE:#}")?;
            writeln!(out, "  “{words}”")?;
            writeln!(
                out,
                "  {DIM}Describe what you're looking for: narrow preferences describe \"…\"{DIM:#}\n"
            )?;
        }
        (None, _) => {
            writeln!(
                out,
                "What kind of job are you looking for? Say it in a few words:\n  narrow preferences describe \"Small technical teams, backend or platform work, startups. No early-career roles.\"\n"
            )?;
        }
    }
    let items: Vec<&TasteItemView> = v.understood.iter().flat_map(|l| &l.items).collect();
    if !items.is_empty() {
        writeln!(out, "{TITLE}What Narrow understands{TITLE:#}")?;
        for i in items {
            item(out, i)?;
        }
        writeln!(out)?;
    }
    let avoided: Vec<&TasteItemView> = v.avoid.iter().flat_map(|l| &l.items).collect();
    if !avoided.is_empty() {
        writeln!(out, "{TITLE}You tend to avoid{TITLE:#}")?;
        for i in avoided {
            item(out, i)?;
        }
        writeln!(out)?;
    }
    if !v.unsure.is_empty() {
        writeln!(out, "{TITLE}Not sure yet{TITLE:#}")?;
        for i in &v.unsure {
            item(out, i)?;
        }
        writeln!(out)?;
    }
    if !v.constraints.is_empty() {
        writeln!(out, "{TITLE}Practical constraints{TITLE:#}")?;
        for c in &v.constraints {
            writeln!(out, "  {}  {DIM}{}{DIM:#}", c.text, c.layer)?;
        }
        writeln!(out)?;
    }
    if !v.learned.is_empty() {
        writeln!(out, "{TITLE}Learned over time{TITLE:#}")?;
        for i in &v.learned {
            let direction = if i.polarity == "avoid" {
                "less of"
            } else {
                "more of"
            };
            writeln!(
                out,
                "  {direction} {}  {DIM}{} · {} confidence{DIM:#}",
                i.text.to_lowercase(),
                short(&i.id),
                i.confidence
            )?;
        }
        writeln!(out)?;
    }
    if let Some(i) = &v.interpretation {
        for a in &i.ambiguities {
            writeln!(out, "  ? {a}")?;
        }
        if let Some(note) = &i.note {
            writeln!(out, "  {DIM}{note}{DIM:#}")?;
        }
    }
    if hint {
        if v.needs_confirmation {
            writeln!(
                out,
                "{DIM}Does this look right? narrow preferences confirm · correct a line: narrow preferences correct <id> \"…\" (or --polarity neutral) · remove: narrow preferences remove <id>{DIM:#}"
            )?;
        }
        writeln!(
            out,
            "{DIM}Read by {}. Every setting: narrow preferences show --all{DIM:#}",
            v.reader.name
        )?;
    }
    Ok(())
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
