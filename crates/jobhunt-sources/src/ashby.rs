//! Ashby job boards.
//!
//! Ashby hosts career pages for many startups at `jobs.ashbyhq.com/<board>`
//! and exposes each board through a public, unauthenticated JSON endpoint:
//!
//! ```text
//! GET https://api.ashbyhq.com/posting-api/job-board/<board>?includeCompensation=true
//! ```
//!
//! The payload carries structured employment type, workplace type, remote
//! flag, addresses and compensation, which map directly onto the canonical
//! model. It does not include the company's display name, so that comes from
//! configuration (falling back to the board name).
//!
//! One response is the whole board, so a successful fetch is a complete
//! listing. The endpoint sends an `ETag` and honors `If-None-Match`, which
//! makes an unchanged board cost a single empty `304` response.

use chrono::{DateTime, Utc};
use jobhunt_core::text::{clean_block_opt, clean_line, clean_line_opt};
use jobhunt_core::{
    CanonicalUrl, FetchRequest, Fetched, Provenance, RecordError, RecordErrorReason, Source,
    SourceBatch, SourceError, SourceKey,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, EmploymentType, JobPosting, PayInterval,
    SourceLocation, WorkplaceType,
};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::debug;
use url::Url;

use crate::http::{Conditional, HttpClient, not_found_as};

/// Source kind used in keys such as `ashby:linear`.
pub const KIND: &str = "ashby";

pub const DEFAULT_API_BASE: &str = "https://api.ashbyhq.com";

/// One Ashby job board to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AshbyBoard {
    /// Board name as it appears in `https://jobs.ashbyhq.com/<board>`.
    pub board: String,
    /// Company display name. Ashby's API does not return one; when unset the
    /// board name is shown as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
}

impl AshbyBoard {
    fn company_name(&self) -> String {
        self.company
            .as_deref()
            .and_then(clean_line)
            .unwrap_or_else(|| self.board.clone())
    }
}

/// Boards read when the user has not configured any: a small set of software
/// companies with active Ashby boards.
pub fn default_boards() -> Vec<AshbyBoard> {
    [
        ("linear", "Linear"),
        ("ramp", "Ramp"),
        ("notion", "Notion"),
        ("supabase", "Supabase"),
        ("posthog", "PostHog"),
        ("modal", "Modal"),
        ("replit", "Replit"),
        ("vanta", "Vanta"),
    ]
    .into_iter()
    .map(|(board, company)| AshbyBoard {
        board: board.to_owned(),
        company: Some(company.to_owned()),
    })
    .collect()
}

/// Reads one Ashby job board.
#[derive(Debug)]
pub struct AshbySource {
    key: SourceKey,
    board: AshbyBoard,
    endpoint: Url,
    context: BoardContext,
    http: HttpClient,
}

impl AshbySource {
    pub fn new(key: SourceKey, board: AshbyBoard, http: HttpClient) -> Result<Self, SourceError> {
        Self::with_api_base(key, board, http, DEFAULT_API_BASE)
    }

    /// Like [`AshbySource::new`] but against another API host (used by tests).
    pub fn with_api_base(
        key: SourceKey,
        board: AshbyBoard,
        http: HttpClient,
        api_base: &str,
    ) -> Result<Self, SourceError> {
        let endpoint = board_endpoint(api_base, &board.board)?;
        let context = BoardContext {
            source: key.clone(),
            company: board.company_name(),
            fetched_from: CanonicalUrl::parse(endpoint.as_str()).ok(),
        };
        Ok(Self {
            key,
            board,
            endpoint,
            context,
            http,
        })
    }

    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }
}

fn board_endpoint(api_base: &str, board: &str) -> Result<Url, SourceError> {
    let mut endpoint = Url::parse(api_base)
        .map_err(|e| SourceError::Config(format!("invalid Ashby API base {api_base:?}: {e}")))?;
    endpoint
        .path_segments_mut()
        .map_err(|()| SourceError::Config(format!("invalid Ashby API base {api_base:?}")))?
        .pop_if_empty()
        .extend(["posting-api", "job-board", board]);
    endpoint
        .query_pairs_mut()
        .append_pair("includeCompensation", "true");
    Ok(endpoint)
}

#[async_trait::async_trait]
impl Source for AshbySource {
    type Record = JobPosting;

    fn key(&self) -> &SourceKey {
        &self.key
    }

    async fn fetch(&self, request: &FetchRequest) -> Result<Fetched<JobPosting>, SourceError> {
        let response = self
            .http
            .get_conditional(&self.endpoint, request.validator.as_deref())
            .await
            .map_err(not_found_as(|| {
                format!("Ashby job board {:?}", self.board.board)
            }))?;
        let (body, etag) = match response {
            Conditional::NotModified => return Ok(Fetched::NotModified),
            Conditional::Modified { body, etag } => (body, etag),
        };
        let mut batch = parse_board(&body, &self.context).map_err(|error| SourceError::Decode {
            url: self.endpoint.to_string(),
            source: Box::new(error),
        })?;
        batch.complete = true;
        batch.validator = etag;
        Ok(Fetched::Batch(batch))
    }
}

