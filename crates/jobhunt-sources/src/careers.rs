//! First-party careers pages.
//!
//! JobHunt does not scrape arbitrary career sites: every site is different,
//! and a generic scraper would produce exactly the kind of noisy, fragile
//! data discovery must avoid. Most company careers pages, however, are a
//! thin shell around a hosted ATS board: they link to
//! `jobs.lever.co/<site>`, embed `boards.greenhouse.io/embed/job_board?for=…`,
//! or load `jobs.ashbyhq.com/<board>/embed`. Reading that board through its
//! adapter gives complete, structured data.
//!
//! This module provides that bridge:
//!
//! * [`board_for_url`] recognizes a supported board from its URL, without
//!   any network access (`https://jobs.lever.co/spotify` → `lever:spotify`);
//! * [`find_boards`] finds board references inside a page's HTML;
//! * [`detect`] fetches a careers page and returns the boards it uses.
//!
//! A future first-party adapter (for example one reading schema.org
//! `JobPosting` data) would be a regular [`jobhunt_core::Source`]; nothing
//! here would need to change.

use jobhunt_core::SourceError;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::http::HttpClient;
use crate::lever::LeverRegion;
use crate::{ashby, greenhouse, lever, yc};

/// A company careers page to read through the board it embeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CareersPage {
    /// The page's URL, e.g. `https://www.example.com/careers`.
    pub url: String,
    /// Company display name for the detected board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
}

/// A supported board, as referenced by a URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BoardRef {
    /// Source kind: `ashby`, `greenhouse`, `lever` or `yc`.
    pub kind: &'static str,
    /// Board, site or company slug.
    pub name: String,
    pub lever_region: LeverRegion,
}

impl BoardRef {
    fn new(kind: &'static str, name: &str) -> Option<Self> {
        let valid = !name.is_empty()
            && name.len() <= 100
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        valid.then(|| Self {
            kind,
            name: name.to_ascii_lowercase(),
            lever_region: LeverRegion::Global,
        })
    }
}

/// Path segments that are part of a host's own routes, never board names.
const RESERVED: &[&str] = &[
    "embed",
    "api",
    "v0",
    "v1",
    "static",
    "assets",
    "images",
    "favicon.ico",
    "robots.txt",
    "industry",
    "location",
    "batch",
    "tags",
];

/// Recognizes a supported board from a URL (a board page, a job page, an
/// embed script or an API endpoint).
pub fn board_for_url(url: &Url) -> Option<BoardRef> {
    let host = url.host_str()?.to_ascii_lowercase();
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();
    let query = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let first = segments.first().copied().filter(|s| !RESERVED.contains(s));

    match host.as_str() {
        "jobs.ashbyhq.com" => BoardRef::new(ashby::KIND, first?),
        "api.ashbyhq.com" => match segments.as_slice() {
            ["posting-api", "job-board", board, ..] => BoardRef::new(ashby::KIND, board),
            _ => None,
        },
        "boards.greenhouse.io"
        | "job-boards.greenhouse.io"
        | "boards.eu.greenhouse.io"
        | "job-boards.eu.greenhouse.io" => match segments.as_slice() {
            ["embed", ..] => BoardRef::new(greenhouse::KIND, &query("for")?),
            _ => BoardRef::new(greenhouse::KIND, first?),
        },
        "boards-api.greenhouse.io" => match segments.as_slice() {
            ["v1", "boards", board, ..] => BoardRef::new(greenhouse::KIND, board),
            _ => None,
        },
        "jobs.lever.co" | "jobs.eu.lever.co" => {
            let mut board = BoardRef::new(lever::KIND, first?)?;
            if host == "jobs.eu.lever.co" {
                board.lever_region = LeverRegion::Eu;
            }
            Some(board)
        }
        "api.lever.co" | "api.eu.lever.co" => match segments.as_slice() {
            ["v0", "postings", site, ..] => {
                let mut board = BoardRef::new(lever::KIND, site)?;
                if host == "api.eu.lever.co" {
                    board.lever_region = LeverRegion::Eu;
                }
                Some(board)
            }
            _ => None,
        },
        "www.ycombinator.com" | "ycombinator.com" => match segments.as_slice() {
            ["companies", slug, ..] if !RESERVED.contains(slug) => BoardRef::new(yc::KIND, slug),
            _ => None,
        },
        _ => None,
    }
}

/// Hosts whose URLs can reference a supported board.
const BOARD_HOSTS: &[&str] = &[
    "jobs.ashbyhq.com/",
    "api.ashbyhq.com/",
    "boards.greenhouse.io/",
    "job-boards.greenhouse.io/",
    "boards.eu.greenhouse.io/",
    "job-boards.eu.greenhouse.io/",
    "boards-api.greenhouse.io/",
    "jobs.lever.co/",
    "jobs.eu.lever.co/",
    "api.lever.co/",
    "api.eu.lever.co/",
];

