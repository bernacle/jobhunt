//! Greenhouse job boards.
//!
//! Greenhouse hosts career pages at `job-boards.greenhouse.io/<board>` (and
//! the older `boards.greenhouse.io/<board>`), and many companies embed the
//! same board on their own site. Every board is readable through the public,
//! unauthenticated Job Board API:
//!
//! ```text
//! GET https://boards-api.greenhouse.io/v1/boards/<board>/jobs?content=true&pay_transparency=true
//! ```
//!
//! One response is the whole board, with `meta.total` giving the number of
//! jobs; a response whose job count disagrees with it is treated as partial.
//! The endpoint sends an `ETag` and honors `If-None-Match`.
//!
//! Mapping notes, from real boards:
//!
//! * `absolute_url` is the job's public page. For boards embedded on a
//!   company site it is that site's URL (`stripe.com/jobs/search?gh_jid=…`),
//!   which is exactly what makes cross-source matching possible.
//! * `content` is HTML that has been HTML-escaped once more
//!   (`&lt;p&gt;…`). It is unescaped into `description_html` and converted
//!   into readable `description_text`.
//! * `company_name`, `first_published` (posted) and `updated_at` are used
//!   as given. Configuration can override the company name.
//! * Workplace and employment type are not standard Greenhouse fields; some
//!   companies publish them as custom `metadata` ("Location Type",
//!   "Workplace Type", "Employment Type"), which is used when present.
//! * `pay_input_ranges` (with `pay_transparency=true`) become salary
//!   components, labeled with the range's title ("Canada Annual Pay Range").

use jobhunt_core::html::{decode_entities, html_to_text};
use jobhunt_core::text::{clean_block, clean_line, clean_line_opt, search_key};
use jobhunt_core::{
    CanonicalUrl, FetchRequest, Fetched, Provenance, RecordError, RecordErrorReason, Source,
    SourceBatch, SourceError, SourceKey,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, SourceLocation,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use url::Url;

use crate::common::{
    employment_type, endpoint, interval_in_label, null_as_default, raw_record_id, rfc3339,
    workplace_type,
};
use crate::http::{Conditional, HttpClient, not_found_as};

/// Source kind used in keys such as `greenhouse:stripe`.
pub const KIND: &str = "greenhouse";

pub const DEFAULT_API_BASE: &str = "https://boards-api.greenhouse.io";

/// Custom metadata fields (normalized names) that state the workplace type.
const WORKPLACE_FIELDS: &[&str] = &[
    "location type",
    "workplace type",
    "workplace",
    "work location type",
    "remote status",
    "work arrangement",
];

/// Custom metadata fields (normalized names) that state the employment type.
const EMPLOYMENT_FIELDS: &[&str] = &["employment type", "job type", "employee type"];

/// One Greenhouse job board to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GreenhouseBoard {
    /// Board token as it appears in `https://job-boards.greenhouse.io/<board>`.
    pub board: String,
    /// Company display name. Defaults to the name Greenhouse reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
}

/// Boards read when the user has not configured any.
pub fn default_boards() -> Vec<GreenhouseBoard> {
    ["anthropic", "stripe", "figma", "airbnb"]
        .into_iter()
        .map(|board| GreenhouseBoard {
            board: board.to_owned(),
            company: None,
        })
        .collect()
}

/// Reads one Greenhouse job board.
#[derive(Debug)]
pub struct GreenhouseSource {
    key: SourceKey,
    board: GreenhouseBoard,
    endpoint: Url,
    context: BoardContext,
    http: HttpClient,
}

impl GreenhouseSource {
    pub fn new(
        key: SourceKey,
        board: GreenhouseBoard,
        http: HttpClient,
    ) -> Result<Self, SourceError> {
        Self::with_api_base(key, board, http, DEFAULT_API_BASE)
    }

