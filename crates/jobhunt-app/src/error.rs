//! What can go wrong in a use case, in terms a person (or an MCP client)
//! can act on.
//!
//! Every error has a stable [`ErrorKind`] code (`unknown_opportunity`,
//! `no_profile`, ...), a message that says what happened, and, where there
//! is one, the next step. Storage failures keep their cause for local
//! diagnostics (`Debug`, the CLI's "caused by" chain) but
//! [`AppError::public_message`] never includes it, so paths and driver
//! details are not sent to MCP clients.

use std::fmt;

use jobhunt_core::BoxError;

/// Stable error codes, shared by the CLI and the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// No stored job or opportunity has that id.
    UnknownOpportunity,
    /// A short id matches more than one opportunity.
    AmbiguousId,
    /// There is no career profile yet.
    NoProfile,
    /// Nothing has been discovered yet.
    NoJobs,
    /// A preference could not be understood or is not valid.
    InvalidPreference,
    /// The request itself is malformed (an id that is not an id, a limit
    /// out of range, an unknown source).
    InvalidArguments,
    /// Every source failed during a refresh.
    SourceUnavailable,
    /// Verification could not be attempted.
    VerificationUnavailable,
    /// Something else changed the same state at the same moment.
    Conflict,
    /// An import file is not a valid JobHunt state file.
    InvalidImport,
    /// The configuration is invalid.
    Config,
    /// The local database failed.
    Storage,
    /// No valid credentials: sign in (`narrow login`) or send a token.
    Unauthenticated,
    /// JobHunt Cloud could not be reached, or answered with an error.
    CloudUnavailable,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownOpportunity => "unknown_opportunity",
            Self::AmbiguousId => "ambiguous_id",
            Self::NoProfile => "no_profile",
            Self::NoJobs => "no_jobs",
            Self::InvalidPreference => "invalid_preference",
            Self::InvalidArguments => "invalid_arguments",
            Self::SourceUnavailable => "source_unavailable",
            Self::VerificationUnavailable => "verification_unavailable",
            Self::Conflict => "conflict",
            Self::InvalidImport => "invalid_import",
            Self::Config => "config",
            Self::Storage => "storage",
            Self::Unauthenticated => "unauthenticated",
            Self::CloudUnavailable => "cloud_unavailable",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A use case failed.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("no stored opportunity or job has the id {input}")]
    UnknownOpportunity { input: String },
    #[error("{input} matches more than one opportunity ({}); type more of the id", candidates.join(", "))]
    AmbiguousId {
        input: String,
        candidates: Vec<String>,
    },
    #[error("No profile found. Start with:\n  narrow init resume.pdf")]
    NoProfile,
    #[error("No discovered opportunities yet. Run:\n  narrow find --refresh")]
    NoJobs,
    #[error("{0}")]
    InvalidPreference(String),
    #[error("{0}")]
    InvalidArguments(String),
    #[error("could not reach any source ({failed} failed): {detail}")]
    SourceUnavailable { failed: usize, detail: String },
    /// An evidence source (GitHub's API) could not be read; the message
    /// says why and what to do.
    #[error("{0}")]
    EvidenceUnavailable(String),
    #[error("{0}")]
    VerificationUnavailable(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    InvalidImport(String),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("{0}")]
    Unauthenticated(String),
    #[error("{0}")]
    CloudUnavailable(String),
    #[error("the local database failed while {operation}")]
    Storage {
        operation: String,
        #[source]
        source: BoxError,
    },
}

