//! GitHub as a source of evidence.
//!
//! A reader of GitHub's public API (`jobhunt_sources::github`) fills a
//! [`GithubSnapshot`]: the account, its public repositories and their
//! language statistics, its public organizations. [`merge_github`] decides
//! what of it is evidence and folds that into the profile with the same
//! engine as every source ([`crate::import`]).
//!
//! GitHub is evidence of public activity, not of professional expertise:
//!
//! * only public repositories the account **owns** become projects; forks
//!   (someone else's work), empty repositories, the profile README
//!   repository (`login/login`) and repositories owned by others are
//!   skipped, and counted;
//! * a repository becomes facts about public code: that the account owns
//!   it, and its main languages (at least [`MAIN_LANGUAGE_SHARE`] of its
//!   code, at most [`MAIN_LANGUAGES`]). Stars and forks are context in the
//!   snippet, never a measure of quality; language share is never a level;
//! * organization membership stays in the snapshot text; it is not
//!   employment;
//! * the one conclusion drawn, "recent hands-on work" in a language, is an
//!   inference that waits for the user's review. Nothing says "expert".

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::aggregate::ProfileData;
use crate::date::PartialDate;
use crate::evidence::{ClaimKind, Confidence};
use crate::import::{ImportReport, Inference, merge_source};
use crate::infer::{known_technology, topic_key};
use crate::model::{DocumentKind, Origin, SourceDocument};
use crate::resume::{ParsedProject, ParsedResume};
use crate::sources::upsert_source_document;

/// Repositories kept, most recently pushed first.
pub const MAX_REPOSITORIES: usize = 30;
/// A language counts as a repository's main language from this share of
/// its code (by bytes, as GitHub measures it).
pub const MAIN_LANGUAGE_SHARE: f64 = 0.15;
/// Main languages kept per repository.
pub const MAIN_LANGUAGES: usize = 3;
/// "Recent" work: pushed within this many days of the snapshot.
pub const RECENT_DAYS: i64 = 365;

/// The account, as GitHub's API describes it publicly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubAccount {
    pub login: String,
    /// `https://github.com/<login>`.
    pub html_url: String,
    /// `User` or `Organization`.
    pub kind: String,
    pub public_repos: u32,
    pub created_at: Option<DateTime<Utc>>,
}

/// One repository, as GitHub's API lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GithubRepo {
    /// GitHub's id: stable when the repository is renamed.
    pub id: u64,
    pub name: String,
    /// `owner/name`.
    pub full_name: String,
    pub owner: String,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub archived: bool,
    pub is_template: bool,
    pub private: bool,
    /// Kilobytes; 0 for an empty repository.
    pub size: u64,
    pub stars: u32,
    pub forks: u32,
    /// The language GitHub shows for it.
    pub language: Option<String>,
    /// Bytes of code per language, when they could be read (`None` when
    /// that request failed: the primary language is used instead).
    pub languages: Option<Vec<(String, u64)>>,
    pub topics: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub pushed_at: Option<DateTime<Utc>>,
}

/// What a reader of GitHub's API hands to the profile domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GithubSnapshot {
    pub account: GithubAccount,
    /// The account's public repositories, as listed.
    pub repos: Vec<GithubRepo>,
    /// Logins of the account's public organization memberships.
    pub orgs: Vec<String>,
    pub fetched_at: DateTime<Utc>,
    /// What could not be read, per request ("languages of alice/lox:
    /// HTTP 502"). A snapshot with problems is still usable; they are
    /// reported.
    pub problems: Vec<String>,
}

/// How the snapshot's repositories were sorted out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoSelection {
    /// Kept as evidence (full names, most recently pushed first).
    pub kept: Vec<String>,
    pub archived: usize,
    pub forks: usize,
    pub empty: usize,
    /// Owned by someone else (an organization) or private.
    pub not_owned: usize,
    /// Owned and eligible, but beyond [`MAX_REPOSITORIES`].
    pub beyond_limit: usize,
    /// Distinct main languages across the kept repositories.
    pub languages: Vec<String>,
    /// Public organizations (evidence only).
    pub orgs: usize,
}

/// What importing a GitHub account did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GithubImport {
    pub report: ImportReport,
    pub selection: RepoSelection,
    /// The account imported (`alice`).
    pub login: String,
}

fn month(at: DateTime<Utc>) -> String {
    at.format("%Y-%m").to_string()
}

