//! Canonical URL normalization.
//!
//! The same resource is routinely linked with different spellings: tracking
//! parameters appended by a referrer, a trailing slash, an uppercase host, a
//! fragment pointing at a heading. [`CanonicalUrl`] folds those variations
//! together so that identity and duplicate detection can rely on plain string
//! equality.
//!
//! Normalization is deliberately conservative. It only removes information
//! that never changes which resource is addressed:
//!
//! * scheme and host are lowercased, default ports and a trailing host dot
//!   are dropped, and embedded credentials are removed;
//! * known tracking parameters (`utm_*`, `gclid`, `fbclid`, `gh_src`, ...)
//!   are removed and the remaining query parameters are sorted by key;
//! * a trailing slash on a non-root path is removed;
//! * fragments are removed unless they look like client-side routes
//!   (`#/jobs/123` or `#!/jobs/123`), which some career sites use to address
//!   individual postings.
//!
//! It does **not** rewrite `http` to `https`, strip `www.`, or apply any
//! site-specific rules; those can change the addressed resource.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

/// Query parameters that only carry attribution or tracking information.
/// Compared case-insensitively.
const TRACKING_PARAMS: &[&str] = &[
    "_ga",
    "_gl",
    "_hsenc",
    "_hsmi",
    "dclid",
    "fbclid",
    "gbraid",
    "gclid",
    "gh_src",
    "hsctatracking",
    "igshid",
    "li_fat_id",
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "msclkid",
    "ref",
    "ref_src",
    "referer",
    "referrer",
    "ttclid",
    "twclid",
    "wbraid",
    "yclid",
];

/// Parameter prefixes that only carry tracking information (`utm_source`,
/// Lever's `lever-source[]`, `lever-origin` and `lever-via`, ...). Compared
/// case-insensitively.
const TRACKING_PREFIXES: &[&str] = &["utm_", "lever-source", "lever-origin", "lever-via"];

/// Returns true when a query parameter only carries tracking/attribution data.
pub fn is_tracking_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TRACKING_PARAMS.contains(&lower.as_str())
        || TRACKING_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}

/// An absolute `http(s)` URL in canonical form.
///
/// Construct it with [`CanonicalUrl::parse`]; normalization is idempotent, so
/// parsing an already-canonical URL yields the same value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CanonicalUrl(String);

