//! A public GitHub account, through GitHub's official REST API.
//!
//! Not a job source: evidence for the profile. [`GithubReader::snapshot`]
//! reads what [`jobhunt_profile::github`] turns into evidence, with
//! documented REST endpoints only (no HTML):
//!
//! | Request | For |
//! | --- | --- |
//! | `GET /users/{login}` | the account |
//! | `GET /users/{login}/repos?type=owner` (paginated) | its public repositories |
//! | `GET /users/{login}/orgs` | public organization memberships |
//! | `GET /repos/{owner}/{repo}/languages` | language statistics of the kept repositories, when asked for |
//!
//! Without statistics an import costs about three requests (each
//! repository's primary language comes with the list), which matters
//! under GitHub's limit of 60 unauthenticated requests an hour; with them,
//! up to one more per kept repository.
//!
//! A token is optional. Public data needs none; one (no scopes) only
//! raises the rate limit. It is sent to the API as `Authorization` and
//! never stored or logged.
//!
//! Failures: an unknown account, a rate limit, or a repository list that
//! could not be read completely fail the import (a partial list would
//! make missing repositories look deleted). A language statistic or the
//! organizations failing is reported in the snapshot's problems and the
//! import goes on (the repository's primary language is used instead).

use chrono::{DateTime, Utc};
use futures::StreamExt;
use jobhunt_core::SourceError;
use jobhunt_profile::github::{GithubAccount, GithubRepo, GithubSnapshot, select};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use url::Url;

use crate::http::{HttpClient, Probe};

/// GitHub's API.
pub const API: &str = "https://api.github.com";

/// Repository pages read at most (100 each).
const MAX_PAGES: usize = 10;
/// Language statistics requested at once.
const CONCURRENCY: usize = 4;

/// A repository's language statistics, or why they could not be read.
type LanguageStats = (u64, String, Result<Vec<(String, u64)>, GithubError>);

/// Why an account could not be read.
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error(
        "{0:?} is not a GitHub username or profile URL (expected something like `octocat` or https://github.com/octocat)"
    )]
    InvalidLogin(String),
    #[error("GitHub has no public account named {0:?}")]
    NotFound(String),
    #[error("{0:?} is an organization, not a person's account")]
    Organization(String),
    #[error(
        "GitHub's rate limit is used up{}; try again later, or set GITHUB_TOKEN (a token without scopes) for a higher limit",
        reset.map(|r| format!(" until {}", r.format("%H:%M UTC"))).unwrap_or_default()
    )]
    RateLimited { reset: Option<DateTime<Utc>> },
    #[error("GitHub rejected the token; check GITHUB_TOKEN, or import without it")]
    BadToken,
    #[error("could not read {what} from GitHub: {detail}")]
    Unavailable { what: String, detail: String },
}

/// The login in what a person typed: `octocat`, `@octocat`, or a profile
/// URL (`https://github.com/octocat`, `github.com/octocat/`).
pub fn parse_login(input: &str) -> Result<String, GithubError> {
    let invalid = || GithubError::InvalidLogin(input.trim().to_owned());
    let mut text = input.trim();
    for prefix in ["https://", "http://"] {
        text = text.strip_prefix(prefix).unwrap_or(text);
    }
    text = text.strip_prefix("www.").unwrap_or(text);
    if let Some(rest) = text.strip_prefix("github.com/") {
        text = rest.split(['/', '?', '#']).next().unwrap_or_default();
    } else if text.contains('/') || text.contains('.') {
        return Err(invalid());
    }
    let login = text.strip_prefix('@').unwrap_or(text);
    // GitHub's rule: letters, digits and single hyphens, not at either end,
    // at most 39 characters.
    let valid = !login.is_empty()
        && login.len() <= 39
        && !login.starts_with('-')
        && !login.ends_with('-')
        && !login.contains("--")
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(login.to_owned())
    } else {
        Err(invalid())
    }
}

/// Reads public accounts from GitHub's API.
#[derive(Debug, Clone)]
pub struct GithubReader {
    http: HttpClient,
    base: Url,
    headers: HeaderMap,
}

#[derive(Deserialize)]
struct UserPayload {
    login: String,
    html_url: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    public_repos: u32,
    created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct OwnerPayload {
    login: String,
}

#[derive(Deserialize)]
struct RepoPayload {
    id: u64,
    name: String,
    full_name: String,
    owner: OwnerPayload,
    html_url: String,
    description: Option<String>,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    is_template: bool,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    stargazers_count: u32,
    #[serde(default)]
    forks_count: u32,
    language: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    created_at: Option<DateTime<Utc>>,
    pushed_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct OrgPayload {
    login: String,
}

impl GithubReader {
    /// A reader of `base` (GitHub's API, or a test server), with an
    /// optional token.
    pub fn new(http: HttpClient, base: &str, token: Option<&str>) -> Result<Self, GithubError> {
        let base =
            Url::parse(base.trim_end_matches('/')).map_err(|e| GithubError::Unavailable {
                what: "the API address".into(),
                detail: e.to_string(),
            })?;
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static("2022-11-28"),
        );
        if let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| GithubError::BadToken)?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        Ok(Self {
            http,
            base,
            headers,
        })
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.base.clone();
        let joined = format!("{}{path}", url.path().trim_end_matches('/'));
        url.set_path(&joined);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        url
    }