/// What conversion needs to know about the board being read.
#[derive(Debug, Clone)]
pub struct BoardContext {
    pub source: SourceKey,
    pub company: String,
    pub fetched_from: Option<CanonicalUrl>,
}

/// Parses a board response into canonical postings.
///
/// Fails only if the response as a whole is not an Ashby board payload.
/// Individual postings that cannot be decoded or converted are returned as
/// rejections; unlisted postings are counted as skipped.
pub fn parse_board(
    body: &[u8],
    context: &BoardContext,
) -> Result<SourceBatch<JobPosting>, serde_json::Error> {
    let envelope: BoardEnvelope = serde_json::from_slice(body)?;
    let mut batch = SourceBatch::default();
    for value in envelope.jobs {
        let record_id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let raw: AshbyPosting = match serde_json::from_value(value) {
            Ok(raw) => raw,
            Err(error) => {
                batch.rejected.push(RecordError::new(
                    record_id,
                    RecordErrorReason::Malformed(error.to_string()),
                ));
                continue;
            }
        };
        if raw.is_listed == Some(false) {
            batch.skipped += 1;
            continue;
        }
        match to_posting(raw, context) {
            Ok(posting) => batch.records.push(posting),
            Err(error) => batch.rejected.push(error),
        }
    }
    Ok(batch)
}

/// Converts one Ashby posting into the canonical model.
pub fn to_posting(raw: AshbyPosting, context: &BoardContext) -> Result<JobPosting, RecordError> {
    let record_id = clean_line_opt(raw.id.as_deref());
    let reject = |reason| RecordError::new(record_id.clone(), reason);

    let title = clean_line_opt(raw.title.as_deref())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("title")))?;
    let job_url = raw
        .job_url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("jobUrl")))?;
    let url = CanonicalUrl::parse(job_url).map_err(|error| {
        reject(RecordErrorReason::InvalidUrl {
            field: "jobUrl",
            error,
        })
    })?;

    // The application URL is secondary: a bad one is dropped, not fatal.
    let apply_url = match raw.apply_url.as_deref().filter(|v| !v.trim().is_empty()) {
        None => None,
        Some(value) => match CanonicalUrl::parse(value) {
            Ok(apply) => (apply != url).then_some(apply),
            Err(error) => {
                debug!(record = ?record_id, %error, "ignoring invalid applyUrl");
                None
            }
        },
    };

    let mut locations = Vec::with_capacity(1 + raw.secondary_locations.len());
    locations.push(source_location(
        raw.location.as_deref(),
        raw.address.as_ref(),
    ));
    for secondary in &raw.secondary_locations {
        locations.push(source_location(
            secondary.location.as_deref(),
            secondary.address.as_ref(),
        ));
    }
    locations.retain(|l| !l.is_empty());

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
        department: clean_line_opt(raw.department.as_deref()),
        team: clean_line_opt(raw.team.as_deref()),
        location: clean_line_opt(raw.location.as_deref()),
        locations,
        employment_type: clean_line_opt(raw.employment_type.as_deref())
            .map(|v| employment_type(&v)),
        workplace_type: clean_line_opt(raw.workplace_type.as_deref()).map(|v| workplace_type(&v)),
        is_remote: raw.is_remote,
        compensation: compensation(
            raw.compensation,
            raw.should_display_compensation_on_job_postings,
        ),
        work_authorization: None,
        description_text: clean_block_opt(raw.description_plain.as_deref()),
        description_html: clean_block_opt(raw.description_html.as_deref()),
        posted_at: raw
            .published_at
            .as_deref()
            .and_then(|v| parse_timestamp(v, record_id.as_deref())),
        // Ashby's posting API does not expose an update timestamp.
        source_updated_at: None,
    })
}

fn source_location(name: Option<&str>, address: Option<&AshbyAddress>) -> SourceLocation {
    let postal = address.and_then(|a| a.postal_address.as_ref());
    SourceLocation {
        name: clean_line_opt(name),
        locality: clean_line_opt(postal.and_then(|p| p.address_locality.as_deref())),
        region: clean_line_opt(postal.and_then(|p| p.address_region.as_deref())),
        country: clean_line_opt(postal.and_then(|p| p.address_country.as_deref())),
    }
}

fn employment_type(value: &str) -> EmploymentType {
    match value {
        "FullTime" => EmploymentType::FullTime,
        "PartTime" => EmploymentType::PartTime,
        "Contract" => EmploymentType::Contract,
        "Temporary" => EmploymentType::Temporary,
        "Intern" => EmploymentType::Internship,
        other => EmploymentType::Other(other.to_owned()),
    }
}

fn workplace_type(value: &str) -> WorkplaceType {
    match value {
        "Remote" => WorkplaceType::Remote,
        "Hybrid" => WorkplaceType::Hybrid,
        "OnSite" => WorkplaceType::OnSite,
        other => WorkplaceType::Other(other.to_owned()),
    }
}

