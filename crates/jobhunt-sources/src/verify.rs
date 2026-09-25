//! First-party verification over HTTP: [`HttpVerifier`] asks one job's
//! authoritative source whether it is still listed, using each family's
//! narrowest public endpoint and the same conversion code as discovery.
//!
//! | kind         | listing                                                  | application path                                  |
//! |--------------|----------------------------------------------------------|---------------------------------------------------|
//! | `greenhouse` | `boards-api…/v1/boards/<board>/jobs/<id>` (404 = closed) | the board's hosted job page, which holds the form |
//! | `lever`      | `api.lever.co/v0/postings/<site>/<id>` (404 = closed)    | the posting's `/apply` page                       |
//! | `ashby`      | the whole board (Ashby has no single-job endpoint); a job missing from it is closed | the `applyUrl` the board publishes (not requested: Ashby pages render in the browser) |
//! | `yc`         | the job's Work at a Startup page (its embedded page data) | the application URL the page names               |
//!
//! Everything goes through the shared [`HttpClient`] (timeouts, bounded
//! retries honoring `Retry-After`, per-host limits). No browser is used:
//! every supported family publishes what verification needs as plain HTTP
//! responses. A family whose listing is only visible after JavaScript runs
//! would get a browser-backed [`ListingVerifier`] of its own, isolated from
//! the domain.

use jobhunt_core::text::clean_line;
use jobhunt_core::{CanonicalUrl, SourceError};
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationCheck, ApplicationStatus, AuthorityLink, LinkKind,
    ListingVerifier, ObserveError, ObservedListing, SourceObservation, VerificationMethod,
};
use jobhunt_jobs::{JobPosting, JobRecord};
use url::Url;

use crate::http::{HttpClient, Probe};
use crate::{ashby, greenhouse, lever, yc};

/// Where each family's endpoints live. The defaults are the real hosts;
/// tests point them at a local server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierHosts {
    pub greenhouse_api: String,
    /// Greenhouse's hosted boards (`job-boards.greenhouse.io`).
    pub greenhouse_boards: String,
    pub lever_api: String,
    pub lever_eu_api: String,
    /// Lever's hosted pages (`jobs.lever.co`).
    pub lever_jobs: String,
    pub ashby_api: String,
    pub yc: String,
    /// Work at a Startup, where YC applications live.
    pub workatastartup: String,
}

impl Default for VerifierHosts {
    fn default() -> Self {
        Self {
            greenhouse_api: greenhouse::DEFAULT_API_BASE.into(),
            greenhouse_boards: "https://job-boards.greenhouse.io".into(),
            lever_api: lever::DEFAULT_API_BASE.into(),
            lever_eu_api: lever::EU_API_BASE.into(),
            lever_jobs: "https://jobs.lever.co".into(),
            ashby_api: ashby::DEFAULT_API_BASE.into(),
            yc: yc::DEFAULT_BASE.into(),
            workatastartup: "https://www.workatastartup.com".into(),
        }
    }
}

impl VerifierHosts {
    /// Every host at one base URL (a local mock server).
    pub fn all_at(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        Self {
            greenhouse_api: base.clone(),
            greenhouse_boards: base.clone(),
            lever_api: base.clone(),
            lever_eu_api: base.clone(),
            lever_jobs: base.clone(),
            ashby_api: base.clone(),
            yc: base.clone(),
            workatastartup: base,
        }
    }
}

/// Verifies Ashby, Greenhouse, Lever and YC jobs over plain HTTP.
#[derive(Debug, Clone)]
pub struct HttpVerifier {
    http: HttpClient,
    hosts: VerifierHosts,
}

impl HttpVerifier {
    pub fn new(http: HttpClient) -> Self {
        Self::with_hosts(http, VerifierHosts::default())
    }

    pub fn with_hosts(http: HttpClient, hosts: VerifierHosts) -> Self {
        Self { http, hosts }
    }
}

#[async_trait::async_trait]
impl ListingVerifier for HttpVerifier {
    async fn observe(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let kind = record.posting.provenance.source.kind();
        match kind {
            greenhouse::KIND => self.greenhouse(record).await,
            lever::KIND => self.lever(record).await,
            ashby::KIND => self.ashby(record).await,
            yc::KIND => self.yc(record).await,
            other => Err(ObserveError::NotSupported {
                kind: other.to_owned(),
            }),
        }
    }
}

