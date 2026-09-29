//! The career profile: basics, source documents, experiences, projects,
//! education and skills.
//!
//! Records carry a [`RecordMeta`] saying where they came from (a resume, a
//! LinkedIn export, GitHub, or the user), which document and snippet
//! supports them (and which other sources corroborate them), whether the
//! user confirmed or rejected them, which fields the user edited (and a
//! re-import must therefore leave alone), and whether the sources still
//! contain them. Unknown values stay `None`: nothing is guessed.
//!
//! Responsibilities, accomplishments, technologies, domains and role
//! signals are not columns of an experience: they are [`crate::Claim`]s
//! about it, each with its own provenance and verification state. Use
//! [`crate::ProfileData`] to read them per experience.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::date::{PartialDate, Period};
use crate::ids::{DocumentId, EducationId, ExperienceId, ProfileId, ProjectId, SkillId};

/// Where a record came from: an imported source, or the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Read from an imported resume.
    Resume,
    /// Entered by the user.
    User,
    /// Read from a LinkedIn data export the user downloaded and provided.
    Linkedin,
    /// Read from a public GitHub account through GitHub's API.
    Github,
}

impl Origin {
    /// The sources a record can be imported from.
    pub const IMPORTED: [Origin; 3] = [Self::Resume, Self::Linkedin, Self::Github];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::User => "user",
            Self::Linkedin => "linkedin",
            Self::Github => "github",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "resume" => Some(Self::Resume),
            "user" => Some(Self::User),
            "linkedin" => Some(Self::Linkedin),
            "github" => Some(Self::Github),
            _ => None,
        }
    }

    /// Whether it came from an imported source (not typed by the user).
    pub fn is_imported(self) -> bool {
        self != Self::User
    }

    /// How people call it: "resume", "LinkedIn export", "GitHub", "you".
    pub fn label(self) -> &'static str {
        match self {
            Self::Resume => "resume",
            Self::User => "you",
            Self::Linkedin => "LinkedIn export",
            Self::Github => "GitHub",
        }
    }
}

/// The user's decision about a record or claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// Nobody has reviewed it yet.
    #[default]
    Unverified,
    /// The user said it is true.
    Confirmed,
    /// The user said it is wrong. Rejected records are kept (so a re-import
    /// cannot bring them back as trusted) but never used.
    Rejected,
}

impl Verification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "unverified" => Some(Self::Unverified),
            "confirmed" => Some(Self::Confirmed),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// Where in a source document something was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub document: DocumentId,
    /// The document's own words (line breaks joined), never a paraphrase.
    pub snippet: String,
    /// The resume section it was found in ("Experience", "Skills", ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// Bookkeeping shared by experiences, projects, education and skills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordMeta {
    pub origin: Origin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    /// For resume records: what identifies this record across re-imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_key: Option<String>,
    #[serde(default)]
    pub verification: Verification,
    /// Set when the latest resume import no longer contained the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<DateTime<Utc>>,
    /// Fields the user set by hand; re-imports never overwrite them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edited_fields: Vec<String>,
    /// What the parser was unsure about ("no dates found", ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Other sources that contain the same record (a LinkedIn export
    /// listing the position a resume also lists), at most one per source.
    /// See [`crate::support`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corroborations: Vec<SourceRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RecordMeta {
    pub fn user(now: DateTime<Utc>) -> Self {
        Self {
            origin: Origin::User,
            source: None,
            import_key: None,
            verification: Verification::Confirmed,
            stale_since: None,
            edited_fields: Vec::new(),
            notes: Vec::new(),
            corroborations: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_edited(&self, field: &str) -> bool {
        self.edited_fields.iter().any(|f| f == field)
    }

    pub fn mark_edited(&mut self, field: &str) {
        if !self.is_edited(field) {
            self.edited_fields.push(field.to_owned());
            self.edited_fields.sort();
        }
    }

    /// Rejected records are hidden from views and never used.
    pub fn is_rejected(&self) -> bool {
        self.verification == Verification::Rejected
    }

    pub fn is_stale(&self) -> bool {
        self.stale_since.is_some()
    }
}

/// Kinds of contact information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactKind {
    Email,
    Phone,
    Linkedin,
    Github,
    Website,
}

impl ContactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Linkedin => "linkedin",
            Self::Github => "github",
            Self::Website => "website",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub kind: ContactKind,
    pub value: String,
}

/// A spoken language and the level the user gave for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpokenLanguage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// Who the user is, briefly. Contact details are kept only because resumes
/// contain them; JobHunt does not need them for anything yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub languages: Vec<SpokenLanguage>,
    /// Basics the user set by hand (`name`, `headline`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edited_fields: Vec<String>,
    /// Incremented by every change; guards against concurrent writers.
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Profile {
    pub fn new(id: ProfileId, now: DateTime<Utc>) -> Self {
        Self {
            id,
            name: None,
            headline: None,
            location: None,
            summary: None,
            contacts: Vec::new(),
            languages: Vec::new(),
            edited_fields: Vec::new(),
            revision: 0,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_edited(&self, field: &str) -> bool {
        self.edited_fields.iter().any(|f| f == field)
    }
}

/// What a document was read from: a resume file (PDF, text, Markdown), a
/// LinkedIn data export, or a GitHub account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Pdf,
    Text,
    Markdown,
    /// The career files of a LinkedIn data export.
    Linkedin,
    /// A snapshot of a public GitHub account.
    Github,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Linkedin => "linkedin",
            Self::Github => "github",
        }
    }

    pub fn from_canonical(value: &str) -> Option<Self> {
        match value {
            "pdf" => Some(Self::Pdf),
            "text" => Some(Self::Text),
            "markdown" => Some(Self::Markdown),
            "linkedin" => Some(Self::Linkedin),
            "github" => Some(Self::Github),
            _ => None,
        }
    }

    /// The source a document of this kind is.
    pub fn origin(self) -> Origin {
        match self {
            Self::Pdf | Self::Text | Self::Markdown => Origin::Resume,
            Self::Linkedin => Origin::Linkedin,
            Self::Github => Origin::Github,
        }
    }
}

