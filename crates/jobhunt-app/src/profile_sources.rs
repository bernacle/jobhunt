//! Evidence sources beyond the resume: a LinkedIn data export the person
//! downloaded, and their public GitHub account. Both feed the same
//! profile and evidence graph as the resume, by the profile domain's rules
//! ([`jobhunt_profile::import`]); nothing here decides what is evidence.
//!
//! LinkedIn is read from the file only (no LinkedIn API, no login). GitHub
//! is read through its official REST API; a token is optional and only
//! used for the request (never stored).

use std::path::Path;

use chrono::{DateTime, Utc};
use jobhunt_profile::github::RepoSelection;
use jobhunt_profile::{
    ContactKind, ImportReport, Origin, ProfileData, SourceRemoval, source_document_id,
};
use jobhunt_resume::{LinkedinError, LinkedinExport};
use jobhunt_sources::github::parse_login;
use jobhunt_sources::{GithubError, GithubReader, HttpClient};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::profile_edit::TallyView;
use crate::profile_view::ProfileView;

/// Sends every GitHub API request to this base URL instead of
/// `api.github.com`. For offline tests against a local server; not a user
/// setting.
pub const GITHUB_ENDPOINT_OVERRIDE: &str = "JOBHUNT_GITHUB_ENDPOINT";

/// The most a LinkedIn export upload may be. Basic exports are well under
/// a megabyte; complete ones with years of messages can be larger, and
/// only their career files are read.
pub const MAX_LINKEDIN_BYTES: usize = 64 * 1024 * 1024;

/// How to reach GitHub: its API (or a test server) and an optional token.
#[derive(Clone, PartialEq, Eq)]
pub struct GithubAccess {
    pub api: String,
    /// No scopes needed; raises the rate limit and lets language
    /// statistics be read. Used for the requests only.
    pub token: Option<String>,
}

impl std::fmt::Debug for GithubAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GithubAccess")
            .field("api", &self.api)
            .field("token", &self.token.as_ref().map(|_| "set"))
            .finish()
    }
}

impl GithubAccess {
    /// GitHub's API (or [`GITHUB_ENDPOINT_OVERRIDE`] in tests), with
    /// `token`.
    pub fn new(token: Option<String>) -> Self {
        Self::at(std::env::var(GITHUB_ENDPOINT_OVERRIDE).ok(), token)
    }

    /// `api` when given (a test server), else GitHub's.
    pub fn at(api: Option<String>, token: Option<String>) -> Self {
        Self {
            api: api
                .filter(|a| !a.trim().is_empty())
                .unwrap_or_else(|| jobhunt_sources::github::API.to_owned()),
            token: token.filter(|t| !t.trim().is_empty()),
        }
    }
}

/// A source that can be imported and removed besides the resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Linkedin,
    Github,
}

impl EvidenceSource {
    pub fn origin(self) -> Origin {
        match self {
            Self::Linkedin => Origin::Linkedin,
            Self::Github => Origin::Github,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "linkedin" => Some(Self::Linkedin),
            "github" => Some(Self::Github),
            _ => None,
        }
    }
}

/// What importing a LinkedIn export or a GitHub account changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceImportResult {
    /// `linkedin` or `github`.
    pub source: String,
    /// What was imported: the export's file name, or `github.com/<login>`.
    pub label: String,
    /// The profile had no such source before.
    pub first_import: bool,
    /// The same content as the last import: nothing new to read.
    pub unchanged: bool,
    /// What Narrow read ("3 positions", "12 repositories").
    pub read: Vec<String>,
    /// What it deliberately did not use ("2 forks", "38 other files in
    /// the export, never opened").
    pub skipped: Vec<String>,
    pub experiences: TallyView,
    pub projects: TallyView,
    pub education: TallyView,
    pub skills: TallyView,
    pub claims: TallyView,
    /// Confirmed claims found again (still confirmed).
    pub kept_confirmed: usize,
    /// Rejected claims found again (still rejected, never used).
    pub kept_rejected: usize,
    /// Confirmed claims whose wording changed: they need confirming again.
    pub reconfirm: usize,
    /// Confirmed claims no source supports any more.
    pub stale_confirmed: usize,
    /// Records where the person's own edits were kept.
    pub preserved_edits: usize,
    /// Statements that disagree with another source (different dates for
    /// one position); each waits for review.
    pub conflicts: usize,
    /// Doubts about particular rows or records.
    pub notes: Vec<String>,
    /// Parts that could not be read (the rest was imported).
    pub problems: Vec<String>,
    /// Claims now waiting for the person's review.
    pub needs_review: usize,
    /// The profile after the import.
    pub profile: ProfileView,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl SourceImportResult {
    fn of(
        source: EvidenceSource,
        label: String,
        report: &ImportReport,
        read: Vec<String>,
        skipped: Vec<String>,
        problems: Vec<String>,
        data: &ProfileData,
    ) -> Self {
        let profile = ProfileView::of(data);
        Self {
            source: source.origin().as_str().to_owned(),
            label,
            first_import: report.first_import,
            unchanged: report.same_file,
            read,
            skipped,
            experiences: (&report.experiences).into(),
            projects: (&report.projects).into(),
            education: (&report.education).into(),
            skills: (&report.skills).into(),
            claims: (&report.claims).into(),
            kept_confirmed: report.kept_confirmed,
            kept_rejected: report.kept_rejected,
            reconfirm: report.reconfirm,
            stale_confirmed: report.stale_confirmed,
            preserved_edits: report.preserved_edits,
            conflicts: report.conflicts,
            notes: report
                .notes
                .iter()
                .filter(|n| !problems.contains(n))
                .cloned()
                .collect(),
            problems,
            needs_review: profile.needs_review_total,
            profile,
        }
    }
}

