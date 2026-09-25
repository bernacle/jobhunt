//! The `jobhunt` command.

mod claims;
mod config;
mod find;
mod init;
mod logging;
mod preferences;
mod profile;
mod profile_args;
mod profile_render;
mod render;
mod show;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};

use crate::config::{LoadedConfig, LogFormat, Paths};

/// High-signal job discovery.
#[derive(Debug, Parser)]
#[command(name = "jobhunt", version, about, propagate_version = true)]
struct Cli {
    /// Config file to use instead of the default location.
    #[arg(long, global = true, env = "JOBHUNT_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,

    /// SQLite database file to use instead of the configured one.
    #[arg(long, global = true, env = "JOBHUNT_DATABASE", value_name = "PATH")]
    database: Option<PathBuf>,

    /// Show more logs on stderr (-v: progress, -vv: debug, -vvv: trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Log format on stderr.
    #[arg(long, global = true, value_enum, value_name = "FORMAT")]
    log_format: Option<LogFormat>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Fetch jobs from the configured sources, save them locally, and show them.
    Find(find::FindArgs),
    /// Show everything stored about one job: every source listing it and its history.
    Show(show::ShowArgs),
    /// Import your resume (PDF, .txt or .md) into your career profile. Run it
    /// again after updating the resume; your edits and decisions are kept.
    Init(init::InitArgs),
    /// Show, correct, export or import your career profile.
    Profile(profile::ProfileArgs),
    /// Review the evidence behind your profile: list, confirm or reject claims.
    #[command(alias = "claim")]
    Claims(claims::ClaimsArgs),
    /// What you want next: roles, pay, location, companies, domains, work style.
    #[command(alias = "prefs", alias = "preference")]
    Preferences(preferences::PreferencesArgs),
    /// Show where JobHunt keeps its files and the effective configuration.
    Config,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            let mut stderr = anstream::stderr().lock();
            let red = anstyle::Style::new()
                .bold()
                .fg_color(Some(anstyle::AnsiColor::Red.into()));
            let _ = writeln!(stderr, "{red}error:{red:#} {error}");
            for cause in error.chain().skip(1) {
                let _ = writeln!(stderr, "  caused by: {cause}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let paths = Paths::platform();
    let loaded = config::load(
        cli.config.as_deref(),
        cli.database.as_deref(),
        paths.as_ref(),
    )?;
    let format = cli.log_format.unwrap_or(loaded.config.logging.format);
    logging::init(cli.verbose, &loaded.config.logging, format)?;
    tracing::debug!(
        config_file = ?loaded.file,
        database = %loaded.database.display(),
        "configuration loaded"
    );

    match cli.command {
        Command::Find(args) => find::run(args, &loaded, cli.verbose).await,
        Command::Show(args) => show::run(args, &loaded).await,
        Command::Init(args) => init::run(args, &loaded).await,
        Command::Profile(args) => profile::run(args, &loaded).await,
        Command::Claims(args) => claims::run(args, &loaded).await,
        Command::Preferences(args) => preferences::run(args, &loaded).await,
        Command::Config => show_config(&loaded),
    }
}

fn show_config(loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let mut out = anstream::stdout().lock();
    let config_file = match (&loaded.file, &loaded.default_file) {
        (Some(file), _) => file.display().to_string(),
        (None, Some(default)) => format!("{} (not found, using defaults)", default.display()),
        (None, None) => "(none)".to_owned(),
    };
    let effective =
        toml::to_string_pretty(&loaded.config).context("could not render the configuration")?;
    writeln!(out, "Config file: {config_file}")?;
    writeln!(out, "Database:    {}", loaded.database.display())?;
    writeln!(out)?;
    writeln!(out, "# Effective configuration")?;
    write!(out, "{effective}")?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_find_arguments() {
        let cli = Cli::try_parse_from([
            "jobhunt",
            "find",
            "rust",
            "backend",
            "-n",
            "5",
            "--source",
            "ashby:linear",
            "-vv",
        ])
        .unwrap();
        assert_eq!(cli.verbose, 2);
        let Command::Find(args) = cli.command else {
            panic!("expected find");
        };
        assert_eq!(args.query, ["rust", "backend"]);
        assert_eq!(args.limit, 5);
        assert_eq!(
            args.sources[0],
            find::SourceArg::Key("ashby:linear".parse().unwrap())
        );
        assert!(!args.offline);
    }

    #[test]
    fn parses_show_and_url_sources() {
        let cli = Cli::try_parse_from(["jobhunt", "show", "job_02e51190085f8a9a0772e845ddd9f329"])
            .unwrap();
        assert!(matches!(cli.command, Command::Show(_)));
        let cli = Cli::try_parse_from([
            "jobhunt",
            "find",
            "--source",
            "https://www.notion.com/careers",
        ])
        .unwrap();
        let Command::Find(args) = cli.command else {
            panic!("expected find");
        };
        assert!(matches!(args.sources[0], find::SourceArg::Url(_)));
    }

    #[test]
    fn parses_profile_commands() {
        let cli = Cli::try_parse_from(["jobhunt", "init", "resume.pdf"]).unwrap();
        assert!(matches!(cli.command, Command::Init(_)));
        let cli = Cli::try_parse_from(["jobhunt", "profile"]).unwrap();
        let Command::Profile(args) = cli.command else {
            panic!("expected profile");
        };
        assert!(args.command.is_none());
        let cli = Cli::try_parse_from([
            "jobhunt",
            "profile",
            "edit",
            "experience",
            "exp_12",
            "--title",
            "Staff Engineer",
            "--start",
            "2021-03",
            "--end",
            "none",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::Profile(_)));
        let cli = Cli::try_parse_from(["jobhunt", "claim", "confirm", "clm_1", "clm_2"]).unwrap();
        assert!(matches!(cli.command, Command::Claims(_)));
        let cli = Cli::try_parse_from(["jobhunt", "claims", "--state", "review"]).unwrap();
        assert!(matches!(cli.command, Command::Claims(_)));
        let cli = Cli::try_parse_from([
            "jobhunt",
            "preferences",
            "add",
            "I want small product teams",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::Preferences(_)));
        let cli = Cli::try_parse_from([
            "jobhunt",
            "prefs",
            "set",
            "compensation",
            "--minimum",
            "120k",
            "--currency",
            "USD",
        ])
        .unwrap();
        assert!(matches!(cli.command, Command::Preferences(_)));
        assert!(
            Cli::try_parse_from([
                "jobhunt",
                "profile",
                "edit",
                "experience",
                "exp_1",
                "--start",
                "soon"
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_malformed_source_keys() {
        assert!(Cli::try_parse_from(["jobhunt", "find", "--source", "linear"]).is_err());
    }
}
