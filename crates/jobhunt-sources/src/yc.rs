//! Y Combinator companies' jobs (Work at a Startup).
//!
//! YC publishes every company's open Work at a Startup jobs on
//! `www.ycombinator.com/companies/<slug>/jobs`, without authentication.
//! There is no documented jobs API. The pages are server-rendered Inertia.js
//! pages: each one embeds its complete data as JSON in a `data-page`
//! attribute, which is what the browser app itself renders from. That JSON
//! is stable, structured and far less fragile than scraping markup, so this
//! adapter reads it over plain HTTP; no browser is needed.
//!
//! * `GET /companies/<slug>/jobs` (`WaasShowJobsPage`) lists the company's
//!   open jobs, all of them, so it is a complete listing for that company.
//! * `GET /companies/<slug>/jobs/<id>-<title>` (`WaasShowJobPage`) adds the
//!   description (Markdown). Detail pages are fetched with bounded
//!   concurrency; a job whose detail page fails is reported as unavailable
//!   (not dropped, and never closed because of it).
//!
//! The listing's `/jobs` pages by role (`/jobs/role/...`) show only a
//! sample of jobs and are therefore not used. The site-wide set of hiring
//! companies comes from the directory's Algolia index; enumerating it means
//! thousands of page loads per scan, so it is left to configuration: list
//! the companies to follow.
//!
//! Mapping notes: `location` is a " / "-separated list ("San Francisco, CA,
//! US / Remote (US)"); a job is remote when every option is remote, hybrid
//! when every option is hybrid, and otherwise its workplace stays unknown
//! (with `is_remote` set when any option is remote). `salaryRange` and
//! `equityRange` are display strings ("$190K - $215K", "0.5% - 1.0%") parsed
//! into components when unambiguous. Publish dates are only given as
//! relative text ("5 months"), so they stay unknown. The apply link is YC's
//! sign-in redirect; the Work at a Startup application URL it wraps is used.

use futures::StreamExt;
use jobhunt_core::html::decode_entities;
use jobhunt_core::text::{clean_block, clean_line, clean_line_opt};
use jobhunt_core::{
    CanonicalUrl, FetchRequest, Fetched, Provenance, RecordError, RecordErrorReason, Source,
    SourceBatch, SourceError, SourceKey,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, SourceLocation,
    WorkplaceType,
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::common::{employment_type, endpoint, null_as_default, raw_record_id};
use crate::http::{HttpClient, not_found_as};

/// Source kind used in keys such as `yc:posthog`.
pub const KIND: &str = "yc";

pub const DEFAULT_BASE: &str = "https://www.ycombinator.com";

/// Detail pages fetched at the same time for one company.
const DETAIL_CONCURRENCY: usize = 4;

/// One YC company to follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YcCompany {
    /// The company's slug in `https://www.ycombinator.com/companies/<slug>`.
    pub slug: String,
    /// Company display name. Defaults to the name YC reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
}

/// Companies read when the user has not configured any.
pub fn default_companies() -> Vec<YcCompany> {
    ["posthog", "doordash"]
        .into_iter()
        .map(|slug| YcCompany {
            slug: slug.to_owned(),
            company: None,
        })
        .collect()
}

/// Reads one YC company's jobs.
#[derive(Debug)]
pub struct YcSource {
    key: SourceKey,
    company: YcCompany,
    base: Url,
    listing: Url,
    http: HttpClient,
}

impl YcSource {
    pub fn new(key: SourceKey, company: YcCompany, http: HttpClient) -> Result<Self, SourceError> {
        Self::with_base(key, company, http, DEFAULT_BASE)
    }

    /// Like [`YcSource::new`] but against another host (tests).
    pub fn with_base(
        key: SourceKey,
        company: YcCompany,
        http: HttpClient,
        base: &str,
    ) -> Result<Self, SourceError> {
        let listing = endpoint(base, &["companies", &company.slug, "jobs"], &[])?;
        let base = Url::parse(base)
            .map_err(|e| SourceError::Config(format!("invalid YC base URL {base:?}: {e}")))?;
        Ok(Self {
            key,
            company,
            base,
            listing,
            http,
        })
    }