impl CanonicalUrl {
    /// Parses and normalizes `input`.
    pub fn parse(input: &str) -> Result<Self, UrlError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(UrlError::Empty);
        }
        let mut url = Url::parse(trimmed).map_err(|source| UrlError::Invalid {
            input: trimmed.to_owned(),
            source,
        })?;

        match url.scheme() {
            "http" | "https" => {}
            other => {
                return Err(UrlError::UnsupportedScheme {
                    input: trimmed.to_owned(),
                    scheme: other.to_owned(),
                });
            }
        }

        let host = url
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| UrlError::MissingHost {
                input: trimmed.to_owned(),
            })?;
        if host.len() > 1 && host.ends_with('.') {
            let bare = host.trim_end_matches('.').to_owned();
            url.set_host(Some(&bare))
                .map_err(|source| UrlError::Invalid {
                    input: trimmed.to_owned(),
                    source,
                })?;
        }

        // Credentials never identify a posting; http(s) URLs always accept
        // clearing them, so the results can be ignored.
        let _ = url.set_username("");
        let _ = url.set_password(None);

        normalize_query(&mut url);
        normalize_path(&mut url);
        normalize_fragment(&mut url);

        Ok(Self(url.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

fn normalize_query(url: &mut Url) {
    if url.query().is_none() {
        return;
    }
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| !name.is_empty() && !is_tracking_param(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    if pairs.is_empty() {
        url.set_query(None);
        return;
    }
    // Stable sort by key only: repeated keys (`a=1&a=2`) keep their order,
    // which can be meaningful.
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut serializer = url.query_pairs_mut();
    serializer.clear();
    serializer.extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
}

fn normalize_path(url: &mut Url) {
    let path = url.path();
    if path.len() > 1 && path.ends_with('/') {
        let trimmed = path.trim_end_matches('/');
        let new_path = if trimmed.is_empty() { "/" } else { trimmed }.to_owned();
        url.set_path(&new_path);
    }
}

fn normalize_fragment(url: &mut Url) {
    let is_route = url
        .fragment()
        .is_some_and(|fragment| fragment.starts_with('/') || fragment.starts_with("!/"));
    if !is_route {
        url.set_fragment(None);
    }
}

impl fmt::Display for CanonicalUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CanonicalUrl {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for CanonicalUrl {
    type Err = UrlError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for CanonicalUrl {
    type Error = UrlError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<CanonicalUrl> for String {
    fn from(value: CanonicalUrl) -> Self {
        value.0
    }
}

/// Why a string could not be turned into a [`CanonicalUrl`].
#[derive(Debug, thiserror::Error)]
pub enum UrlError {
    #[error("URL is empty")]
    Empty,
    #[error("invalid URL {input:?}")]
    Invalid {
        input: String,
        #[source]
        source: url::ParseError,
    },
    #[error("unsupported URL scheme {scheme:?} in {input:?} (only http and https are allowed)")]
    UnsupportedScheme { input: String, scheme: String },
    #[error("URL {input:?} has no host")]
    MissingHost { input: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(input: &str) -> String {
        CanonicalUrl::parse(input).unwrap().into_string()
    }

    #[test]
    fn lowercases_scheme_and_host_but_not_path() {
        assert_eq!(
            canon("HTTPS://Jobs.AshbyHQ.com/Linear/ABC"),
            "https://jobs.ashbyhq.com/Linear/ABC"
        );
    }

    #[test]
    fn strips_default_port_credentials_and_trailing_host_dot() {
        assert_eq!(
            canon("https://user:pw@example.com.:443/jobs/1"),
            "https://example.com/jobs/1"
        );
        assert_eq!(canon("http://example.com:80/"), "http://example.com/");
        assert_eq!(
            canon("https://example.com:8443/x"),
            "https://example.com:8443/x"
        );
    }

    #[test]
    fn removes_tracking_parameters() {
        assert_eq!(
            canon(
                "https://boards.greenhouse.io/acme/jobs/123?gh_src=abc&utm_source=linkedin&UTM_Medium=x&gclid=1&ref=hn"
            ),
            "https://boards.greenhouse.io/acme/jobs/123"
        );
        assert_eq!(
            canon("https://jobs.lever.co/acme/1?lever-source%5B%5D=LinkedIn&lever-origin=applied"),
            "https://jobs.lever.co/acme/1"
        );
    }

    #[test]
    fn keeps_meaningful_parameters_sorted_by_key() {
        assert_eq!(
            canon("https://acme.com/careers?utm_campaign=x&gh_jid=4567&dept=eng"),
            "https://acme.com/careers?dept=eng&gh_jid=4567"
        );
    }

    #[test]
    fn keeps_order_of_repeated_keys() {
        assert_eq!(
            canon("https://acme.com/search?tag=b&q=rust&tag=a"),
            "https://acme.com/search?q=rust&tag=b&tag=a"
        );
    }

    #[test]
    fn drops_empty_query_and_trailing_slash() {
        assert_eq!(
            canon("https://acme.com/jobs/42/?"),
            "https://acme.com/jobs/42"
        );
        assert_eq!(canon("https://acme.com/jobs//"), "https://acme.com/jobs");
        assert_eq!(canon("https://acme.com"), "https://acme.com/");
    }

    #[test]
    fn drops_anchor_fragments_but_keeps_route_fragments() {
        assert_eq!(
            canon("https://acme.com/jobs/42#apply"),
            "https://acme.com/jobs/42"
        );
        assert_eq!(
            canon("https://acme.com/careers#/jobs/42"),
            "https://acme.com/careers#/jobs/42"
        );
        assert_eq!(
            canon("https://acme.com/careers#!/jobs/42"),
            "https://acme.com/careers#!/jobs/42"
        );
    }

    #[test]
    fn variants_of_the_same_posting_collapse_to_one_value() {
        let variants = [
            "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
            "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/",
            "  HTTPS://JOBS.ASHBYHQ.COM/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff?utm_source=x ",
            "https://jobs.ashbyhq.com:443/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff#top",
        ];
        let canonical: Vec<String> = variants.iter().map(|v| canon(v)).collect();
        assert!(canonical.windows(2).all(|pair| pair[0] == pair[1]));
    }

    /// Spellings of the same posting seen in the wild for each ATS: shared
    /// links carry tracking parameters, anchors and trailing slashes.
    #[test]
    fn real_ats_url_variants_collapse() {
        let groups: &[&[&str]] = &[
            &[
                "https://job-boards.greenhouse.io/anthropic/jobs/4461450008",
                "https://job-boards.greenhouse.io/anthropic/jobs/4461450008/",
                "https://job-boards.greenhouse.io/anthropic/jobs/4461450008?gh_src=abc123#app",
                "https://JOB-BOARDS.greenhouse.io/anthropic/jobs/4461450008?utm_source=linkedin",
            ],
            &[
                "https://stripe.com/jobs/search?gh_jid=8172510",
                "https://stripe.com/jobs/search?gh_src=x&gh_jid=8172510&utm_medium=social",
            ],
            &[
                "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1",
                "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1/?lever-via=abc",
                "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1?lever-origin=applied&lever-source%5B%5D=LinkedIn",
            ],
            &[
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff?utm_source=hn#overview",
            ],
            &[
                "https://www.ycombinator.com/companies/posthog/jobs/abc123-security-engineer",
                "https://www.ycombinator.com/companies/posthog/jobs/abc123-security-engineer/?ref=hn",
            ],
        ];
        for group in groups {
            let canonical: Vec<String> = group.iter().map(|u| canon(u)).collect();
            assert!(
                canonical.windows(2).all(|pair| pair[0] == pair[1]),
                "{canonical:#?}"
            );
        }
    }

    /// Distinct postings (or pages) must never normalize to one value; ATS
    /// equivalence beyond exact URLs is decided by `jobhunt_jobs::identity`.
    #[test]
    fn distinct_urls_stay_distinct() {
        let distinct: &[(&str, &str)] = &[
            // Different job ids in the parameter that identifies the job.
            (
                "https://stripe.com/jobs/search?gh_jid=8172510",
                "https://stripe.com/jobs/search?gh_jid=8172511",
            ),
            // A listing and its application form.
            (
                "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1",
                "https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1/apply",
            ),
            (
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application",
            ),
            // The same id on another board, host, scheme or path spelling
            // is left to the identity layer, not folded here.
            (
                "https://boards.greenhouse.io/figma/jobs/5426468004",
                "https://job-boards.greenhouse.io/figma/jobs/5426468004",
            ),
            ("http://acme.com/jobs/1", "https://acme.com/jobs/1"),
            ("https://www.acme.com/jobs/1", "https://acme.com/jobs/1"),
            (
                "https://jobs.ashbyhq.com/Linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
                "https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff",
            ),
            // Hash routes address postings in single-page career sites.
            (
                "https://acme.com/careers#/jobs/1",
                "https://acme.com/careers#/jobs/2",
            ),
            // A query parameter that is not tracking.
            (
                "https://www.workatastartup.com/application?signup_job_id=93346",
                "https://www.workatastartup.com/application?signup_job_id=93347",
            ),
        ];
        for (a, b) in distinct {
            assert_ne!(canon(a), canon(b), "{a} and {b} must stay distinct");
        }
    }

    #[test]
    fn normalization_is_idempotent() {
        let inputs = [
            "https://acme.com/a b/?q=hello world&z=1&a=%2F&utm_source=x",
            "http://EXAMPLE.org/path/?b=2&a=1#frag",
            "https://acme.com/careers#/jobs/42?x=1",
        ];
        for input in inputs {
            let once = canon(input);
            assert_eq!(canon(&once), once, "not idempotent for {input}");
        }
    }

    #[test]
    fn rejects_invalid_input() {
        assert!(matches!(CanonicalUrl::parse("  "), Err(UrlError::Empty)));
        assert!(matches!(
            CanonicalUrl::parse("not a url"),
            Err(UrlError::Invalid { .. })
        ));
        assert!(matches!(
            CanonicalUrl::parse("mailto:jobs@acme.com"),
            Err(UrlError::UnsupportedScheme { .. })
        ));
        assert!(matches!(
            CanonicalUrl::parse("ftp://acme.com/file"),
            Err(UrlError::UnsupportedScheme { .. })
        ));
    }

    #[test]
    fn serde_round_trip_normalizes() {
        let url: CanonicalUrl =
            serde_json::from_str("\"https://ACME.com/jobs/1/?utm_source=x\"").unwrap();
        assert_eq!(url.as_str(), "https://acme.com/jobs/1");
        assert_eq!(
            serde_json::to_string(&url).unwrap(),
            "\"https://acme.com/jobs/1\""
        );
        assert!(serde_json::from_str::<CanonicalUrl>("\"nope\"").is_err());
    }
}
