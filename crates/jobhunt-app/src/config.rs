//! Configuration, shared by every front-end (`narrow` commands and
//! `narrow mcp` read exactly the same file and database).
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
    pub verification: VerificationConfig,
    pub sources: SourcesConfig,
    pub cloud: CloudClientConfig,
    pub ai: AiConfig,
}

/// An optional model for reading the person's words into their taste
/// profile and, when `review_fit` is on, for reviewing the fit of the
/// ranking's shortlist. Without it (the default) Narrow reads words and
/// ranks with its built-in rules, offline. See `jobhunt_ai` and
/// `jobhunt_ranking::review` for what is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AiConfig {
    /// `anthropic` (the Anthropic API) or `openai` (any OpenAI-compatible
    /// server: OpenAI, Ollama, vLLM, LM Studio…). Unset: no model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The model (Anthropic default: `claude-opus-5-5`; OpenAI-compatible
    /// servers must name one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Where the API is, for a proxy or a self-hosted server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The environment variable holding the API key (default
    /// `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`). Keys never go in this
    /// file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Per-request timeout, in seconds.
    pub timeout_secs: u64,
    /// Also review the fit of each ranking's shortlist with the model (a
    /// few calls per new job, each (profile, posting) reviewed once; see
    /// `jobhunt_ranking::review`). Off by default: it costs model calls.
    pub review_fit: bool,
    /// The model that reviews fit, when not `model`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_model: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            base_url: None,
            api_key_env: None,
            timeout_secs: 40,
            review_fit: false,
            review_model: None,
        }
    }
}

impl AiConfig {
    /// The configured model, `None` when none is configured, or why the
    /// configuration can't be used (a missing key: Narrow then reads with
    /// its rules and says so).
    pub fn model(&self) -> Result<Option<jobhunt_ai::ModelInterpreter>, String> {
        let Some(config) = self.model_config(self.model.as_deref())? else {
            return Ok(None);
        };
        jobhunt_ai::ModelInterpreter::new(config)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// The fit reviewer, when `review_fit` is on and a model is configured
    /// (`Err`: why it can't be used; ranking then relies on its rules).
    pub fn fit_reviewer(&self) -> Result<Option<jobhunt_ai::ModelFitReviewer>, String> {
        if !self.review_fit {
            return Ok(None);
        }
        let model = self.review_model.as_deref().or(self.model.as_deref());
        let Some(config) = self.model_config(model)? else {
            return Err("review_fit needs an [ai] provider".into());
        };
        jobhunt_ai::ModelFitReviewer::new(config)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    fn model_config(&self, model: Option<&str>) -> Result<Option<jobhunt_ai::ModelConfig>, String> {
        let Some(provider) = self.provider.as_deref().filter(|p| !p.trim().is_empty()) else {
            return Ok(None);
        };
        let parsed = jobhunt_ai::Provider::parse(provider)
            .ok_or_else(|| format!("unknown [ai] provider {provider:?} (anthropic or openai)"))?;
        let env = self
            .api_key_env
            .clone()
            .unwrap_or_else(|| parsed.default_key_env().to_owned());
        let key = std::env::var(&env).ok().filter(|k| !k.trim().is_empty());
        let mut config =
            jobhunt_ai::ModelConfig::new(provider, model, self.base_url.as_deref(), key, &env)
                .map_err(|e| e.to_string())?;
        config.timeout = Duration::from_secs(self.timeout_secs.max(1));
        Ok(Some(config))
    }
}

/// JobHunt Cloud, as this machine's client (`narrow login`, `jobhunt
/// sync`). Nothing here is needed to use JobHunt locally.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudClientConfig {
    /// The server `narrow login` signs in to when `--server` is not
    /// given (`JOBHUNT_CLOUD_URL` overrides it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Requests in flight to one host at the same time, across all sources.
    pub max_requests_per_host: usize,
    /// Per-request timeout, in seconds.
    pub request_timeout_secs: u64,
    /// Retries for transient HTTP failures.
    pub max_retries: u32,
    /// Hours a source's "not modified" answer may stand in for a full
    /// fetch. After that the listing is re-read in full. 0 always re-reads.
    pub revalidate_after_hours: u32,
    /// Hours the stored jobs of a source count as fresh. `narrow find`
    /// (and the MCP `search_jobs` tool) refresh sources read longer ago than
    /// this, and otherwise work from what is stored. 0 refreshes every time.
    pub refresh_after_hours: u32,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            concurrency: 8,
            max_requests_per_host: 4,
            request_timeout_secs: 30,
            max_retries: 2,
            revalidate_after_hours: 24,
            refresh_after_hours: 12,
        }
    }
}

