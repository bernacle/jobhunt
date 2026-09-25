//! What one verification attempt found, as stored.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};
use jobhunt_core::{CanonicalUrl, ParseIdError, SourceKey, StableId};
use serde::{Deserialize, Serialize};

use crate::model::{JobId, OpportunityId};
use crate::verification::compensation::CompensationCheck;

/// Revision of the verification rules: how observations become states,
/// authority levels and compensation facts. Stored with every record, and
/// part of every derived result's cache identity, so a rule change never
/// silently keeps conclusions reached under the old rules.
pub const VERIFICATION_REVISION: &str = "1";

const VERIFICATION_ID_NAMESPACE: &str = "jobhunt.verification.v1";

/// Identifier of one verification attempt, rendered `ver_<32 hex>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VerificationId(StableId);

impl VerificationId {
    const PREFIX: &'static str = "ver_";

    /// The id of the attempt to verify `job` at `at` (one attempt per job
    /// per instant).
    pub fn derive(job: JobId, at: DateTime<Utc>) -> Self {
        Self(StableId::derive(
            VERIFICATION_ID_NAMESPACE,
            &[
                &job.to_string(),
                &at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        ))
    }
}

impl fmt::Display for VerificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::PREFIX, self.0)
    }
}

impl fmt::Debug for VerificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerificationId({self})")
    }
}

