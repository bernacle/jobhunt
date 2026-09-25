//! The `jobhunt` command.

mod config;
mod find;
mod logging;
mod render;

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
        Command::Find(args) => find::run(args, &loaded).await,
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
        assert_eq!(args.sources[0].to_string(), "ashby:linear");
        assert!(!args.offline);
    }

    #[test]
    fn rejects_malformed_source_keys() {
        assert!(Cli::try_parse_from(["jobhunt", "find", "--source", "linear"]).is_err());
    }
}
