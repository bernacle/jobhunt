//! The canonical job model.
//!
//! Every source adapter converts its payloads into a [`JobPosting`]. Fields a
//! source does not provide stay `None`/empty: the model never guesses.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};
use jobhunt_core::text::search_key;
use jobhunt_core::{
    CanonicalUrl, Fingerprint, FingerprintBuilder, ParseIdError, Provenance, RecordError,
    RecordErrorReason, SourceKey, StableId,
};
use serde::{Deserialize, Serialize};

/// Namespace for [`JobId`] derivation. Changing it re-keys every stored job.
const JOB_ID_NAMESPACE: &str = "jobhunt.job.v1";

/// Schema tag for [`JobPosting::fingerprint`]. Bump it when the set of
/// fingerprinted fields changes.
const FINGERPRINT_SCHEMA: &str = "jobhunt.job.content.v1";

/// Stable internal identifier of a job, rendered as `job_<32 hex chars>`.
///
/// Derived from the source instance plus the source's own identifier for the
/// posting, or the canonical URL when the source has no identifier. The same
/// posting discovered again therefore always maps to the same `JobId`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(StableId);

impl JobId {
    const PREFIX: &'static str = "job_";

    pub fn derive(source: &SourceKey, source_record_id: Option<&str>, url: &CanonicalUrl) -> Self {
        let id = match source_record_id {
            Some(native) => StableId::derive(
                JOB_ID_NAMESPACE,
                &[source.kind(), source.instance(), "id", native],
            ),
            None => StableId::derive(
                JOB_ID_NAMESPACE,
                &[source.kind(), source.instance(), "url", url.as_str()],
            ),
        };
        Self(id)
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::PREFIX, self.0)
    }
}

impl fmt::Debug for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JobId({self})")
    }
}

impl FromStr for JobId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| ParseIdError(s.to_owned()))?;
        hex.parse()
            .map(Self)
            .map_err(|_| ParseIdError(s.to_owned()))
    }
}

/// Employment arrangement as stated by the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmploymentType {
    FullTime,
    PartTime,
    Contract,
    Temporary,
    Internship,
    /// A value the source provided that has no canonical mapping yet.
    Other(String),
}

