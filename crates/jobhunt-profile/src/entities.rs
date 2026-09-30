//! A profile as a set of independent entities, for sync and for storage
//! that keeps each record as one opaque (encrypted) value.
//!
//! [`decompose`] turns a [`ProfileData`] into one [`Entity`] per record: the
//! basics, every document, experience, project, education entry, skill,
//! claim, preference, statement and taste statement, and the taste brief, each with its stable id, its JSON body
//! and a digest of that body. [`compose`] is the inverse. Records keep the
//! ids the profile domain gave them (derived from the profile and the
//! resume's import keys, or from content and creation time), so the same
//! record has the same identity on every machine and in the cloud.
//!
//! Bodies are canonical: object keys sorted (serde_json's map), the
//! profile's revision left out (it is a storage counter, not content), and
//! timestamps cut to microseconds, the precision every store keeps. Two
//! stores holding the same record therefore produce the same digest, which
//! is what sync compares.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::aggregate::ProfileData;
use crate::evidence::Claim;
use crate::export::{ExportError, ProfileExport};
use crate::ids::ProfileId;
use crate::model::{Education, Experience, Profile, Project, Skill, SourceDocument};
use crate::preferences::{Preference, PreferenceStatement};
use crate::taste::{TasteAssertion, TasteBrief};

/// What kind of record an entity is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// The basics (name, headline, location, contacts, languages).
    Profile,
    Document,
    Experience,
    Project,
    Education,
    Skill,
    Claim,
    Preference,
    Statement,
    /// A statement of the candidate taste profile.
    Taste,
    /// What the person is looking for, in their words (one per profile).
    TasteBrief,
}

impl EntityKind {
    pub const ALL: [EntityKind; 11] = [
        Self::Profile,
        Self::Document,
        Self::Experience,
        Self::Project,
        Self::Education,
        Self::Skill,
        Self::Claim,
        Self::Preference,
        Self::Statement,
        Self::Taste,
        Self::TasteBrief,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Document => "document",
            Self::Experience => "experience",
            Self::Project => "project",
            Self::Education => "education",
            Self::Skill => "skill",
            Self::Claim => "claim",
            Self::Preference => "preference",
            Self::Statement => "statement",
            Self::Taste => "taste",
            Self::TasteBrief => "taste_brief",
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EntityKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| format!("unknown profile entity kind {s:?}"))
    }
}

/// Identifies an entity within a profile.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityKey {
    pub kind: EntityKind,
    /// The record's id (`exp_…`, `clm_…`, …; the profile's own id for
    /// [`EntityKind::Profile`]).
    pub id: String,
}

impl EntityKey {
    pub fn new(kind: EntityKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

impl fmt::Display for EntityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.id)
    }
}

/// One record of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub key: EntityKey,
    /// The record as canonical JSON.
    pub body: Value,
    /// `sha256` hex of the canonical body.
    pub digest: String,
}

impl Entity {
    fn of<T: Serialize>(kind: EntityKind, id: String, record: &T) -> Result<Self, EntityError> {
        let mut body = serde_json::to_value(record).map_err(|e| EntityError::Encode {
            key: EntityKey::new(kind, id.clone()),
            source: e,
        })?;
        canonicalize(&mut body);
        Ok(Self::from_body(EntityKey::new(kind, id), body))
    }

    /// An entity from a body received from elsewhere (canonicalized again,
    /// so its digest is comparable).
    pub fn from_body(key: EntityKey, mut body: Value) -> Self {
        canonicalize(&mut body);
        let digest = digest(&body);
        Self { key, body, digest }
    }
}