fn url(base: &str, segments: &[&str], query: &[(&str, &str)]) -> Result<Url, ObserveError> {
    crate::common::endpoint(base, segments, query).map_err(|e| ObserveError::Request {
        url: base.to_owned(),
        detail: e.to_string(),
    })
}

/// A source error as an observation error; `None` for "not found".
fn observe_error(error: SourceError) -> Option<ObserveError> {
    Some(match error {
        SourceError::NotFound { .. }
        | SourceError::Status {
            status: 404 | 410, ..
        } => {
            return None;
        }
        SourceError::Timeout { url } => ObserveError::Timeout { url },
        SourceError::Status { url, status } => ObserveError::Unavailable {
            url,
            status: Some(status),
            detail: format!("HTTP {status}"),
        },
        SourceError::Request { url, source } => ObserveError::Request {
            url,
            detail: jobhunt_core::ErrorChain(source.as_ref()).to_string(),
        },
        SourceError::Decode { url, source } => ObserveError::Malformed {
            url,
            detail: jobhunt_core::ErrorChain(source.as_ref()).to_string(),
        },
        SourceError::Config(detail) => ObserveError::Request {
            url: String::new(),
            detail,
        },
    })
}

fn source_id(record: &JobRecord) -> Result<&str, ObserveError> {
    record
        .posting
        .provenance
        .source_record_id
        .as_deref()
        .ok_or_else(|| ObserveError::NotSupported {
            kind: format!(
                "{} records without a source id",
                record.posting.provenance.source.kind()
            ),
        })
}

fn malformed(url: &Url, detail: impl std::fmt::Display) -> ObserveError {
    ObserveError::Malformed {
        url: url.to_string(),
        detail: detail.to_string(),
    }
}

fn link(kind: LinkKind, url: &Url, checked: bool, note: impl Into<String>) -> AuthorityLink {
    AuthorityLink {
        kind,
        url: url.to_string(),
        checked,
        note: note.into(),
    }
}

/// `url` moved onto `base` when its host is one of `hosts` (so tests can
/// serve pages the source names by their real host).
fn rebase(url: &str, hosts: &[&str], base: &str) -> Option<Url> {
    let mut parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_owned();
    if hosts.contains(&host.as_str()) {
        let base = Url::parse(base).ok()?;
        parsed.set_scheme(base.scheme()).ok()?;
        parsed.set_host(base.host_str()).ok()?;
        parsed.set_port(base.port()).ok()?;
    }
    Some(parsed)
}

impl HttpVerifier {
    /// `url` on the configured hosts: unchanged against the real hosts
    /// (so an EU Lever page stays on `jobs.eu.lever.co`), moved onto the
    /// test server otherwise.
    fn target(&self, url: &str, hosts: &[&str], base: &str) -> Option<Url> {
        if self.hosts == VerifierHosts::default() {
            Url::parse(url).ok()
        } else {
            rebase(url, hosts, base)
        }
    }

    /// Requests an application route and says what its answer means.
    async fn application(
        &self,
        target: Url,
        is_open: impl Fn(&Probe) -> bool,
        chain: &mut Vec<AuthorityLink>,
        note: &str,
    ) -> ApplicationCheck {
        match self.http.probe(&target).await {
            Ok(probe) => {
                let open = (200..300).contains(&probe.status) && is_open(&probe);
                let gone = matches!(probe.status, 404 | 410)
                    || ((200..300).contains(&probe.status) && !open);
                chain.push(link(
                    LinkKind::ApplicationPage,
                    &probe.final_url,
                    true,
                    note.to_owned(),
                ));
                ApplicationCheck {
                    status: if open {
                        ApplicationStatus::Active
                    } else if gone {
                        ApplicationStatus::Closed
                    } else {
                        ApplicationStatus::Unavailable
                    },
                    basis: ApplicationBasis::Probed,
                    url: Some(target.to_string()),
                    http_status: Some(probe.status),
                    detail: (!open).then(|| {
                        if probe.final_url != target {
                            format!(
                                "HTTP {} after redirecting to {}",
                                probe.status, probe.final_url
                            )
                        } else {
                            format!("HTTP {}", probe.status)
                        }
                    }),
                }
            }
            Err(error) => ApplicationCheck {
                status: ApplicationStatus::Unavailable,
                basis: ApplicationBasis::Probed,
                url: Some(target.to_string()),
                http_status: None,
                detail: Some(jobhunt_core::ErrorChain(&error).to_string()),
            },
        }
    }