/// Finds references to supported boards in HTML (links, iframes, embed
/// scripts, inline JSON). Each board is returned once, in page order.
pub fn find_boards(html: &str) -> Vec<BoardRef> {
    // Inline JSON often escapes slashes ("jobs.lever.co\/spotify").
    let html = html.replace("\\/", "/");
    let mut found: Vec<(usize, BoardRef)> = Vec::new();
    for host in BOARD_HOSTS {
        let mut from = 0;
        while let Some(pos) = html[from..].find(host) {
            let start = from + pos;
            from = start + host.len();
            // Skip matches that are part of a longer host name
            // ("myjobs.lever.co" is not Lever's host).
            let preceded_by_host_char = html[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
            if preceded_by_host_char {
                continue;
            }
            let end = html[start..]
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | '\\' | '`')
                })
                .map_or(html.len(), |e| start + e);
            let candidate = format!("https://{}", html[start..end].replace("&amp;", "&"));
            if let Some(board) = Url::parse(&candidate).ok().as_ref().and_then(board_for_url)
                && !found.iter().any(|(_, b)| *b == board)
            {
                found.push((start, board));
            }
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, board)| board).collect()
}

/// Fetches a careers page and returns the supported boards it references.
/// A URL that already is a board URL is recognized without fetching.
pub async fn detect(http: &HttpClient, page: &Url) -> Result<Vec<BoardRef>, SourceError> {
    if let Some(board) = board_for_url(page) {
        return Ok(vec![board]);
    }
    let body = http.get_bytes(page).await?;
    Ok(find_boards(&String::from_utf8_lossy(&body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(url: &str) -> Option<(String, String)> {
        board_for_url(&Url::parse(url).unwrap()).map(|b| (b.kind.to_owned(), b.name))
    }

    fn pair(kind: &str, name: &str) -> Option<(String, String)> {
        Some((kind.to_owned(), name.to_owned()))
    }

    #[test]
    fn recognizes_board_urls() {
        assert_eq!(
            board("https://jobs.ashbyhq.com/Linear"),
            pair("ashby", "linear")
        );
        assert_eq!(
            board(
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application"
            ),
            pair("ashby", "linear")
        );
        assert_eq!(
            board("https://jobs.ashbyhq.com/linear/embed?version=2"),
            pair("ashby", "linear")
        );
        assert_eq!(
            board("https://job-boards.greenhouse.io/anthropic"),
            pair("greenhouse", "anthropic")
        );
        assert_eq!(
            board("https://boards.greenhouse.io/figma/jobs/5426468004?gh_jid=5426468004"),
            pair("greenhouse", "figma")
        );
        assert_eq!(
            board("https://boards.greenhouse.io/embed/job_board/js?for=stripe"),
            pair("greenhouse", "stripe")
        );
        assert_eq!(
            board("https://boards-api.greenhouse.io/v1/boards/airbnb/jobs"),
            pair("greenhouse", "airbnb")
        );
        assert_eq!(
            board("https://jobs.lever.co/spotify/abc"),
            pair("lever", "spotify")
        );
        assert_eq!(
            board("https://www.ycombinator.com/companies/posthog/jobs"),
            pair("yc", "posthog")
        );

        let eu = board_for_url(&Url::parse("https://jobs.eu.lever.co/acme").unwrap()).unwrap();
        assert_eq!(eu.lever_region, LeverRegion::Eu);

        assert_eq!(board("https://boards.greenhouse.io/embed/job_board"), None);
        assert_eq!(
            board("https://www.ycombinator.com/companies/industry/fintech"),
            None
        );
        assert_eq!(board("https://jobs.lever.co/"), None);
        assert_eq!(board("https://stripe.com/jobs"), None);
    }

    #[test]
    fn finds_embedded_boards_in_html() {
        let html = r#"
            <a href="https://jobs.lever.co/spotify/abc?lever-origin=applied">Apply</a>
            <script src="https://boards.greenhouse.io/embed/job_board/js?for=stripe&amp;b=x"></script>
            <script>window.__DATA__ = {"jobs":"https:\/\/jobs.ashbyhq.com\/linear"};</script>
            <a href="https://jobs.lever.co/spotify">All jobs</a>
            <a href="https://myjobs.lever.co/other">Not Lever</a>
        "#;
        let found: Vec<(String, String)> = find_boards(html)
            .into_iter()
            .map(|b| (b.kind.to_owned(), b.name))
            .collect();
        assert_eq!(
            found,
            vec![
                ("lever".to_owned(), "spotify".to_owned()),
                ("greenhouse".to_owned(), "stripe".to_owned()),
                ("ashby".to_owned(), "linear".to_owned()),
            ]
        );
        assert!(find_boards("<p>No boards here</p>").is_empty());
    }
}
