//! Usage events: privacy-conscious product analytics, off the request path.
//!
//! Handlers call [`UsageLog::record`] with an event name and safe metadata
//! (counts, tool names, outcome codes: never what the person wrote, never
//! profile content). Events go through a bounded channel to a background
//! writer that stores them in batches; when the channel is full an event is
//! dropped rather than slowing a request. Usage is not product data: nothing
//! reads it to decide anything.

use std::time::Duration;

use chrono::Utc;
use jobhunt_storage::postgres::{PgStore, UsageEvent, UserId};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const CAPACITY: usize = 4096;
const BATCH: usize = 200;
const FLUSH_EVERY: Duration = Duration::from_secs(2);

/// Records usage events (cheap to clone).
#[derive(Debug, Clone)]
pub struct UsageLog {
    tx: Option<mpsc::Sender<UsageEvent>>,
}

impl UsageLog {
    /// A log that records nothing.
    pub fn disabled() -> Self {
        Self { tx: None }
    }

    /// Starts the background writer. It stops, after writing what is
    /// queued, once every clone of the log is dropped.
    pub fn start(store: PgStore) -> (Self, JoinHandle<()>) {
        let (tx, mut rx) = mpsc::channel::<UsageEvent>(CAPACITY);
        let handle = tokio::spawn(async move {
            let mut batch = Vec::with_capacity(BATCH);
            loop {
                let first = rx.recv().await;
                let Some(first) = first else { break };
                batch.push(first);
                let deadline = tokio::time::sleep(FLUSH_EVERY);
                tokio::pin!(deadline);
                while batch.len() < BATCH {
                    tokio::select! {
                        next = rx.recv() => match next {
                            Some(e) => batch.push(e),
                            None => break,
                        },
                        () = &mut deadline => break,
                    }
                }
                if let Err(error) = store.record_usage(&batch).await {
                    tracing::warn!(%error, events = batch.len(), "could not record usage");
                }
                batch.clear();
            }
        });
        (Self { tx: Some(tx) }, handle)
    }

    /// Records an event. Never blocks; drops it when the writer is behind.
    pub fn record(&self, user: Option<&UserId>, event: &str, metadata: serde_json::Value) {
        let Some(tx) = &self.tx else { return };
        let event = UsageEvent {
            user: user.cloned(),
            at: Utc::now(),
            event: event.to_owned(),
            metadata,
        };
        if tx.try_send(event).is_err() {
            tracing::debug!("usage event dropped");
        }
    }
}