    /// Like [`GreenhouseSource::new`] but against another API host (tests).
    pub fn with_api_base(
        key: SourceKey,
        board: GreenhouseBoard,
        http: HttpClient,
        api_base: &str,
    ) -> Result<Self, SourceError> {
        let endpoint = endpoint(
            api_base,
            &["v1", "boards", &board.board, "jobs"],
            &[("content", "true"), ("pay_transparency", "true")],
        )?;
        let context = BoardContext {
            source: key.clone(),
            board: board.board.clone(),
            company: board.company.as_deref().and_then(clean_line),
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

#[async_trait::async_trait]
impl Source for GreenhouseSource {
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
                format!("Greenhouse job board {:?}", self.board.board)
            }))?;
        let (body, etag) = match response {
            Conditional::NotModified => return Ok(Fetched::NotModified),
            Conditional::Modified { body, etag } => (body, etag),
        };
        let mut batch = parse_board(&body, &self.context).map_err(|error| SourceError::Decode {
            url: self.endpoint.to_string(),
            source: Box::new(error),
        })?;
        batch.validator = etag;
        Ok(Fetched::Batch(batch))
    }
}

/// What conversion needs to know about the board being read.
#[derive(Debug, Clone)]
pub struct BoardContext {
    pub source: SourceKey,
    pub board: String,
    /// Configured company name; overrides the one Greenhouse reports.
    pub company: Option<String>,
    pub fetched_from: Option<CanonicalUrl>,
}

/// Parses a board response into canonical postings.
///
/// Fails only if the response as a whole is not a Greenhouse board payload.
/// Individual jobs that cannot be decoded or converted are rejections. The
/// batch is marked complete when the number of jobs matches `meta.total`.
pub fn parse_board(
    body: &[u8],
    context: &BoardContext,
) -> Result<SourceBatch<JobPosting>, serde_json::Error> {
    let envelope: BoardEnvelope = serde_json::from_slice(body)?;
    let received = envelope.jobs.len();
    let mut batch = SourceBatch::default();
    for value in envelope.jobs {
        let record_id = raw_record_id(&value, "id");
        let raw: GreenhousePosting = match serde_json::from_value(value) {
            Ok(raw) => raw,
            Err(error) => {
                batch.rejected.push(RecordError::new(
                    record_id,
                    RecordErrorReason::Malformed(error.to_string()),
                ));
                continue;
            }
        };
        match to_posting(raw, context) {
            Ok(posting) => batch.records.push(posting),
            Err(error) => batch.rejected.push(error),
        }
    }
    batch.complete = match envelope.meta.and_then(|m| m.total) {
        Some(total) => {
            let complete = usize::try_from(total).is_ok_and(|t| t == received);
            if !complete {
                warn!(
                    board = %context.board,
                    total,
                    received,
                    "Greenhouse returned fewer jobs than it reported; treating the listing as partial"
                );
            }
            complete
        }
        // Every board response carries meta.total; without it nothing
        // vouches for completeness.
        None => false,
    };
    Ok(batch)
}

/// Converts one Greenhouse job into the canonical model.
pub fn to_posting(
    raw: GreenhousePosting,
    context: &BoardContext,
) -> Result<JobPosting, RecordError> {
    let record_id = raw.id.map(|id| id.to_string());
    let reject = |reason| RecordError::new(record_id.clone(), reason);
    if record_id.is_none() {
        return Err(reject(RecordErrorReason::MissingField("id")));
    }

    let title = clean_line_opt(raw.title.as_deref())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("title")))?;
    let absolute_url = raw
        .absolute_url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("absolute_url")))?;
    let url = CanonicalUrl::parse(absolute_url).map_err(|error| {
        reject(RecordErrorReason::InvalidUrl {
            field: "absolute_url",
            error,
        })
    })?;

    let company = context
        .company
        .clone()
        .or_else(|| clean_line_opt(raw.company_name.as_deref()))
        .unwrap_or_else(|| context.board.clone());

    let description_html = raw
        .content
        .as_deref()
        .map(|escaped| decode_entities(escaped).into_owned())
        .and_then(|html| clean_block(&html));
    let description_text = description_html.as_deref().and_then(html_to_text);

    let location = raw
        .location
        .as_ref()
        .and_then(|l| clean_line_opt(l.name.as_deref()));
    let mut locations: Vec<SourceLocation> = raw.offices.iter().map(office_location).collect();
    locations.retain(|l| !l.is_empty());
    if locations.is_empty()
        && let Some(name) = &location
    {
        locations.push(SourceLocation {
            name: Some(name.clone()),
            ..SourceLocation::default()
        });
    }

    let record = record_id.as_deref();
    Ok(JobPosting {
        provenance: Provenance {
            source: context.source.clone(),
            source_record_id: record_id.clone(),
            fetched_from: context.fetched_from.clone(),
        },
        url,
        // Greenhouse applications live on the job page itself.
        apply_url: None,
        company,
        title,
        department: raw
            .departments
            .iter()
            .find_map(|d| clean_line_opt(d.name.as_deref())),
        team: None,
        location,
        locations,
        employment_type: metadata_value(&raw.metadata, EMPLOYMENT_FIELDS)
            .and_then(|v| employment_type(&v)),
        workplace_type: metadata_value(&raw.metadata, WORKPLACE_FIELDS)
            .and_then(|v| workplace_type(&v)),
        is_remote: None,
        compensation: compensation(&raw.pay_input_ranges, record),
        description_text,
        description_html,
        posted_at: rfc3339(raw.first_published.as_deref(), "first_published", record),
        source_updated_at: rfc3339(raw.updated_at.as_deref(), "updated_at", record),
    })
}