fn compensation(raw: Option<AshbyCompensation>, displayed: Option<bool>) -> Option<Compensation> {
    // Respect the company's choice not to publish compensation.
    if displayed == Some(false) {
        return None;
    }
    let raw = raw?;
    let summary = clean_line_opt(raw.compensation_tier_summary.as_deref());
    let components: Vec<CompensationComponent> = raw
        .summary_components
        .into_iter()
        .filter_map(compensation_component)
        .collect();
    if summary.is_none() && components.is_empty() {
        return None;
    }
    Some(Compensation {
        summary,
        components,
    })
}

fn compensation_component(raw: AshbyCompensationComponent) -> Option<CompensationComponent> {
    let kind = match clean_line_opt(raw.compensation_type.as_deref())?.as_str() {
        "Salary" => CompensationKind::Salary,
        "EquityPercentage" => CompensationKind::EquityPercentage,
        "EquityCashValue" => CompensationKind::EquityCashValue,
        "Bonus" => CompensationKind::Bonus,
        "Commission" => CompensationKind::Commission,
        other => CompensationKind::Other(other.to_owned()),
    };
    Some(CompensationComponent {
        kind,
        label: None,
        currency: clean_line_opt(raw.currency_code.as_deref()),
        min: raw.min_value,
        max: raw.max_value,
        interval: raw.interval.as_deref().and_then(pay_interval),
    })
}

/// Ashby intervals look like `"1 YEAR"`; `"NONE"` (used for equity) and
/// anything unrecognized stay unknown.
fn pay_interval(value: &str) -> Option<PayInterval> {
    match value.trim() {
        "1 HOUR" => Some(PayInterval::Hour),
        "1 DAY" => Some(PayInterval::Day),
        "1 WEEK" => Some(PayInterval::Week),
        "1 MONTH" => Some(PayInterval::Month),
        "1 YEAR" => Some(PayInterval::Year),
        "ONE_TIME" => Some(PayInterval::OneTime),
        _ => None,
    }
}

fn parse_timestamp(value: &str, record_id: Option<&str>) -> Option<DateTime<Utc>> {
    match DateTime::parse_from_rfc3339(value.trim()) {
        Ok(t) => Some(t.with_timezone(&Utc)),
        Err(error) => {
            debug!(record = ?record_id, value, %error, "ignoring unparseable publishedAt");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Raw payload types. Almost every field is optional so that a missing value
// becomes a precise conversion error (or stays unknown) instead of failing
// deserialization of the whole record.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BoardEnvelope {
    jobs: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AshbyPosting {
    pub id: Option<String>,
    pub title: Option<String>,
    pub department: Option<String>,
    pub team: Option<String>,
    pub employment_type: Option<String>,
    pub location: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub secondary_locations: Vec<AshbySecondaryLocation>,
    pub published_at: Option<String>,
    pub is_listed: Option<bool>,
    pub is_remote: Option<bool>,
    pub workplace_type: Option<String>,
    pub address: Option<AshbyAddress>,
    pub job_url: Option<String>,
    pub apply_url: Option<String>,
    pub description_html: Option<String>,
    pub description_plain: Option<String>,
    pub should_display_compensation_on_job_postings: Option<bool>,
    pub compensation: Option<AshbyCompensation>,
}

#[derive(Debug, Deserialize)]
pub struct AshbySecondaryLocation {
    pub location: Option<String>,
    pub address: Option<AshbyAddress>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AshbyAddress {
    pub postal_address: Option<AshbyPostalAddress>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AshbyPostalAddress {
    pub address_locality: Option<String>,
    pub address_region: Option<String>,
    pub address_country: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AshbyCompensation {
    pub compensation_tier_summary: Option<String>,
    /// Components aggregated across all tiers (e.g. the overall salary range
    /// when there are per-location tiers).
    #[serde(default, deserialize_with = "null_as_default")]
    pub summary_components: Vec<AshbyCompensationComponent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AshbyCompensationComponent {
    pub compensation_type: Option<String>,
    pub interval: Option<String>,
    pub currency_code: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_board_endpoint() {
        let url = board_endpoint(DEFAULT_API_BASE, "linear").unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.ashbyhq.com/posting-api/job-board/linear?includeCompensation=true"
        );
        let url = board_endpoint("http://127.0.0.1:9999/", "a/b").unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:9999/posting-api/job-board/a%2Fb?includeCompensation=true"
        );
    }

    #[test]
    fn maps_intervals() {
        assert_eq!(pay_interval("1 YEAR"), Some(PayInterval::Year));
        assert_eq!(pay_interval("1 HOUR"), Some(PayInterval::Hour));
        assert_eq!(pay_interval("NONE"), None);
        assert_eq!(pay_interval("2 WEEK"), None);
    }

    #[test]
    fn company_falls_back_to_board_name() {
        let board = AshbyBoard {
            board: "posthog".into(),
            company: Some("  ".into()),
        };
        assert_eq!(board.company_name(), "posthog");
    }
}
