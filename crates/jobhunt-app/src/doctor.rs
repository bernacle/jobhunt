//! `jobhunt doctor`: where JobHunt keeps things, what is stored, and
//! whether an MCP client can use it.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use jobhunt_storage::StoreStats;

use crate::LocalApp;
use crate::discover::RefreshReason;
use crate::error::AppError;

/// What `jobhunt doctor` reports.
#[derive(Debug, Clone)]
pub struct Diagnostics {
    pub config_file: Option<PathBuf>,
    pub default_config_file: Option<PathBuf>,
    pub database: PathBuf,
    pub stats: StoreStats,
    /// Configured sources (careers pages not counted).
    pub sources: usize,
    pub careers_pages: usize,
    pub freshness: RefreshReason,
    pub profile: Option<ProfileSummary>,
}

/// The profile, counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileSummary {
    pub experiences: usize,
    pub claims: usize,
    pub usable_claims: usize,
    pub needs_review: usize,
    pub preferences: usize,
}

impl LocalApp {
    pub async fn doctor(&self, now: DateTime<Utc>) -> Result<Diagnostics, AppError> {
        let loaded = self.loaded();
        let stats = self.store().stats().await?;
        let profile = self.profiles().load().await?.map(|data| ProfileSummary {
            experiences: data.visible_experiences().len(),
            claims: data.claims.len(),
            usable_claims: data
                .claims
                .iter()
                .filter(|c| data.standing(c).is_usable())
                .count(),
            needs_review: data.review_queue().len(),
            preferences: data.preferences.iter().filter(|p| p.active).count(),
        });
        let sources = self
            .config()
            .sources
            .specs()
            .map_err(|e| AppError::Config(e.to_string()))?
            .len();
        Ok(Diagnostics {
            config_file: loaded.file.clone(),
            default_config_file: loaded.default_file.clone(),
            database: loaded.database.clone(),
            stats,
            sources,
            careers_pages: self.config().sources.careers.len(),
            freshness: self.freshness(now).await?,
            profile,
        })
    }
}
