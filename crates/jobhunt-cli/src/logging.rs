//! Structured logging setup. Logs go to stderr so they never mix with the
//! command's output on stdout.

use tracing_subscriber::EnvFilter;

use crate::config::{LogFormat, LoggingConfig};

/// Environment variables holding a filter directive, in priority order.
const FILTER_ENV_VARS: [&str; 2] = ["JOBHUNT_LOG", "RUST_LOG"];

#[derive(Debug, thiserror::Error)]
#[error("invalid log filter {directive:?}")]
pub struct LoggingError {
    directive: String,
    #[source]
    source: tracing_subscriber::filter::ParseError,
}

/// Picks the filter directive: an environment variable wins, then `-v`
/// flags, then the configured level.
pub fn filter_directive(verbosity: u8, config: &LoggingConfig) -> String {
    for var in FILTER_ENV_VARS {
        if let Ok(value) = std::env::var(var)
            && !value.trim().is_empty()
        {
            return value;
        }
    }
    match verbosity {
        0 => config.level.clone(),
        1 => "warn,jobhunt=info".to_owned(),
        2 => "info,jobhunt=debug".to_owned(),
        _ => "debug,jobhunt=trace".to_owned(),
    }
}

pub fn init(verbosity: u8, config: &LoggingConfig, format: LogFormat) -> Result<(), LoggingError> {
    let directive = filter_directive(verbosity, config);
    let filter = EnvFilter::try_new(&directive).map_err(|source| LoggingError {
        directive: directive.clone(),
        source,
    })?;
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    // `try_init` only fails if a subscriber is already installed, which
    // cannot happen in the binary; ignoring it keeps tests robust.
    let _ = match format {
        LogFormat::Text => builder.with_target(false).compact().try_init(),
        LogFormat::Json => builder.json().flatten_event(true).try_init(),
    };
    Ok(())
}