impl AppError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::UnknownOpportunity { .. } => ErrorKind::UnknownOpportunity,
            Self::AmbiguousId { .. } => ErrorKind::AmbiguousId,
            Self::NoProfile => ErrorKind::NoProfile,
            Self::NoJobs => ErrorKind::NoJobs,
            Self::InvalidPreference(_) => ErrorKind::InvalidPreference,
            Self::InvalidArguments(_) => ErrorKind::InvalidArguments,
            Self::SourceUnavailable { .. } | Self::EvidenceUnavailable(_) => {
                ErrorKind::SourceUnavailable
            }
            Self::VerificationUnavailable(_) => ErrorKind::VerificationUnavailable,
            Self::Conflict(_) => ErrorKind::Conflict,
            Self::InvalidImport(_) => ErrorKind::InvalidImport,
            Self::Config(_) => ErrorKind::Config,
            Self::Storage { .. } => ErrorKind::Storage,
            Self::Unauthenticated(_) => ErrorKind::Unauthenticated,
            Self::CloudUnavailable(_) => ErrorKind::CloudUnavailable,
        }
    }

    /// The message to show someone who is not at this machine (an MCP
    /// client): the same as `Display`, except that storage failures are
    /// described without their cause, which can name local paths.
    pub fn public_message(&self) -> String {
        match self {
            Self::Storage { .. } => "The local JobHunt database failed. Run `narrow doctor` \
                                     on this machine for details."
                .to_owned(),
            Self::Config(_) => "The JobHunt configuration is invalid. Run `narrow doctor` \
                                on this machine for details."
                .to_owned(),
            other => other.to_string(),
        }
    }

    /// What to do next, when there is something specific.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::UnknownOpportunity { .. } => Some(
                "Use an id from search results (opp_…); `narrow find` or the search_jobs tool lists them.",
            ),
            Self::AmbiguousId { .. } => Some("Use the full id (opp_ followed by 32 characters)."),
            Self::NoProfile => Some(
                "Import a resume with `narrow init <resume>`, or state where you live with \
                 `narrow preferences set location <place>`.",
            ),
            Self::NoJobs => Some(
                "Refresh discovery: `narrow find --refresh`, or search_jobs with refresh \"always\".",
            ),
            Self::SourceUnavailable { .. } => Some(
                "Check the network connection and try again; stored jobs are still available offline.",
            ),
            Self::Conflict(_) => Some("Run the same request again."),
            Self::Unauthenticated(_) => Some("Sign in with `narrow login`."),
            Self::CloudUnavailable(_) => Some(
                "Everything still works offline on this machine; run `narrow sync` again later.",
            ),
            _ => None,
        }
    }

    pub(crate) fn storage(operation: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self::Storage {
            operation: operation.into(),
            source: source.into(),
        }
    }
}

impl From<jobhunt_jobs::StorageError> for AppError {
    fn from(error: jobhunt_jobs::StorageError) -> Self {
        Self::storage("reading or writing jobs", error)
    }
}

impl From<jobhunt_profile::StorageError> for AppError {
    fn from(error: jobhunt_profile::StorageError) -> Self {
        match error {
            jobhunt_profile::StorageError::Conflict { .. } => Self::Conflict(error.to_string()),
            other => Self::storage("reading or writing the profile", other),
        }
    }
}

impl From<jobhunt_profile::ProfileError> for AppError {
    fn from(error: jobhunt_profile::ProfileError) -> Self {
        use jobhunt_profile::ProfileError as E;
        match error {
            E::Storage(e) => e.into(),
            E::NoProfile => Self::NoProfile,
            E::NotFound { .. } => Self::InvalidArguments(error.to_string()),
            E::Ambiguous { what, input } => Self::AmbiguousId {
                input,
                candidates: vec![format!("several {what} records")],
            },
            E::Invalid(message) => Self::InvalidArguments(message),
            E::Export(e) => Self::InvalidImport(e.to_string()),
        }
    }
}

impl From<jobhunt_ranking::RankingError> for AppError {
    fn from(error: jobhunt_ranking::RankingError) -> Self {
        use jobhunt_ranking::RankingError as E;
        match error {
            E::Storage(e) => e.into(),
            E::Profile(e) => e.into(),
            E::NoProfile => Self::NoProfile,
            E::NoRecords => Self::UnknownOpportunity {
                input: "(no source records)".into(),
            },
        }
    }
}

impl From<crate::config::ConfigError> for AppError {
    fn from(error: crate::config::ConfigError) -> Self {
        Self::Config(jobhunt_core::ErrorChain(&error).to_string())
    }
}
