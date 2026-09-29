//! Configuration: the application's (shared with `narrow mcp`), plus the
//! command-line spelling of the log format.

pub use jobhunt_app::config::*;

/// `--log-format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogFormatArg {
    /// Human-readable lines.
    Text,
    /// One JSON object per line.
    Json,
}

impl From<LogFormatArg> for LogFormat {
    fn from(value: LogFormatArg) -> Self {
        match value {
            LogFormatArg::Text => LogFormat::Text,
            LogFormatArg::Json => LogFormat::Json,
        }
    }
}
