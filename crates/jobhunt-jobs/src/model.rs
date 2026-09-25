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
const FINGERPRINT_SCHEMA: &str = "jobhunt.job.content.v2";

/// Schema tag for [`JobSnapshot::fingerprint`] (material content).
const MATERIAL_SCHEMA: &str = "jobhunt.job.material.v1";

/// Revision of the canonical conversion as a whole: the model plus every
/// adapter's mapping into it. Stored with each scan; a listing validator
/// (ETag) recorded under another revision is not reused, so a conversion
/// change always re-reads sources instead of trusting "not modified".
/// Bump it whenever an adapter or the model changes what a posting contains.
pub const CANONICAL_REVISION: &str = "3";

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
        match source_record_id {
            Some(native) => Self::derive_from_record_id(source, native),
            None => Self(StableId::derive(
                JOB_ID_NAMESPACE,
                &[source.kind(), source.instance(), "url", url.as_str()],
            )),
        }
    }

    /// The id of the job a source identifies as `record_id`.
    pub fn derive_from_record_id(source: &SourceKey, record_id: &str) -> Self {
        Self(StableId::derive(
            JOB_ID_NAMESPACE,
            &[source.kind(), source.instance(), "id", record_id],
        ))
    }
}

impl JobId {
    fn stable(&self) -> StableId {
        self.0
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

impl Serialize for JobId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for JobId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Identifier of a logical opportunity: one real job that may be listed by
/// several sources. Rendered as `opp_<32 hex chars>`.
///
/// A group of equivalent source records takes the id of its founding record
/// (the one JobHunt saw first), so a job listed by a single source has the
/// opportunity id `opp_<same hex as its job id>`, and the id stays put as
/// other sources join the group.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpportunityId(StableId);

impl OpportunityId {
    const PREFIX: &'static str = "opp_";

    pub fn founded_by(job: JobId) -> Self {
        Self(job.stable())
    }
}

impl fmt::Display for OpportunityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::PREFIX, self.0)
    }
}

impl fmt::Debug for OpportunityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OpportunityId({self})")
    }
}

impl FromStr for OpportunityId {
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

impl Serialize for OpportunityId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for OpportunityId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Whether a source still lists the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobStatus {
    /// Listed at the last successful scan of its source.
    Open,
    /// Missing from a complete listing of its source.
    Closed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            _ => None,
        }
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
    /// The source's own name for this component, when it has one (for
    /// example Greenhouse's `Canada Annual Pay Range`, which says where the
    /// range applies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    /// Who may be hired, when the source publishes it as a field of its own
    /// (Work at a Startup's "US citizen/visa only"), verbatim. Statements in
    /// the description are not copied here.
    pub work_authorization: Option<String>,
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

    /// Digest of everything the posting stores except provenance. When it
    /// matches the stored value, nothing needs to be rewritten.
    ///
    /// This is deliberately broader than [`JobPosting::content_fingerprint`]:
    /// a changed HTML markup or source update timestamp rewrites the stored
    /// row but does not count as a material update.
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
                        .optional("component.label", c.label.as_deref())
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
        fp.optional("work_authorization", self.work_authorization.as_deref())
            .optional("description_text", self.description_text.as_deref())
            .optional("description_html", self.description_html.as_deref())
            .optional("posted_at", self.posted_at.map(timestamp).as_deref())
            .optional(
                "source_updated_at",
                self.source_updated_at.map(timestamp).as_deref(),
            )
            .finish()
    }

    /// Digest of the posting's material content (see [`JobSnapshot`]). A
    /// different value means the job was UPDATED.
    pub fn content_fingerprint(&self) -> Fingerprint {
        self.snapshot().fingerprint()
    }

    /// The material content of the posting.
    pub fn snapshot(&self) -> JobSnapshot {
        JobSnapshot {
            url: self.url.clone(),
            apply_url: self.apply_url.clone(),
            company: self.company.clone(),
            title: self.title.clone(),
            department: self.department.clone(),
            team: self.team.clone(),
            location: self.location.clone(),
            locations: self.locations.clone(),
            employment_type: self.employment_type.as_ref().map(|t| t.as_str().to_owned()),
            workplace_type: self.workplace_type.as_ref().map(|t| t.as_str().to_owned()),
            is_remote: self.is_remote,
            compensation: self.compensation.clone(),
            work_authorization: self.work_authorization.clone(),
            description_text: self.description_text.clone(),
            posted_at: self.posted_at,
        }
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

/// The material content of a posting: what a person reading it would
/// consider a change. Excludes provenance, the description's markup (the
/// plain text is compared instead) and the source's own update timestamp,
/// which sources bump for edits nobody can see.
///
/// Snapshots are what job history stores: the version a change replaced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub url: CanonicalUrl,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_url: Option<CanonicalUrl>,
    pub company: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<SourceLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employment_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_remote: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<Compensation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<DateTime<Utc>>,
}

