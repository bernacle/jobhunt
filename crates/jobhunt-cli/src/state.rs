//! `jobhunt export` and `jobhunt import`: everything that is yours, as one
//! versioned file (see [`jobhunt_app::state`]). `jobhunt profile export`
//! remains the profile alone.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use jobhunt_app::state::StateExport;

use crate::config::LoadedConfig;
use crate::local::{finish, with_app};
use crate::render::plural;

#[derive(Debug, clap::Args)]
pub struct ExportArgs {
    /// Write to this file instead of standard output.
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// A file written by `jobhunt export`.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Replace the existing profile with the file's (required when both
    /// have one; export first to keep a copy). Feedback is always merged.
    #[arg(long)]
    pub replace: bool,
}

pub async fn export(args: ExportArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let generator = Some(format!("jobhunt {}", env!("CARGO_PKG_VERSION")));
        let state = app.export_state(jobhunt_app::now(), generator).await?;
        let json = state.to_json()?;
        match &args.output {
            Some(path) => {
                // Write next to the target, then rename, so a failure never
                // leaves half a file behind.
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, format!("{json}\n"))
                    .with_context(|| format!("could not write {}", tmp.display()))?;
                std::fs::rename(&tmp, path)
                    .with_context(|| format!("could not write {}", path.display()))?;
                eprintln!(
                    "Exported {}, {} and {} to {}.",
                    if state.profile.is_some() {
                        "your profile"
                    } else {
                        "no profile"
                    },
                    plural(
                        state.feedback.len() as u64,
                        "piece of feedback",
                        "pieces of feedback"
                    ),
                    plural(
                        state.jobs.len() as u64,
                        "job it is about",
                        "jobs it is about"
                    ),
                    path.display()
                );
                Ok(ExitCode::SUCCESS)
            }
            None => finish(writeln!(std::io::stdout().lock(), "{json}"), "the export"),
        }
    })
}

pub async fn import(args: ImportArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let json = std::fs::read_to_string(&args.file)
        .with_context(|| format!("could not read {}", args.file.display()))?;
    let state = StateExport::parse(&json)?;
    with_app!(loaded, |app| {
        let imported = app
            .import_state(state, args.replace, jobhunt_app::now())
            .await?;
        let s = imported.stored;
        let mut out = anstream::stdout().lock();
        finish(
            writeln!(
                out,
                "Imported {}{} ({} already here) on {}, and {} ({} already here).",
                if s.profile { "your profile, " } else { "" },
                plural(
                    s.feedback_added as u64,
                    "piece of feedback",
                    "pieces of feedback"
                ),
                s.feedback_present,
                plural(
                    imported.opportunities as u64,
                    "opportunity",
                    "opportunities"
                ),
                plural(s.jobs_added as u64, "job record", "job records"),
                s.jobs_present
            ),
            "the import summary",
        )
    })
}