/// The digest of a canonical body.
pub fn digest(body: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.to_string().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Why a profile could not be split or rebuilt.
#[derive(Debug, thiserror::Error)]
pub enum EntityError {
    #[error("could not encode {key}")]
    Encode {
        key: EntityKey,
        #[source]
        source: serde_json::Error,
    },
    #[error("stored {key} is not a valid record: {source}")]
    Decode {
        key: EntityKey,
        #[source]
        source: serde_json::Error,
    },
    #[error("{key} has the id {found} inside its body")]
    Mismatch { key: EntityKey, found: String },
    #[error("the profile has no basics record")]
    NoBasics,
    #[error("the profile is inconsistent: {0}")]
    Invalid(#[source] ExportError),
}

/// Keys whose values are timestamps (`created_at`, `stale_since`, ...).
fn is_timestamp_key(key: &str) -> bool {
    key.ends_with("_at") || key.ends_with("_since")
}

/// Sorted keys (serde_json's map is ordered), no `revision` at the top of
/// the basics, timestamps at microsecond precision.
fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if is_timestamp_key(key)
                    && let Value::String(text) = v
                    && let Ok(at) = chrono::DateTime::parse_from_rfc3339(text)
                {
                    *text = at
                        .with_timezone(&chrono::Utc)
                        .format("%Y-%m-%dT%H:%M:%S%.6fZ")
                        .to_string();
                } else {
                    canonicalize(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize),
        _ => {}
    }
}

/// Every record of `data` as an entity, sorted by key.
pub fn decompose(data: &ProfileData) -> Result<Vec<Entity>, EntityError> {
    let mut out = Vec::new();
    let mut basics = Entity::of(EntityKind::Profile, data.id().to_string(), &data.profile)?;
    if let Value::Object(map) = &mut basics.body {
        map.remove("revision");
    }
    out.push(Entity::from_body(basics.key, basics.body));
    for d in &data.documents {
        out.push(Entity::of(EntityKind::Document, d.id.to_string(), d)?);
    }
    for e in &data.experiences {
        out.push(Entity::of(EntityKind::Experience, e.id.to_string(), e)?);
    }
    for p in &data.projects {
        out.push(Entity::of(EntityKind::Project, p.id.to_string(), p)?);
    }
    for e in &data.education {
        out.push(Entity::of(EntityKind::Education, e.id.to_string(), e)?);
    }
    for s in &data.skills {
        out.push(Entity::of(EntityKind::Skill, s.id.to_string(), s)?);
    }
    for c in &data.claims {
        out.push(Entity::of(EntityKind::Claim, c.id.to_string(), c)?);
    }
    for p in &data.preferences {
        out.push(Entity::of(EntityKind::Preference, p.id.to_string(), p)?);
    }
    for s in &data.statements {
        out.push(Entity::of(EntityKind::Statement, s.id.to_string(), s)?);
    }
    for t in &data.taste {
        out.push(Entity::of(EntityKind::Taste, t.id.to_string(), t)?);
    }
    if let Some(b) = &data.taste_brief {
        out.push(Entity::of(EntityKind::TasteBrief, b.id.to_string(), b)?);
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

fn decode<T: serde::de::DeserializeOwned>(entity: &Entity) -> Result<T, EntityError> {
    serde_json::from_value(entity.body.clone()).map_err(|source| EntityError::Decode {
        key: entity.key.clone(),
        source,
    })
}

fn check_id(entity: &Entity, id: String) -> Result<(), EntityError> {
    if entity.key.id == id {
        Ok(())
    } else {
        Err(EntityError::Mismatch {
            key: entity.key.clone(),
            found: id,
        })
    }
}

/// Rebuilds a profile from its entities, at `revision`, in the order the
/// local store loads records (so both backends return the same aggregate).
/// With `validate`, every reference must resolve (the export format's
/// rules).
pub fn compose(
    id: ProfileId,
    revision: u64,
    entities: &[Entity],
    validate: bool,
) -> Result<ProfileData, EntityError> {
    let mut profile: Option<Profile> = None;
    let mut data = ProfileData::new(id, chrono::DateTime::<chrono::Utc>::UNIX_EPOCH);
    for entity in entities {
        match entity.key.kind {
            EntityKind::Profile => {
                let mut body = entity.body.clone();
                if let Value::Object(map) = &mut body {
                    map.insert("revision".into(), Value::from(revision));
                }
                let p: Profile =
                    serde_json::from_value(body).map_err(|source| EntityError::Decode {
                        key: entity.key.clone(),
                        source,
                    })?;
                check_id(entity, p.id.to_string())?;
                profile = Some(p);
            }
            EntityKind::Document => {
                let d: SourceDocument = decode(entity)?;
                check_id(entity, d.id.to_string())?;
                data.documents.push(d);
            }
            EntityKind::Experience => {
                let e: Experience = decode(entity)?;
                check_id(entity, e.id.to_string())?;
                data.experiences.push(e);
            }
            EntityKind::Project => {
                let p: Project = decode(entity)?;
                check_id(entity, p.id.to_string())?;
                data.projects.push(p);
            }
            EntityKind::Education => {
                let e: Education = decode(entity)?;
                check_id(entity, e.id.to_string())?;
                data.education.push(e);
            }
            EntityKind::Skill => {
                let s: Skill = decode(entity)?;
                check_id(entity, s.id.to_string())?;
                data.skills.push(s);
            }
            EntityKind::Claim => {
                let c: Claim = decode(entity)?;
                check_id(entity, c.id.to_string())?;
                data.claims.push(c);
            }
            EntityKind::Preference => {
                let p: Preference = decode(entity)?;
                check_id(entity, p.id.to_string())?;
                data.preferences.push(p);
            }
            EntityKind::Statement => {
                let s: PreferenceStatement = decode(entity)?;
                check_id(entity, s.id.to_string())?;
                data.statements.push(s);
            }
            EntityKind::Taste => {
                let t: TasteAssertion = decode(entity)?;
                check_id(entity, t.id.to_string())?;
                data.taste.push(t);
            }
            EntityKind::TasteBrief => {
                let b: TasteBrief = decode(entity)?;
                check_id(entity, b.id.to_string())?;
                data.taste_brief = Some(b);
            }
        }
    }
    let mut profile = profile.ok_or(EntityError::NoBasics)?;
    profile.id = id;
    profile.revision = revision;
    data.profile = profile;
    sort_like_storage(&mut data);
    if validate {
        ProfileExport::from_data(&data, data.profile.updated_at, None)
            .validate()
            .map_err(EntityError::Invalid)?;
    }
    Ok(data)
}

/// The order the local store returns records in.
pub fn sort_like_storage(data: &mut ProfileData) {
    data.documents.sort_by(|a, b| {
        a.first_imported_at
            .cmp(&b.first_imported_at)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.experiences.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.projects.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.education.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.skills.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.claims.sort_by(|a, b| {
        a.subject
            .kind_str()
            .cmp(b.subject.kind_str())
            .then_with(|| a.subject.id_string().cmp(&b.subject.id_string()))
            .then_with(|| a.position.cmp(&b.position))
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.statements.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.preferences.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
    data.taste.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
    });
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::model::RecordMeta;

    fn sample() -> ProfileData {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 25, 12, 0, 0)
            .unwrap()
            .checked_add_signed(chrono::Duration::nanoseconds(123_456_789))
            .unwrap();
        let mut data = ProfileData::new(ProfileId::local(), now);
        data.profile.headline = Some("Backend engineer".into());
        data.profile.revision = 7;
        let meta = RecordMeta::user(now);
        data.experiences.push(Experience {
            id: crate::ids::ExperienceId::derive(&["x"]),
            company: Some("Acme".into()),
            title: Some("Engineer".into()),
            employment: None,
            start: None,
            end: None,
            current: true,
            location: None,
            summary: None,
            position: 0,
            meta,
        });
        data
    }

    #[test]
    fn round_trips_and_ignores_the_revision() {
        let data = sample();
        let entities = decompose(&data).unwrap();
        assert_eq!(entities.len(), 2);
        assert!(entities[0].body.get("revision").is_none());
        let back = compose(ProfileId::local(), 7, &entities, true).unwrap();
        assert_eq!(back.profile.revision, 7);
        assert_eq!(back.experiences, data.experiences_truncated());
        // Same records, another revision: same digests.
        let mut other = data.clone();
        other.profile.revision = 99;
        let again = decompose(&other).unwrap();
        assert_eq!(
            entities.iter().map(|e| &e.digest).collect::<Vec<_>>(),
            again.iter().map(|e| &e.digest).collect::<Vec<_>>()
        );
    }

    #[test]
    fn timestamps_are_cut_to_microseconds() {
        let entities = decompose(&sample()).unwrap();
        let created = entities[1].body["meta"]["created_at"].as_str().unwrap();
        assert_eq!(created, "2026-09-25T12:00:00.123456Z");
        let reencoded = Entity::from_body(entities[1].key.clone(), entities[1].body.clone());
        assert_eq!(reencoded.digest, entities[1].digest);
    }

    #[test]
    fn invalid_references_are_rejected_when_validating() {
        let mut data = sample();
        let mut project = Project {
            id: crate::ids::ProjectId::derive(&["p"]),
            name: "JobHunt".into(),
            description: None,
            role: None,
            url: None,
            start: None,
            end: None,
            current: false,
            experience: Some(crate::ids::ExperienceId::derive(&["missing"])),
            position: 0,
            meta: RecordMeta::user(data.profile.created_at),
        };
        data.projects.push(project.clone());
        let entities = decompose(&data).unwrap();
        assert!(matches!(
            compose(ProfileId::local(), 1, &entities, true),
            Err(EntityError::Invalid(_))
        ));
        assert!(compose(ProfileId::local(), 1, &entities, false).is_ok());
        project.experience = None;
        data.projects = vec![project];
        assert!(compose(ProfileId::local(), 1, &decompose(&data).unwrap(), true).is_ok());
    }

    #[test]
    fn kinds_parse_back() {
        for kind in EntityKind::ALL {
            assert_eq!(kind.as_str().parse::<EntityKind>(), Ok(kind));
        }
    }

    impl ProfileData {
        fn experiences_truncated(&self) -> Vec<Experience> {
            let entities = decompose(self).unwrap();
            compose(self.id(), 0, &entities, false).unwrap().experiences
        }
    }
}
