//! Opening the local application and small output helpers every command
//! shares.

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use jobhunt_app::{LoadedConfig, LocalApp, Progress, ProgressEvent};

use crate::render::plural;

/// The application over the configured database (created and migrated on
/// first use).
pub async fn open(loaded: &LoadedConfig) -> anyhow::Result<LocalApp> {
    Ok(LocalApp::open(loaded.clone()).await?)
}

/// Runs a command against the application, closing it afterwards.
macro_rules! with_app {
    ($loaded:expr, |$app:ident| $body:expr) => {{
        let $app = crate::local::open($loaded).await?;
        let result: anyhow::Result<std::process::ExitCode> = async { $body }.await;
        $app.close().await;
        result
    }};
}
pub(crate) use with_app;

/// Progress on stderr, next to the logs: stdout stays for results.
pub struct StderrProgress {
    /// Say "verifying" once for a whole search instead of once per job.
    verify_once: bool,
    verified: AtomicBool,
}

impl StderrProgress {
    /// Every event.
    pub fn each() -> Self {
        Self {
            verify_once: false,
            verified: AtomicBool::new(false),
        }
    }

    /// One "verifying" line for a whole search.
    pub fn search() -> Self {
        Self {
            verify_once: true,
            verified: AtomicBool::new(false),
        }
    }
}

impl Progress for StderrProgress {
    fn note(&self, event: ProgressEvent) {
        let mut err = anstream::stderr().lock();
        let now = jobhunt_app::now();
        let _ = match event {
            ProgressEvent::Refreshing { sources, reason } => writeln!(
                err,
                "Refreshing {} ({})…",
                plural(sources as u64, "source", "sources"),
                reason.describe(now)
            ),
            ProgressEvent::Verifying { .. } if self.verify_once => {
                if self.verified.swap(true, Ordering::Relaxed) {
                    return;
                }
                writeln!(err, "Verifying the best candidates at their sources…")
            }
            ProgressEvent::Verifying { records, sources } => writeln!(
                err,
                "Verifying {} at {}…",
                plural(records as u64, "source record", "source records"),
                plural(sources as u64, "source", "sources")
            ),
        };
    }
}

/// Ignores a closed pipe (`narrow find | head`).
pub fn finish(result: io::Result<()>, what: &str) -> anyhow::Result<ExitCode> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.with_context(|| format!("could not write {what}"))?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Prints a view as pretty JSON (the same structure the MCP tools return).
pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<ExitCode> {
    let json = serde_json::to_string_pretty(value).context("could not encode JSON")?;
    let mut out = io::stdout().lock();
    finish(writeln!(out, "{json}"), "the JSON output")
}
