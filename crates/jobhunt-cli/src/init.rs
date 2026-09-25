//! `jobhunt init <resume>`: import (or re-import) a resume into the profile.

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use jobhunt_profile::ProfileService;
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};

use crate::config::LoadedConfig;
use crate::profile_args::{finish, open_store};
use crate::profile_render;

#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// Your resume: a PDF, or a .txt/.md file. Run it again after updating
    /// the resume; your edits and confirmations are kept.
    #[arg(value_name = "RESUME")]
    pub resume: PathBuf,
}

pub async fn run(args: InitArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let file = ResumeFile::read(&args.resume)?;
    let parser = DeterministicParser;
    let parsed = parser.parse(&file.text);
    let now = Utc::now();
    let store = open_store(loaded).await?;
    let service = ProfileService::new(&store);
    let result = service
        .import_resume(file.source_document(parser.name(), now), &parsed, now)
        .await;
    store.close().await;
    let (report, data) = result?;
    let mut out = anstream::stdout().lock();
    finish(profile_render::import_summary(
        &mut out,
        &data,
        &report,
        file.text.pages,
        file.text.removed.len(),
    ))
}