/// The repositories that are evidence, in order, and the tally of the
/// rest. A reader of the API uses it to ask for language statistics of
/// these repositories only.
pub fn select(snapshot: &GithubSnapshot) -> (Vec<&GithubRepo>, RepoSelection) {
    let login = snapshot.account.login.to_lowercase();
    let mut selection = RepoSelection {
        orgs: snapshot.orgs.len(),
        ..RepoSelection::default()
    };
    let mut eligible: Vec<&GithubRepo> = Vec::new();
    for repo in &snapshot.repos {
        if repo.private || repo.owner.to_lowercase() != login {
            selection.not_owned += 1;
        } else if repo.fork {
            selection.forks += 1;
        } else if repo.size == 0 || repo.name.to_lowercase() == login {
            // Empty, or the profile README repository.
            selection.empty += 1;
        } else {
            eligible.push(repo);
        }
    }
    eligible.sort_by(|a, b| {
        b.pushed_at
            .cmp(&a.pushed_at)
            .then_with(|| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()))
    });
    selection.beyond_limit = eligible.len().saturating_sub(MAX_REPOSITORIES);
    eligible.truncate(MAX_REPOSITORIES);
    selection.archived = eligible.iter().filter(|r| r.archived).count();
    selection.kept = eligible.iter().map(|r| r.full_name.clone()).collect();
    (eligible, selection)
}

/// A repository's main languages with their share, largest first.
pub fn main_languages(repo: &GithubRepo) -> Vec<(String, u32)> {
    match &repo.languages {
        Some(bytes) if !bytes.is_empty() => {
            let total: u64 = bytes.iter().map(|(_, b)| *b).sum();
            if total == 0 {
                return Vec::new();
            }
            let mut shares: Vec<(String, u32)> = bytes
                .iter()
                .filter(|(_, b)| (*b as f64) / (total as f64) >= MAIN_LANGUAGE_SHARE)
                .map(|(name, b)| {
                    let percent = ((*b as f64) * 100.0 / (total as f64)).round();
                    (name.clone(), percent as u32)
                })
                .collect();
            shares.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            shares.truncate(MAIN_LANGUAGES);
            shares
        }
        // Without statistics, the language GitHub shows (no share).
        _ => repo
            .language
            .clone()
            .map(|l| vec![(l, 0)])
            .unwrap_or_default(),
    }
}

fn language_name(name: &str) -> String {
    known_technology(name).map_or_else(|| name.to_owned(), |t| t.name.to_owned())
}

/// The snippet for one repository: what GitHub says about it, on two
/// lines.
fn repo_block(repo: &GithubRepo, languages: &[(String, u32)]) -> String {
    let mut first = repo.full_name.clone();
    if let Some(d) = repo
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        first.push_str(" — ");
        first.push_str(d);
    }
    first.push_str(" · ");
    first.push_str(&repo.html_url);
    let mut facts: Vec<String> = Vec::new();
    if !languages.is_empty() {
        let list: Vec<String> = languages
            .iter()
            .map(|(l, share)| {
                if *share > 0 {
                    format!("{} {share}%", language_name(l))
                } else {
                    language_name(l)
                }
            })
            .collect();
        facts.push(format!("Languages: {}", list.join(", ")));
    }
    if !repo.topics.is_empty() {
        facts.push(format!("Topics: {}", repo.topics.join(", ")));
    }
    facts.push(format!(
        "{} {}, {} {}",
        repo.stars,
        if repo.stars == 1 { "star" } else { "stars" },
        repo.forks,
        if repo.forks == 1 { "fork" } else { "forks" }
    ));
    if let Some(at) = repo.created_at {
        facts.push(format!("Created {}", month(at)));
    }
    if let Some(at) = repo.pushed_at {
        facts.push(format!("Last push {}", month(at)));
    }
    if repo.archived {
        facts.push("Archived".into());
    }
    if repo.is_template {
        facts.push("Template".into());
    }
    format!("{first}\n  {}", facts.join(" · "))
}

/// The document Narrow keeps of a snapshot: what it read, as text. Its
/// hash changes only when what was read changed.
fn document(
    snapshot: &GithubSnapshot,
    kept: &[&GithubRepo],
    selection: &RepoSelection,
) -> SourceDocument {
    let account = &snapshot.account;
    let mut text = format!("GitHub account {} ({})\n", account.login, account.html_url);
    text.push_str(&format!("Public repositories: {}", account.public_repos));
    if let Some(at) = account.created_at {
        text.push_str(&format!(" · Account created {}", month(at)));
    }
    text.push('\n');
    if !snapshot.orgs.is_empty() {
        text.push_str(&format!(
            "Public organizations (membership, not employment): {}\n",
            snapshot.orgs.join(", ")
        ));
    }
    text.push_str("\nRepositories owned by the account, most recently pushed first:\n");
    for repo in kept {
        text.push_str(&repo_block(repo, &main_languages(repo)));
        text.push('\n');
    }
    let mut skipped: Vec<String> = Vec::new();
    let mut count = |n: usize, one: &str, many: &str| {
        if n > 0 {
            skipped.push(format!("{n} {}", if n == 1 { one } else { many }));
        }
    };
    count(selection.forks, "fork", "forks");
    count(selection.empty, "empty repository", "empty repositories");
    count(
        selection.not_owned,
        "repository owned by others",
        "repositories owned by others",
    );
    count(
        selection.beyond_limit,
        "older repository",
        "older repositories",
    );
    if !skipped.is_empty() {
        text.push_str(&format!(
            "\nNot used as evidence: {}.\n",
            skipped.join(", ")
        ));
    }
    let sha256 = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    SourceDocument {
        id: crate::ids::DocumentId::derive(&[]),
        kind: DocumentKind::Github,
        file_name: Some(format!("github.com/{}", account.login)),
        sha256,
        pages: None,
        text,
        parser: "github-api/1".into(),
        first_imported_at: snapshot.fetched_at,
        last_imported_at: snapshot.fetched_at,
    }
}

