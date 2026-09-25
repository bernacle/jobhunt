//! The local JobHunt application: one composition of configuration,
//! storage and the domain services, shared by every front-end.
//!
//! `jobhunt` commands and the MCP server (`jobhunt mcp`) are both thin
//! interfaces over [`LocalApp`]: they parse arguments, call a use case
//! here, and present the answer. So both read the same configuration, open
//! the same SQLite database, and reach the same decisions: a job rejected
//! from the terminal is rejected for an MCP client too, with the same
//! reason, and the next search reflects it either way.
//!
//! The use cases:
//!
//! * [`LocalApp::find`] ([`shortlist`]): refresh when stale, rank, verify
//!   the best candidates, and return the few worth the person's time;
//! * [`LocalApp::resolve`] ([`resolve`]): `opp_…` / `job_…` / short ids to
//!   a logical opportunity and its source records;
//! * [`LocalApp::inspect`] ([`inspect`]): one opportunity, with its
//!   verification, eligibility, decision brief and pipeline state;
//! * [`LocalApp::verify`] ([`verify`]): ask the authoritative sources;
//! * [`LocalApp::record_feedback`] ([`feedback`]): save, reject, applied, …
//!   (repeats are not recorded twice);
//! * [`LocalApp::update_preferences`] ([`preferences`]);
//! * [`LocalApp::profile_view`] ([`profile_view`]): the profile, without
//!   contact details;
//! * [`LocalApp::application_context`] ([`context`]): evidence a client may
//!   use to help with an application, only what the evidence policy allows;
//! * [`LocalApp::export_state`] / [`LocalApp::import_state`] ([`state`]);
//! * [`LocalApp::doctor`] ([`doctor`]).
//!
//! [`views`] holds the typed, serializable answers (the MCP tools'
//! structured content and output schemas).
//!
//! Nothing here prints: progress is reported through [`Progress`], and
//! results are returned. That keeps an MCP server's stdout for the
//! protocol alone.

pub mod config;
pub mod context;
pub mod discover;
pub mod doctor;
pub mod error;
pub mod feedback;
pub mod inspect;
pub mod preferences;
pub mod profile_view;
pub mod resolve;
pub mod shortlist;
pub mod state;
pub mod verify;
pub mod views;

use chrono::{DateTime, Utc};
use jobhunt_jobs::verification::FreshnessPolicy;
use jobhunt_profile::ProfileService;
use jobhunt_ranking::{RankingService, RuleReader};
use jobhunt_storage::SqliteJobStore;
use tokio::sync::Mutex;

pub use config::{AppConfig, LoadedConfig, Paths};
pub use discover::{Refresh, RefreshMode, RefreshReason, SourceArg};
pub use error::{AppError, ErrorKind};
pub use resolve::{Opportunity, short_id};
pub use shortlist::{FindRequest, Found, SearchResults};

/// Something a long use case is doing, for front-ends that show progress
/// (the CLI prints it on stderr; the MCP server logs it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// Reading sources.
    Refreshing {
        sources: usize,
        reason: RefreshReason,
    },
    /// Asking authoritative sources about listings.
    Verifying { records: usize, sources: usize },
}

/// Receives [`ProgressEvent`]s.
pub trait Progress: Send + Sync {
    fn note(&self, event: ProgressEvent);
}

/// Ignores progress.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quiet;

impl Progress for Quiet {
    fn note(&self, _event: ProgressEvent) {}
}

/// The local application: configuration plus the open database.
///
/// Safe to share between concurrent requests (`Arc<LocalApp>`): the SQLite
/// store is a connection pool in WAL mode with a busy timeout, and
/// read-modify-write use cases (feedback, preferences, imports) take a
/// process-wide write lock so concurrent requests cannot interleave their
/// checks and writes. Other processes (a CLI command next to an MCP
/// server) are covered by SQLite's own locking and the profile's
/// optimistic revisions: a lost race is reported as
/// [`ErrorKind::Conflict`], never silently merged.
pub struct LocalApp {
    loaded: LoadedConfig,
    store: SqliteJobStore,
    pub(crate) writes: Mutex<()>,
}

impl std::fmt::Debug for LocalApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApp")
            .field("database", &self.loaded.database)
            .finish_non_exhaustive()
    }
}

impl LocalApp {
    /// Opens (creating and migrating if needed) the configured database.
    pub async fn open(loaded: LoadedConfig) -> Result<Self, AppError> {
        let store = SqliteJobStore::open(&loaded.database)
            .await
            .map_err(|e| AppError::storage("opening the local database", e))?;
        Ok(Self::with_store(loaded, store))
    }

    /// An application over an already open store (tests).
    pub fn with_store(loaded: LoadedConfig, store: SqliteJobStore) -> Self {
        Self {
            loaded,
            store,
            writes: Mutex::new(()),
        }
    }

    /// Closes the database, flushing the write-ahead log.
    pub async fn close(self) {
        self.store.close().await;
    }

    pub fn loaded(&self) -> &LoadedConfig {
        &self.loaded
    }

    pub fn config(&self) -> &AppConfig {
        &self.loaded.config
    }

    pub fn store(&self) -> &SqliteJobStore {
        &self.store
    }

    /// When a verification is fresh, stale, or reused.
    pub fn policy(&self) -> FreshnessPolicy {
        self.config().verification.policy()
    }

    /// The ranking use cases, with the configured freshness policy.
    pub fn ranking(&self) -> RankingService<'_, SqliteJobStore> {
        RankingService::new(&self.store, &RuleReader).with_policy(self.policy())
    }

    /// The profile use cases for the local profile.
    pub fn profiles(&self) -> ProfileService<'_, SqliteJobStore> {
        ProfileService::new(&self.store)
    }

    /// The opportunity an id (`opp_…`, `job_…`, or a unique prefix of
    /// either) refers to, with every source record.
    pub async fn resolve(&self, id: &str) -> Result<Opportunity, AppError> {
        resolve::resolve(&self.store, id).await
    }

    /// Runs a read-modify-write use case under the process-wide write
    /// lock.
    pub(crate) async fn exclusive<T, F>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let _guard = self.writes.lock().await;
        f.await
    }
}

/// The current time. One place, so every use case in a request agrees.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}