    async fn greenhouse(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let board = record.posting.provenance.source.instance();
        let id = source_id(record)?;
        let endpoint = url(
            &self.hosts.greenhouse_api,
            &["v1", "boards", board, "jobs", id],
            &[("pay_transparency", "true")],
        )?;
        let mut chain = vec![link(
            LinkKind::AtsApi,
            &endpoint,
            true,
            format!("Greenhouse Job Board API for the {board:?} board"),
        )];
        let body = match self.http.get_bytes(&endpoint).await {
            Ok(body) => body,
            Err(error) => {
                return match observe_error(error) {
                    Some(error) => Err(error),
                    None => Ok(SourceObservation {
                        method: VerificationMethod::GreenhouseJobApi,
                        checked_url: endpoint.to_string(),
                        listing: ObservedListing::NotFound {
                            detail: "Greenhouse answered 404 for the job".into(),
                        },
                        application: ApplicationCheck::unknown(
                            "not checked: the listing is closed",
                        ),
                        chain,
                        unknowns: Vec::new(),
                    }),
                };
            }
        };
        let raw: greenhouse::GreenhousePosting =
            serde_json::from_slice(&body).map_err(|e| malformed(&endpoint, e))?;
        let context = greenhouse::BoardContext {
            source: record.posting.provenance.source.clone(),
            board: board.to_owned(),
            company: Some(record.posting.company.clone()),
            fetched_from: CanonicalUrl::parse(endpoint.as_str()).ok(),
        };
        let posting = greenhouse::to_posting(raw, &context).map_err(|e| malformed(&endpoint, e))?;

        // The application form is on the board's hosted job page. A closed
        // job redirects to the board with `error=true`.
        let page = url(&self.hosts.greenhouse_boards, &[board, "jobs", id], &[])?;
        let job_path = format!("/jobs/{id}");
        let application = self
            .application(
                page,
                |probe| {
                    probe.final_url.path().contains(&job_path)
                        && !probe
                            .final_url
                            .query_pairs()
                            .any(|(k, v)| k == "error" && v == "true")
                },
                &mut chain,
                "the board's hosted job page, which holds the application form",
            )
            .await;
        Ok(SourceObservation {
            method: VerificationMethod::GreenhouseJobApi,
            checked_url: endpoint.to_string(),
            listing: ObservedListing::Found(Box::new(posting)),
            application,
            chain,
            unknowns: Vec::new(),
        })
    }

    async fn lever(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let site = record.posting.provenance.source.instance();
        let id = source_id(record)?;
        let eu = record
            .posting
            .provenance
            .fetched_from
            .as_ref()
            .is_some_and(|u| u.host() == "api.eu.lever.co");
        let base = if eu {
            &self.hosts.lever_eu_api
        } else {
            &self.hosts.lever_api
        };
        let endpoint = url(base, &["v0", "postings", site, id], &[("mode", "json")])?;
        let mut chain = vec![link(
            LinkKind::AtsApi,
            &endpoint,
            true,
            format!("Lever Postings API for the {site:?} site"),
        )];
        let body = match self.http.get_bytes(&endpoint).await {
            Ok(body) => body,
            Err(error) => {
                return match observe_error(error) {
                    Some(error) => Err(error),
                    None => Ok(SourceObservation {
                        method: VerificationMethod::LeverPostingApi,
                        checked_url: endpoint.to_string(),
                        listing: ObservedListing::NotFound {
                            detail: "Lever answered 404 for the posting".into(),
                        },
                        application: ApplicationCheck::unknown(
                            "not checked: the listing is closed",
                        ),
                        chain,
                        unknowns: Vec::new(),
                    }),
                };
            }
        };
        let raw: lever::LeverPosting =
            serde_json::from_slice(&body).map_err(|e| malformed(&endpoint, e))?;
        let context = lever::SiteContext {
            source: record.posting.provenance.source.clone(),
            company: record.posting.company.clone(),
            fetched_from: CanonicalUrl::parse(endpoint.as_str()).ok(),
        };
        let posting = lever::to_posting(raw, &context).map_err(|e| malformed(&endpoint, e))?;
        let apply = posting
            .apply_url
            .as_ref()
            .map(|u| u.to_string())
            .unwrap_or_else(|| format!("{}/apply", posting.url.as_str().trim_end_matches('/')));
        let application = match self.target(
            &apply,
            &["jobs.lever.co", "jobs.eu.lever.co"],
            &self.hosts.lever_jobs,
        ) {
            Some(target) => {
                self.application(
                    target,
                    |_| true,
                    &mut chain,
                    "the posting's application page",
                )
                .await
            }
            None => ApplicationCheck::unknown(format!("unusable application URL {apply:?}")),
        };
        Ok(SourceObservation {
            method: VerificationMethod::LeverPostingApi,
            checked_url: endpoint.to_string(),
            listing: ObservedListing::Found(Box::new(posting)),
            application,
            chain,
            unknowns: Vec::new(),
        })
    }