    pub fn listing_url(&self) -> &Url {
        &self.listing
    }

    fn decode_error(&self, url: &Url, error: PageError) -> SourceError {
        SourceError::Decode {
            url: url.to_string(),
            source: Box::new(error),
        }
    }

    async fn detail(&self, stub: &ListingStub) -> Result<serde_json::Value, RecordErrorReason> {
        let url = self
            .base
            .join(&stub.path)
            .map_err(|e| RecordErrorReason::Malformed(format!("job url: {e}")))?;
        let body = self.http.get_bytes(&url).await.map_err(|e| {
            RecordErrorReason::Unavailable(jobhunt_core::ErrorChain(&e).to_string())
        })?;
        let page = page_data(&body).map_err(|e| RecordErrorReason::Unavailable(e.to_string()))?;
        page.props
            .get("job")
            .cloned()
            .filter(serde_json::Value::is_object)
            .ok_or_else(|| RecordErrorReason::Unavailable("detail page has no job".into()))
    }
}

#[async_trait::async_trait]
impl Source for YcSource {
    type Record = JobPosting;

    fn key(&self) -> &SourceKey {
        &self.key
    }

    async fn fetch(&self, _request: &FetchRequest) -> Result<Fetched<JobPosting>, SourceError> {
        // YC pages embed per-request tokens, so their validators change on
        // every response; conditional requests are pointless.
        let body = self
            .http
            .get_bytes(&self.listing)
            .await
            .map_err(not_found_as(|| {
                format!("YC company {:?}", self.company.slug)
            }))?;
        let listing =
            parse_listing(&body, &self.company).map_err(|e| self.decode_error(&self.listing, e))?;
        let context = CompanyContext {
            source: self.key.clone(),
            company: listing.company.clone(),
            fetched_from: CanonicalUrl::parse(self.listing.as_str()).ok(),
        };

        let mut batch = SourceBatch {
            rejected: listing.rejected,
            complete: true,
            ..SourceBatch::default()
        };
        let details: Vec<(ListingStub, Result<serde_json::Value, RecordErrorReason>)> =
            futures::stream::iter(listing.jobs)
                .map(|stub| async move {
                    let detail = self.detail(&stub).await;
                    (stub, detail)
                })
                .buffered(DETAIL_CONCURRENCY)
                .collect()
                .await;
        for (stub, detail) in details {
            let converted = detail
                .map_err(|reason| RecordError::new(Some(stub.id.clone()), reason))
                .and_then(|job| to_posting(job, &context));
            match converted {
                Ok(posting) => batch.records.push(posting),
                Err(error) => batch.rejected.push(error),
            }
        }
        Ok(Fetched::Batch(batch))
    }
}

/// What conversion needs to know about the company being read.
#[derive(Debug, Clone)]
pub struct CompanyContext {
    pub source: SourceKey,
    pub company: String,
    pub fetched_from: Option<CanonicalUrl>,
}

/// A job listed on a company's jobs page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingStub {
    pub id: String,
    /// Site-relative path of the job's page.
    pub path: String,
}

/// A parsed company jobs page.
#[derive(Debug)]
pub struct Listing {
    /// Display name: configured, else YC's.
    pub company: String,
    pub jobs: Vec<ListingStub>,
    pub rejected: Vec<RecordError>,
}