/// An office as a location: its name, plus address parts when Greenhouse
/// gives a formatted place ("San Francisco, California, United States":
/// the last part is the country, a middle part the region).
fn office_location(office: &GreenhouseOffice) -> SourceLocation {
    let mut location = SourceLocation {
        name: clean_line_opt(office.name.as_deref()),
        ..SourceLocation::default()
    };
    let place = clean_line_opt(office.location.as_deref());
    if let Some(place) = place {
        let parts: Vec<&str> = place
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        match parts.as_slice() {
            [locality, .., region, country] => {
                location.locality = Some((*locality).to_owned());
                location.region = Some((*region).to_owned());
                location.country = Some((*country).to_owned());
            }
            [_, country] => location.country = Some((*country).to_owned()),
            _ => {}
        }
        if location.name.is_none() {
            location.name = Some(place);
        }
    }
    location
}

/// The value of the first metadata field whose (normalized) name is one of
/// `names`. Values may be strings or single-element lists.
fn metadata_value(metadata: &[GreenhouseMetadata], names: &[&str]) -> Option<String> {
    metadata
        .iter()
        .filter(|m| {
            m.name
                .as_deref()
                .is_some_and(|n| names.contains(&search_key(n).as_str()))
        })
        .find_map(|m| match &m.value {
            serde_json::Value::String(s) => clean_line(s),
            serde_json::Value::Array(values) if values.len() == 1 => {
                values[0].as_str().and_then(clean_line)
            }
            _ => None,
        })
}

/// Pay ranges as salary components. A range without amounts, or with
/// negative or inverted amounts, is dropped on its own.
fn compensation(ranges: &[GreenhousePayRange], record: Option<&str>) -> Option<Compensation> {
    let components: Vec<CompensationComponent> = ranges
        .iter()
        .filter_map(|range| {
            let min = range.min_cents.map(|c| c as f64 / 100.0);
            let max = range.max_cents.map(|c| c as f64 / 100.0);
            let valid = [min, max].into_iter().flatten().all(|v| v >= 0.0)
                && min.zip(max).is_none_or(|(lo, hi)| lo <= hi);
            if (min.is_none() && max.is_none()) || !valid {
                debug!(record, ?min, ?max, "ignoring unusable pay range");
                return None;
            }
            let label = clean_line_opt(range.title.as_deref())
                .map(|t| t.trim_end_matches(':').trim_end().to_owned())
                .filter(|t| !t.is_empty());
            Some(CompensationComponent {
                kind: CompensationKind::Salary,
                interval: label.as_deref().and_then(interval_in_label),
                label,
                currency: clean_line_opt(range.currency_type.as_deref()),
                min,
                max,
            })
        })
        .collect();
    (!components.is_empty()).then_some(Compensation {
        summary: None,
        components,
    })
}

// ---------------------------------------------------------------------------
// Raw payload types. Almost every field is optional so that a missing value
// becomes a precise conversion error (or stays unknown) instead of failing
// deserialization of the whole record.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BoardEnvelope {
    jobs: Vec<serde_json::Value>,
    meta: Option<BoardMeta>,
}