    async fn ashby(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let board = record.posting.provenance.source.instance();
        let id = source_id(record)?;
        let endpoint = url(
            &self.hosts.ashby_api,
            &["posting-api", "job-board", board],
            &[("includeCompensation", "true")],
        )?;
        let chain = vec![link(
            LinkKind::AtsApi,
            &endpoint,
            true,
            format!(
                "Ashby posting API for the {board:?} board (the whole board: Ashby has no single-job endpoint)"
            ),
        )];
        let body = match self.http.get_bytes(&endpoint).await {
            Ok(body) => body,
            Err(error) => {
                return match observe_error(error) {
                    Some(error) => Err(error),
                    None => Ok(SourceObservation {
                        method: VerificationMethod::AshbyBoardApi,
                        checked_url: endpoint.to_string(),
                        listing: ObservedListing::NotFound {
                            detail: "the Ashby board no longer exists (HTTP 404)".into(),
                        },
                        application: ApplicationCheck::unknown(
                            "not checked: the listing is closed",
                        ),
                        chain,
                        unknowns: Vec::new(),
                    }),
                };
            }
        };
        let context = ashby::BoardContext {
            source: record.posting.provenance.source.clone(),
            company: record.posting.company.clone(),
            fetched_from: CanonicalUrl::parse(endpoint.as_str()).ok(),
        };
        let batch = ashby::parse_board(&body, &context).map_err(|e| malformed(&endpoint, e))?;
        let listed = batch
            .records
            .into_iter()
            .find(|p| p.provenance.source_record_id.as_deref() == Some(id));
        let rejected = batch
            .rejected
            .iter()
            .any(|r| r.record_id.as_deref() == Some(id));
        let (listing, application) = match listed {
            Some(posting) => {
                let application = match &posting.apply_url {
                    Some(apply) => ApplicationCheck {
                        status: ApplicationStatus::Active,
                        basis: ApplicationBasis::Published,
                        url: Some(apply.to_string()),
                        http_status: None,
                        detail: Some(
                            "published by the board API; Ashby's application pages render in the browser and were not requested"
                                .into(),
                        ),
                    },
                    None => ApplicationCheck::unknown("the board publishes no application URL"),
                };
                (ObservedListing::Found(Box::new(posting)), application)
            }
            None if rejected => {
                return Err(ObserveError::Malformed {
                    url: endpoint.to_string(),
                    detail: "the board lists the job, but its record could not be read".into(),
                });
            }
            None => (
                ObservedListing::MissingFromCompleteListing {
                    detail: "the job is missing from the board's complete listing".into(),
                },
                ApplicationCheck::unknown("not checked: the listing is closed"),
            ),
        };
        Ok(SourceObservation {
            method: VerificationMethod::AshbyBoardApi,
            checked_url: endpoint.to_string(),
            listing,
            application,
            chain,
            unknowns: Vec::new(),
        })
    }

