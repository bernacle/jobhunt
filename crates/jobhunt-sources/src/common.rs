//! Small helpers shared by adapters. Everything here is source-agnostic and
//! conservative: a value that does not clearly map stays as the source wrote
//! it (`Other(..)`) or unknown (`None`).

use chrono::{DateTime, Utc};
use jobhunt_core::text::{clean_line, search_key};
use jobhunt_core::{CanonicalUrl, SourceError};
use jobhunt_jobs::{EmploymentType, PayInterval, WorkplaceType};
use serde::{Deserialize, Deserializer};
use tracing::debug;
use url::Url;

/// Builds `<api_base>/<segments...>?<query>`, percent-encoding each segment.
pub(crate) fn endpoint(
    api_base: &str,
    segments: &[&str],
    query: &[(&str, &str)],
) -> Result<Url, SourceError> {
    let invalid = || SourceError::Config(format!("invalid API base URL {api_base:?}"));
    let mut url = Url::parse(api_base).map_err(|_| invalid())?;
    url.path_segments_mut()
        .map_err(|()| invalid())?
        .pop_if_empty()
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

/// Treats JSON `null` like a missing field for collections.
pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// A record's id from a raw JSON value, whether the source sends it as a
/// string or a number. Used to attribute records that fail to decode.
pub(crate) fn raw_record_id(value: &serde_json::Value, field: &str) -> Option<String> {
    match value.get(field)? {
        serde_json::Value::String(s) => clean_line(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parses an RFC 3339 timestamp; an unparseable one stays unknown.
pub(crate) fn rfc3339(
    value: Option<&str>,
    field: &str,
    record: Option<&str>,
) -> Option<DateTime<Utc>> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    match DateTime::parse_from_rfc3339(value) {
        Ok(t) => Some(t.with_timezone(&Utc)),
        Err(error) => {
            debug!(record, field, value, %error, "ignoring unparseable timestamp");
            None
        }
    }
}

/// Milliseconds since the Unix epoch; out-of-range values stay unknown.
pub(crate) fn epoch_millis(value: Option<i64>) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value?)
}

/// Parses a secondary URL (such as an application link). A bad one is
/// dropped rather than rejecting the record, and one equal to the posting's
/// own URL is dropped as redundant.
pub(crate) fn secondary_url(
    value: Option<&str>,
    primary: &CanonicalUrl,
    field: &str,
    record: Option<&str>,
) -> Option<CanonicalUrl> {
    let value = value.filter(|v| !v.trim().is_empty())?;
    match CanonicalUrl::parse(value) {
        Ok(url) => (url != *primary).then_some(url),
        Err(error) => {
            debug!(record, field, %error, "ignoring invalid URL");
            None
        }
    }
}

/// Maps an employment label ("Full-time", "Full Time", "Contractor", ...)
/// onto the canonical type. Only unambiguous labels are mapped; anything
/// else ("Permanent", "Fixed-Term", "Full Time/Part Time") is kept verbatim.
pub(crate) fn employment_type(label: &str) -> Option<EmploymentType> {
    let raw = clean_line(label)?;
    let mapped = match search_key(&raw).as_str() {
        "full time" | "fulltime" | "full time employee" => EmploymentType::FullTime,
        "part time" | "parttime" | "part time employee" => EmploymentType::PartTime,
        "contract" | "contractor" | "contractors" => EmploymentType::Contract,
        "temporary" | "temp" => EmploymentType::Temporary,
        "intern" | "internship" | "internships" => EmploymentType::Internship,
        _ => EmploymentType::Other(raw),
    };
    Some(mapped)
}

/// Maps a workplace label onto the canonical type by its leading word:
/// "Remote", "Remote (US only)" → remote; "Hybrid (Travel-Required)" →
/// hybrid; "On-site", "Onsite", "In office" → on-site. Anything else is
/// kept verbatim; blank or "None"/"Unspecified" stays unknown.
pub(crate) fn workplace_type(label: &str) -> Option<WorkplaceType> {
    let raw = clean_line(label)?;
    let key = search_key(&raw);
    let starts = |word: &str| key == word || key.starts_with(&format!("{word} "));
    let mapped = if starts("remote") || starts("fully remote") {
        WorkplaceType::Remote
    } else if starts("hybrid") {
        WorkplaceType::Hybrid
    } else if starts("on site") || starts("onsite") || starts("in office") || starts("in person") {
        WorkplaceType::OnSite
    } else if matches!(
        key.as_str(),
        "" | "none" | "unspecified" | "n a" | "not specified"
    ) {
        return None;
    } else {
        WorkplaceType::Other(raw)
    };
    Some(mapped)
}

/// Pay interval named in a free-text label ("Annual Salary",
/// "Hourly Base Pay Range", "Mexico Monthly Pay Range"). A label without an
/// explicit interval word stays unknown.
pub(crate) fn interval_in_label(label: &str) -> Option<PayInterval> {
    let key = format!(" {} ", search_key(label));
    let has = |words: &[&str]| words.iter().any(|w| key.contains(&format!(" {w} ")));
    if has(&["annual", "annually", "yearly", "year", "per year"]) {
        Some(PayInterval::Year)
    } else if has(&["monthly", "month"]) {
        Some(PayInterval::Month)
    } else if has(&["weekly", "week"]) {
        Some(PayInterval::Week)
    } else if has(&["daily", "day"]) {
        Some(PayInterval::Day)
    } else if has(&["hourly", "hour"]) {
        Some(PayInterval::Hour)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_employment_labels_conservatively() {
        assert_eq!(employment_type("Full-time"), Some(EmploymentType::FullTime));
        assert_eq!(employment_type("Full Time"), Some(EmploymentType::FullTime));
        assert_eq!(employment_type("Full-Time"), Some(EmploymentType::FullTime));
        assert_eq!(
            employment_type("Contractor"),
            Some(EmploymentType::Contract)
        );
        assert_eq!(
            employment_type("Internship"),
            Some(EmploymentType::Internship)
        );
        assert_eq!(
            employment_type("Permanent"),
            Some(EmploymentType::Other("Permanent".into()))
        );
        assert_eq!(
            employment_type("Full Time/Part Time"),
            Some(EmploymentType::Other("Full Time/Part Time".into()))
        );
        assert_eq!(employment_type("  "), None);
    }

    #[test]
    fn maps_workplace_labels_by_leading_word() {
        assert_eq!(workplace_type("Remote"), Some(WorkplaceType::Remote));
        assert_eq!(workplace_type("remote"), Some(WorkplaceType::Remote));
        assert_eq!(workplace_type("On-Site"), Some(WorkplaceType::OnSite));
        assert_eq!(workplace_type("onsite"), Some(WorkplaceType::OnSite));
        assert_eq!(
            workplace_type("Hybrid (Travel-Required)"),
            Some(WorkplaceType::Hybrid)
        );
        assert_eq!(workplace_type("unspecified"), None);
        assert_eq!(workplace_type("None"), None);
        assert_eq!(
            workplace_type("Flexible"),
            Some(WorkplaceType::Other("Flexible".into()))
        );
        // "Remoteness" is not "remote".
        assert_eq!(
            workplace_type("Remoteness"),
            Some(WorkplaceType::Other("Remoteness".into()))
        );
    }

    #[test]
    fn finds_intervals_only_when_named() {
        assert_eq!(interval_in_label("Annual Salary:"), Some(PayInterval::Year));
        assert_eq!(
            interval_in_label("Mexico Monthly Pay Range"),
            Some(PayInterval::Month)
        );
        assert_eq!(
            interval_in_label("Hourly Base Pay Range:"),
            Some(PayInterval::Hour)
        );
        assert_eq!(interval_in_label("Pay Range"), None);
        assert_eq!(interval_in_label("Internship"), None);
    }

    #[test]
    fn builds_encoded_endpoints() {
        let url = endpoint(
            "http://127.0.0.1:9/",
            &["v1", "boards", "a/b"],
            &[("content", "true")],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:9/v1/boards/a%2Fb?content=true"
        );
        assert!(endpoint("not a url", &[], &[]).is_err());
    }

    #[test]
    fn reads_ids_of_either_type() {
        let v: serde_json::Value = serde_json::json!({"a": 12, "b": " x ", "c": [1]});
        assert_eq!(raw_record_id(&v, "a").as_deref(), Some("12"));
        assert_eq!(raw_record_id(&v, "b").as_deref(), Some("x"));
        assert_eq!(raw_record_id(&v, "c"), None);
        assert_eq!(raw_record_id(&v, "d"), None);
    }
}