#[derive(Debug, Deserialize)]
struct BoardMeta {
    total: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct GreenhousePosting {
    pub id: Option<u64>,
    pub title: Option<String>,
    pub absolute_url: Option<String>,
    pub company_name: Option<String>,
    pub location: Option<GreenhouseLocation>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub offices: Vec<GreenhouseOffice>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub departments: Vec<GreenhouseDepartment>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub metadata: Vec<GreenhouseMetadata>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub pay_input_ranges: Vec<GreenhousePayRange>,
    pub content: Option<String>,
    pub first_published: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GreenhouseLocation {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GreenhouseOffice {
    pub name: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GreenhouseDepartment {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GreenhouseMetadata {
    pub name: Option<String>,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct GreenhousePayRange {
    pub min_cents: Option<i64>,
    pub max_cents: Option<i64>,
    pub currency_type: Option<String>,
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_board_endpoint() {
        let url = endpoint(
            DEFAULT_API_BASE,
            &["v1", "boards", "stripe", "jobs"],
            &[("content", "true"), ("pay_transparency", "true")],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://boards-api.greenhouse.io/v1/boards/stripe/jobs?content=true&pay_transparency=true"
        );
    }

    #[test]
    fn splits_formatted_office_places() {
        let office = |name: Option<&str>, place: Option<&str>| GreenhouseOffice {
            name: name.map(str::to_owned),
            location: place.map(str::to_owned),
        };
        let sf = office_location(&office(
            Some("San Francisco, CA"),
            Some("San Francisco, California, United States"),
        ));
        assert_eq!(sf.name.as_deref(), Some("San Francisco, CA"));
        assert_eq!(sf.locality.as_deref(), Some("San Francisco"));
        assert_eq!(sf.region.as_deref(), Some("California"));
        assert_eq!(sf.country.as_deref(), Some("United States"));

        let london = office_location(&office(None, Some("London, United Kingdom")));
        assert_eq!(london.name.as_deref(), Some("London, United Kingdom"));
        assert_eq!(london.locality, None, "two parts are ambiguous");
        assert_eq!(london.country.as_deref(), Some("United Kingdom"));

        let region = office_location(&office(Some("US"), None));
        assert_eq!(region.name.as_deref(), Some("US"));
        assert_eq!(region.country, None, "office names are not parsed");
    }

    #[test]
    fn reads_metadata_values() {
        let md = |name: &str, value: serde_json::Value| GreenhouseMetadata {
            name: Some(name.to_owned()),
            value,
        };
        let metadata = vec![
            md("Is this job part of ACC?", serde_json::json!(false)),
            md("Workplace Type", serde_json::json!("Remote")),
            md("Employment Type", serde_json::json!(["Full-time"])),
        ];
        assert_eq!(
            metadata_value(&metadata, WORKPLACE_FIELDS).as_deref(),
            Some("Remote")
        );
        assert_eq!(
            metadata_value(&metadata, EMPLOYMENT_FIELDS).as_deref(),
            Some("Full-time")
        );
        let none = vec![md("Location Type", serde_json::Value::Null)];
        assert_eq!(metadata_value(&none, WORKPLACE_FIELDS), None);
    }

    #[test]
    fn drops_unusable_pay_ranges_individually() {
        let range = |min, max, title: &str| GreenhousePayRange {
            min_cents: min,
            max_cents: max,
            currency_type: Some("USD".into()),
            title: Some(title.into()),
        };
        let comp = compensation(
            &[
                range(Some(20_000_000), Some(10_000_000), "Inverted"),
                range(None, None, "Empty"),
                range(Some(-5), Some(10), "Negative"),
                range(
                    Some(15_000_000),
                    Some(20_000_000),
                    "Canada Annual Pay Range:",
                ),
            ],
            None,
        )
        .unwrap();
        assert_eq!(comp.components.len(), 1);
        let c = &comp.components[0];
        assert_eq!(c.label.as_deref(), Some("Canada Annual Pay Range"));
        assert_eq!((c.min, c.max), (Some(150_000.0), Some(200_000.0)));
        assert_eq!(c.interval, Some(jobhunt_jobs::PayInterval::Year));
        assert_eq!(compensation(&[range(None, None, "x")], None), None);
    }
}
