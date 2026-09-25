//! Source adapters.
//!
//! Each adapter implements [`jobhunt_core::Source`] with
//! `Record = JobPosting` and owns everything specific to its source: the
//! endpoint, the payload format, and the mapping into the canonical model.
//!
//! Adding a source (Greenhouse, Lever, ...) means:
//! 1. a module with the adapter, its raw payload types and conversion;
//! 2. a variant on [`SourceSpec`] plus its arm in [`SourceSpec::from_key`] and
//!    [`SourceSpec::build`];
//! 3. a config list on [`SourcesConfig`];
//! 4. fixture tests under `tests/fixtures/<source>/`.

pub mod ashby;
pub mod http;

use jobhunt_core::{SourceError, SourceKey, SourceKeyError};
use jobhunt_jobs::JobSource;
use serde::{Deserialize, Serialize};

pub use ashby::{AshbyBoard, AshbySource};
pub use http::{HttpClient, HttpClientError, HttpSettings};

/// Which sources discovery reads by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    /// Ashby job boards. Defaults to a small set of software companies.
    #[serde(default = "ashby::default_boards")]
    pub ashby: Vec<AshbyBoard>,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            ashby: ashby::default_boards(),
        }
    }
}

impl SourcesConfig {
    /// Every configured source, validated.
    pub fn specs(&self) -> Result<Vec<SourceSpec>, SourceKeyError> {
        self.ashby.iter().cloned().map(SourceSpec::ashby).collect()
    }
}

/// A fully described source instance that can be turned into an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceSpec {
    Ashby { key: SourceKey, board: AshbyBoard },
}

impl SourceSpec {
    pub fn ashby(board: AshbyBoard) -> Result<Self, SourceKeyError> {
        let key = SourceKey::new(ashby::KIND, &board.board)?;
        Ok(Self::Ashby { key, board })
    }

    /// Describes an unconfigured source from its key alone (for example
    /// `ashby:posthog` given on the command line).
    pub fn from_key(key: &SourceKey) -> Result<Self, UnknownSourceKind> {
        match key.kind() {
            ashby::KIND => Ok(Self::Ashby {
                key: key.clone(),
                board: AshbyBoard {
                    board: key.instance().to_owned(),
                    company: None,
                },
            }),
            other => Err(UnknownSourceKind(other.to_owned())),
        }
    }

    pub fn key(&self) -> &SourceKey {
        match self {
            Self::Ashby { key, .. } => key,
        }
    }

    pub fn build(&self, http: &HttpClient) -> Result<Box<JobSource>, SourceError> {
        match self {
            Self::Ashby { key, board } => Ok(Box::new(AshbySource::new(
                key.clone(),
                board.clone(),
                http.clone(),
            )?)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown source kind {0:?} (supported: {kinds})", kinds = SUPPORTED_KINDS.join(", "))]
pub struct UnknownSourceKind(pub String);

/// Source kinds this build can read.
pub const SUPPORTED_KINDS: &[&str] = &[ashby::KIND];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_yields_valid_specs() {
        let specs = SourcesConfig::default().specs().unwrap();
        assert!(!specs.is_empty());
        assert!(specs.iter().all(|s| s.key().kind() == "ashby"));
    }

    #[test]
    fn specs_from_keys() {
        let key: SourceKey = "ashby:posthog".parse().unwrap();
        let spec = SourceSpec::from_key(&key).unwrap();
        assert_eq!(spec.key(), &key);

        let unknown: SourceKey = "workday:acme".parse().unwrap();
        let err = SourceSpec::from_key(&unknown).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown source kind \"workday\" (supported: ashby)"
        );
    }

    #[test]
    fn config_rejects_invalid_board_names() {
        let config = SourcesConfig {
            ashby: vec![AshbyBoard {
                board: "has space".into(),
                company: None,
            }],
        };
        assert!(config.specs().is_err());
    }
}