/// What taking a source out did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRemovalResult {
    /// `linkedin` or `github`.
    pub source: String,
    /// Records only this source supported, deleted.
    pub records_deleted: usize,
    /// Records other sources (or the person) still support.
    pub records_kept: usize,
    /// Claims only this source supported, deleted.
    pub claims_deleted: usize,
    /// Claims other sources still support (their provenance moved there).
    pub claims_kept: usize,
    /// Claims only this source supported that the person had confirmed,
    /// rejected or rewritten: kept without a source (confirmations need
    /// renewing before use).
    pub decisions_kept: usize,
    pub needs_review: usize,
    pub profile: ProfileView,
}

impl SourceRemovalResult {
    fn of(source: EvidenceSource, removal: &SourceRemoval, data: &ProfileData) -> Self {
        let profile = ProfileView::of(data);
        Self {
            source: source.origin().as_str().to_owned(),
            records_deleted: removal.records_deleted,
            records_kept: removal.records_kept,
            claims_deleted: removal.claims_deleted,
            claims_kept: removal.claims_kept,
            decisions_kept: removal.decisions_kept,
            needs_review: profile.needs_review_total,
            profile,
        }
    }
}

fn linkedin_error(error: LinkedinError) -> AppError {
    match error {
        LinkedinError::Io { .. } => {
            AppError::InvalidArguments(jobhunt_core::ErrorChain(&error).to_string())
        }
        other => AppError::InvalidArguments(other.to_string()),
    }
}

fn github_error(error: GithubError) -> AppError {
    match error {
        GithubError::Unavailable { .. } | GithubError::RateLimited { .. } => {
            AppError::EvidenceUnavailable(error.to_string())
        }
        other => AppError::InvalidArguments(other.to_string()),
    }
}

/// What a LinkedIn import read and left out, for people.
fn linkedin_summary(export: &LinkedinExport) -> (Vec<String>, Vec<String>) {
    let read = export
        .read
        .iter()
        .map(|(file, rows)| {
            let what = jobhunt_resume::linkedin::CATEGORIES
                .iter()
                .find(|c| c.file == file)
                .map_or(file.as_str(), |c| c.what);
            format!("{what} ({})", plural(*rows, "row", "rows"))
        })
        .collect();
    let mut skipped = Vec::new();
    if export.unopened > 0 {
        skipped.push(format!(
            "{} in the export, never opened (messages, connections, contacts, …)",
            plural(export.unopened, "other file", "other files")
        ));
    }
    (read, skipped)
}

/// What a GitHub import used and left out, for people.
fn github_summary(selection: &RepoSelection) -> (Vec<String>, Vec<String>) {
    let mut read = vec![plural(
        selection.kept.len(),
        "public repository you own",
        "public repositories you own",
    )];
    if selection.archived > 0 {
        read.push(format!("{} of them archived", selection.archived));
    }
    if !selection.languages.is_empty() {
        read.push(format!(
            "main languages: {}",
            selection.languages.join(", ")
        ));
    }
    if selection.orgs > 0 {
        read.push(format!(
            "{} (kept as evidence, not as employment)",
            plural(
                selection.orgs,
                "public organization",
                "public organizations"
            )
        ));
    }
    let mut skipped = Vec::new();
    let mut add = |n: usize, one: &str, many: &str| {
        if n > 0 {
            skipped.push(plural(n, one, many));
        }
    };
    add(selection.forks, "fork", "forks");
    add(selection.empty, "empty repository", "empty repositories");
    add(
        selection.not_owned,
        "repository owned by an organization or someone else",
        "repositories owned by organizations or someone else",
    );
    add(
        selection.beyond_limit,
        "older repository beyond the 30 most recent",
        "older repositories beyond the 30 most recent",
    );
    (read, skipped)
}

/// The GitHub account the profile names (a link on the resume), if any.
fn linked_github(data: &ProfileData) -> Option<String> {
    data.profile
        .contacts
        .iter()
        .filter(|c| c.kind == ContactKind::Github)
        .find_map(|c| parse_login(&c.value).ok())
}

