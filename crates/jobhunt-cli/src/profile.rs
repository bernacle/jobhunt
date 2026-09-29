//! `narrow profile`: inspect, correct, export and import the profile.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};
use chrono::Utc;
use clap::Subcommand;
use jobhunt_profile::{
    BasicsEdit, EducationEdit, ExperienceEdit, PartialDate, ProfileExport, ProjectEdit, Removal,
};

use crate::config::LoadedConfig;
use crate::profile_args::{Clearable, Employment, date, finish};
use crate::profile_render::{self, date_or_unknown, short};

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ProfileArgs {
    /// Also list every bullet, when each skill was last used, and all gaps.
    #[arg(long)]
    pub all: bool,

    #[command(subcommand)]
    pub command: Option<ProfileCommand>,
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Show the profile (the default).
    Show {
        #[arg(long)]
        all: bool,
    },
    /// Correct a value. Fields you edit are never overwritten by a resume re-import.
    #[command(subcommand)]
    Edit(EditTarget),
    /// Add something your resume does not say.
    #[command(subcommand)]
    Add(AddTarget),
    /// Remove something: records you added are deleted; imported ones are
    /// rejected (hidden, and not brought back by a re-import).
    Remove {
        /// An id or unique id prefix (exp_…, proj_…, edu_…, skill_…, clm_…, pref_…, stmt_…).
        id: String,
    },
    /// Write the whole profile as versioned JSON.
    Export {
        /// Write to this file instead of standard output.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Replace the profile with an exported JSON file. The file is checked
    /// first; nothing changes if it is invalid.
    Import {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Required when a profile already exists (export it first to keep a copy).
        #[arg(long)]
        replace: bool,
    },
    /// Recent changes to the profile.
    History {
        #[arg(short = 'n', long, default_value_t = 20, value_name = "N")]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum EditTarget {
    /// Name, headline, location, summary.
    Basics(BasicsArgs),
    /// An experience (exp_…).
    Experience {
        id: String,
        #[command(flatten)]
        fields: ExperienceArgs,
    },
    /// A project (proj_…).
    Project {
        id: String,
        #[command(flatten)]
        fields: ProjectArgs,
    },
    /// An education entry (edu_…).
    Education {
        id: String,
        #[command(flatten)]
        fields: EducationArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum AddTarget {
    Experience(ExperienceArgs),
    Project(ProjectArgs),
    Education(EducationArgs),
    /// A skill (or confirm one already in the profile).
    Skill {
        name: String,
        #[arg(long)]
        category: Option<String>,
    },
}

#[derive(Debug, Clone, clap::Args)]
pub struct BasicsArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub headline: Option<String>,
    #[arg(long)]
    pub location: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct ExperienceArgs {
    /// Company; `none` clears it.
    #[arg(long)]
    pub company: Option<Clearable<String>>,
    #[arg(long)]
    pub title: Option<Clearable<String>>,
    /// full-time, part-time, contract, freelance, internship.
    #[arg(long)]
    pub employment: Option<Clearable<Employment>>,
    /// YYYY or YYYY-MM.
    #[arg(long, value_parser = date)]
    pub start: Option<Clearable<PartialDate>>,
    /// YYYY or YYYY-MM.
    #[arg(long, value_parser = date)]
    pub end: Option<Clearable<PartialDate>>,
    /// The position continues today.
    #[arg(long, conflicts_with = "not_current")]
    pub current: bool,
    #[arg(long)]
    pub not_current: bool,
    #[arg(long)]
    pub location: Option<Clearable<String>>,
    #[arg(long)]
    pub summary: Option<Clearable<String>>,
    /// Technologies used there (comma-separated).
    #[arg(long = "tech", value_delimiter = ',')]
    pub technologies: Vec<String>,
}

impl ExperienceArgs {
    fn edit(self) -> ExperienceEdit {
        ExperienceEdit {
            company: self.company.map(|c| c.0),
            title: self.title.map(|c| c.0),
            employment: self.employment.map(|c| c.0.map(|e| e.0)),
            start: self.start.map(|c| c.0),
            end: self.end.map(|c| c.0),
            current: if self.current {
                Some(true)
            } else if self.not_current {
                Some(false)
            } else {
                None
            },
            location: self.location.map(|c| c.0),
            summary: self.summary.map(|c| c.0),
            technologies: self.technologies,
        }
    }
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct ProjectArgs {
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<Clearable<String>>,
    #[arg(long)]
    pub role: Option<Clearable<String>>,
    #[arg(long)]
    pub url: Option<Clearable<String>>,
    #[arg(long, value_parser = date)]
    pub start: Option<Clearable<PartialDate>>,
    #[arg(long, value_parser = date)]
    pub end: Option<Clearable<PartialDate>>,
    #[arg(long, conflicts_with = "not_current")]
    pub current: bool,
    #[arg(long)]
    pub not_current: bool,
    /// The experience (exp_…) it was part of; `none` detaches it.
    #[arg(long = "for", value_name = "EXPERIENCE")]
    pub experience: Option<Clearable<String>>,
    #[arg(long = "tech", value_delimiter = ',')]
    pub technologies: Vec<String>,
}

impl ProjectArgs {
    fn edit(self) -> ProjectEdit {
        ProjectEdit {
            name: self.name,
            description: self.description.map(|c| c.0),
            role: self.role.map(|c| c.0),
            url: self.url.map(|c| c.0),
            start: self.start.map(|c| c.0),
            end: self.end.map(|c| c.0),
            current: if self.current {
                Some(true)
            } else if self.not_current {
                Some(false)
            } else {
                None
            },
            experience: self.experience.map(|c| c.0),
            technologies: self.technologies,
        }
    }
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct EducationArgs {
    #[arg(long)]
    pub institution: Option<String>,
    #[arg(long)]
    pub degree: Option<Clearable<String>>,
    #[arg(long)]
    pub field: Option<Clearable<String>>,
    #[arg(long, value_parser = date)]
    pub start: Option<Clearable<PartialDate>>,
    #[arg(long, value_parser = date)]
    pub end: Option<Clearable<PartialDate>>,
    #[arg(long)]
    pub current: bool,
}

impl EducationArgs {
    fn edit(self) -> EducationEdit {
        EducationEdit {
            institution: self.institution,
            degree: self.degree.map(|c| c.0),
            field: self.field.map(|c| c.0),
            start: self.start.map(|c| c.0),
            end: self.end.map(|c| c.0),
            current: self.current.then_some(true),
        }
    }
}

pub async fn run(args: ProfileArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let app = crate::local::open(loaded).await?;
    let result = execute(args, &app).await;
    app.close().await;
    result
}

async fn execute(args: ProfileArgs, app: &jobhunt_app::LocalApp) -> anyhow::Result<ExitCode> {
    let service = app.profiles();
    let now = Utc::now();
    let mut out = anstream::stdout().lock();
    match args
        .command
        .unwrap_or(ProfileCommand::Show { all: args.all })
    {
        ProfileCommand::Show { all } => {
            let data = service.require().await?;
            finish(profile_render::profile(&mut out, &data, all || args.all))
        }
        ProfileCommand::Edit(target) => {
            let message = match target {
                EditTarget::Basics(b) => {
                    service
                        .edit_basics(
                            BasicsEdit {
                                name: b.name,
                                headline: b.headline,
                                location: b.location,
                                summary: b.summary,
                            },
                            now,
                        )
                        .await?;
                    "Updated your basics.".to_owned()
                }
                EditTarget::Experience { id, fields } => {
                    let e = service.edit_experience(&id, fields.edit(), now).await?;
                    format!(
                        "Updated {} ({}). Your edits are kept across resume re-imports.",
                        e.label(),
                        short(e.id)
                    )
                }
                EditTarget::Project { id, fields } => {
                    let p = service.edit_project(&id, fields.edit(), now).await?;
                    format!("Updated project {} ({}).", p.name, short(p.id))
                }
                EditTarget::Education { id, fields } => {
                    let e = service.edit_education(&id, fields.edit(), now).await?;
                    format!("Updated {} ({}).", e.institution, short(e.id))
                }
            };
            finish(writeln!(out, "{message}"))
        }
        ProfileCommand::Add(target) => {
            let message = match target {
                AddTarget::Experience(fields) => {
                    let e = service.add_experience(fields.edit(), now).await?;
                    format!(
                        "Added {} ({}), from {}.",
                        e.label(),
                        short(e.id),
                        date_or_unknown(e.start)
                    )
                }
                AddTarget::Project(fields) => {
                    let p = service.add_project(fields.edit(), now).await?;
                    format!("Added project {} ({}).", p.name, short(p.id))
                }
                AddTarget::Education(fields) => {
                    let e = service.add_education(fields.edit(), now).await?;
                    format!("Added {} ({}).", e.institution, short(e.id))
                }
                AddTarget::Skill { name, category } => {
                    let s = service.add_skill(&name, category.as_deref(), now).await?;
                    format!("Added skill {} ({}).", s.name, short(s.id))
                }
            };
            finish(writeln!(out, "{message}"))
        }
        ProfileCommand::Remove { id } => {
            let message = match service.remove(&id, now).await? {
                Removal::Deleted(id) => format!("Removed {}.", short(id)),
                Removal::Rejected(id) => format!(
                    "Rejected {}: it stays hidden and is not used, even if a re-imported resume contains it.",
                    short(id)
                ),
            };
            finish(writeln!(out, "{message}"))
        }
        ProfileCommand::Export { output } => {
            let export = service
                .export(now, Some(format!("narrow {}", env!("CARGO_PKG_VERSION"))))
                .await?;
            let json = export.to_json().context("could not encode the profile")?;
            match output {
                Some(path) => {
                    std::fs::write(&path, format!("{json}\n"))
                        .with_context(|| format!("could not write {}", path.display()))?;
                    eprintln!(
                        "Exported {} experiences, {} claims and {} preferences to {}.",
                        export.experiences.len(),
                        export.claims.len(),
                        export.preferences.len(),
                        path.display()
                    );
                    Ok(ExitCode::SUCCESS)
                }
                None => finish(writeln!(out, "{json}")),
            }
        }
        ProfileCommand::Import { file, replace } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("could not read {}", file.display()))?;
            let export = ProfileExport::parse(&text).with_context(|| {
                format!(
                    "{} was not imported; your profile is unchanged",
                    file.display()
                )
            })?;
            if let Some(existing) = service.load().await?
                && !existing.is_empty()
                && !replace
            {
                bail!(
                    "a profile already exists; pass --replace to overwrite it \
                     (narrow profile export -o backup.json keeps a copy)"
                );
            }
            let data = service.import_export(export, now).await?;
            finish(writeln!(
                out,
                "Imported the profile from {}: {} experiences, {} claims, {} preferences.",
                file.display(),
                data.experiences.len(),
                data.claims.len(),
                data.preferences.iter().filter(|p| p.active).count()
            ))
        }
        ProfileCommand::History { limit } => {
            let events = service.history(limit).await?;
            finish(profile_render::history(&mut out, &events))
        }
    }
}