impl FromStr for VerificationId {
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

impl Serialize for VerificationId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for VerificationId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

macro_rules! canonical_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn from_canonical(value: &str) -> Option<Self> {
                match value {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

canonical_enum! {
    /// Whether the listing is live at its authoritative source.
    ListingStatus {
        /// The authoritative source lists the job now.
        Active => "active",
        /// The authoritative source says the job is gone (its own endpoint
        /// answered "not found", or a complete listing no longer has it).
        Closed => "closed",
        /// The source could not be reached (timeout, 5xx, connection
        /// failure), so nothing is known about the listing now.
        Unreachable => "unreachable",
        /// The source answered, but not in a way that settles it (an
        /// unreadable response, a record that does not match the job).
        Ambiguous => "ambiguous",
        /// Not checked: no verifier supports the source.
        Unknown => "unknown",
    }
}

impl ListingStatus {
    /// Whether this is a definitive answer from the source.
    pub fn is_definitive(self) -> bool {
        matches!(self, Self::Active | Self::Closed)
    }
}

canonical_enum! {
    /// Whether the job can be applied to.
    ApplicationStatus {
        /// The application route exists (see [`ApplicationBasis`] for how
        /// that is known).
        Active => "active",
        /// The application route is gone.
        Closed => "closed",
        /// The route was checked but did not answer usefully.
        Unavailable => "unavailable",
        /// Not checked, or not checkable.
        Unknown => "unknown",
    }
}

canonical_enum! {
    /// How the application status is known.
    ApplicationBasis {
        /// JobHunt requested the application page and it resolved.
        Probed => "probed",
        /// The authoritative source publishes the application route for
        /// the job (Ashby's `applyUrl`); the page itself renders in a
        /// browser and was not requested.
        Published => "published",
        /// The application form is part of the listing page, which the
        /// authoritative source answered for.
        ListingPage => "listing_page",
        /// Nothing was checked.
        NotChecked => "not_checked",
    }
}

/// The application path, as checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationCheck {
    pub status: ApplicationStatus,
    pub basis: ApplicationBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP status of the probe, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ApplicationCheck {
    pub fn unknown(detail: impl Into<String>) -> Self {
        Self {
            status: ApplicationStatus::Unknown,
            basis: ApplicationBasis::NotChecked,
            url: None,
            http_status: None,
            detail: Some(detail.into()),
        }
    }
}

canonical_enum! {
    /// Who stands behind a listing, strongest first.
    Authority {
        /// A page on the employer's own domain, verified to publish the
        /// job (or to embed the board that does).
        EmployerFirstParty => "employer_first_party",
        /// The employer's own applicant-tracking board (Ashby, Greenhouse,
        /// Lever): the company configures and publishes it itself.
        EmployerConfiguredAts => "employer_configured_ats",
        /// A platform the company posts to itself, but that is not its own
        /// system (Y Combinator's Work at a Startup).
        TrustedSource => "trusted_source",
        /// A reposting by someone other than the employer.
        SecondarySource => "secondary_source",
        /// JobHunt does not know who publishes the source.
        Unknown => "unknown",
    }
}

impl Authority {
    /// Higher is stronger.
    pub fn rank(self) -> u8 {
        match self {
            Self::EmployerFirstParty => 4,
            Self::EmployerConfiguredAts => 3,
            Self::TrustedSource => 2,
            Self::SecondarySource => 1,
            Self::Unknown => 0,
        }
    }

    /// The authority of a source family, before any page is checked.
    pub fn of_kind(kind: &str) -> Self {
        match kind {
            "ashby" | "greenhouse" | "lever" => Self::EmployerConfiguredAts,
            "yc" => Self::TrustedSource,
            _ => Self::Unknown,
        }
    }

    /// Whether it is the employer's own publication (not a platform or a
    /// repost).
    pub fn is_employer(self) -> bool {
        matches!(self, Self::EmployerFirstParty | Self::EmployerConfiguredAts)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EmployerFirstParty => "the employer's own site",
            Self::EmployerConfiguredAts => "the employer's own job board",
            Self::TrustedSource => "a platform the employer posts to itself",
            Self::SecondarySource => "a secondary source (a reposting)",
            Self::Unknown => "a source of unknown authority",
        }
    }
}

canonical_enum! {
    /// One step of an authority chain.
    LinkKind {
        /// The ATS's public API for the board.
        AtsApi => "ats_api",
        /// The job's page on the ATS.
        AtsListingPage => "ats_listing_page",
        /// A page on the employer's own domain.
        EmployerPage => "employer_page",
        /// The job's page on a hiring platform.
        PlatformPage => "platform_page",
        /// The application route.
        ApplicationPage => "application_page",
    }
}

/// One step from the employer to the verified data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityLink {
    pub kind: LinkKind,
    pub url: String,
    /// Whether JobHunt requested this URL during the attempt (as opposed
    /// to the source naming it).
    pub checked: bool,
    pub note: String,
}

canonical_enum! {
    /// How a verification was done.
    VerificationMethod {
        /// `boards-api.greenhouse.io/v1/boards/<board>/jobs/<id>`.
        GreenhouseJobApi => "greenhouse_job_api",
        /// `api.lever.co/v0/postings/<site>/<id>`.
        LeverPostingApi => "lever_posting_api",
        /// The whole Ashby board (Ashby has no single-job endpoint).
        AshbyBoardApi => "ashby_board_api",
        /// The job's Work at a Startup page (its embedded page data).
        YcJobPage => "yc_job_page",
        /// No verifier supports the source.
        Unsupported => "unsupported",
    }
}

canonical_enum! {
    /// Why an attempt could not reach a definitive answer.
    FailureKind {
        /// No verification mechanism for this source.
        NotSupported => "not_supported",
        Timeout => "timeout",
        /// The source answered with an error (5xx, 429, other statuses).
        Unavailable => "unavailable",
        /// The source answered with something JobHunt cannot read.
        Malformed => "malformed",
        /// The request itself failed (connection, TLS, DNS).
        Request => "request",
        /// The source answered for a different record than the job.
        Mismatch => "mismatch",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationFailure {
    pub kind: FailureKind,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

/// What the source published about where and how the job is done, at the
/// time of verification, verbatim. Reading it (places, restrictions) is
/// the eligibility engine's job; this is the evidence it reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedPlace {
    /// Primary location text and every listed location, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_remote: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_authorization: Option<String>,
}

/// One verification attempt of one source record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationRecord {
    pub id: VerificationId,
    pub job_id: JobId,
    pub opportunity_id: OpportunityId,
    pub source: SourceKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
    pub attempted_at: DateTime<Utc>,
    pub method: VerificationMethod,
    pub listing: ListingStatus,
    pub application: ApplicationCheck,
    pub authority: Authority,
    /// From the employer to the data, as far as it could be traced.
    #[serde(default)]
    pub authority_chain: Vec<AuthorityLink>,
    /// The authoritative URL that was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_url: Option<String>,
    /// The job's canonical listing page, as the source publishes it now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_url: Option<CanonicalUrl>,
    /// Material fingerprint of what the source published at this attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_fingerprint: Option<String>,
    /// Material fields that differ from the stored record this attempt
    /// verified (the stored record is then refreshed through the job
    /// lifecycle, which records the change as UPDATED or REOPENED).
    #[serde(default)]
    pub changed_fields: Vec<String>,
    /// Whether the published content differs from the previous successful
    /// verification. `None` when there was none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_since_last_verification: Option<bool>,
    /// The lifecycle outcome of refreshing the stored record
    /// (`unchanged`, `updated`, `reopened`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    pub compensation: CompensationCheck,
    #[serde(default)]
    pub published: PublishedPlace,
    /// What this attempt could not establish, in words.
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<VerificationFailure>,
    /// [`VERIFICATION_REVISION`] at the time.
    pub revision: String,
}

impl VerificationRecord {
    /// Whether the attempt reached a definitive answer (active or closed).
    pub fn succeeded(&self) -> bool {
        self.listing.is_definitive()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn ids_are_stable_per_job_and_instant() {
        let job: JobId = "job_02e51190085f8a9a0772e845ddd9f329".parse().unwrap();
        let at = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
        let id = VerificationId::derive(job, at);
        assert_eq!(id, VerificationId::derive(job, at));
        assert_ne!(
            id,
            VerificationId::derive(job, at + chrono::Duration::nanoseconds(1))
        );
        assert_eq!(id.to_string().parse::<VerificationId>().unwrap(), id);
        assert!(id.to_string().starts_with("ver_"));
        assert!(job.to_string().parse::<VerificationId>().is_err());
    }

    #[test]
    fn states_round_trip_and_rank() {
        for s in ListingStatus::ALL {
            assert_eq!(ListingStatus::from_canonical(s.as_str()), Some(*s));
        }
        for s in ApplicationStatus::ALL {
            assert_eq!(ApplicationStatus::from_canonical(s.as_str()), Some(*s));
        }
        for a in Authority::ALL {
            assert_eq!(Authority::from_canonical(a.as_str()), Some(*a));
        }
        assert!(ListingStatus::Closed.is_definitive());
        assert!(!ListingStatus::Unreachable.is_definitive());
        assert_eq!(
            Authority::of_kind("greenhouse"),
            Authority::EmployerConfiguredAts
        );
        assert_eq!(Authority::of_kind("yc"), Authority::TrustedSource);
        assert_eq!(Authority::of_kind("somewhere"), Authority::Unknown);
        assert!(!Authority::TrustedSource.is_employer());
        assert!(Authority::EmployerFirstParty.rank() > Authority::EmployerConfiguredAts.rank());
    }
}