/// An imported source: a resume, a LinkedIn export or a GitHub account. Its
/// text (what was read, rendered as text for exports and APIs) is kept, so
/// every snippet can be traced back to the document it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDocument {
    pub id: DocumentId,
    pub kind: DocumentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// SHA-256 of the file's bytes.
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    /// The extracted text the parser read.
    pub text: String,
    /// Which parser read it (`deterministic/1`, `linkedin-export/1`, ...).
    pub parser: String,
    pub first_imported_at: DateTime<Utc>,
    pub last_imported_at: DateTime<Utc>,
}

/// Employment arrangement as the source stated it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmploymentKind {
    FullTime,
    PartTime,
    Contract,
    Freelance,
    Internship,
    Other(String),
}

impl EmploymentKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::FullTime => "full_time",
            Self::PartTime => "part_time",
            Self::Contract => "contract",
            Self::Freelance => "freelance",
            Self::Internship => "internship",
            Self::Other(raw) => raw,
        }
    }

    pub fn from_canonical(value: &str) -> Self {
        match value {
            "full_time" => Self::FullTime,
            "part_time" => Self::PartTime,
            "contract" => Self::Contract,
            "freelance" => Self::Freelance,
            "internship" => Self::Internship,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Recognizes the usual ways of writing an arrangement.
    pub fn recognize(text: &str) -> Option<Self> {
        let key = jobhunt_core::text::search_key(text);
        match key.as_str() {
            "full time" | "fulltime" | "permanent" | "clt" => Some(Self::FullTime),
            "part time" | "parttime" => Some(Self::PartTime),
            "contract" | "contractor" | "pj" | "b2b" => Some(Self::Contract),
            "freelance"
            | "freelancer"
            | "self employed"
            | "independent"
            | "independent contractor" => Some(Self::Freelance),
            "intern" | "internship" | "estágio" | "estagio" => Some(Self::Internship),
            _ => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::FullTime => "Full-time",
            Self::PartTime => "Part-time",
            Self::Contract => "Contract",
            Self::Freelance => "Freelance",
            Self::Internship => "Internship",
            Self::Other(raw) => raw,
        }
    }
}

/// One position held at one organization. Two titles at one company are
/// two experiences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experience {
    pub id: ExperienceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub employment: Option<EmploymentKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<PartialDate>,
    /// The source said the position continues ("Present").
    #[serde(default)]
    pub current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Order in the profile (resume order: most recent first, usually).
    pub position: u32,
    pub meta: RecordMeta,
}

impl Experience {
    pub fn period(&self) -> Period {
        Period {
            start: self.start,
            end: self.end,
            current: self.current,
        }
    }

    /// "Senior Engineer at Acme", falling back to whichever part is known.
    pub fn label(&self) -> String {
        match (&self.title, &self.company) {
            (Some(title), Some(company)) => format!("{title} at {company}"),
            (Some(title), None) => title.clone(),
            (None, Some(company)) => company.clone(),
            (None, None) => "(untitled experience)".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<PartialDate>,
    #[serde(default)]
    pub current: bool,
    /// The experience this project was part of, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experience: Option<ExperienceId>,
    pub position: u32,
    pub meta: RecordMeta,
}

impl Project {
    pub fn period(&self) -> Period {
        Period {
            start: self.start,
            end: self.end,
            current: self.current,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Education {
    pub id: EducationId,
    pub institution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<PartialDate>,
    #[serde(default)]
    pub current: bool,
    pub position: u32,
    pub meta: RecordMeta,
}

impl Education {
    pub fn period(&self) -> Period {
        Period {
            start: self.start,
            end: self.end,
            current: self.current,
        }
    }
}

/// A skill. Its evidence (where it was listed or used) is the set of
/// technology and skill claims linked to it; see
/// [`crate::ProfileData::skill_evidence`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub id: SkillId,
    /// Display name as first written ("PostgreSQL").
    pub name: String,
    /// Normalized name used for matching ("postgresql").
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub meta: RecordMeta,
}

/// How well a skill is backed, from strongest to weakest. Deliberately not
/// a number: it says what kind of evidence exists, not how good someone is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStrength {
    /// No usable evidence (everything behind it was rejected or is stale).
    Unsupported,
    /// Only named in a skills list.
    Listed,
    /// The user said so.
    UserStated,
    /// Used in at least one experience or project, with the source's words
    /// (a resume bullet, a LinkedIn description, a repository's code).
    Demonstrated,
}

impl EvidenceStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Listed => "listed",
            Self::UserStated => "user_stated",
            Self::Demonstrated => "demonstrated",
        }
    }
}
