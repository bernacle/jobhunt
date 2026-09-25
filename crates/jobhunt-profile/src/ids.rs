//! Identifiers of profile records.
//!
//! Every id is a [`StableId`] rendered with a short type prefix
//! (`exp_<32 hex>`, `clm_<32 hex>`, ...). Records derived from a resume get
//! ids derived from their profile and their *import key* (see
//! [`crate::import`]), so importing the same resume on any machine produces
//! the same ids. Records the user creates get ids derived from their content
//! and creation time.

use std::fmt;
use std::str::FromStr;

use jobhunt_core::{ParseIdError, StableId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! profile_id {
    ($(#[$doc:meta])* $name:ident, $prefix:literal, $namespace:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(StableId);

        impl $name {
            pub const PREFIX: &'static str = $prefix;
            const NAMESPACE: &'static str = $namespace;

            /// Derives the id deterministically from `parts`.
            pub fn derive(parts: &[&str]) -> Self {
                Self(StableId::derive(Self::NAMESPACE, parts))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", Self::PREFIX, self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.strip_prefix(Self::PREFIX)
                    .and_then(|hex| hex.parse().ok())
                    .map(Self)
                    .ok_or_else(|| ParseIdError(s.to_owned()))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                raw.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

profile_id!(
    /// A career profile (`prof_…`). Local installs have one, [`ProfileId::local`].
    ProfileId, "prof_", "jobhunt.profile.v1"
);
profile_id!(
    /// An imported source document such as a resume (`doc_…`).
    DocumentId, "doc_", "jobhunt.profile.document.v1"
);
profile_id!(
    /// One position held at one organization (`exp_…`).
    ExperienceId, "exp_", "jobhunt.profile.experience.v1"
);
profile_id!(
    /// A project (`proj_…`).
    ProjectId, "proj_", "jobhunt.profile.project.v1"
);
profile_id!(
    /// An education entry (`edu_…`).
    EducationId, "edu_", "jobhunt.profile.education.v1"
);
profile_id!(
    /// A skill (`skill_…`).
    SkillId, "skill_", "jobhunt.profile.skill.v1"
);
profile_id!(
    /// A professional claim in the evidence graph (`clm_…`).
    ClaimId, "clm_", "jobhunt.profile.claim.v1"
);
profile_id!(
    /// A structured preference (`pref_…`).
    PreferenceId, "pref_", "jobhunt.profile.preference.v1"
);
profile_id!(
    /// A preference statement, in the user's own words (`stmt_…`).
    StatementId, "stmt_", "jobhunt.profile.statement.v1"
);

impl ProfileId {
    /// The single profile of a local install. The same on every machine, so
    /// exported profiles import back onto it.
    pub fn local() -> Self {
        Self::derive(&["local", "default"])
    }
}

/// Any record a user can address by id on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordId {
    Experience(ExperienceId),
    Project(ProjectId),
    Education(EducationId),
    Skill(SkillId),
    Claim(ClaimId),
    Preference(PreferenceId),
    Statement(StatementId),
}

impl FromStr for RecordId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.starts_with(ExperienceId::PREFIX) {
            s.parse().map(Self::Experience)
        } else if s.starts_with(ProjectId::PREFIX) {
            s.parse().map(Self::Project)
        } else if s.starts_with(EducationId::PREFIX) {
            s.parse().map(Self::Education)
        } else if s.starts_with(SkillId::PREFIX) {
            s.parse().map(Self::Skill)
        } else if s.starts_with(ClaimId::PREFIX) {
            s.parse().map(Self::Claim)
        } else if s.starts_with(PreferenceId::PREFIX) {
            s.parse().map(Self::Preference)
        } else if s.starts_with(StatementId::PREFIX) {
            s.parse().map(Self::Statement)
        } else {
            Err(ParseIdError(s.to_owned()))
        }
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Experience(id) => id.fmt(f),
            Self::Project(id) => id.fmt(f),
            Self::Education(id) => id.fmt(f),
            Self::Skill(id) => id.fmt(f),
            Self::Claim(id) => id.fmt(f),
            Self::Preference(id) => id.fmt(f),
            Self::Statement(id) => id.fmt(f),
        }
    }
}

/// Parses an id the user typed, accepting a unique prefix of the hex part
/// (`clm_3fa2`) as long as it matches exactly one candidate.
pub fn resolve_prefix<T: Copy + fmt::Display>(input: &str, candidates: &[T]) -> Option<T> {
    let input = input.trim().to_ascii_lowercase();
    if input.is_empty() {
        return None;
    }
    let mut found = None;
    for candidate in candidates {
        let text = candidate.to_string();
        if text == input {
            return Some(*candidate);
        }
        if text.starts_with(&input) {
            if found.is_some() {
                return None;
            }
            found = Some(*candidate);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_with_prefixes() {
        let id = ClaimId::derive(&["a", "b"]);
        let text = id.to_string();
        assert!(text.starts_with("clm_"));
        assert_eq!(text.len(), 4 + 32);
        assert_eq!(text.parse::<ClaimId>().unwrap(), id);
        assert!(text.parse::<ExperienceId>().is_err());
        assert_eq!(
            text.parse::<RecordId>().unwrap(),
            RecordId::Claim(id),
            "record ids dispatch on the prefix"
        );
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<ClaimId>(&json).unwrap(), id);
    }

    #[test]
    fn local_profile_is_stable() {
        assert_eq!(ProfileId::local(), ProfileId::local());
        assert_eq!(
            ProfileId::local().to_string(),
            "prof_".to_owned()
                + &StableId::derive("jobhunt.profile.v1", &["local", "default"]).to_hex()
        );
    }

    #[test]
    fn prefixes_resolve_only_when_unique() {
        let a = ClaimId::derive(&["a"]);
        let b = ClaimId::derive(&["b"]);
        let ids = [a, b];
        let full = a.to_string();
        assert_eq!(resolve_prefix(&full, &ids), Some(a));
        assert_eq!(resolve_prefix(&full[..12], &ids), Some(a));
        assert_eq!(resolve_prefix("clm_", &ids), None, "ambiguous");
        assert_eq!(resolve_prefix("exp_", &ids), None);
    }
}
