//! Configuration.
//!
//! Everything has a default, so no config file is needed. Values are resolved
//! in this order (later wins):
//!
//! 1. built-in defaults;
//! 2. the config file: `--config`/`JOBHUNT_CONFIG` if given (must exist),
//!    otherwise `<config dir>/jobhunt/config.toml` if it exists;
//! 3. `JOBHUNT_DATABASE` / `--database` for the database path.

use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use jobhunt_sources::{HttpSettings, SourcesConfig};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const DATABASE_FILE_NAME: &str = "jobhunt.db";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub storage: StorageConfig,
    pub logging: LoggingConfig,
    pub discovery: DiscoveryConfig,
    pub sources: SourcesConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// SQLite database file. Relative paths in a config file are resolved
    /// against the config file's directory. Defaults to the platform data
    /// directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// A `tracing` filter directive, e.g. `"error"` or `"warn,jobhunt=debug"`.
    /// The default is `"error"` because the CLI reports failed sources and
    /// unreadable postings itself; logs are for diagnostics (`-v`).
    pub level: String,
    pub format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "error".to_owned(),
            format: LogFormat::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable lines.
    #[default]
    Text,
    /// One JSON object per line.
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Sources fetched at the same time.
    pub concurrency: usize,
    /// Per-request timeout, in seconds.
    pub request_timeout_secs: u64,
    /// Retries for transient HTTP failures.
    pub max_retries: u32,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            request_timeout_secs: 30,
            max_retries: 2,
        }
    }
}

impl DiscoveryConfig {
    pub fn http_settings(&self) -> HttpSettings {
        HttpSettings {
            timeout: Duration::from_secs(self.request_timeout_secs.max(1)),
            max_retries: self.max_retries,
            ..HttpSettings::default()
        }
    }
}

/// Platform locations for JobHunt's files.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    /// `~/Library/Application Support/jobhunt` on macOS, `$XDG_CONFIG_HOME/jobhunt`
    /// and `$XDG_DATA_HOME/jobhunt` on Linux, `%APPDATA%\jobhunt` on Windows.
    pub fn platform() -> Option<Self> {
        let dirs = ProjectDirs::from("", "", "jobhunt")?;
        Some(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
        })
    }

    pub fn default_config_file(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    pub fn default_database(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE_NAME)
    }
}

/// The resolved configuration plus where it came from.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: AppConfig,
    /// The file that was read, if any.
    pub file: Option<PathBuf>,
    /// Where a config file would be looked up by default.
    pub default_file: Option<PathBuf>,
    pub database: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config file {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config file {}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not determine a data directory for this platform; pass --database <PATH>")]
    NoDataDir,
}

/// Loads configuration. `explicit_file` and `database_override` come from
/// the command line or environment.
pub fn load(
    explicit_file: Option<&Path>,
    database_override: Option<&Path>,
    paths: Option<&Paths>,
) -> Result<LoadedConfig, ConfigError> {
    let default_file = paths.map(Paths::default_config_file);
    let file = match explicit_file {
        Some(path) => Some(path.to_path_buf()),
        None => default_file.clone().filter(|p| p.is_file()),
    };

    let mut config = match &file {
        Some(path) => read_file(path)?,
        None => AppConfig::default(),
    };

    let database = match (database_override, &config.storage.database) {
        (Some(path), _) => path.to_path_buf(),
        (None, Some(path)) => {
            let base = file.as_deref().and_then(Path::parent);
            match base {
                Some(base) if path.is_relative() => base.join(path),
                _ => path.clone(),
            }
        }
        (None, None) => paths
            .map(Paths::default_database)
            .ok_or(ConfigError::NoDataDir)?,
    };
    config.storage.database = Some(database.clone());

    Ok(LoadedConfig {
        config,
        file,
        default_file,
        database,
    })
}

fn read_file(path: &Path) -> Result<AppConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
        }
    }

    #[test]
    fn works_without_any_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(None, None, Some(&paths(dir.path()))).unwrap();
        assert_eq!(loaded.file, None);
        assert_eq!(loaded.database, dir.path().join("data").join("jobhunt.db"));
        assert_eq!(loaded.config.logging.level, "error");
        assert!(!loaded.config.sources.ashby.is_empty());
    }

    #[test]
    fn default_config_file_is_used_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(
            paths.default_config_file(),
            r#"
                [storage]
                database = "db/jobs.db"

                [logging]
                level = "info"
                format = "json"

                [discovery]
                concurrency = 2

                [[sources.ashby]]
                board = "posthog"
                company = "PostHog"
            "#,
        )
        .unwrap();

        let loaded = load(None, None, Some(&paths)).unwrap();
        assert_eq!(loaded.file, Some(paths.default_config_file()));
        assert_eq!(loaded.database, paths.config_dir.join("db/jobs.db"));
        assert_eq!(loaded.config.logging.format, LogFormat::Json);
        assert_eq!(loaded.config.discovery.concurrency, 2);
        assert_eq!(loaded.config.discovery.request_timeout_secs, 30);
        assert_eq!(loaded.config.sources.ashby.len(), 1);
        assert_eq!(loaded.config.sources.ashby[0].board, "posthog");
    }

    #[test]
    fn database_override_wins() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.toml");
        std::fs::write(&file, "[storage]\ndatabase = \"/elsewhere.db\"\n").unwrap();
        let override_path = dir.path().join("override.db");
        let loaded = load(Some(&file), Some(&override_path), None).unwrap();
        assert_eq!(loaded.database, override_path);
    }

    #[test]
    fn explicit_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(Some(&dir.path().join("nope.toml")), None, None).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "[storage]\ndatabse = \"typo.db\"\n").unwrap();
        let err = load(Some(&file), None, Some(&paths(dir.path()))).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn no_data_dir_requires_an_explicit_database() {
        assert!(matches!(
            load(None, None, None).unwrap_err(),
            ConfigError::NoDataDir
        ));
        let loaded = load(None, Some(Path::new("x.db")), None).unwrap();
        assert_eq!(loaded.database, PathBuf::from("x.db"));
    }

    #[test]
    fn example_config_file_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config.example.toml");
        let loaded = load(Some(&example), None, Some(&paths(dir.path()))).unwrap();
        let boards: Vec<_> = loaded
            .config
            .sources
            .ashby
            .iter()
            .map(|b| b.board.as_str())
            .collect();
        assert_eq!(boards, ["linear", "posthog"]);
        assert_eq!(loaded.config.discovery, DiscoveryConfig::default());
        assert_eq!(loaded.config.logging, LoggingConfig::default());
    }

    #[test]
    fn effective_config_serializes_to_toml() {
        let text = toml::to_string_pretty(&AppConfig::default()).unwrap();
        let parsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed, AppConfig::default());
    }
}