/// Parses a `/companies/<slug>/jobs` page.
pub fn parse_listing(body: &[u8], company: &YcCompany) -> Result<Listing, PageError> {
    let page = page_data(body)?;
    if page.component != "WaasShowJobsPage" {
        return Err(PageError::UnexpectedPage(page.component));
    }
    let props: ListingProps = serde_json::from_value(page.props).map_err(PageError::Json)?;
    let name = company
        .company
        .as_deref()
        .and_then(clean_line)
        .or_else(|| {
            props
                .company
                .and_then(|c| c.name)
                .as_deref()
                .and_then(clean_line)
        })
        .unwrap_or_else(|| company.slug.clone());

    let prefix = format!("/companies/{}/jobs/", company.slug);
    let mut listing = Listing {
        company: name,
        jobs: Vec::new(),
        rejected: Vec::new(),
    };
    for value in props.job_postings {
        let id = raw_record_id(&value, "id");
        let path = value
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim);
        match (id, path) {
            (Some(id), Some(path)) if path.starts_with(&prefix) => {
                listing.jobs.push(ListingStub {
                    id,
                    path: path.to_owned(),
                });
            }
            (id, path) => listing.rejected.push(RecordError::new(
                id,
                RecordErrorReason::InvalidValue {
                    field: "url",
                    detail: format!("expected a path under {prefix}, got {path:?}"),
                },
            )),
        }
    }
    Ok(listing)
}

