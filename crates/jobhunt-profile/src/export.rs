//! The portable profile format.
//!
//! `narrow profile export` writes a [`ProfileExport`]: one JSON object
//! with a `format` name and a `version`, then every record of the profile
//! with its provenance and verification state. `narrow profile import`
//! reads it back. Import is all or nothing: the file is parsed strictly
//! (unknown fields are errors), then [`ProfileExport::validate`] checks
//! that every reference points to a record in the file, and only then is
//! the stored profile replaced, in one transaction.
//!
//! Version history:
//!
//! * `1`: resume and user data in the original schema.
//! * `2`: LinkedIn and GitHub sources, corroborations, and source-specific
//!   field support. A profile that still has the original shape writes v1.
//! * `3`: the candidate taste profile (`taste_brief`, `taste`), with every
//!   statement's provenance and the person's corrections and removals. A
//!   profile without one writes v2 (or v1) exactly as before, so older
//!   versions of JobHunt still read it; a v3 file is refused by them with a
//!   clear message rather than losing the corrections.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::aggregate::ProfileData;
use crate::evidence::{Claim, Subject};
use crate::ids::{DocumentId, ProfileId};
use crate::model::{Education, Experience, Profile, Project, Skill, SourceDocument};
use crate::preferences::{Preference, PreferenceStatement};
use crate::taste::{TasteAssertion, TasteBrief};

pub const EXPORT_FORMAT: &str = "jobhunt.profile";
pub const EXPORT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileExport {
    pub format: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    /// The program that wrote the file (`narrow 0.1.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    pub profile: Profile,
    #[serde(default)]
    pub documents: Vec<SourceDocument>,
    #[serde(default)]
    pub experiences: Vec<Experience>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub education: Vec<Education>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub preferences: Vec<Preference>,
    #[serde(default)]
    pub statements: Vec<PreferenceStatement>,
    /// What the person is looking for, in their words (v3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taste_brief: Option<TasteBrief>,
    /// The taste profile's statements, tombstones included (v3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taste: Vec<TasteAssertion>,
}

/// Why a profile file cannot be imported.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("not a JSON document")]
    Json(#[source] serde_json::Error),
    #[error("not a JobHunt profile file (format is {found:?}, expected {EXPORT_FORMAT:?})")]
    Format { found: String },
    #[error(
        "profile file version {found} is not supported (this JobHunt reads versions 1 to {EXPORT_VERSION}); update JobHunt"
    )]
    Version { found: String },
    #[error("invalid profile file: {0}")]
    Schema(#[source] serde_json::Error),
    #[error("invalid profile file:\n  - {}", .0.join("\n  - "))]
    Invalid(Vec<String>),
}