    async fn get(&self, url: &Url, what: &str) -> Result<Probe, GithubError> {
        let probe = self
            .http
            .probe_with(url, &self.headers)
            .await
            .map_err(|e| GithubError::Unavailable {
                what: what.to_owned(),
                detail: describe(&e),
            })?;
        match probe.status {
            200..=299 => Ok(probe),
            401 => Err(GithubError::BadToken),
            403 | 429 if rate_limited(&probe) => Err(GithubError::RateLimited {
                reset: reset_of(&probe),
            }),
            status => Err(GithubError::Unavailable {
                what: what.to_owned(),
                detail: format!("HTTP {status}"),
            }),
        }
    }

    fn json<T: serde::de::DeserializeOwned>(probe: &Probe, what: &str) -> Result<T, GithubError> {
        serde_json::from_slice(&probe.body).map_err(|e| GithubError::Unavailable {
            what: what.to_owned(),
            detail: format!("unexpected answer ({e})"),
        })
    }

    /// Everything [`jobhunt_profile::github::merge_github`] needs about
    /// `login`, read now; with `language_statistics`, also how much of each
    /// kept repository is in which language (otherwise its primary
    /// language stands for it).
    pub async fn snapshot(
        &self,
        login: &str,
        now: DateTime<Utc>,
        language_statistics: bool,
    ) -> Result<GithubSnapshot, GithubError> {
        let login = parse_login(login)?;
        let user_url = self.url(&format!("/users/{login}"), &[]);
        let user = match self.get(&user_url, "the account").await {
            Err(GithubError::Unavailable { detail, .. }) if detail == "HTTP 404" => {
                return Err(GithubError::NotFound(login));
            }
            other => other?,
        };
        let user: UserPayload = Self::json(&user, "the account")?;
        if user.kind.eq_ignore_ascii_case("organization") {
            return Err(GithubError::Organization(user.login));
        }
        let account = GithubAccount {
            login: user.login.clone(),
            html_url: user.html_url,
            kind: user.kind,
            public_repos: user.public_repos,
            created_at: user.created_at,
        };

        // Every page, or nothing: a partial list would make the missing
        // repositories look deleted.
        let mut repos = Vec::new();
        let mut next = Some(self.url(
            &format!("/users/{}/repos", user.login),
            &[("type", "owner"), ("sort", "pushed"), ("per_page", "100")],
        ));
        let mut pages = 0;
        let mut problems = Vec::new();
        while let Some(url) = next.take() {
            pages += 1;
            let page = self.get(&url, "the repository list").await?;
            let payload: Vec<RepoPayload> = Self::json(&page, "the repository list")?;
            repos.extend(payload.into_iter().map(|r| GithubRepo {
                id: r.id,
                name: r.name,
                full_name: r.full_name,
                owner: r.owner.login,
                html_url: r.html_url,
                description: r.description,
                fork: r.fork,
                archived: r.archived,
                is_template: r.is_template,
                private: r.private,
                size: r.size,
                stars: r.stargazers_count,
                forks: r.forks_count,
                language: r.language,
                languages: None,
                topics: r.topics,
                created_at: r.created_at,
                pushed_at: r.pushed_at,
            }));
            next = next_page(&page.headers, &self.base);
            if next.is_some() && pages >= MAX_PAGES {
                problems.push(format!(
                    "only the {} most recently pushed repositories were read",
                    repos.len()
                ));
                next = None;
            }
        }

        let orgs = match self
            .get(
                &self.url(
                    &format!("/users/{}/orgs", user.login),
                    &[("per_page", "100")],
                ),
                "the organizations",
            )
            .await
            .and_then(|p| Self::json::<Vec<OrgPayload>>(&p, "the organizations"))
        {
            Ok(orgs) => orgs.into_iter().map(|o| o.login).collect(),
            Err(e) => {
                problems.push(format!("public organizations unavailable ({e})"));
                Vec::new()
            }
        };

        let mut snapshot = GithubSnapshot {
            account,
            repos,
            orgs,
            fetched_at: now,
            problems,
        };

        // Language statistics for the repositories that will be evidence,
        // a few at a time. A failure keeps the primary language.
        let wanted: Vec<(u64, String)> = if language_statistics {
            select(&snapshot)
                .0
                .into_iter()
                .map(|r| (r.id, r.full_name.clone()))
                .collect()
        } else {
            Vec::new()
        };
        let results: Vec<LanguageStats> = futures::stream::iter(wanted)
            .map(|(id, full_name)| async move {
                let url = self.url(&format!("/repos/{full_name}/languages"), &[]);
                let result = self
                    .get(&url, "language statistics")
                    .await
                    .and_then(|p| {
                        Self::json::<serde_json::Map<String, serde_json::Value>>(
                            &p,
                            "language statistics",
                        )
                    })
                    .map(|map| {
                        map.into_iter()
                            .filter_map(|(name, bytes)| bytes.as_u64().map(|b| (name, b)))
                            .collect()
                    });
                (id, full_name, result)
            })
            .buffered(CONCURRENCY)
            .collect()
            .await;
        let mut rate_limited = 0;
        for (id, full_name, result) in results {
            match result {
                Ok(languages) => {
                    if let Some(repo) = snapshot.repos.iter_mut().find(|r| r.id == id) {
                        repo.languages = Some(languages);
                    }
                }
                Err(GithubError::RateLimited { .. }) => rate_limited += 1,
                Err(GithubError::Unavailable { detail, .. }) => snapshot.problems.push(format!(
                    "languages of {full_name} unavailable ({detail}); used its primary language"
                )),
                Err(e) => snapshot.problems.push(format!(
                    "languages of {full_name} unavailable ({e}); used its primary language"
                )),
            }
        }
        if rate_limited > 0 {
            snapshot.problems.push(format!(
                "languages of {rate_limited} repositories unavailable (rate limit); used their primary language"
            ));
        }
        Ok(snapshot)
    }
}