/// Folds a GitHub snapshot into `data`. See the module docs for what
/// becomes evidence.
pub fn merge_github(
    data: &mut ProfileData,
    snapshot: &GithubSnapshot,
    now: DateTime<Utc>,
) -> GithubImport {
    let (kept, mut selection) = select(snapshot);
    let doc = document(snapshot, &kept, &selection);
    let mut report = upsert_source_document(data, Origin::Github, doc, now);
    report.notes.extend(snapshot.problems.iter().cloned());
    let document = report
        .document
        .unwrap_or_else(|| crate::import::source_document_id(data.id(), Origin::Github));

    let mut projects = Vec::new();
    // language key → (name, recent repositories, the first one's snippet)
    let mut recent: BTreeMap<String, (String, Vec<String>, String)> = BTreeMap::new();
    let cutoff = snapshot.fetched_at - Duration::days(RECENT_DAYS);
    for repo in &kept {
        let languages = main_languages(repo);
        let header = repo_block(repo, &languages);
        let names: Vec<String> = languages.iter().map(|(l, _)| language_name(l)).collect();
        for name in &names {
            if !selection.languages.contains(name) {
                selection.languages.push(name.clone());
            }
            if repo.pushed_at.is_some_and(|at| at >= cutoff) {
                let entry = recent
                    .entry(topic_key(name))
                    .or_insert_with(|| (name.clone(), Vec::new(), header.clone()));
                entry.1.push(repo.full_name.clone());
            }
        }
        let mut notes = Vec::new();
        if repo.archived {
            notes.push("archived".to_owned());
        }
        projects.push(ParsedProject {
            name: repo.full_name.clone(),
            key: Some(format!("repo|{}", repo.id)),
            description: repo
                .description
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_owned),
            role: None,
            url: Some(repo.html_url.clone()),
            start: repo.created_at.map(PartialDate::of),
            end: repo.pushed_at.map(PartialDate::of),
            current: false,
            tech_line: (!names.is_empty()).then(|| {
                header
                    .lines()
                    .nth(1)
                    .unwrap_or_default()
                    .split(" · ")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            }),
            technologies: names,
            header,
            bullets: Vec::new(),
            notes,
        });
    }
    selection.languages.sort();

    let inferences = recent
        .into_iter()
        .map(|(key, (name, repos, snippet))| {
            let shown: Vec<&str> = repos.iter().take(5).map(String::as_str).collect();
            let more = repos.len().saturating_sub(shown.len());
            Inference {
                kind: ClaimKind::Technology,
                key: format!("github|recent|{key}"),
                text: format!("Recent hands-on {name} work in public GitHub repositories"),
                topic: Some(key),
                confidence: Confidence::Medium,
                snippet,
                section: "Repositories".into(),
                basis: format!(
                    "{} public {} you own with {name} as a main language, pushed since {}: {}{}",
                    repos.len(),
                    if repos.len() == 1 {
                        "repository"
                    } else {
                        "repositories"
                    },
                    month(cutoff),
                    shown.join(", "),
                    if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    }
                ),
            }
        })
        .collect();

    let parsed = ParsedResume {
        projects,
        ..ParsedResume::default()
    };
    let report = merge_source(
        data,
        Origin::Github,
        document,
        &parsed,
        inferences,
        report,
        now,
    );
    GithubImport {
        report,
        selection,
        login: snapshot.account.login.clone(),
    }
}

/// The GitHub login a profile's GitHub document is for.
pub fn imported_login(data: &ProfileData) -> Option<String> {
    data.documents
        .iter()
        .find(|d| d.kind == DocumentKind::Github)
        .and_then(|d| d.file_name.as_deref())
        .and_then(|name| name.strip_prefix("github.com/"))
        .map(str::to_owned)
}