impl DiscoveryConfig {
    pub fn http_settings(&self) -> HttpSettings {
        HttpSettings {
            timeout: Duration::from_secs(self.request_timeout_secs.max(1)),
            max_retries: self.max_retries,
            max_per_host: self.max_requests_per_host.max(1),
            ..HttpSettings::default()
        }
    }

    pub fn validator_max_age(&self) -> chrono::Duration {
        chrono::Duration::hours(i64::from(self.revalidate_after_hours))
    }

    pub fn refresh_after(&self) -> chrono::Duration {
        chrono::Duration::hours(i64::from(self.refresh_after_hours))
    }
}

/// When a verification is recent enough to rely on (see
/// `jobhunt_jobs::verification::FreshnessPolicy`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VerificationConfig {
    /// Minutes an attempt is reused by `narrow verify` instead of asking
    /// the source again (`--force` always asks).
    pub reuse_minutes: u32,
    /// Hours a successful verification counts as fresh.
    pub fresh_hours: u32,
    /// Hours after which a verification is stale and the job is not
    /// recommended until verified again.
    pub stale_hours: u32,
    /// Source records verified at the same time.
    pub concurrency: usize,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            reuse_minutes: 15,
            fresh_hours: 24,
            stale_hours: 72,
            concurrency: 4,
        }
    }
}

impl VerificationConfig {
    pub fn policy(&self) -> jobhunt_jobs::verification::FreshnessPolicy {
        let fresh = chrono::Duration::hours(i64::from(self.fresh_hours));
        jobhunt_jobs::verification::FreshnessPolicy {
            reuse_within: chrono::Duration::minutes(i64::from(self.reuse_minutes)),
            fresh_for: fresh,
            stale_after: chrono::Duration::hours(i64::from(self.stale_hours)).max(fresh),
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
        let sources = &loaded.config.sources;
        assert!(!sources.ashby.is_empty());
        assert!(!sources.greenhouse.is_empty());
        assert!(!sources.lever.is_empty());
        assert!(!sources.yc.is_empty());
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
        assert!(
            loaded.config.sources.greenhouse.is_empty(),
            "configuring sources replaces every default"
        );
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
        let keys: Vec<String> = loaded
            .config
            .sources
            .specs()
            .unwrap()
            .iter()
            .map(|s| s.key().to_string())
            .collect();
        assert_eq!(
            keys,
            [
                "ashby:linear",
                "ashby:posthog",
                "greenhouse:anthropic",
                "greenhouse:stripe",
                "lever:spotify",
                "lever:qonto",
                "yc:posthog"
            ]
        );
        assert_eq!(loaded.config.sources.careers.len(), 1);
        assert_eq!(loaded.config.discovery, DiscoveryConfig::default());
        assert_eq!(loaded.config.verification, VerificationConfig::default());
        assert_eq!(loaded.config.logging, LoggingConfig::default());
    }

    #[test]
    fn verification_policy_from_config() {
        let config: AppConfig = toml::from_str(
            "[verification]\nreuse_minutes = 5\nfresh_hours = 12\nstale_hours = 1\n",
        )
        .unwrap();
        let policy = config.verification.policy();
        assert_eq!(policy.reuse_within, chrono::Duration::minutes(5));
        assert_eq!(policy.fresh_for, chrono::Duration::hours(12));
        assert_eq!(
            policy.stale_after,
            chrono::Duration::hours(12),
            "never before fresh ends"
        );
        assert_eq!(
            AppConfig::default().verification.policy(),
            jobhunt_jobs::verification::FreshnessPolicy::default()
        );
    }

    #[test]
    fn effective_config_serializes_to_toml() {
        let text = toml::to_string_pretty(&AppConfig::default()).unwrap();
        let parsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed, AppConfig::default());
    }
}