/// A transport failure, briefly (no URL: it may carry a login, not a
/// secret, but logs stay small).
fn describe(error: &SourceError) -> String {
    match error {
        SourceError::Timeout { .. } => "timed out".into(),
        SourceError::Status { status, .. } => format!("HTTP {status}"),
        _ => "connection failed".into(),
    }
}

fn header<'a>(probe: &'a Probe, name: &str) -> Option<&'a str> {
    probe.headers.get(name).and_then(|v| v.to_str().ok())
}

fn rate_limited(probe: &Probe) -> bool {
    header(probe, "x-ratelimit-remaining") == Some("0")
        || probe.headers.contains_key("retry-after")
        || probe.status == 429
}

fn reset_of(probe: &Probe) -> Option<DateTime<Utc>> {
    header(probe, "x-ratelimit-reset")
        .and_then(|v| v.parse::<i64>().ok())
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
}

/// The `rel="next"` link of GitHub's pagination, kept on the same API.
fn next_page(headers: &HeaderMap, base: &Url) -> Option<Url> {
    let link = headers.get("link")?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (target, params) = part.split_once(';')?;
        if !params.split(';').any(|p| p.trim() == "rel=\"next\"") {
            return None;
        }
        let url = Url::parse(target.trim().trim_start_matches('<').trim_end_matches('>')).ok()?;
        (url.host_str() == base.host_str()
            && url.port_or_known_default() == base.port_or_known_default())
        .then_some(url)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logins_from_what_people_type() {
        for input in [
            "octocat",
            "@octocat",
            " https://github.com/octocat ",
            "github.com/octocat/",
            "https://www.github.com/octocat?tab=repositories",
            "http://github.com/octocat/hello-world",
        ] {
            assert_eq!(parse_login(input).unwrap(), "octocat", "{input}");
        }
        assert_eq!(parse_login("rust-lang").unwrap(), "rust-lang");
        for bad in [
            "",
            "-octo",
            "octo-",
            "oc--to",
            "octo cat",
            "https://gitlab.com/octocat",
            "../../users",
            "octo/../../x",
            &"a".repeat(40),
        ] {
            assert!(parse_login(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn pagination_follows_only_the_same_api() {
        let base = Url::parse("https://api.github.com").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            HeaderValue::from_static(
                "<https://api.github.com/user/1/repos?page=2>; rel=\"next\", <https://api.github.com/user/1/repos?page=5>; rel=\"last\"",
            ),
        );
        assert_eq!(
            next_page(&headers, &base).unwrap().as_str(),
            "https://api.github.com/user/1/repos?page=2"
        );
        headers.insert(
            "link",
            HeaderValue::from_static("<https://evil.example/repos?page=2>; rel=\"next\""),
        );
        assert_eq!(next_page(&headers, &base), None);
        assert_eq!(next_page(&HeaderMap::new(), &base), None);
    }
}