impl EmploymentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::FullTime => "full_time",
            Self::PartTime => "part_time",
            Self::Contract => "contract",
            Self::Temporary => "temporary",
            Self::Internship => "internship",
            Self::Other(raw) => raw,
        }
    }

    /// Inverse of [`EmploymentType::as_str`].
    pub fn from_canonical(value: &str) -> Self {
        match value {
            "full_time" => Self::FullTime,
            "part_time" => Self::PartTime,
            "contract" => Self::Contract,
            "temporary" => Self::Temporary,
            "internship" => Self::Internship,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Where the work happens, as stated by the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkplaceType {
    Remote,
    Hybrid,
    OnSite,
    /// A value the source provided that has no canonical mapping yet.
    Other(String),
}

impl WorkplaceType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Remote => "remote",
            Self::Hybrid => "hybrid",
            Self::OnSite => "on_site",
            Self::Other(raw) => raw,
        }
    }

    /// Inverse of [`WorkplaceType::as_str`].
    pub fn from_canonical(value: &str) -> Self {
        match value {
            "remote" => Self::Remote,
            "hybrid" => Self::Hybrid,
            "on_site" => Self::OnSite,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// A location as reported by the source. Values are the source's raw text
/// (cleaned of stray whitespace); they are not geocoded or normalized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

impl SourceLocation {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.locality.is_none()
            && self.region.is_none()
            && self.country.is_none()
    }
}

/// Compensation as published by the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Compensation {
    /// The source's own human-readable summary, e.g. `"$180K – $250K • Offers Equity"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub components: Vec<CompensationComponent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompensationComponent {
    pub kind: CompensationKind,
    /// ISO 4217 currency code, when the component is monetary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<PayInterval>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationKind {
    Salary,
    /// Equity expressed as a percentage of the company.
    EquityPercentage,
    /// Equity expressed as a cash value.
    EquityCashValue,
    Bonus,
    Commission,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayInterval {
    Hour,
    Day,
    Week,
    Month,
    Year,
    OneTime,
}

/// A job posting in canonical form, as produced by a source adapter.
///
/// This is the unit of discovery: it carries everything the source said about
/// the job plus where it came from, but none of JobHunt's own bookkeeping
/// (that lives on [`JobRecord`]).
#[derive(Debug, Clone, PartialEq)]
pub struct JobPosting {
    pub provenance: Provenance,
    /// Canonical URL of the job posting page.
    pub url: CanonicalUrl,
    /// Canonical application URL, only when it differs from `url`.
    pub apply_url: Option<CanonicalUrl>,
    pub company: String,
    pub title: String,
    pub department: Option<String>,
    pub team: Option<String>,
    /// The source's primary location text, verbatim (cleaned of whitespace).
    pub location: Option<String>,
    /// Every location the source listed (primary first), with any structured
    /// address parts the source provided.
    pub locations: Vec<SourceLocation>,
    pub employment_type: Option<EmploymentType>,
    pub workplace_type: Option<WorkplaceType>,
    /// The source's own remote flag, when it has one.
    pub is_remote: Option<bool>,
    pub compensation: Option<Compensation>,
    pub description_text: Option<String>,
    pub description_html: Option<String>,
    /// When the source says the job was first published.
    pub posted_at: Option<DateTime<Utc>>,
    /// When the source says the job was last updated.
    pub source_updated_at: Option<DateTime<Utc>>,
}

impl JobPosting {
    pub fn id(&self) -> JobId {
        JobId::derive(
            &self.provenance.source,
            self.provenance.source_record_id.as_deref(),
            &self.url,
        )
    }

    /// Digest of the posting's content (everything except provenance), used
    /// to tell whether a re-discovered job actually changed.
    pub fn fingerprint(&self) -> Fingerprint {
        let mut fp = FingerprintBuilder::new(FINGERPRINT_SCHEMA)
            .field("url", self.url.as_str())
            .optional(
                "apply_url",
                self.apply_url.as_ref().map(CanonicalUrl::as_str),
            )
            .field("company", &self.company)
            .field("title", &self.title)
            .optional("department", self.department.as_deref())
            .optional("team", self.team.as_deref())
            .optional("location", self.location.as_deref())
            .field("locations.len", &self.locations.len().to_string());
        for location in &self.locations {
            fp = fp
                .optional("location.name", location.name.as_deref())
                .optional("location.locality", location.locality.as_deref())
                .optional("location.region", location.region.as_deref())
                .optional("location.country", location.country.as_deref());
        }
        fp = fp
            .optional(
                "employment_type",
                self.employment_type.as_ref().map(EmploymentType::as_str),
            )
            .optional(
                "workplace_type",
                self.workplace_type.as_ref().map(WorkplaceType::as_str),
            )
            .optional("is_remote", self.is_remote.map(bool_str));
        match &self.compensation {
            None => fp = fp.optional("compensation", None),
            Some(comp) => {
                fp = fp
                    .optional("compensation.summary", comp.summary.as_deref())
                    .field("compensation.len", &comp.components.len().to_string());
                for c in &comp.components {
                    fp = fp
                        .field("component.kind", &format!("{:?}", c.kind))
                        .optional("component.currency", c.currency.as_deref())
                        .optional("component.min", c.min.map(|v| v.to_string()).as_deref())
                        .optional("component.max", c.max.map(|v| v.to_string()).as_deref())
                        .optional(
                            "component.interval",
                            c.interval.map(|i| format!("{i:?}")).as_deref(),
                        );
                }
            }
        }
        fp.optional("description_text", self.description_text.as_deref())
            .optional("description_html", self.description_html.as_deref())
            .optional("posted_at", self.posted_at.map(timestamp).as_deref())
            .optional(
                "source_updated_at",
                self.source_updated_at.map(timestamp).as_deref(),
            )
            .finish()
    }

    /// Normalized text that search terms are matched against: title, company,
    /// department, team, every location, and the workplace type, passed
    /// through [`search_key`] and padded with spaces so that `" <term>"`
    /// matches the start of a word.
    pub fn search_document(&self) -> String {
        let mut fields: Vec<&str> = vec![&self.title, &self.company];
        fields.extend(self.department.as_deref());
        fields.extend(self.team.as_deref());
        fields.extend(self.location.as_deref());
        for location in &self.locations {
            fields.extend(location.name.as_deref());
            fields.extend(location.locality.as_deref());
            fields.extend(location.country.as_deref());
        }
        fields.extend(self.workplace_type.as_ref().map(WorkplaceType::as_str));
        let words: Vec<String> = fields
            .into_iter()
            .map(search_key)
            .filter(|w| !w.is_empty())
            .collect();
        format!(" {} ", words.join(" "))
    }

    /// Checks the invariants every canonical posting must satisfy regardless
    /// of which adapter produced it.
    pub fn validate(&self) -> Result<(), RecordError> {
        let fail = |reason| {
            Err(RecordError::new(
                self.provenance.source_record_id.clone(),
                reason,
            ))
        };
        if self.title.trim().is_empty() {
            return fail(RecordErrorReason::MissingField("title"));
        }
        if self.company.trim().is_empty() {
            return fail(RecordErrorReason::MissingField("company"));
        }
        if let Some(comp) = &self.compensation {
            for c in &comp.components {
                let bounds_ok = [c.min, c.max]
                    .into_iter()
                    .flatten()
                    .all(|v| v.is_finite() && v >= 0.0);
                let ordered = match (c.min, c.max) {
                    (Some(min), Some(max)) => min <= max,
                    _ => true,
                };
                if !bounds_ok || !ordered {
                    return fail(RecordErrorReason::InvalidValue {
                        field: "compensation",
                        detail: format!("invalid range {:?}..{:?}", c.min, c.max),
                    });
                }
            }
        }
        Ok(())
    }
}

fn bool_str(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// A job as stored by JobHunt: the canonical posting plus bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub id: JobId,
    pub posting: JobPosting,
    /// When JobHunt first discovered the job.
    pub first_seen_at: DateTime<Utc>,
    /// When JobHunt most recently saw the job at its source.
    pub last_seen_at: DateTime<Utc>,
    /// When the job's content last changed (equals `first_seen_at` until then).
    pub content_updated_at: DateTime<Utc>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn posting(source: &str, native_id: Option<&str>, title: &str) -> JobPosting {
        let key: SourceKey = source.parse().unwrap();
        let url = CanonicalUrl::parse(&format!(
            "https://jobs.example.com/{}/{}",
            key.instance(),
            native_id.unwrap_or(title)
        ))
        .unwrap();
        JobPosting {
            provenance: Provenance {
                source: key,
                source_record_id: native_id.map(str::to_owned),
                fetched_from: None,
            },
            url,
            apply_url: None,
            company: "Acme".into(),
            title: title.into(),
            department: None,
            team: None,
            location: Some("Remote".into()),
            locations: vec![],
            employment_type: Some(EmploymentType::FullTime),
            workplace_type: Some(WorkplaceType::Remote),
            is_remote: Some(true),
            compensation: None,
            description_text: None,
            description_html: None,
            posted_at: None,
            source_updated_at: None,
        }
    }

    #[test]
    fn job_id_prefers_source_record_id() {
        let a = posting("ashby:acme", Some("123"), "Engineer");
        let mut b = a.clone();
        b.url = CanonicalUrl::parse("https://elsewhere.example.com/x").unwrap();
        b.title = "Renamed".into();
        assert_eq!(a.id(), b.id(), "same native id must keep the same JobId");
    }

    #[test]
    fn job_id_falls_back_to_url() {
        let a = posting("ashby:acme", None, "engineer");
        let b = posting("ashby:acme", None, "designer");
        assert_ne!(a.id(), b.id());
        assert_eq!(a.id(), posting("ashby:acme", None, "engineer").id());
    }

    #[test]
    fn job_id_is_scoped_to_the_source_instance() {
        let a = posting("ashby:acme", Some("123"), "Engineer");
        let b = posting("ashby:other", Some("123"), "Engineer");
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn job_id_round_trips_through_text() {
        let id = posting("ashby:acme", Some("123"), "Engineer").id();
        let text = id.to_string();
        assert!(text.starts_with("job_"));
        assert_eq!(text.parse::<JobId>().unwrap(), id);
        assert!("123".parse::<JobId>().is_err());
        assert!("job_nothex".parse::<JobId>().is_err());
    }

    #[test]
    fn fingerprint_tracks_content_not_provenance() {
        let a = posting("ashby:acme", Some("123"), "Engineer");
        let mut same_content = a.clone();
        same_content.provenance.fetched_from =
            Some(CanonicalUrl::parse("https://api.example.com/board").unwrap());
        assert_eq!(a.fingerprint(), same_content.fingerprint());

        let mut changed = a.clone();
        changed.compensation = Some(Compensation {
            summary: Some("$100K".into()),
            components: vec![],
        });
        assert_ne!(a.fingerprint(), changed.fingerprint());

        let mut moved = a.clone();
        moved.locations.push(SourceLocation {
            name: Some("Berlin".into()),
            ..Default::default()
        });
        assert_ne!(a.fingerprint(), moved.fingerprint());
    }

    #[test]
    fn search_document_covers_searchable_fields() {
        let mut p = posting("ashby:acme", Some("1"), "Staff Engineer (Rust/Go)");
        p.department = Some("Trust & Safety".into());
        p.locations = vec![SourceLocation {
            name: Some("Remote (Canada)".into()),
            country: Some("Canada".into()),
            ..Default::default()
        }];
        let doc = p.search_document();
        assert!(doc.starts_with(" staff engineer rust go acme trust safety"));
        assert!(doc.contains(" remote canada "));
        assert!(
            doc.ends_with(" remote "),
            "workplace type is searchable: {doc:?}"
        );
        assert!(doc.contains(" rust"));
        assert!(!doc.contains(" rust safety"));
    }

    #[test]
    fn validate_rejects_missing_required_fields() {
        let mut p = posting("ashby:acme", Some("1"), "  ");
        assert!(matches!(
            p.validate().unwrap_err().reason,
            RecordErrorReason::MissingField("title")
        ));
        p.title = "Engineer".into();
        p.company = String::new();
        assert!(matches!(
            p.validate().unwrap_err().reason,
            RecordErrorReason::MissingField("company")
        ));
    }

    #[test]
    fn validate_rejects_inverted_compensation() {
        let mut p = posting("ashby:acme", Some("1"), "Engineer");
        p.compensation = Some(Compensation {
            summary: None,
            components: vec![CompensationComponent {
                kind: CompensationKind::Salary,
                currency: Some("USD".into()),
                min: Some(200.0),
                max: Some(100.0),
                interval: Some(PayInterval::Year),
            }],
        });
        assert!(matches!(
            p.validate().unwrap_err().reason,
            RecordErrorReason::InvalidValue { .. }
        ));
    }

    #[test]
    fn enums_round_trip_canonical_strings() {
        for t in [
            EmploymentType::FullTime,
            EmploymentType::PartTime,
            EmploymentType::Contract,
            EmploymentType::Temporary,
            EmploymentType::Internship,
            EmploymentType::Other("Seasonal".into()),
        ] {
            assert_eq!(EmploymentType::from_canonical(t.as_str()), t);
        }
        for w in [
            WorkplaceType::Remote,
            WorkplaceType::Hybrid,
            WorkplaceType::OnSite,
            WorkplaceType::Other("Flexible".into()),
        ] {
            assert_eq!(WorkplaceType::from_canonical(w.as_str()), w);
        }
    }
}
