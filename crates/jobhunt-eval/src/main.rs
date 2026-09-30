//! `narrow-eval`: runs Narrow's evaluation benchmarks and prints their
//! reports.
//!
//! ```text
//! cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark
//! cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark --details
//! cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark --candidate senior-startup-generalist
//! ```
//!
//! Exits 2 when the fixtures are unusable. Cases failing their judgment
//! are the current ranker's quality, reported rather than fatal, unless
//! `--strict` is given (for when ranking work turns them into a gate).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use jobhunt_eval::report::{Options, render};
use jobhunt_eval::{Fixtures, Metrics, default_dir, run};

#[derive(Parser)]
#[command(name = "narrow-eval", about = "Narrow's evaluation benchmarks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the recommendation-quality benchmark against the current ranker.
    RecommendationBenchmark {
        /// Fixture directory (default: the crate's own fixtures).
        #[arg(long)]
        fixtures: Option<PathBuf>,
        /// Only this candidate.
        #[arg(long)]
        candidate: Option<String>,
        /// Every case's reasons, not only the failing ones.
        #[arg(long)]
        details: bool,
        /// Exit 1 when any case fails its judgment.
        #[arg(long)]
        strict: bool,
    },
}

fn main() -> ExitCode {
    let Command::RecommendationBenchmark {
        fixtures,
        candidate,
        details,
        strict,
    } = Cli::parse().command;
    let dir = fixtures.unwrap_or_else(default_dir);
    let mut loaded = match Fixtures::load(&dir) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("narrow-eval: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(id) = candidate {
        if loaded.candidate(&id).is_none() {
            eprintln!("narrow-eval: no candidate {id}");
            return ExitCode::from(2);
        }
        loaded.candidates.retain(|c| c.id == id);
    }
    let result = match run(&loaded) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("narrow-eval: {e}");
            return ExitCode::from(2);
        }
    };
    let text = render(&loaded, &result, Options { details });
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(text.as_bytes()).is_err() {
        return ExitCode::FAILURE;
    }
    let failures = Metrics::of(result.candidates.iter().flat_map(|c| &c.cases)).failures;
    if strict && failures > 0 {
        eprintln!("narrow-eval: {failures} cases fail their judgment");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
