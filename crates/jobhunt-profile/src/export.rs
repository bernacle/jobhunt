//! The portable profile format.
//!
//! `jobhunt profile export` writes a [`ProfileExport`]: one JSON object
//! with a `format` name and a `version`, then every record of the profile
//! with its provenance and verification state. `jobhunt profile import`
//! reads it back. Import is all or nothing: the file is parsed strictly
//! (unknown fields are errors), then [`ProfileExport::validate`] checks
//! that every reference points to a record in the file, and only then is
//! the stored profile replaced, in one transaction.
//!
//! Version history:
//!
//! * `1`: first version.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::aggregate::ProfileData;
use crate::evidence::{Claim, Subject};
use crate::ids::{DocumentId, ProfileId};
use crate::model::{Education, Experience, Profile, Project, Skill, SourceDocument};
use crate::preferences::{Preference, PreferenceStatement};

pub const EXPORT_FORMAT: &str = "jobhunt.profile";
pub const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileExport {
    pub format: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    /// The program that wrote the file (`jobhunt 0.1.0`).
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
}

/// Why a profile file cannot be imported.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("not a JSON document")]
    Json(#[source] serde_json::Error),
    #[error("not a JobHunt profile file (format is {found:?}, expected {EXPORT_FORMAT:?})")]
    Format { found: String },
    #[error(
        "profile file version {found} is not supported (this JobHunt reads version {EXPORT_VERSION}); update JobHunt"
    )]
    Version { found: String },
    #[error("invalid profile file: {0}")]
    Schema(#[source] serde_json::Error),
    #[error("invalid profile file:\n  - {}", .0.join("\n  - "))]
    Invalid(Vec<String>),
}

impl ProfileExport {
    pub fn from_data(data: &ProfileData, now: DateTime<Utc>, generator: Option<String>) -> Self {
        Self {
            format: EXPORT_FORMAT.to_owned(),
            version: EXPORT_VERSION,
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
            Some(v) if v == u64::from(EXPORT_VERSION) => {}
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
        for e in &self.experiences {
            check_source(e.id.to_string(), e.meta.source.as_ref());
        }
        for p in &self.projects {
            check_source(p.id.to_string(), p.meta.source.as_ref());
        }
        for e in &self.education {
            check_source(e.id.to_string(), e.meta.source.as_ref());
        }
        for s in &self.skills {
            check_source(s.id.to_string(), s.meta.source.as_ref());
        }
        for c in &self.claims {
            check_source(c.id.to_string(), c.source.as_ref());
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
        }
    }
}
