//! Use cases over a stored profile.
//!
//! [`ProfileService`] is what front-ends (the CLI today, MCP and web later)
//! call. Every mutation loads the profile, applies a pure change from this
//! crate, bumps the revision and saves it through the
//! [`ProfileRepository`] with a history entry, so a front-end never
//! touches storage or re-implements a rule.

use chrono::{DateTime, Utc};
use jobhunt_core::text::{clean_block, clean_line};

use crate::aggregate::ProfileData;
use crate::date::PartialDate;
use crate::evidence::{Claim, ClaimKind, Confidence, Provenance, Subject};
use crate::export::{ExportError, ProfileExport};
use crate::ids::{
    ClaimId, EducationId, ExperienceId, PreferenceId, ProfileId, ProjectId, RecordId, SkillId,
    StatementId, resolve_prefix,
};
use crate::import::{ImportReport, merge_resume};
use crate::infer::{known_technology, topic_key};
use crate::model::{
    Education, EmploymentKind, Experience, Origin, Project, RecordMeta, Skill, SourceDocument,
    Verification,
};
use crate::preferences::{
    Certainty, Preference, PreferenceOrigin, PreferenceStatement, PreferenceValue, Stance,
    StatementReading,
};
use crate::repository::{ProfileEvent, ProfileEventKind, ProfileRepository, StorageError};
use crate::resume::ParsedResume;
use crate::statement::StatementParser;

/// A use case failed.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("no {what} matches {input:?}")]
    NotFound { what: &'static str, input: String },
    #[error("{input:?} matches more than one {what}; type more of the id")]
    Ambiguous { what: &'static str, input: String },
    #[error("{0}")]
    Invalid(String),
    #[error("could not import the profile file")]
    Export(#[from] ExportError),
    #[error("there is no profile yet; import a resume with `jobhunt init <resume>` first")]
    NoProfile,
}

/// Changes to the profile basics. `Some("")` clears a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasicsEdit {
    pub name: Option<String>,
    pub headline: Option<String>,
    pub location: Option<String>,
    pub summary: Option<String>,
}

/// Values for an experience. On edit, `None` leaves a field alone and
/// `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperienceEdit {
    pub company: Option<Option<String>>,
    pub title: Option<Option<String>>,
    pub employment: Option<Option<EmploymentKind>>,
    pub start: Option<Option<PartialDate>>,
    pub end: Option<Option<PartialDate>>,
    pub current: Option<bool>,
    pub location: Option<Option<String>>,
    pub summary: Option<Option<String>>,
    /// Technologies to record as used in it (user-entered claims).
    pub technologies: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectEdit {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub role: Option<Option<String>>,
    pub url: Option<Option<String>>,
    pub start: Option<Option<PartialDate>>,
    pub end: Option<Option<PartialDate>>,
    pub current: Option<bool>,
    /// An experience id (or prefix) the project belongs to; `Some(None)`
    /// detaches it.
    pub experience: Option<Option<String>>,
    pub technologies: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EducationEdit {
    pub institution: Option<String>,
    pub degree: Option<Option<String>>,
    pub field: Option<Option<String>>,
    pub start: Option<Option<PartialDate>>,
    pub end: Option<Option<PartialDate>>,
    pub current: Option<bool>,
}

/// What removing a record did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// A record the user created: deleted.
    Deleted(String),
    /// A resume record or claim: kept but rejected, so a re-import does not
    /// bring it back.
    Rejected(String),
}

/// Result of adding a preference statement.
#[derive(Debug, Clone, PartialEq)]
pub struct StatementOutcome {
    pub statement: PreferenceStatement,
    pub preferences: Vec<Preference>,
    /// Older preferences the new ones replaced.
    pub replaced: Vec<Preference>,
}

pub struct ProfileService<'a, R: ProfileRepository + ?Sized> {
    repo: &'a R,
    id: ProfileId,
}

fn clean_opt(value: Option<String>) -> Option<String> {
    value.as_deref().and_then(clean_line)
}

impl<'a, R: ProfileRepository + ?Sized> ProfileService<'a, R> {
    /// The local profile.
    pub fn new(repo: &'a R) -> Self {
        Self::for_profile(repo, ProfileId::local())
    }

    pub fn for_profile(repo: &'a R, id: ProfileId) -> Self {
        Self { repo, id }
    }

    pub fn profile_id(&self) -> ProfileId {
        self.id
    }

    pub async fn load(&self) -> Result<Option<ProfileData>, ProfileError> {
        Ok(self.repo.load_profile(self.id).await?)
    }

    /// The stored profile, or an error telling the user to create one.
    pub async fn require(&self) -> Result<ProfileData, ProfileError> {
        self.load().await?.ok_or(ProfileError::NoProfile)
    }

    pub async fn load_or_new(&self, now: DateTime<Utc>) -> Result<ProfileData, ProfileError> {
        Ok(self
            .load()
            .await?
            .unwrap_or_else(|| ProfileData::new(self.id, now)))
    }

    pub async fn history(&self, limit: usize) -> Result<Vec<ProfileEvent>, ProfileError> {
        Ok(self.repo.profile_events(self.id, limit).await?)
    }

