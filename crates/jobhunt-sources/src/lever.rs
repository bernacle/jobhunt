//! Lever job sites.
//!
//! Lever hosts career pages at `jobs.lever.co/<site>` (`jobs.eu.lever.co`
//! for its EU region) and publishes every site through the public Postings
//! API:
//!
//! ```text
//! GET https://api.lever.co/v0/postings/<site>?mode=json
//! ```
//!
//! Without `limit`/`skip` the response is the site's entire list of
//! published postings, so a successful fetch is a complete listing. The API
//! sends an `ETag` and honors `If-None-Match`. An unknown site is a 404; a
//! site with no postings (or one that moved off Lever) is `[]`.
//!
//! Mapping notes, from real sites:
//!
//! * The description is split across `description` (opening + body),
//!   `lists` (titled bullet lists such as "What you'll do"), `additional`
//!   and `salaryDescription`. They are joined in page order into one HTML
//!   document and one plain-text description, so nothing is lost.
//! * `categories.commitment` is free text ("Full-time", "Permanent",
//!   "Fixed-Term"); only unambiguous values map to an employment type.
//! * `workplaceType` is `remote`, `hybrid`, `onsite` or `unspecified`.
//! * `country` is an ISO code for the primary location.
//! * `createdAt` (epoch milliseconds) is the closest thing to a publish
//!   date Lever exposes; there is no update timestamp.
//! * Lever does not return the company name, so it comes from
//!   configuration (falling back to the site name).

use jobhunt_core::html::html_to_text;
use jobhunt_core::text::{clean_block, clean_line, clean_line_opt};
use jobhunt_core::{
    CanonicalUrl, FetchRequest, Fetched, Provenance, RecordError, RecordErrorReason, Source,
    SourceBatch, SourceError, SourceKey,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, PayInterval, SourceLocation,
};
use serde::{Deserialize, Serialize};
use tracing::debug;
use url::Url;

use crate::common::{
    employment_type, endpoint, epoch_millis, null_as_default, raw_record_id, secondary_url,
    workplace_type,
};
use crate::http::{Conditional, HttpClient, not_found_as};

/// Source kind used in keys such as `lever:spotify`.
pub const KIND: &str = "lever";

pub const DEFAULT_API_BASE: &str = "https://api.lever.co";
pub const EU_API_BASE: &str = "https://api.eu.lever.co";

/// Which Lever region hosts the site.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeverRegion {
    /// `jobs.lever.co` / `api.lever.co`.
    #[default]
    Global,
    /// `jobs.eu.lever.co` / `api.eu.lever.co`.
    Eu,
}

impl LeverRegion {
    fn is_global(&self) -> bool {
        *self == Self::Global
    }
}

/// One Lever site to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverSite {
    /// Site name as it appears in `https://jobs.lever.co/<site>`.
    pub site: String,
    /// Company display name. Lever's API does not return one; when unset
    /// the site name is shown as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(default, skip_serializing_if = "LeverRegion::is_global")]
    pub region: LeverRegion,
}

/// Sites read when the user has not configured any.
pub fn default_sites() -> Vec<LeverSite> {
    [
        ("spotify", "Spotify"),
        ("palantir", "Palantir"),
        ("zoox", "Zoox"),
    ]
    .into_iter()
    .map(|(site, company)| LeverSite {
        site: site.to_owned(),
        company: Some(company.to_owned()),
        region: LeverRegion::Global,
    })
    .collect()
}

/// Reads one Lever site.
#[derive(Debug)]
pub struct LeverSource {
    key: SourceKey,
    site: LeverSite,
    endpoint: Url,
    context: SiteContext,
    http: HttpClient,
}

impl LeverSource {
    pub fn new(key: SourceKey, site: LeverSite, http: HttpClient) -> Result<Self, SourceError> {
        let base = match site.region {
            LeverRegion::Global => DEFAULT_API_BASE,
            LeverRegion::Eu => EU_API_BASE,
        };
        Self::with_api_base(key, site, http, base)
    }