    async fn yc(&self, record: &JobRecord) -> Result<SourceObservation, ObserveError> {
        let page = self
            .target(
                record.posting.url.as_str(),
                &["www.ycombinator.com", "ycombinator.com"],
                &self.hosts.yc,
            )
            .ok_or_else(|| ObserveError::Request {
                url: record.posting.url.to_string(),
                detail: "unusable job URL".into(),
            })?;
        let mut chain = vec![link(
            LinkKind::PlatformPage,
            &page,
            true,
            "the job's page on Y Combinator's Work at a Startup, posted by the company",
        )];
        let not_found = |detail: &str, chain: Vec<AuthorityLink>| {
            Ok(SourceObservation {
                method: VerificationMethod::YcJobPage,
                checked_url: page.to_string(),
                listing: ObservedListing::NotFound {
                    detail: detail.to_owned(),
                },
                application: ApplicationCheck::unknown("not checked: the listing is closed"),
                chain,
                unknowns: Vec::new(),
            })
        };
        let probe = self.http.probe(&page).await.map_err(|e| {
            observe_error(e).unwrap_or_else(|| ObserveError::Request {
                url: page.to_string(),
                detail: "not found".into(),
            })
        })?;
        match probe.status {
            404 | 410 => {
                return not_found(
                    &format!("the job page answered HTTP {}", probe.status),
                    chain,
                );
            }
            200..=299 => {}
            status => {
                return Err(ObserveError::Unavailable {
                    url: page.to_string(),
                    status: Some(status),
                    detail: format!("HTTP {status}"),
                });
            }
        }
        let data = yc::page_data(&probe.body).map_err(|e| malformed(&page, e))?;
        let job = data.props.get("job").filter(|j| j.is_object()).cloned();
        let job = match (data.component.as_str(), job) {
            ("WaasShowJobPage", Some(job)) => job,
            // A removed job's page redirects to the company's jobs page.
            ("WaasShowJobsPage", _) => {
                return not_found("the job page now shows the company's job list", chain);
            }
            (other, _) => {
                return Err(malformed(&page, format!("unexpected page {other:?}")));
            }
        };
        let context = yc::CompanyContext {
            source: record.posting.provenance.source.clone(),
            company: record.posting.company.clone(),
            fetched_from: CanonicalUrl::parse(page.as_str()).ok(),
        };
        let posting: JobPosting = yc::to_posting(job, &context).map_err(|e| malformed(&page, e))?;
        let mut unknowns = Vec::new();
        let application = match posting.apply_url.as_ref().and_then(|u| {
            self.target(
                u.as_str(),
                &["www.workatastartup.com", "workatastartup.com"],
                &self.hosts.workatastartup,
            )
        }) {
            Some(target) => {
                unknowns
                    .push("whether the application needs a Work at a Startup account".to_owned());
                self.application(
                    target,
                    |_| true,
                    &mut chain,
                    "the Work at a Startup application page",
                )
                .await
            }
            None => ApplicationCheck::unknown("the page names no application URL"),
        };
        if let Some(visa) = posting.work_authorization.as_deref().and_then(clean_line) {
            chain.push(AuthorityLink {
                kind: LinkKind::PlatformPage,
                url: page.to_string(),
                checked: true,
                note: format!("the company's own visa field: {visa:?}"),
            });
        }
        Ok(SourceObservation {
            method: VerificationMethod::YcJobPage,
            checked_url: page.to_string(),
            listing: ObservedListing::Found(Box::new(posting)),
            application,
            chain,
            unknowns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebases_known_hosts_only() {
        let moved = rebase(
            "https://jobs.lever.co/spotify/abc/apply",
            &["jobs.lever.co"],
            "http://127.0.0.1:9999",
        )
        .unwrap();
        assert_eq!(moved.as_str(), "http://127.0.0.1:9999/spotify/abc/apply");
        let kept = rebase(
            "https://careers.example.com/apply",
            &["jobs.lever.co"],
            "http://127.0.0.1:9999",
        )
        .unwrap();
        assert_eq!(kept.host_str(), Some("careers.example.com"));
    }

    #[test]
    fn not_found_is_an_answer_not_an_error() {
        assert!(
            observe_error(SourceError::Status {
                url: "u".into(),
                status: 404
            })
            .is_none()
        );
        assert!(matches!(
            observe_error(SourceError::Timeout { url: "u".into() }),
            Some(ObserveError::Timeout { .. })
        ));
        assert!(matches!(
            observe_error(SourceError::Status {
                url: "u".into(),
                status: 503
            }),
            Some(ObserveError::Unavailable {
                status: Some(503),
                ..
            })
        ));
    }
}