    async fn commit(
        &self,
        data: &mut ProfileData,
        events: Vec<ProfileEvent>,
        now: DateTime<Utc>,
    ) -> Result<(), ProfileError> {
        let expected = data.profile.revision;
        data.profile.revision += 1;
        data.profile.updated_at = now;
        self.repo.save_profile(data, expected, &events).await?;
        Ok(())
    }

    /// Imports (or re-imports) a resume. See [`crate::import`] for the rules.
    pub async fn import_resume(
        &self,
        document: SourceDocument,
        parsed: &ParsedResume,
        now: DateTime<Utc>,
    ) -> Result<(ImportReport, ProfileData), ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let name = document
            .file_name
            .clone()
            .unwrap_or_else(|| "resume".to_owned());
        let report = merge_resume(&mut data, document, parsed, now);
        let detail = format!(
            "{name}: {} experiences, {} claims added, {} stale",
            report.experiences.added + report.experiences.updated + report.experiences.unchanged,
            report.claims.added,
            report.claims.stale
        );
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::ResumeImported,
            report.document.map(|d| d.to_string()),
            detail,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok((report, data))
    }

    /// Resolves user input (a full id or a unique prefix) to a record.
    pub fn resolve(&self, data: &ProfileData, input: &str) -> Result<RecordId, ProfileError> {
        let input = input.trim();
        let err_not_found = |what| ProfileError::NotFound {
            what,
            input: input.to_owned(),
        };
        let ambiguous = |what| ProfileError::Ambiguous {
            what,
            input: input.to_owned(),
        };
        macro_rules! pick {
            ($prefix:expr, $ids:expr, $variant:ident, $what:literal) => {
                if input.starts_with($prefix) {
                    let ids: Vec<_> = $ids;
                    if ids
                        .iter()
                        .filter(|id| id.to_string().starts_with(input))
                        .count()
                        > 1
                        && !ids.iter().any(|id| id.to_string() == input)
                    {
                        return Err(ambiguous($what));
                    }
                    return resolve_prefix(input, &ids)
                        .map(RecordId::$variant)
                        .ok_or_else(|| err_not_found($what));
                }
            };
        }
        pick!(
            ExperienceId::PREFIX,
            data.experiences.iter().map(|e| e.id).collect(),
            Experience,
            "experience"
        );
        pick!(
            ProjectId::PREFIX,
            data.projects.iter().map(|p| p.id).collect(),
            Project,
            "project"
        );
        pick!(
            EducationId::PREFIX,
            data.education.iter().map(|e| e.id).collect(),
            Education,
            "education entry"
        );
        pick!(
            SkillId::PREFIX,
            data.skills.iter().map(|s| s.id).collect(),
            Skill,
            "skill"
        );
        pick!(
            ClaimId::PREFIX,
            data.claims.iter().map(|c| c.id).collect(),
            Claim,
            "claim"
        );
        pick!(
            PreferenceId::PREFIX,
            data.preferences.iter().map(|p| p.id).collect(),
            Preference,
            "preference"
        );
        pick!(
            StatementId::PREFIX,
            data.statements.iter().map(|s| s.id).collect(),
            Statement,
            "statement"
        );
        Err(ProfileError::Invalid(format!(
            "{input:?} is not a profile id (exp_…, proj_…, edu_…, skill_…, clm_…, pref_…, stmt_…)"
        )))
    }

    fn resolve_claim(&self, data: &ProfileData, input: &str) -> Result<ClaimId, ProfileError> {
        match self.resolve(data, input)? {
            RecordId::Claim(id) => Ok(id),
            _ => Err(ProfileError::Invalid(format!(
                "{input:?} is not a claim id (clm_…)"
            ))),
        }
    }

    fn resolve_subject(&self, data: &ProfileData, input: &str) -> Result<Subject, ProfileError> {
        match self.resolve(data, input)? {
            RecordId::Experience(id) => Ok(Subject::Experience(id)),
            RecordId::Project(id) => Ok(Subject::Project(id)),
            RecordId::Education(id) => Ok(Subject::Education(id)),
            _ => Err(ProfileError::Invalid(format!(
                "{input:?} is not an experience, project or education id"
            ))),
        }
    }

    /// Confirms, rejects or resets (to unverified) claims.
    pub async fn decide_claims(
        &self,
        inputs: &[String],
        decision: Verification,
        note: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Vec<Claim>, ProfileError> {
        let mut data = self.require().await?;
        let mut ids = Vec::new();
        for input in inputs {
            let id = self.resolve_claim(&data, input)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        self.decide(&mut data, &ids, decision, note, now).await
    }

    async fn decide(
        &self,
        data: &mut ProfileData,
        ids: &[ClaimId],
        decision: Verification,
        note: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Vec<Claim>, ProfileError> {
        let kind = match decision {
            Verification::Confirmed => ProfileEventKind::ClaimConfirmed,
            Verification::Rejected => ProfileEventKind::ClaimRejected,
            Verification::Unverified => ProfileEventKind::ClaimReset,
        };
        let mut events = Vec::new();
        let mut changed = Vec::new();
        for id in ids {
            let Some(claim) = data.claims.iter_mut().find(|c| c.id == *id) else {
                return Err(ProfileError::NotFound {
                    what: "claim",
                    input: id.to_string(),
                });
            };
            claim.verification = decision;
            claim.verified_at = (decision != Verification::Unverified).then_some(now);
            if note.is_some() {
                claim.note = note.clone();
            }
            claim.updated_at = now;
            events.push(ProfileEvent::new(
                now,
                kind,
                Some(id.to_string()),
                claim.text.clone(),
            ));
            changed.push(claim.clone());
        }
        if !events.is_empty() {
            self.commit(data, events, now).await?;
        }
        Ok(changed)
    }

    /// Adds a claim in the user's words, about a record or the profile.
    pub async fn add_claim(
        &self,
        text: &str,
        kind: ClaimKind,
        about: Option<&str>,
        topic: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Claim, ProfileError> {
        let text = clean_line(text)
            .ok_or_else(|| ProfileError::Invalid("a claim needs some text".into()))?;
        let mut data = self.require().await?;
        let subject = match about {
            Some(input) => self.resolve_subject(&data, input)?,
            None => Subject::Profile,
        };
        let topic = topic.map(topic_key).filter(|t| !t.is_empty());
        let claim = user_claim(&data, subject, kind, text, topic, now);
        data.claims.push(claim.clone());
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::ClaimAdded,
            Some(claim.id.to_string()),
            claim.text.clone(),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(claim)
    }

    /// Rewrites a claim's text. The user's wording is kept across
    /// re-imports, and a rewritten claim counts as confirmed.
    pub async fn edit_claim(
        &self,
        input: &str,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Claim, ProfileError> {
        let text = clean_line(text)
            .ok_or_else(|| ProfileError::Invalid("a claim needs some text".into()))?;
        let mut data = self.require().await?;
        let id = self.resolve_claim(&data, input)?;
        let Some(claim) = data.claims.iter_mut().find(|c| c.id == id) else {
            return Err(ProfileError::NotFound {
                what: "claim",
                input: input.to_owned(),
            });
        };
        if claim.note.is_none() && claim.provenance != Provenance::UserEntered {
            claim.note = Some(format!("Originally: “{}”", claim.text));
        }
        claim.text = text;
        claim.edited = true;
        claim.verification = Verification::Confirmed;
        claim.verified_at = Some(now);
        claim.updated_at = now;
        let claim = claim.clone();
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::ClaimEdited,
            Some(id.to_string()),
            claim.text.clone(),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(claim)
    }

    /// Stores a statement verbatim and the preferences read from it. A new
    /// preference replaces an active one with the same key.
    pub async fn add_statement(
        &self,
        text: &str,
        parser: &dyn StatementParser,
        now: DateTime<Utc>,
    ) -> Result<StatementOutcome, ProfileError> {
        let text = clean_block(text)
            .ok_or_else(|| ProfileError::Invalid("the statement is empty".into()))?;
        let mut data = self.load_or_new(now).await?;
        let readout = parser.read(&text);
        let id = StatementId::derive(&[&self.id.to_string(), &text, &now.to_rfc3339()]);
        let any_uncertain = readout
            .preferences
            .iter()
            .any(|p| p.certainty == Certainty::Uncertain);
        let reading = if readout.preferences.is_empty() {
            StatementReading::NotUnderstood
        } else if readout.unparsed.is_empty() && !any_uncertain {
            StatementReading::Understood
        } else {
            StatementReading::Partial
        };
        let statement = PreferenceStatement {
            id,
            text: text.clone(),
            parser: parser.name().to_owned(),
            reading,
            unparsed: readout.unparsed.clone(),
            created_at: now,
        };
        data.statements.push(statement.clone());
        let mut preferences = Vec::new();
        let mut replaced = Vec::new();
        for read in readout.preferences {
            let pref = Preference {
                id: PreferenceId::derive(&[&id.to_string(), &read.value.key()]),
                value: read.value,
                stance: read.stance,
                origin: PreferenceOrigin::Statement,
                statement: Some(id),
                snippet: Some(read.snippet),
                certainty: read.certainty,
                note: read.note,
                active: true,
                superseded_by: None,
                created_at: now,
                updated_at: now,
            };
            replaced.extend(supersede(&mut data, &pref, now));
            data.preferences.push(pref.clone());
            preferences.push(pref);
        }
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::StatementAdded,
            Some(id.to_string()),
            text,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(StatementOutcome {
            statement,
            preferences,
            replaced,
        })
    }

    /// Sets one structured preference.
    pub async fn set_preference(
        &self,
        value: PreferenceValue,
        stance: Stance,
        now: DateTime<Utc>,
    ) -> Result<(Preference, Vec<Preference>), ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let pref = Preference {
            id: PreferenceId::derive(&[
                &self.id.to_string(),
                "user",
                &value.key(),
                &now.to_rfc3339(),
            ]),
            value,
            stance,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: now,
            updated_at: now,
        };
        let replaced = supersede(&mut data, &pref, now);
        data.preferences.push(pref.clone());
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::PreferenceSet,
            Some(pref.id.to_string()),
            format!("{} {}", pref.stance.as_str(), pref.value),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok((pref, replaced))
    }

    pub async fn edit_basics(
        &self,
        edit: BasicsEdit,
        now: DateTime<Utc>,
    ) -> Result<ProfileData, ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let profile = &mut data.profile;
        let mut fields = Vec::new();
        for (field, value, target) in [
            ("name", edit.name, &mut profile.name),
            ("headline", edit.headline, &mut profile.headline),
            ("location", edit.location, &mut profile.location),
        ] {
            if let Some(value) = value {
                *target = clean_line(&value);
                fields.push(field);
            }
        }
        if let Some(summary) = edit.summary {
            profile.summary = clean_block(&summary);
            fields.push("summary");
        }
        if fields.is_empty() {
            return Err(ProfileError::Invalid("nothing to change".into()));
        }
        for field in &fields {
            if !profile.is_edited(field) {
                profile.edited_fields.push((*field).to_owned());
            }
        }
        profile.edited_fields.sort();
        let event = ProfileEvent::new(now, ProfileEventKind::BasicsEdited, None, fields.join(", "));
        self.commit(&mut data, vec![event], now).await?;
        Ok(data)
    }

    /// Adds an experience the resume does not have.
    pub async fn add_experience(
        &self,
        edit: ExperienceEdit,
        now: DateTime<Utc>,
    ) -> Result<Experience, ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let company = edit.company.clone().flatten().and_then(|c| clean_line(&c));
        let title = edit.title.clone().flatten().and_then(|t| clean_line(&t));
        if company.is_none() && title.is_none() {
            return Err(ProfileError::Invalid(
                "an experience needs at least a company or a title".into(),
            ));
        }
        let id = ExperienceId::derive(&[
            &self.id.to_string(),
            "user",
            company.as_deref().unwrap_or(""),
            title.as_deref().unwrap_or(""),
            &now.to_rfc3339(),
        ]);
        let position = data
            .experiences
            .iter()
            .map(|e| e.position + 1)
            .max()
            .unwrap_or(0);
        let mut experience = Experience {
            id,
            company: None,
            title: None,
            employment: None,
            start: None,
            end: None,
            current: false,
            location: None,
            summary: None,
            position,
            meta: RecordMeta::user(now),
        };
        apply_experience_edit(&mut experience, &edit, false);
        validate_period(experience.start, experience.end, experience.current)?;
        data.experiences.push(experience.clone());
        let subject = Subject::Experience(id);
        let label = experience.label();
        let employment = user_claim(
            &data,
            subject,
            ClaimKind::Employment,
            format!(
                "{label}{}",
                experience
                    .period()
                    .display()
                    .map(|p| format!(" ({p})"))
                    .unwrap_or_default()
            ),
            None,
            now,
        );
        data.claims.push(employment);
        add_technology_claims(&mut data, subject, &label, &edit.technologies, now);
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordAdded,
            Some(id.to_string()),
            label,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(experience)
    }

    /// Corrects an experience. Edited fields are never overwritten by a
    /// resume re-import, and the experience's employment claim is restated
    /// and confirmed.
    pub async fn edit_experience(
        &self,
        input: &str,
        edit: ExperienceEdit,
        now: DateTime<Utc>,
    ) -> Result<Experience, ProfileError> {
        let mut data = self.require().await?;
        let RecordId::Experience(id) = self.resolve(&data, input)? else {
            return Err(ProfileError::Invalid(format!(
                "{input:?} is not an experience id"
            )));
        };
        let Some(experience) = data.experiences.iter_mut().find(|e| e.id == id) else {
            return Err(ProfileError::NotFound {
                what: "experience",
                input: input.to_owned(),
            });
        };
        let fields = apply_experience_edit(experience, &edit, true);
        if fields.is_empty() && edit.technologies.is_empty() {
            return Err(ProfileError::Invalid("nothing to change".into()));
        }
        validate_period(experience.start, experience.end, experience.current)?;
        experience.meta.updated_at = now;
        experience.meta.notes.clear();
        let experience = experience.clone();
        let label = experience.label();
        let statement = format!(
            "{label}{}",
            experience
                .period()
                .display()
                .map(|p| format!(" ({p})"))
                .unwrap_or_default()
        );
        let identity_changed = fields
            .iter()
            .any(|f| matches!(*f, "company" | "title" | "start" | "end" | "current"));
        if identity_changed {
            let subject = Subject::Experience(id);
            let mut found = false;
            for claim in data
                .claims
                .iter_mut()
                .filter(|c| c.subject == subject && c.kind == ClaimKind::Employment)
            {
                found = true;
                claim.text = statement.clone();
                claim.edited = true;
                claim.verification = Verification::Confirmed;
                claim.verified_at = Some(now);
                claim.updated_at = now;
                claim.note = Some("Corrected by you".into());
            }
            if !found {
                let claim = user_claim(&data, subject, ClaimKind::Employment, statement, None, now);
                data.claims.push(claim);
            }
        }
        add_technology_claims(
            &mut data,
            Subject::Experience(id),
            &label,
            &edit.technologies,
            now,
        );
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordEdited,
            Some(id.to_string()),
            format!("{label}: {}", fields.join(", ")),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(experience)
    }

    pub async fn add_project(
        &self,
        edit: ProjectEdit,
        now: DateTime<Utc>,
    ) -> Result<Project, ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let name = edit
            .name
            .as_deref()
            .and_then(clean_line)
            .ok_or_else(|| ProfileError::Invalid("a project needs a name".into()))?;
        let id = ProjectId::derive(&[&self.id.to_string(), "user", &name, &now.to_rfc3339()]);
        let position = data
            .projects
            .iter()
            .map(|p| p.position + 1)
            .max()
            .unwrap_or(0);
        let mut project = Project {
            id,
            name: name.clone(),
            description: None,
            role: None,
            url: None,
            start: None,
            end: None,
            current: false,
            experience: None,
            position,
            meta: RecordMeta::user(now),
        };
        let experience = self.project_experience(&data, &edit)?;
        apply_project_edit(&mut project, &edit, experience, false);
        validate_period(project.start, project.end, project.current)?;
        data.projects.push(project.clone());
        let subject = Subject::Project(id);
        let claim = user_claim(
            &data,
            subject,
            ClaimKind::Project,
            format!("Built {name}"),
            None,
            now,
        );
        data.claims.push(claim);
        add_technology_claims(&mut data, subject, &name, &edit.technologies, now);
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordAdded,
            Some(id.to_string()),
            name,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(project)
    }

    fn project_experience(
        &self,
        data: &ProfileData,
        edit: &ProjectEdit,
    ) -> Result<Option<Option<ExperienceId>>, ProfileError> {
        match &edit.experience {
            None => Ok(None),
            Some(None) => Ok(Some(None)),
            Some(Some(input)) => match self.resolve(data, input)? {
                RecordId::Experience(id) => Ok(Some(Some(id))),
                _ => Err(ProfileError::Invalid(format!(
                    "{input:?} is not an experience id"
                ))),
            },
        }
    }

    pub async fn edit_project(
        &self,
        input: &str,
        edit: ProjectEdit,
        now: DateTime<Utc>,
    ) -> Result<Project, ProfileError> {
        let mut data = self.require().await?;
        let RecordId::Project(id) = self.resolve(&data, input)? else {
            return Err(ProfileError::Invalid(format!(
                "{input:?} is not a project id"
            )));
        };
        let experience = self.project_experience(&data, &edit)?;
        let Some(project) = data.projects.iter_mut().find(|p| p.id == id) else {
            return Err(ProfileError::NotFound {
                what: "project",
                input: input.to_owned(),
            });
        };
        let fields = apply_project_edit(project, &edit, experience, true);
        if fields.is_empty() && edit.technologies.is_empty() {
            return Err(ProfileError::Invalid("nothing to change".into()));
        }
        validate_period(project.start, project.end, project.current)?;
        project.meta.updated_at = now;
        project.meta.notes.clear();
        let project = project.clone();
        add_technology_claims(
            &mut data,
            Subject::Project(id),
            &project.name,
            &edit.technologies,
            now,
        );
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordEdited,
            Some(id.to_string()),
            format!("{}: {}", project.name, fields.join(", ")),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(project)
    }

    pub async fn add_education(
        &self,
        edit: EducationEdit,
        now: DateTime<Utc>,
    ) -> Result<Education, ProfileError> {
        let mut data = self.load_or_new(now).await?;
        let institution = edit
            .institution
            .as_deref()
            .and_then(clean_line)
            .ok_or_else(|| {
                ProfileError::Invalid("an education entry needs an institution".into())
            })?;
        let id = EducationId::derive(&[
            &self.id.to_string(),
            "user",
            &institution,
            &now.to_rfc3339(),
        ]);
        let position = data
            .education
            .iter()
            .map(|e| e.position + 1)
            .max()
            .unwrap_or(0);
        let mut education = Education {
            id,
            institution: institution.clone(),
            degree: None,
            field: None,
            start: None,
            end: None,
            current: false,
            position,
            meta: RecordMeta::user(now),
        };
        apply_education_edit(&mut education, &edit, false);
        validate_period(education.start, education.end, education.current)?;
        data.education.push(education.clone());
        let what = match (&education.degree, &education.field) {
            (Some(d), Some(f)) => format!("{d} in {f}, {institution}"),
            (Some(d), None) => format!("{d}, {institution}"),
            (None, Some(f)) => format!("Studied {f} at {institution}"),
            (None, None) => format!("Studied at {institution}"),
        };
        let claim = user_claim(
            &data,
            Subject::Education(id),
            ClaimKind::Education,
            what,
            None,
            now,
        );
        data.claims.push(claim);
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordAdded,
            Some(id.to_string()),
            institution,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(education)
    }

    pub async fn edit_education(
        &self,
        input: &str,
        edit: EducationEdit,
        now: DateTime<Utc>,
    ) -> Result<Education, ProfileError> {
        let mut data = self.require().await?;
        let RecordId::Education(id) = self.resolve(&data, input)? else {
            return Err(ProfileError::Invalid(format!(
                "{input:?} is not an education id"
            )));
        };
        let Some(education) = data.education.iter_mut().find(|e| e.id == id) else {
            return Err(ProfileError::NotFound {
                what: "education entry",
                input: input.to_owned(),
            });
        };
        let fields = apply_education_edit(education, &edit, true);
        if fields.is_empty() {
            return Err(ProfileError::Invalid("nothing to change".into()));
        }
        validate_period(education.start, education.end, education.current)?;
        education.meta.updated_at = now;
        education.meta.notes.clear();
        let education = education.clone();
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordEdited,
            Some(id.to_string()),
            format!("{}: {}", education.institution, fields.join(", ")),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(education)
    }

    /// Adds a skill (or confirms an existing one with the same name).
    pub async fn add_skill(
        &self,
        name: &str,
        category: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Skill, ProfileError> {
        let name =
            clean_line(name).ok_or_else(|| ProfileError::Invalid("a skill needs a name".into()))?;
        let name = known_technology(&name).map_or(name, |t| t.name.to_owned());
        let key = topic_key(&name);
        let mut data = self.load_or_new(now).await?;
        let category = category.and_then(clean_line);
        let skill = match data.skills.iter_mut().find(|s| s.key == key) {
            Some(existing) => {
                existing.meta.verification = Verification::Confirmed;
                existing.meta.stale_since = None;
                if let Some(category) = category {
                    existing.category = Some(category);
                    existing.meta.mark_edited("category");
                }
                existing.meta.updated_at = now;
                existing.clone()
            }
            None => {
                let skill = Skill {
                    id: SkillId::derive(&[&self.id.to_string(), "user", &key]),
                    name: name.clone(),
                    key: key.clone(),
                    category,
                    meta: RecordMeta::user(now),
                };
                data.skills.push(skill.clone());
                skill
            }
        };
        if !data.claims.iter().any(|c| {
            c.kind == ClaimKind::Skill
                && c.provenance == Provenance::UserEntered
                && c.topic.as_deref() == Some(key.as_str())
        }) {
            let claim = user_claim(
                &data,
                Subject::Profile,
                ClaimKind::Skill,
                format!("Has {name} as a skill"),
                Some(key),
                now,
            );
            data.claims.push(claim);
        }
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::RecordAdded,
            Some(skill.id.to_string()),
            name,
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(skill)
    }

    /// Removes a record, claim, preference or statement. Records and claims
    /// the user created are deleted; resume records and claims are
    /// rejected instead (kept, hidden, and never brought back as trusted).
    pub async fn remove(&self, input: &str, now: DateTime<Utc>) -> Result<Removal, ProfileError> {
        let mut data = self.require().await?;
        let record = self.resolve(&data, input)?;
        let id = record.to_string();
        let removal = match record {
            RecordId::Experience(x) => {
                let origin = data.experience(x).map(|e| e.meta.origin);
                reject_or_delete(
                    &mut data,
                    origin,
                    Subject::Experience(x),
                    |d| d.experiences.retain(|e| e.id != x),
                    |d| {
                        d.experiences
                            .iter_mut()
                            .filter(|e| e.id == x)
                            .for_each(|e| {
                                e.meta.verification = Verification::Rejected;
                                e.meta.updated_at = now;
                            })
                    },
                )
            }
            RecordId::Project(x) => {
                let origin = data
                    .projects
                    .iter()
                    .find(|p| p.id == x)
                    .map(|p| p.meta.origin);
                reject_or_delete(
                    &mut data,
                    origin,
                    Subject::Project(x),
                    |d| d.projects.retain(|p| p.id != x),
                    |d| {
                        d.projects.iter_mut().filter(|p| p.id == x).for_each(|p| {
                            p.meta.verification = Verification::Rejected;
                            p.meta.updated_at = now;
                        })
                    },
                )
            }
            RecordId::Education(x) => {
                let origin = data
                    .education
                    .iter()
                    .find(|e| e.id == x)
                    .map(|e| e.meta.origin);
                reject_or_delete(
                    &mut data,
                    origin,
                    Subject::Education(x),
                    |d| d.education.retain(|e| e.id != x),
                    |d| {
                        d.education.iter_mut().filter(|e| e.id == x).for_each(|e| {
                            e.meta.verification = Verification::Rejected;
                            e.meta.updated_at = now;
                        })
                    },
                )
            }
            RecordId::Skill(x) => {
                let skill = data.skills.iter().find(|s| s.id == x).cloned();
                match skill {
                    Some(s) if s.meta.origin == Origin::User => {
                        data.skills.retain(|k| k.id != x);
                        data.claims.retain(|c| {
                            !(c.provenance == Provenance::UserEntered
                                && c.kind == ClaimKind::Skill
                                && c.topic.as_deref() == Some(s.key.as_str()))
                        });
                        Removal::Deleted(id.clone())
                    }
                    Some(s) => {
                        for k in data.skills.iter_mut().filter(|k| k.id == x) {
                            k.meta.verification = Verification::Rejected;
                            k.meta.updated_at = now;
                        }
                        // Its evidence goes with it.
                        for c in data.claims.iter_mut().filter(|c| {
                            matches!(c.kind, ClaimKind::Skill | ClaimKind::Technology)
                                && c.topic.as_deref() == Some(s.key.as_str())
                        }) {
                            c.verification = Verification::Rejected;
                            c.verified_at = Some(now);
                            c.updated_at = now;
                        }
                        Removal::Rejected(id.clone())
                    }
                    None => Removal::Deleted(id.clone()),
                }
            }
            RecordId::Claim(x) => {
                let provenance = data.claim(x).map(|c| c.provenance);
                if provenance == Some(Provenance::UserEntered) {
                    data.claims.retain(|c| c.id != x);
                    Removal::Deleted(id.clone())
                } else {
                    for c in data.claims.iter_mut().filter(|c| c.id == x) {
                        c.verification = Verification::Rejected;
                        c.verified_at = Some(now);
                        c.updated_at = now;
                    }
                    Removal::Rejected(id.clone())
                }
            }
            RecordId::Preference(x) => {
                for p in data.preferences.iter_mut().filter(|p| p.id == x) {
                    p.active = false;
                    p.updated_at = now;
                }
                Removal::Deleted(id.clone())
            }
            RecordId::Statement(x) => {
                data.statements.retain(|s| s.id != x);
                data.preferences.retain(|p| p.statement != Some(x));
                // Preferences it had replaced become current again.
                reactivate_latest(&mut data, now);
                Removal::Deleted(id.clone())
            }
        };
        let kind = match (&removal, record) {
            (_, RecordId::Preference(_)) => ProfileEventKind::PreferenceRemoved,
            (_, RecordId::Statement(_)) => ProfileEventKind::StatementRemoved,
            (Removal::Rejected(_), RecordId::Claim(_)) => ProfileEventKind::ClaimRejected,
            (Removal::Rejected(_), _) => ProfileEventKind::RecordRejected,
            (Removal::Deleted(_), _) => ProfileEventKind::RecordRemoved,
        };
        let event = ProfileEvent::new(now, kind, Some(id), "removed by you");
        self.commit(&mut data, vec![event], now).await?;
        Ok(removal)
    }

    pub async fn export(
        &self,
        now: DateTime<Utc>,
        generator: Option<String>,
    ) -> Result<ProfileExport, ProfileError> {
        let data = self.require().await?;
        Ok(ProfileExport::from_data(&data, now, generator))
    }

    /// Replaces the stored profile with a validated export, atomically.
    pub async fn import_export(
        &self,
        export: ProfileExport,
        now: DateTime<Utc>,
    ) -> Result<ProfileData, ProfileError> {
        export.validate()?;
        let current = self.load().await?;
        let mut data = export.into_data(self.id);
        data.profile.revision = current.as_ref().map_or(0, |d| d.profile.revision);
        let event = ProfileEvent::new(
            now,
            ProfileEventKind::ProfileImported,
            None,
            format!(
                "{} experiences, {} claims, {} preferences",
                data.experiences.len(),
                data.claims.len(),
                data.preferences.len()
            ),
        );
        self.commit(&mut data, vec![event], now).await?;
        Ok(data)
    }
}

fn reject_or_delete(
    data: &mut ProfileData,
    origin: Option<Origin>,
    subject: Subject,
    delete: impl FnOnce(&mut ProfileData),
    reject: impl FnOnce(&mut ProfileData),
) -> Removal {
    let id = subject.id_string().unwrap_or_default();
    match origin {
        Some(Origin::User) => {
            delete(data);
            data.claims.retain(|c| c.subject != subject);
            if let Subject::Experience(x) = subject {
                for p in data.projects.iter_mut().filter(|p| p.experience == Some(x)) {
                    p.experience = None;
                }
            }
            Removal::Deleted(id)
        }
        _ => {
            reject(data);
            Removal::Rejected(id)
        }
    }
}

/// After statements are removed, the newest remaining preference of each
/// key becomes active again if nothing active has that key.
fn reactivate_latest(data: &mut ProfileData, now: DateTime<Utc>) {
    let mut keys: Vec<String> = data.preferences.iter().map(|p| p.value.key()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        if data
            .preferences
            .iter()
            .any(|p| p.active && p.value.key() == key)
        {
            continue;
        }
        let latest = data
            .preferences
            .iter_mut()
            .filter(|p| p.value.key() == key && p.superseded_by.is_some())
            .max_by_key(|p| p.created_at);
        if let Some(p) = latest {
            p.active = true;
            p.superseded_by = None;
            p.updated_at = now;
        }
    }
    let ids: Vec<PreferenceId> = data.preferences.iter().map(|p| p.id).collect();
    for p in &mut data.preferences {
        if p.superseded_by.is_some_and(|s| !ids.contains(&s)) {
            p.superseded_by = None;
        }
    }
}

/// Deactivates active preferences with the same key as `new`.
fn supersede(data: &mut ProfileData, new: &Preference, now: DateTime<Utc>) -> Vec<Preference> {
    let key = new.value.key();
    let mut replaced = Vec::new();
    for p in data
        .preferences
        .iter_mut()
        .filter(|p| p.active && p.value.key() == key)
    {
        p.active = false;
        p.superseded_by = Some(new.id);
        p.updated_at = now;
        replaced.push(p.clone());
    }
    replaced
}

fn user_claim(
    data: &ProfileData,
    subject: Subject,
    kind: ClaimKind,
    text: String,
    topic: Option<String>,
    now: DateTime<Utc>,
) -> Claim {
    let position = u32::try_from(data.claims.iter().filter(|c| c.subject == subject).count())
        .unwrap_or(u32::MAX);
    Claim {
        id: ClaimId::derive(&[
            &data.id().to_string(),
            "user",
            &subject.id_string().unwrap_or_default(),
            kind.as_str(),
            &text,
            &now.to_rfc3339(),
        ]),
        kind,
        text,
        topic,
        subject,
        provenance: Provenance::UserEntered,
        confidence: Confidence::High,
        verification: Verification::Unverified,
        source: None,
        basis: None,
        import_key: None,
        supersedes: None,
        position,
        stale_since: None,
        verified_at: None,
        note: None,
        edited: false,
        created_at: now,
        updated_at: now,
    }
}

fn add_technology_claims(
    data: &mut ProfileData,
    subject: Subject,
    label: &str,
    technologies: &[String],
    now: DateTime<Utc>,
) {
    for raw in technologies {
        let Some(raw) = clean_line(raw) else { continue };
        let name = known_technology(&raw).map_or(raw, |t| t.name.to_owned());
        let key = topic_key(&name);
        let exists = data.claims.iter().any(|c| {
            c.subject == subject
                && c.kind == ClaimKind::Technology
                && c.topic.as_deref() == Some(key.as_str())
                && c.verification != Verification::Rejected
        });
        if !exists {
            let claim = user_claim(
                data,
                subject,
                ClaimKind::Technology,
                format!("Used {name} at {label}"),
                Some(key.clone()),
                now,
            );
            data.claims.push(claim);
        }
        if !data.skills.iter().any(|s| s.key == key) {
            data.skills.push(Skill {
                id: SkillId::derive(&[&data.id().to_string(), "user", &key]),
                name,
                key,
                category: None,
                meta: RecordMeta::user(now),
            });
        }
    }
}

fn validate_period(
    start: Option<PartialDate>,
    end: Option<PartialDate>,
    current: bool,
) -> Result<(), ProfileError> {
    if let (Some(start), Some(end)) = (start, end)
        && end < start
    {
        return Err(ProfileError::Invalid(format!(
            "the end ({end}) is before the start ({start})"
        )));
    }
    if current && end.is_some() {
        return Err(ProfileError::Invalid(
            "a current position has no end date; drop --end or pass --current false".into(),
        ));
    }
    Ok(())
}

/// Applies `edit`; returns the names of fields that changed. With
/// `mark`, they are recorded as edited by the user.
fn apply_experience_edit(
    e: &mut Experience,
    edit: &ExperienceEdit,
    mark: bool,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    let mut set_text =
        |field: &'static str, target: &mut Option<String>, value: &Option<Option<String>>| {
            if let Some(value) = value {
                *target = clean_opt(value.clone());
                fields.push(field);
            }
        };
    set_text("company", &mut e.company, &edit.company);
    set_text("title", &mut e.title, &edit.title);
    set_text("location", &mut e.location, &edit.location);
    if let Some(summary) = &edit.summary {
        e.summary = summary.as_deref().and_then(clean_block);
        fields.push("summary");
    }
    if let Some(employment) = &edit.employment {
        e.employment = employment.clone();
        fields.push("employment");
    }
    if let Some(start) = edit.start {
        e.start = start;
        fields.push("start");
    }
    if let Some(end) = edit.end {
        e.end = end;
        fields.push("end");
        if end.is_some() && edit.current.is_none() {
            e.current = false;
            fields.push("current");
        }
    }
    if let Some(current) = edit.current {
        e.current = current;
        fields.push("current");
        if current && edit.end.is_none() {
            e.end = None;
            fields.push("end");
        }
    }
    if mark {
        for field in &fields {
            e.meta.mark_edited(field);
        }
    }
    fields
}