impl JobSnapshot {
    pub fn fingerprint(&self) -> Fingerprint {
        // serde_json writes fields in declaration order and numbers
        // deterministically, so the encoding is canonical. Serializing plain
        // data cannot fail; the fallback only keeps this function total.
        let encoded = serde_json::to_string(self).unwrap_or_default();
        FingerprintBuilder::new(MATERIAL_SCHEMA)
            .field("snapshot", &encoded)
            .finish()
    }

    /// Names of the material fields that differ between two snapshots, in a
    /// fixed order: `title`, `company`, `url`, `apply_url`, `department`,
    /// `team`, `location`, `employment_type`, `workplace`, `compensation`,
    /// `work_authorization`, `description`, `posted_at`.
    pub fn changed_fields(&self, other: &JobSnapshot) -> Vec<&'static str> {
        let checks = [
            ("title", self.title != other.title),
            ("company", self.company != other.company),
            ("url", self.url != other.url),
            ("apply_url", self.apply_url != other.apply_url),
            ("department", self.department != other.department),
            ("team", self.team != other.team),
            (
                "location",
                self.location != other.location || self.locations != other.locations,
            ),
            (
                "employment_type",
                self.employment_type != other.employment_type,
            ),
            (
                "workplace",
                self.workplace_type != other.workplace_type || self.is_remote != other.is_remote,
            ),
            ("compensation", self.compensation != other.compensation),
            (
                "work_authorization",
                self.work_authorization != other.work_authorization,
            ),
            (
                "description",
                self.description_text != other.description_text,
            ),
            ("posted_at", self.posted_at != other.posted_at),
        ];
        checks
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect()
    }
}

/// A job as stored by JobHunt: the canonical posting plus bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub id: JobId,
    pub posting: JobPosting,
    /// When JobHunt first discovered the job.
    pub first_seen_at: DateTime<Utc>,
    /// When JobHunt most recently saw the job listed at its source.
    pub last_seen_at: DateTime<Utc>,
    /// When the job's material content last changed (equals `first_seen_at`
    /// until then).
    pub content_updated_at: DateTime<Utc>,
    pub status: JobStatus,
    /// When the job was found missing from its source; `None` while open.
    pub closed_at: Option<DateTime<Utc>>,
    /// The logical opportunity this source record belongs to.
    pub opportunity_id: OpportunityId,
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
            work_authorization: None,
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
    fn material_fingerprint_ignores_markup_and_source_timestamps() {
        let a = posting("ashby:acme", Some("123"), "Engineer");
        let mut cosmetic = a.clone();
        cosmetic.description_html = Some("<p style=\"x\">Hi</p>".into());
        cosmetic.source_updated_at = Some(Utc::now());
        cosmetic.provenance.fetched_from =
            Some(CanonicalUrl::parse("https://api.example.com/board").unwrap());
        assert_ne!(a.fingerprint(), cosmetic.fingerprint());
        assert_eq!(a.content_fingerprint(), cosmetic.content_fingerprint());

        let mut edited = a.clone();
        edited.description_text = Some("New text".into());
        assert_ne!(a.content_fingerprint(), edited.content_fingerprint());
    }

    #[test]
    fn snapshots_report_changed_fields() {
        let a = posting("ashby:acme", Some("123"), "Engineer");
        let mut b = a.clone();
        b.title = "Senior Engineer".into();
        b.compensation = Some(Compensation {
            summary: Some("$1".into()),
            components: vec![],
        });
        b.is_remote = Some(false);
        assert_eq!(
            a.snapshot().changed_fields(&b.snapshot()),
            vec!["title", "workplace", "compensation"]
        );
        assert!(a.snapshot().changed_fields(&a.snapshot()).is_empty());

        let json = serde_json::to_string(&b.snapshot()).unwrap();
        let back: JobSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b.snapshot());
        assert_eq!(back.fingerprint(), b.content_fingerprint());
    }

    #[test]
    fn opportunity_ids_follow_their_founding_job() {
        let job = posting("ashby:acme", Some("123"), "Engineer").id();
        let opp = OpportunityId::founded_by(job);
        assert_eq!(
            opp.to_string().trim_start_matches("opp_"),
            job.to_string().trim_start_matches("job_")
        );
        assert_eq!(opp.to_string().parse::<OpportunityId>().unwrap(), opp);
        assert!(job.to_string().parse::<OpportunityId>().is_err());
        assert_eq!(JobStatus::from_canonical("closed"), Some(JobStatus::Closed));
        assert_eq!(JobStatus::from_canonical("x"), None);
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
                label: None,
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