impl LocalApp {
    async fn import_export(
        &self,
        export: LinkedinExport,
        now: DateTime<Utc>,
    ) -> Result<SourceImportResult, AppError> {
        let (read, skipped) = linkedin_summary(&export);
        let label = export
            .file_name
            .clone()
            .unwrap_or_else(|| "LinkedIn export".into());
        let (report, data) = self
            .exclusive(async {
                Ok(self
                    .profiles()
                    .import_linkedin(export.source_document(now), &export.parsed, now)
                    .await?)
            })
            .await?;
        Ok(SourceImportResult::of(
            EvidenceSource::Linkedin,
            label,
            &report,
            read,
            skipped,
            Vec::new(),
            &data,
        ))
    }

    /// Imports (or re-imports) a LinkedIn data export from its bytes: the
    /// `.zip` LinkedIn sends, or one of its CSV files.
    pub async fn import_linkedin(
        &self,
        bytes: &[u8],
        file_name: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<SourceImportResult, AppError> {
        if bytes.len() > MAX_LINKEDIN_BYTES {
            return Err(AppError::InvalidArguments(format!(
                "the export is larger than {} MB",
                MAX_LINKEDIN_BYTES / 1024 / 1024
            )));
        }
        let export = LinkedinExport::from_bytes(bytes, file_name).map_err(linkedin_error)?;
        self.import_export(export, now).await
    }

    /// Imports (or re-imports) a LinkedIn data export from a path: the
    /// `.zip`, the folder it unpacks to, or one of its CSV files. The
    /// archive is read from disk, only its career files decompressed.
    pub async fn import_linkedin_path(
        &self,
        path: &Path,
        now: DateTime<Utc>,
    ) -> Result<SourceImportResult, AppError> {
        let export = LinkedinExport::read(path).map_err(linkedin_error)?;
        self.import_export(export, now).await
    }

    /// Imports (or re-imports) a public GitHub account: `login` (a username
    /// or profile URL), or, when `None`, the account imported before or the
    /// GitHub link on the profile. With a token (no scopes needed) the rate
    /// limit is higher and language statistics are read too; it is used for
    /// these requests only.
    pub async fn import_github(
        &self,
        login: Option<&str>,
        access: &GithubAccess,
        now: DateTime<Utc>,
    ) -> Result<SourceImportResult, AppError> {
        let data = self.profiles().load_or_new(now).await?;
        let linked = linked_github(&data);
        let login = match login.map(str::trim).filter(|l| !l.is_empty()) {
            Some(input) => parse_login(input).map_err(github_error)?,
            None => jobhunt_profile::github::imported_login(&data)
                .or(linked.clone())
                .ok_or_else(|| {
                    AppError::InvalidArguments(
                        "give a GitHub username or profile URL (your profile does not link one)"
                            .into(),
                    )
                })?,
        };
        let http = HttpClient::new(self.config().discovery.http_settings())
            .map_err(|e| AppError::EvidenceUnavailable(format!("HTTP client: {e}")))?;
        let token = access
            .token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let reader = GithubReader::new(http, &access.api, token).map_err(github_error)?;
        let snapshot = reader
            .snapshot(&login, now, token.is_some())
            .await
            .map_err(github_error)?;
        let (import, data) = self
            .exclusive(async { Ok(self.profiles().import_github(&snapshot, now).await?) })
            .await?;
        let (read, skipped) = github_summary(&import.selection);
        let mut problems = snapshot.problems.clone();
        if let Some(linked) = linked.filter(|l| !l.eq_ignore_ascii_case(&import.login)) {
            problems.push(format!(
                "your resume links github.com/{linked}, not github.com/{}; check it is yours",
                import.login
            ));
        }
        Ok(SourceImportResult::of(
            EvidenceSource::Github,
            format!("github.com/{}", import.login),
            &import.report,
            read,
            skipped,
            problems,
            &data,
        ))
    }

    /// Takes an imported LinkedIn export or GitHub account out of the
    /// profile: its document and what only it supported (the person's
    /// decisions are kept). See [`jobhunt_profile::sources`].
    pub async fn remove_source(
        &self,
        source: EvidenceSource,
        now: DateTime<Utc>,
    ) -> Result<SourceRemovalResult, AppError> {
        let (removal, data) = self
            .exclusive(async { Ok(self.profiles().remove_source(source.origin(), now).await?) })
            .await?;
        Ok(SourceRemovalResult::of(source, &removal, &data))
    }

    /// Whether the profile has this source now.
    pub async fn has_source(&self, source: EvidenceSource) -> Result<bool, AppError> {
        let Some(data) = self.profiles().load().await? else {
            return Ok(false);
        };
        let id = source_document_id(data.id(), source.origin());
        Ok(data.documents.iter().any(|d| d.id == id))
    }
}