fn apply_project_edit(
    p: &mut Project,
    edit: &ProjectEdit,
    experience: Option<Option<ExperienceId>>,
    mark: bool,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if let Some(name) = edit.name.as_deref().and_then(clean_line)
        && mark
    {
        p.name = name;
        fields.push("name");
    }
    for (field, target, value) in [
        ("description", &mut p.description, &edit.description),
        ("role", &mut p.role, &edit.role),
        ("url", &mut p.url, &edit.url),
    ] {
        if let Some(value) = value {
            *target = clean_opt(value.clone());
            fields.push(field);
        }
    }
    if let Some(start) = edit.start {
        p.start = start;
        fields.push("start");
    }
    if let Some(end) = edit.end {
        p.end = end;
        fields.push("end");
    }
    if let Some(current) = edit.current {
        p.current = current;
        fields.push("current");
    }
    if let Some(experience) = experience {
        p.experience = experience;
        fields.push("experience");
    }
    if mark {
        for field in &fields {
            p.meta.mark_edited(field);
        }
    }
    fields
}

fn apply_education_edit(e: &mut Education, edit: &EducationEdit, mark: bool) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if let Some(institution) = edit.institution.as_deref().and_then(clean_line)
        && mark
    {
        e.institution = institution;
        fields.push("institution");
    }
    for (field, target, value) in [
        ("degree", &mut e.degree, &edit.degree),
        ("field", &mut e.field, &edit.field),
    ] {
        if let Some(value) = value {
            *target = clean_opt(value.clone());
            fields.push(field);
        }
    }
    if let Some(start) = edit.start {
        e.start = start;
        fields.push("start");
    }
    if let Some(end) = edit.end {
        e.end = end;
        fields.push("end");
    }
    if let Some(current) = edit.current {
        e.current = current;
        fields.push("current");
    }
    if mark {
        for field in &fields {
            e.meta.mark_edited(field);
        }
    }
    fields
}