    /// Like [`LeverSource::new`] but against another API host (tests).
    pub fn with_api_base(
        key: SourceKey,
        site: LeverSite,
        http: HttpClient,
        api_base: &str,
    ) -> Result<Self, SourceError> {
        let endpoint = endpoint(
            api_base,
            &["v0", "postings", &site.site],
            &[("mode", "json")],
        )?;
        let context = SiteContext {
            source: key.clone(),
            company: site
                .company
                .as_deref()
                .and_then(clean_line)
                .unwrap_or_else(|| site.site.clone()),
            fetched_from: CanonicalUrl::parse(endpoint.as_str()).ok(),
        };
        Ok(Self {
            key,
            site,
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
impl Source for LeverSource {
    type Record = JobPosting;

    fn key(&self) -> &SourceKey {
        &self.key
    }

    async fn fetch(&self, request: &FetchRequest) -> Result<Fetched<JobPosting>, SourceError> {
        let response = self
            .http
            .get_conditional(&self.endpoint, request.validator.as_deref())
            .await
            .map_err(not_found_as(|| format!("Lever site {:?}", self.site.site)))?;
        let (body, etag) = match response {
            Conditional::NotModified => return Ok(Fetched::NotModified),
            Conditional::Modified { body, etag } => (body, etag),
        };
        let mut batch = parse_site(&body, &self.context).map_err(|error| SourceError::Decode {
            url: self.endpoint.to_string(),
            source: Box::new(error),
        })?;
        batch.complete = true;
        batch.validator = etag;
        Ok(Fetched::Batch(batch))
    }
}

/// What conversion needs to know about the site being read.
#[derive(Debug, Clone)]
pub struct SiteContext {
    pub source: SourceKey,
    pub company: String,
    pub fetched_from: Option<CanonicalUrl>,
}

/// Parses a postings response into canonical postings.
///
/// Fails only if the response as a whole is not a JSON array. Individual
/// postings that cannot be decoded or converted are rejections.
pub fn parse_site(
    body: &[u8],
    context: &SiteContext,
) -> Result<SourceBatch<JobPosting>, serde_json::Error> {
    let postings: Vec<serde_json::Value> = serde_json::from_slice(body)?;
    let mut batch = SourceBatch::default();
    for value in postings {
        let record_id = raw_record_id(&value, "id");
        let raw: LeverPosting = match serde_json::from_value(value) {
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
    Ok(batch)
}

/// Converts one Lever posting into the canonical model.
pub fn to_posting(raw: LeverPosting, context: &SiteContext) -> Result<JobPosting, RecordError> {
    let record_id = clean_line_opt(raw.id.as_deref());
    let reject = |reason| RecordError::new(record_id.clone(), reason);
    if record_id.is_none() {
        return Err(reject(RecordErrorReason::MissingField("id")));
    }
    let title = clean_line_opt(raw.text.as_deref())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("text")))?;
    let hosted_url = raw
        .hosted_url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| reject(RecordErrorReason::MissingField("hostedUrl")))?;
    let url = CanonicalUrl::parse(hosted_url).map_err(|error| {
        reject(RecordErrorReason::InvalidUrl {
            field: "hostedUrl",
            error,
        })
    })?;
    let record = record_id.as_deref();
    let apply_url = secondary_url(raw.apply_url.as_deref(), &url, "applyUrl", record);

    let description_html = description_html(&raw);
    let description_text = description_html.as_deref().and_then(html_to_text);
    let categories = raw.categories.unwrap_or_default();
    let location = clean_line_opt(categories.location.as_deref());
    let country = clean_line_opt(raw.country.as_deref());
    let mut locations: Vec<SourceLocation> = categories
        .all_locations
        .iter()
        .filter_map(|name| clean_line(name))
        .map(|name| SourceLocation {
            // `country` describes the primary location only.
            country: (Some(&name) == location.as_ref())
                .then(|| country.clone())
                .flatten(),
            name: Some(name),
            ..SourceLocation::default()
        })
        .collect();
    if locations.is_empty()
        && let Some(name) = &location
    {
        locations.push(SourceLocation {
            name: Some(name.clone()),
            country: country.clone(),
            ..SourceLocation::default()
        });
    }
    // Keep the primary location first.
    if let Some(primary) = &location
        && let Some(pos) = locations
            .iter()
            .position(|l| l.name.as_ref() == Some(primary))
    {
        let primary = locations.remove(pos);
        locations.insert(0, primary);
    }

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
        department: clean_line_opt(categories.department.as_deref()),
        team: clean_line_opt(categories.team.as_deref()),
        location,
        locations,
        employment_type: categories.commitment.as_deref().and_then(employment_type),
        workplace_type: raw.workplace_type.as_deref().and_then(workplace_type),
        is_remote: None,
        compensation: raw.salary_range.and_then(|r| compensation(r, record)),
        description_text,
        description_html,
        posted_at: epoch_millis(raw.created_at),
        source_updated_at: None,
    })
}

/// The whole posting body in page order: description, each titled list,
/// the closing "additional" section and the salary description.
fn description_html(raw: &LeverPosting) -> Option<String> {
    let mut html = String::new();
    if let Some(description) = raw.description.as_deref() {
        html.push_str(description);
    }
    for list in &raw.lists {
        if let Some(title) = clean_line_opt(list.text.as_deref()) {
            html.push_str("<h3>");
            html.push_str(&escape_text(&title));
            html.push_str("</h3>");
        }
        if let Some(content) = list.content.as_deref() {
            // Usually bare `<li>` items; some sites send a complete list (or
            // paragraphs), which must not be wrapped into a nested list.
            let lower = content.to_ascii_lowercase();
            let bare_items =
                lower.contains("<li") && !lower.contains("<ul") && !lower.contains("<ol");
            let (open, close) = if bare_items {
                ("<ul>", "</ul>")
            } else {
                ("<div>", "</div>")
            };
            html.push_str(open);
            html.push_str(content);
            html.push_str(close);
        }
    }
    for section in [raw.additional.as_deref(), raw.salary_description.as_deref()]
        .into_iter()
        .flatten()
    {
        html.push_str("<div>");
        html.push_str(section);
        html.push_str("</div>");
    }
    clean_block(&html)
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn compensation(range: LeverSalaryRange, record: Option<&str>) -> Option<Compensation> {
    let valid = [range.min, range.max]
        .into_iter()
        .flatten()
        .all(|v| v.is_finite() && v >= 0.0)
        && range.min.zip(range.max).is_none_or(|(lo, hi)| lo <= hi);
    if (range.min.is_none() && range.max.is_none()) || !valid {
        debug!(record, ?range.min, ?range.max, "ignoring unusable salaryRange");
        return None;
    }
    let interval = range.interval.as_deref().and_then(|i| match i.trim() {
        "per-year-salary" => Some(PayInterval::Year),
        "per-month-salary" => Some(PayInterval::Month),
        "per-week-salary" => Some(PayInterval::Week),
        "per-day-wage" => Some(PayInterval::Day),
        "per-hour-wage" => Some(PayInterval::Hour),
        "one-time" => Some(PayInterval::OneTime),
        _ => None,
    });
    Some(Compensation {
        summary: None,
        components: vec![CompensationComponent {
            kind: CompensationKind::Salary,
            label: None,
            currency: clean_line_opt(range.currency.as_deref()),
            min: range.min,
            max: range.max,
            interval,
        }],
    })
}

// ---------------------------------------------------------------------------
// Raw payload types.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverPosting {
    pub id: Option<String>,
    pub text: Option<String>,
    pub hosted_url: Option<String>,
    pub apply_url: Option<String>,
    pub categories: Option<LeverCategories>,
    pub country: Option<String>,
    pub workplace_type: Option<String>,
    pub created_at: Option<i64>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub lists: Vec<LeverList>,
    pub additional: Option<String>,
    pub salary_range: Option<LeverSalaryRange>,
    pub salary_description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverCategories {
    pub commitment: Option<String>,
    pub department: Option<String>,
    pub location: Option<String>,
    pub team: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub all_locations: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct LeverList {
    pub text: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LeverSalaryRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub currency: Option<String>,
    pub interval: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_endpoints_for_both_regions() {
        let http = crate::HttpClient::new(crate::HttpSettings::default()).unwrap();
        let site = |region| LeverSite {
            site: "spotify".into(),
            company: None,
            region,
        };
        let key = SourceKey::new(KIND, "spotify").unwrap();
        let global =
            LeverSource::new(key.clone(), site(LeverRegion::Global), http.clone()).unwrap();
        assert_eq!(
            global.endpoint().as_str(),
            "https://api.lever.co/v0/postings/spotify?mode=json"
        );
        let eu = LeverSource::new(key, site(LeverRegion::Eu), http).unwrap();
        assert_eq!(
            eu.endpoint().as_str(),
            "https://api.eu.lever.co/v0/postings/spotify?mode=json"
        );
    }

    #[test]
    fn salary_intervals_and_bad_ranges() {
        let range = |min, max, interval: &str| LeverSalaryRange {
            min,
            max,
            currency: Some("CAD".into()),
            interval: Some(interval.into()),
        };
        let hourly = compensation(range(Some(35.0), Some(38.0), "per-hour-wage"), None).unwrap();
        assert_eq!(hourly.components[0].interval, Some(PayInterval::Hour));
        assert_eq!(hourly.components[0].currency.as_deref(), Some("CAD"));
        let odd = compensation(range(Some(1.0), None, "per-fortnight"), None).unwrap();
        assert_eq!(odd.components[0].interval, None);
        assert_eq!(
            compensation(range(Some(9.0), Some(1.0), "per-year-salary"), None),
            None
        );
        assert_eq!(
            compensation(range(None, None, "per-year-salary"), None),
            None
        );
    }

    #[test]
    fn list_sections_are_wrapped_only_when_bare() {
        let posting = |content: &str| LeverPosting {
            id: Some("x".into()),
            text: None,
            hosted_url: None,
            apply_url: None,
            categories: None,
            country: None,
            workplace_type: None,
            created_at: None,
            description: None,
            lists: vec![LeverList {
                text: Some("Duties".into()),
                content: Some(content.into()),
            }],
            additional: None,
            salary_range: None,
            salary_description: None,
        };
        let text = |content: &str| {
            description_html(&posting(content))
                .as_deref()
                .and_then(html_to_text)
                .unwrap()
        };
        assert_eq!(text("<li>One</li><li>Two</li>"), "Duties\n\n• One\n• Two");
        assert_eq!(
            text("<div><ul style=\"x\"><li><p>One</p></li></ul></div>"),
            "Duties\n\n• One",
            "a complete list is not nested again"
        );
        assert_eq!(text("<p>Prose only</p>"), "Duties\n\nProse only");
    }

    #[test]
    fn list_titles_are_escaped_into_html() {
        assert_eq!(escape_text("R&D <team>"), "R&amp;D &lt;team&gt;");
    }
}