impl ProfileExport {
    pub fn from_data(data: &ProfileData, now: DateTime<Utc>, generator: Option<String>) -> Self {
        let modern = !data.profile.basic_sources.is_empty()
            || data.documents.iter().any(|d| {
                matches!(
                    d.kind,
                    crate::model::DocumentKind::Linkedin | crate::model::DocumentKind::Github
                )
            })
            || data
                .experiences
                .iter()
                .map(|e| &e.meta)
                .chain(data.projects.iter().map(|p| &p.meta))
                .chain(data.education.iter().map(|e| &e.meta))
                .chain(data.skills.iter().map(|s| &s.meta))
                .any(|m| {
                    matches!(
                        m.origin,
                        crate::model::Origin::Linkedin | crate::model::Origin::Github
                    ) || !m.corroborations.is_empty()
                        || !m.source_snapshots.is_empty()
                })
            || data.claims.iter().any(|c| !c.corroborations.is_empty());
        let tasteful = data.taste_brief.is_some() || !data.taste.is_empty();
        Self {
            format: EXPORT_FORMAT.to_owned(),
            version: if tasteful {
                3
            } else if modern {
                2
            } else {
                1
            },
            exported_at: now,
            generator,
            profile: data.profile.clone(),
            documents: data.documents.clone(),
            experiences: data.experiences.clone(),
            projects: data.projects.clone(),
            education: data.education.clone(),
            skills: data.skills.clone(),
            claims: data.claims.clone(),
            preferences: data.preferences.clone(),
            statements: data.statements.clone(),
            taste_brief: data.taste_brief.clone(),
            taste: data.taste.clone(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses and validates a profile file. The format name and version
    /// are checked before anything else, so an unsupported version gets a
    /// clear message rather than a schema error.
    pub fn parse(json: &str) -> Result<Self, ExportError> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(ExportError::Json)?;
        let format = value.get("format").and_then(|f| f.as_str()).unwrap_or("");
        if format != EXPORT_FORMAT {
            return Err(ExportError::Format {
                found: format.to_owned(),
            });
        }
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(v) if (1..=u64::from(EXPORT_VERSION)).contains(&v) => {}
            other => {
                return Err(ExportError::Version {
                    found: other.map_or_else(
                        || {
                            value
                                .get("version")
                                .map_or("missing".into(), ToString::to_string)
                        },
                        |v| v.to_string(),
                    ),
                });
            }
        }
        let export: Self = serde_json::from_value(value).map_err(ExportError::Schema)?;
        export.validate()?;
        Ok(export)
    }

    /// Checks internal consistency: unique ids, and every reference (claim
    /// subjects, sources, superseded claims, project experiences,
    /// preference statements) resolving to a record in the file.
    pub fn validate(&self) -> Result<(), ExportError> {
        let mut problems: Vec<String> = Vec::new();
        let mut unique = |what: &str, ids: Vec<String>| -> HashSet<String> {
            let mut seen = HashSet::new();
            for id in ids {
                if !seen.insert(id.clone()) {
                    problems.push(format!("{what} {id} appears twice"));
                }
            }
            seen
        };
        let documents = unique(
            "document",
            self.documents.iter().map(|d| d.id.to_string()).collect(),
        );
        let experiences = unique(
            "experience",
            self.experiences.iter().map(|e| e.id.to_string()).collect(),
        );
        let projects = unique(
            "project",
            self.projects.iter().map(|p| p.id.to_string()).collect(),
        );
        let education = unique(
            "education entry",
            self.education.iter().map(|e| e.id.to_string()).collect(),
        );
        unique(
            "skill",
            self.skills.iter().map(|s| s.id.to_string()).collect(),
        );
        let claims = unique(
            "claim",
            self.claims.iter().map(|c| c.id.to_string()).collect(),
        );
        let preferences = unique(
            "preference",
            self.preferences.iter().map(|p| p.id.to_string()).collect(),
        );
        let statements = unique(
            "statement",
            self.statements.iter().map(|s| s.id.to_string()).collect(),
        );
        let taste = unique(
            "taste statement",
            self.taste.iter().map(|t| t.id.to_string()).collect(),
        );

        let doc_ok = |id: &DocumentId| documents.contains(&id.to_string());
        for d in &self.documents {
            if d.sha256.len() != 64 || !d.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                problems.push(format!("document {} has an invalid sha256", d.id));
            }
        }
        let mut check_source = |owner: String, source: Option<&crate::model::SourceRef>| {
            if let Some(source) = source
                && !doc_ok(&source.document)
            {
                problems.push(format!(
                    "{owner} cites document {} which is not in the file",
                    source.document
                ));
            }
        };
        let metas = self
            .experiences
            .iter()
            .map(|e| (e.id.to_string(), &e.meta))
            .chain(self.projects.iter().map(|p| (p.id.to_string(), &p.meta)))
            .chain(self.education.iter().map(|e| (e.id.to_string(), &e.meta)))
            .chain(self.skills.iter().map(|s| (s.id.to_string(), &s.meta)));
        for (id, meta) in metas {
            check_source(id.clone(), meta.source.as_ref());
            for c in &meta.corroborations {
                check_source(id.clone(), Some(c));
            }
        }
        for c in &self.claims {
            check_source(c.id.to_string(), c.source.as_ref());
            for other in &c.corroborations {
                check_source(c.id.to_string(), Some(other));
            }
        }
        for p in &self.projects {
            if p.name.trim().is_empty() {
                problems.push(format!("project {} has no name", p.id));
            }
            if let Some(e) = p.experience
                && !experiences.contains(&e.to_string())
            {
                problems.push(format!("project {} refers to missing experience {e}", p.id));
            }
        }
        for e in &self.education {
            if e.institution.trim().is_empty() {
                problems.push(format!("education entry {} has no institution", e.id));
            }
        }
        for s in &self.skills {
            if s.name.trim().is_empty() || s.key.trim().is_empty() {
                problems.push(format!("skill {} has no name", s.id));
            }
        }
        for c in &self.claims {
            if c.text.trim().is_empty() {
                problems.push(format!("claim {} has no text", c.id));
            }
            let subject_ok = match c.subject {
                Subject::Profile => true,
                Subject::Experience(id) => experiences.contains(&id.to_string()),
                Subject::Project(id) => projects.contains(&id.to_string()),
                Subject::Education(id) => education.contains(&id.to_string()),
            };
            if !subject_ok {
                problems.push(format!(
                    "claim {} is about {} {} which is not in the file",
                    c.id,
                    c.subject.kind_str(),
                    c.subject.id_string().unwrap_or_default()
                ));
            }
            if let Some(old) = c.supersedes
                && !claims.contains(&old.to_string())
            {
                problems.push(format!("claim {} supersedes missing claim {old}", c.id));
            }
        }
        for p in &self.preferences {
            if let Some(s) = p.statement
                && !statements.contains(&s.to_string())
            {
                problems.push(format!("preference {} cites missing statement {s}", p.id));
            }
            if let Some(next) = p.superseded_by
                && !preferences.contains(&next.to_string())
            {
                problems.push(format!("preference {} superseded by missing {next}", p.id));
            }
        }
        for s in &self.statements {
            if s.text.trim().is_empty() {
                problems.push(format!("statement {} has no text", s.id));
            }
        }
        for t in &self.taste {
            if t.text.trim().is_empty() || t.value.trim().is_empty() {
                problems.push(format!("taste statement {} has no text or value", t.id));
            }
            if let Some(next) = t.superseded_by
                && !taste.contains(&next.to_string())
            {
                problems.push(format!(
                    "taste statement {} superseded by missing {next}",
                    t.id
                ));
            }
        }
        if let Some(b) = &self.taste_brief {
            if b.text.trim().is_empty() {
                problems.push(format!("taste brief {} has no text", b.id));
            }
            if self.version < 3 {
                problems.push("a taste profile needs profile file version 3".to_owned());
            }
        }
        if !self.taste.is_empty() && self.version < 3 {
            problems.push("a taste profile needs profile file version 3".to_owned());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ExportError::Invalid(problems))
        }
    }

    /// The profile data, stored under `target` (the local profile).
    pub fn into_data(self, target: ProfileId) -> ProfileData {
        let mut profile = self.profile;
        profile.id = target;
        ProfileData {
            profile,
            documents: self.documents,
            experiences: self.experiences,
            projects: self.projects,
            education: self.education,
            skills: self.skills,
            claims: self.claims,
            preferences: self.preferences,
            statements: self.statements,
            taste: self.taste,
            taste_brief: self.taste_brief.map(|mut b| {
                b.id = TasteBrief::id_for(target);
                b
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::taste::edit::{add, apply_reading, correct, remove, set_brief};
    use crate::taste::reading::{ReadAssertion, TasteReading};
    use crate::taste::{
        InterpretationOutcome, Polarity, TasteConfidence, TasteDimension, TasteOrigin, TasteReview,
        TasteSource, compose,
    };

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 21, 9, minute, 0).unwrap()
    }

    fn tasteful() -> ProfileData {
        let mut data = ProfileData::new(ProfileId::local(), at(0));
        set_brief(
            &mut data,
            "Small teams, startups. No ML research.",
            None,
            at(0),
        );
        let read = |d, v: &str, p| ReadAssertion {
            dimension: d,
            value: v.into(),
            polarity: p,
            confidence: TasteConfidence::High,
            text: crate::taste::vocab::sentence(d, v, p),
            explanation: None,
            origin: TasteOrigin::Interpreted,
            sources: vec![TasteSource::Words {
                quote: "Small teams".into(),
                statement: None,
            }],
        };
        let reading = TasteReading {
            interpreter: "rules/1".into(),
            assertions: vec![
                read(TasteDimension::Team, "small_team", Polarity::Prefer),
                read(TasteDimension::Company, "startup", Polarity::Prefer),
                read(TasteDimension::WorkShape, "ml_research", Polarity::Avoid),
            ],
            ambiguities: vec!["Small team, or small company?".into()],
            ..TasteReading::default()
        };
        apply_reading(
            &mut data,
            &reading,
            InterpretationOutcome::Read,
            None,
            "digest".into(),
            at(1),
        );
        let p = compose(&data, &[]);
        let id = |key: &str| p.assertions.iter().find(|a| a.key() == key).unwrap().id;
        let startup = id("company:startup");
        let research = id("work_shape:ml_research");
        correct(
            &mut data,
            &p,
            startup,
            Some(Polarity::Open),
            None,
            None,
            at(2),
        )
        .unwrap();
        remove(&mut data, &p, research, at(3)).unwrap();
        add(&mut data, "Real users", &TasteReading::default(), at(4)).unwrap();
        data
    }

    #[test]
    fn profiles_without_taste_keep_their_version_and_shape() {
        let data = ProfileData::new(ProfileId::local(), at(0));
        let export = ProfileExport::from_data(&data, at(0), None);
        assert_eq!(export.version, 1);
        let json = export.to_json().unwrap();
        assert!(!json.contains("taste"), "{json}");
    }

    #[test]
    fn taste_round_trips_with_provenance_and_corrections() {
        let data = tasteful();
        let export = ProfileExport::from_data(&data, at(5), Some("test".into()));
        assert_eq!(export.version, 3);
        let back = ProfileExport::parse(&export.to_json().unwrap()).unwrap();
        assert_eq!(back, export);
        let restored = back.into_data(ProfileId::local());
        assert_eq!(restored.taste, data.taste);
        assert_eq!(restored.taste_brief, data.taste_brief);
        let p = compose(&restored, &[]);
        let startup = p
            .assertions
            .iter()
            .find(|a| a.key() == "company:startup")
            .unwrap();
        assert_eq!(startup.review, TasteReview::Corrected);
        assert_eq!(startup.polarity, Polarity::Open);
        assert!(startup.original.is_some());
        assert_eq!(p.removed.len(), 1, "the removal survives the round trip");
        assert_eq!(
            restored
                .taste_brief
                .as_ref()
                .and_then(|b| b.interpretation.as_ref())
                .map(|i| i.ambiguities.len()),
            Some(1)
        );
    }

    #[test]
    fn a_taste_profile_needs_version_three() {
        let mut export = ProfileExport::from_data(&tasteful(), at(5), None);
        export.version = 2;
        let json = export.to_json().unwrap();
        assert!(matches!(
            ProfileExport::parse(&json),
            Err(ExportError::Invalid(problems)) if problems.iter().any(|p| p.contains("version 3"))
        ));
        let future = json.replace("\"version\": 2", "\"version\": 4");
        assert!(matches!(
            ProfileExport::parse(&future),
            Err(ExportError::Version { .. })
        ));
    }

    #[test]
    fn taste_entities_round_trip() {
        let data = tasteful();
        let entities = crate::entities::decompose(&data).unwrap();
        assert!(
            entities
                .iter()
                .any(|e| e.key.kind == crate::entities::EntityKind::TasteBrief)
        );
        let back = crate::entities::compose(data.id(), 1, &entities, true).unwrap();
        let mut expected = data.clone();
        crate::entities::sort_like_storage(&mut expected);
        assert_eq!(back.taste, expected.taste);
        assert_eq!(back.taste_brief, expected.taste_brief);
    }
}
