//! Process entry points: `narrow server`, `narrow migrate`,
//! `narrow worker discovery|verification|notify`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use jobhunt_storage::postgres::{Keyring, PgStore, SchemaStatus};
use tokio::net::TcpListener;

use crate::api::{self, ApiState};
use crate::config::{CloudConfig, ConfigProblems, Role};
use crate::usage::UsageLog;
use crate::worker::{self, DiscoverySummary, VerificationSummary};

/// Why a cloud process could not run.
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error(transparent)]
    Config(#[from] ConfigProblems),
    #[error("the database failed")]
    Storage(#[from] jobhunt_jobs::StorageError),
    #[error(transparent)]
    App(#[from] jobhunt_app::AppError),
    #[error("could not listen on {addr}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("the server stopped abnormally")]
    Serve(#[source] std::io::Error),
}

/// Connects, retrying for a while: on a fresh deploy the database may
/// still be starting.
pub async fn connect(config: &CloudConfig, keys: Keyring) -> Result<PgStore, CloudError> {
    let url = config
        .database_url()
        .ok_or_else(|| ConfigProblems(vec!["DATABASE_URL is not set".into()]))?;
    let mut attempt = 0u32;
    loop {
        match PgStore::connect(url, &config.db, keys.clone()).await {
            Ok(store) => return Ok(store),
            Err(error) if attempt < 5 => {
                attempt += 1;
                tracing::warn!(attempt, error = %jobhunt_core::ErrorChain(&error), "database not reachable yet; retrying");
                tokio::time::sleep(Duration::from_secs(u64::from(attempt) * 2)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Keys for processes that never read private data (migrations and
/// workers): the configured ones if set, else a throwaway key.
fn keys_or_ephemeral(config: &CloudConfig) -> Keyring {
    config.keyring().unwrap_or_else(|_| Keyring::ephemeral())
}

/// `narrow migrate`: applies pending migrations (Railway's pre-deploy
/// command). Safe to run concurrently.
pub async fn migrate(config: CloudConfig) -> Result<SchemaStatus, CloudError> {
    let config = config.require(Role::Migrate)?;
    let store = connect(&config, keys_or_ephemeral(&config)).await?;
    let status = store.migrate().await?;
    tracing::info!(
        applied = status.applied,
        known = status.known,
        "migrations applied"
    );
    store.close().await;
    Ok(status)
}

/// `narrow worker discovery`: one scheduled discovery run.
pub async fn discovery_worker(
    config: CloudConfig,
    budget: Duration,
) -> Result<DiscoverySummary, CloudError> {
    let config = config.require(Role::Worker)?;
    let store = connect(&config, keys_or_ephemeral(&config)).await?;
    store.migrate().await?;
    let result = worker::discover(&store, &config, budget).await;
    store.close().await;
    Ok(result?)
}

/// `narrow worker verification`: one scheduled re-verification run.
pub async fn verification_worker(config: CloudConfig) -> Result<VerificationSummary, CloudError> {
    let config = config.require(Role::Worker)?;
    let store = connect(&config, keys_or_ephemeral(&config)).await?;
    store.migrate().await?;
    let result = worker::verify(&store, &config).await;
    store.close().await;
    Ok(result?)
}

/// `narrow worker notify`: one notification run (retries, then new strong
/// recommendations).
pub async fn notification_worker(
    config: CloudConfig,
) -> Result<crate::notify::NotifySummary, CloudError> {
    let config = config.require(Role::Notifier)?;
    let keys = config
        .keyring()
        .map_err(|e| ConfigProblems(vec![format!("JOBHUNT_ENCRYPTION_KEYS: {e}")]))?;
    let sender = crate::email::sender(config.email.as_ref())
        .map_err(|e| ConfigProblems(vec![format!("email: {e}")]))?
        .ok_or_else(|| ConfigProblems(vec!["email is not configured".into()]))?;
    let store = connect(&config, keys).await?;
    store.migrate().await?;
    let result = crate::notify::notify(&store, &config, sender.as_ref()).await;
    store.close().await;
    Ok(result?)
}

/// Resolves when the process is asked to stop (SIGTERM from Railway on a
/// new deploy, or Ctrl-C).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down: finishing requests in flight");
}

/// `narrow server`: the HTTP API and hosted MCP until stopped.
pub async fn serve(config: CloudConfig) -> Result<(), CloudError> {
    let config = config.require(Role::Server)?;
    let keys = config
        .keyring()
        .map_err(|e| ConfigProblems(vec![format!("JOBHUNT_ENCRYPTION_KEYS: {e}")]))?;
    let store = connect(&config, keys).await?;
    if config.migrate_on_start {
        store.migrate().await?;
    }
    let (usage, writer) = if config.usage_events {
        let (log, handle) = UsageLog::start(store.clone());
        (log, Some(handle))
    } else {
        (UsageLog::disabled(), None)
    };
    let bind = config.bind;
    let state = ApiState::new(store.clone(), Arc::new(config), usage)?;
    let listener = match TcpListener::bind(bind).await {
        Ok(l) => l,
        // No IPv6 on this host: fall back to IPv4.
        Err(_) if bind.is_ipv6() => {
            let v4 = SocketAddr::from(([0, 0, 0, 0], bind.port()));
            TcpListener::bind(v4)
                .await
                .map_err(|source| CloudError::Bind { addr: v4, source })?
        }
        Err(source) => return Err(CloudError::Bind { addr: bind, source }),
    };
    tracing::info!(addr = %listener.local_addr().map_err(CloudError::Serve)?, "JobHunt Cloud listening");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(CloudError::Serve)?;
    // The router (and its usage log sender) is gone: flush usage, close.
    if let Some(writer) = writer {
        let _ = tokio::time::timeout(Duration::from_secs(5), writer).await;
    }
    store.close().await;
    tracing::info!("stopped");
    Ok(())
}