/// Converts one job (from its detail page) into the canonical model.
pub fn to_posting(
    job: serde_json::Value,
    context: &CompanyContext,
) -> Result<JobPosting, RecordError> {
    let record_id = raw_record_id(&job, "id");
    let reject = |reason| RecordError::new(record_id.clone(), reason);
    if record_id.is_none() {
        return Err(reject(RecordErrorReason::MissingField("id")));
    }
    let raw: YcJob = serde_json::from_value(job)
        .map_err(|e| reject(RecordErrorReason::Malformed(e.to_string())))?;

    let title = clean_line_opt(raw.title.as_deref())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("title")))?;
    let path = raw
        .url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("url")))?;
    let url = CanonicalUrl::parse(&format!("{DEFAULT_BASE}{}", path.trim())).map_err(|error| {
        reject(RecordErrorReason::InvalidUrl {
            field: "url",
            error,
        })
    })?;
    let apply_url = raw.apply_url.as_deref().and_then(application_url);

    let location = clean_line_opt(raw.location.as_deref());
    let options: Vec<&str> = location
        .as_deref()
        .map(|l| {
            l.split(" / ")
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let is = |option: &&str, word: &str| option.to_ascii_lowercase().starts_with(word);
    let workplace_type = if !options.is_empty() && options.iter().all(|o| is(o, "remote")) {
        Some(WorkplaceType::Remote)
    } else if !options.is_empty() && options.iter().all(|o| is(o, "hybrid")) {
        Some(WorkplaceType::Hybrid)
    } else {
        None
    };
    let is_remote = options.iter().any(|o| is(o, "remote")).then_some(true);

    Ok(JobPosting {
        provenance: Provenance {
            source: context.source.clone(),
            source_record_id: record_id.clone(),
            fetched_from: context.fetched_from.clone(),
        },
        url,
        apply_url,
        company: context.company.clone(),
        title,
        department: clean_line_opt(raw.pretty_role.as_deref()),
        team: None,
        locations: options.iter().map(|o| option_location(o)).collect(),
        location,
        employment_type: raw.job_type.as_deref().and_then(employment_type),
        workplace_type,
        is_remote,
        compensation: compensation(raw.salary_range.as_deref(), raw.equity_range.as_deref()),
        work_authorization: clean_line_opt(raw.visa.as_deref()),
        description_text: raw.description.as_deref().and_then(markdown_to_text),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    })
}

/// A location option as a structured location. "Berkeley, CA, US" has a
/// locality, region and country code; anything else keeps just its name.
fn option_location(option: &str) -> SourceLocation {
    let parts: Vec<&str> = option.split(',').map(str::trim).collect();
    let structured =
        parts.len() == 3 && !option.contains('(') && parts.iter().all(|p| !p.is_empty());
    SourceLocation {
        name: Some(option.to_owned()),
        locality: structured.then(|| parts[0].to_owned()),
        region: structured.then(|| parts[1].to_owned()),
        country: structured.then(|| parts[2].to_owned()),
    }
}

/// YC's apply link is a sign-in redirect whose `continue` parameter is the
/// Work at a Startup application URL for the job; that is the stable part.
fn application_url(value: &str) -> Option<CanonicalUrl> {
    let url = Url::parse(value.trim()).ok()?;
    let target = url
        .query_pairs()
        .find(|(k, _)| k == "continue")
        .map(|(_, v)| v.into_owned());
    let parsed = match target {
        Some(target) => CanonicalUrl::parse(&target),
        None => CanonicalUrl::parse(url.as_str()),
    };
    parsed.ok()
}

/// Salary and equity display strings as compensation.
fn compensation(salary: Option<&str>, equity: Option<&str>) -> Option<Compensation> {
    let salary = salary.and_then(clean_line);
    let equity = equity.and_then(clean_line);
    let mut components = Vec::new();
    if let Some((currency, min, max)) = salary.as_deref().and_then(parse_money_range) {
        components.push(CompensationComponent {
            kind: CompensationKind::Salary,
            label: None,
            currency,
            min: Some(min),
            max: Some(max),
            interval: None,
        });
    }
    if let Some((min, max)) = equity.as_deref().and_then(parse_percent_range) {
        components.push(CompensationComponent {
            kind: CompensationKind::EquityPercentage,
            label: None,
            currency: None,
            min: Some(min),
            max: Some(max),
            interval: None,
        });
    }
    let summary = match (&salary, &equity) {
        (Some(s), Some(e)) => Some(format!("{s} • {e} equity")),
        (Some(s), None) => Some(s.clone()),
        (None, Some(e)) => Some(format!("{e} equity")),
        (None, None) => None,
    };
    summary.map(|summary| Compensation {
        summary: Some(summary),
        components,
    })
}

/// "US$190K - US$215K" → (USD, 190000, 215000); "$190K - $215K" leaves the
/// currency unknown ("$" is several currencies). Only unambiguous currency
/// symbols are named; anything unexpected yields `None`.
fn parse_money_range(text: &str) -> Option<(Option<String>, f64, f64)> {
    let (low, high) = split_range(text)?;
    let (currency_a, min) = parse_money(low)?;
    let (currency_b, max) = parse_money(high)?;
    let currency = match (currency_a, currency_b) {
        (Money::Code(a), Money::Code(b)) if a == b => Some(a),
        (Money::Code(a), Money::Bare) | (Money::Bare, Money::Code(a)) => Some(a),
        (Money::Dollar | Money::Bare, Money::Dollar | Money::Bare) => None,
        _ => return None,
    };
    (min <= max).then_some((currency.map(str::to_owned), min, max))
}

/// What an amount's symbol says about its currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Money {
    Code(&'static str),
    /// "$": one of several currencies.
    Dollar,
    /// No symbol at all.
    Bare,
}

fn parse_money(text: &str) -> Option<(Money, f64)> {
    let text = text.trim();
    // "$" is written by several currencies (USD, CAD, SGD, ...) and so
    // leaves the currency unknown; the display text keeps the symbol.
    let (currency, rest) = [
        ("US$", Money::Code("USD")),
        ("CA$", Money::Code("CAD")),
        ("$", Money::Dollar),
        ("€", Money::Code("EUR")),
        ("£", Money::Code("GBP")),
        ("₹", Money::Code("INR")),
    ]
    .iter()
    .find_map(|(symbol, money)| text.strip_prefix(symbol).map(|rest| (*money, rest)))
    .unwrap_or((Money::Bare, text));
    let rest = rest.trim();
    let (number, multiplier) = match rest.chars().last()? {
        'K' | 'k' => (&rest[..rest.len() - 1], 1_000.0),
        'M' | 'm' => (&rest[..rest.len() - 1], 1_000_000.0),
        _ => (rest, 1.0),
    };
    let value: f64 = number.replace(',', "").trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some((currency, value * multiplier))
}

/// "0.5% - 1.0%" → (0.5, 1.0).
fn parse_percent_range(text: &str) -> Option<(f64, f64)> {
    let (low, high) = split_range(text)?;
    let parse = |s: &str| s.trim().strip_suffix('%')?.trim().parse::<f64>().ok();
    let (min, max) = (parse(low)?, parse(high)?);
    (min.is_finite() && max.is_finite() && 0.0 <= min && min <= max).then_some((min, max))
}

fn split_range(text: &str) -> Option<(&str, &str)> {
    text.split_once(" - ")
        .or_else(|| text.split_once(" – "))
        .or_else(|| text.split_once('-'))
}

/// Converts Work at a Startup Markdown into plain text: headings and
/// emphasis markers are dropped, bullets become `•`, links keep their text,
/// and Markdown hard breaks (`\` at the end of a line) become line breaks.
pub fn markdown_to_text(markdown: &str) -> Option<String> {
    let decoded = decode_entities(markdown);
    let mut lines = Vec::new();
    for line in decoded.replace("\r\n", "\n").lines() {
        let trimmed = line.trim_end();
        let trimmed = trimmed.strip_suffix('\\').unwrap_or(trimmed).trim_end();
        let indent = trimmed.len() - trimmed.trim_start().len();
        let content = trimmed.trim_start();
        let content = content.trim_start_matches('#');
        let content = if content.len() != trimmed.trim_start().len() {
            content.trim_start()
        } else {
            content
        };
        let (bullet, body) = match content
            .strip_prefix("* ")
            .or_else(|| content.strip_prefix("- "))
            .or_else(|| content.strip_prefix("+ "))
        {
            Some(rest) => ("• ", rest),
            None => ("", content),
        };
        let pad = if bullet.is_empty() {
            String::new()
        } else {
            " ".repeat(indent.min(8))
        };
        lines.push(format!("{pad}{bullet}{}", inline_markdown(body)));
    }
    clean_block(&lines.join("\n"))
}

/// Strips inline Markdown: `**`/`__` emphasis, `` ` `` code marks, links
/// (`[text](url)` → `text`), images, and backslash escapes.
fn inline_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '*' | '_' if i + 1 < chars.len() && chars[i + 1] == c => i += 2,
            '`' => i += 1,
            '!' if chars.get(i + 1) == Some(&'[') => i += 1,
            '[' => {
                // [text](target): keep the text, drop the target.
                let close = chars[i + 1..]
                    .iter()
                    .position(|&ch| ch == ']')
                    .map(|p| i + 1 + p);
                match close {
                    Some(close) if chars.get(close + 1) == Some(&'(') => {
                        let end = chars[close + 2..].iter().position(|&ch| ch == ')');
                        match end {
                            Some(end) => {
                                out.extend(&chars[i + 1..close]);
                                i = close + 2 + end + 1;
                            }
                            None => {
                                out.push(c);
                                i += 1;
                            }
                        }
                    }
                    _ => {
                        out.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// The `data-page` JSON of an Inertia page.
#[derive(Debug, Deserialize)]
pub struct PageData {
    pub component: String,
    pub props: serde_json::Value,
}

/// A page did not contain the expected embedded data.
#[derive(Debug, thiserror::Error)]
pub enum PageError {
    #[error("the page has no embedded data (data-page attribute)")]
    MissingData,
    #[error("the embedded page data is not valid JSON")]
    Json(#[source] serde_json::Error),
    #[error("expected a company jobs page, got {0:?}")]
    UnexpectedPage(String),
}

/// Extracts and decodes the first `data-page="..."` attribute.
pub fn page_data(body: &[u8]) -> Result<PageData, PageError> {
    let html = String::from_utf8_lossy(body);
    let start = html.find("data-page=\"").ok_or(PageError::MissingData)? + "data-page=\"".len();
    let end = html[start..].find('"').ok_or(PageError::MissingData)? + start;
    let json = decode_entities(&html[start..end]);
    serde_json::from_str(&json).map_err(PageError::Json)
}

// ---------------------------------------------------------------------------
// Raw payload types.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListingProps {
    company: Option<YcCompanyData>,
    #[serde(default, deserialize_with = "null_as_default")]
    job_postings: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct YcCompanyData {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct YcJob {
    title: Option<String>,
    url: Option<String>,
    apply_url: Option<String>,
    location: Option<String>,
    #[serde(rename = "type")]
    job_type: Option<String>,
    pretty_role: Option<String>,
    salary_range: Option<String>,
    equity_range: Option<String>,
    /// Who may apply, as YC asks companies to state it ("US citizen/visa
    /// only", "Will sponsor", ...).
    visa: Option<String>,
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_money_and_equity_ranges() {
        assert_eq!(
            parse_money_range("$190K - $215K"),
            Some((None, 190_000.0, 215_000.0)),
            "“$” does not say which dollar"
        );
        assert_eq!(
            parse_money_range("US$190K - US$215K"),
            Some((Some("USD".into()), 190_000.0, 215_000.0))
        );
        assert_eq!(
            parse_money_range("€50K - €70K"),
            Some((Some("EUR".into()), 50_000.0, 70_000.0))
        );
        assert_eq!(
            parse_money_range("120,000 - 150,000"),
            Some((None, 120_000.0, 150_000.0))
        );
        assert_eq!(parse_money_range("$215K - $190K"), None);
        assert_eq!(parse_money_range("$100K - €120K"), None);
        assert_eq!(parse_money_range("competitive"), None);
        assert_eq!(parse_percent_range("0.5% - 1.0%"), Some((0.5, 1.0)));
        assert_eq!(parse_percent_range("lots"), None);
    }

    #[test]
    fn compensation_keeps_the_display_text() {
        let comp = compensation(Some("$150K - $200K"), Some("0.1% - 0.5%")).unwrap();
        assert_eq!(
            comp.summary.as_deref(),
            Some("$150K - $200K • 0.1% - 0.5% equity")
        );
        assert_eq!(comp.components.len(), 2);
        let odd = compensation(Some("DOE"), None).unwrap();
        assert_eq!(odd.summary.as_deref(), Some("DOE"));
        assert!(odd.components.is_empty());
        assert_eq!(compensation(Some(""), Some(" ")), None);
    }

    #[test]
    fn unwraps_the_sign_in_redirect() {
        let apply = application_url(
            "https://account.ycombinator.com/authenticate?continue=https%3A%2F%2Fwww.workatastartup.com%2Fapplication%3Fsignup_job_id%3D93346&defaults%5BsignUpActive%5D=true&defaults%5Bwaas_company%5D=1965",
        )
        .unwrap();
        assert_eq!(
            apply.as_str(),
            "https://www.workatastartup.com/application?signup_job_id=93346"
        );
    }

    #[test]
    fn converts_markdown() {
        let md = "### **About the Role**\n\nWe're *great*. See [our site](https://x.com).\\\nNext line\n\n* First\n* **Second**\n  - Nested\n\n1. One\n\\* not a bullet";
        assert_eq!(
            markdown_to_text(md).unwrap(),
            "About the Role\n\nWe're *great*. See our site.\nNext line\n\n• First\n• Second\n  • Nested\n\n1. One\n* not a bullet"
        );
        assert_eq!(markdown_to_text(" \n "), None);
    }

    #[test]
    fn structures_location_options() {
        let l = option_location("Berkeley, CA, US");
        assert_eq!(
            (
                l.locality.as_deref(),
                l.region.as_deref(),
                l.country.as_deref()
            ),
            (Some("Berkeley"), Some("CA"), Some("US"))
        );
        let remote = option_location("Remote (San Francisco, CA, US; Oakland, CA, US)");
        assert_eq!(remote.country, None);
        assert_eq!(option_location("San Francisco, CA").locality, None);
    }

    #[test]
    fn page_data_requires_the_attribute() {
        assert!(matches!(
            page_data(b"<html></html>"),
            Err(PageError::MissingData)
        ));
        assert!(matches!(
            page_data(b"<div data-page=\"{nope\"></div>"),
            Err(PageError::Json(_))
        ));
        let page = page_data(
            b"<div data-page=\"{&quot;component&quot;:&quot;X&quot;,&quot;props&quot;:{}}\"></div>",
        )
        .unwrap();
        assert_eq!(page.component, "X");
    }
}
