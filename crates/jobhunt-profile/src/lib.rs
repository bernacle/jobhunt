//! The JobHunt profile domain: the durable, inspectable model of the user.
//!
//! * [`model`]: the career profile (basics, experiences, projects,
//!   education, skills) with per-record provenance ([`RecordMeta`]).
//! * [`evidence`]: the evidence graph. Every professional claim carries its
//!   source snippet, provenance, confidence and the user's verification,
//!   and [`Claim::standing`] is the policy deciding what may be used.
//! * [`preferences`] and [`statement`]: what the user wants, structured,
//!   plus preference statements kept in the user's own words and read by a
//!   pluggable [`StatementParser`].
//! * [`resume`] and [`import`]: the contract resume parsers fill in, and
//!   the re-import rules that fold a resume into a profile without
//!   duplicating records or losing the user's edits and decisions.
//! * [`infer`]: the deterministic vocabularies behind technology, domain,
//!   role and ownership evidence.
//! * [`export`]: the versioned, portable profile format.
//! * [`repository`]: the persistence boundary ([`ProfileRepository`]);
//!   nothing in this crate knows which database is used.
//! * [`service`]: the use cases front-ends call.

pub mod aggregate;
pub mod date;
pub mod evidence;
pub mod export;
pub mod ids;
pub mod import;
pub mod infer;
#[cfg(test)]
mod memory;
pub mod model;
pub mod preferences;
pub mod repository;
pub mod resume;
#[cfg(test)]
mod scenarios;
pub mod service;
pub mod statement;
pub mod words;

pub use aggregate::{DomainEvidence, Gap, LastSeen, ProfileData, SkillEvidence};
pub use date::{ParseDateError, PartialDate, Period};
pub use evidence::{
    Claim, ClaimKind, ClaimQuery, Confidence, Provenance, ReviewReason, Standing, Subject,
    UsableBecause,
};
pub use export::{EXPORT_FORMAT, EXPORT_VERSION, ExportError, ProfileExport};
pub use ids::{
    ClaimId, DocumentId, EducationId, ExperienceId, PreferenceId, ProfileId, ProjectId, RecordId,
    SkillId, StatementId,
};
pub use import::{ImportReport, Tally, document_id, merge_resume};
pub use model::{
    Contact, ContactKind, DocumentKind, Education, EmploymentKind, EvidenceStrength, Experience,
    Origin, Profile, Project, RecordMeta, Skill, SourceDocument, SourceRef, SpokenLanguage,
    Verification,
};
pub use preferences::{
    Arrangement, Certainty, CompanyTrait, CompensationBound, Engagement, PayPeriod, Preference,
    PreferenceCategory, PreferenceOrigin, PreferenceStatement, PreferenceValue, PreferencesView,
    Stance, StatementReading, WorkAspect, WorkMode,
};
pub use repository::{ProfileEvent, ProfileEventKind, ProfileRepository, StorageError};
pub use resume::{
    ParsedBasics, ParsedEducation, ParsedExperience, ParsedOther, ParsedProject, ParsedResume,
    ParsedSkillLine,
};
pub use service::{
    BasicsEdit, EducationEdit, ExperienceEdit, ProfileError, ProfileService, ProjectEdit, Removal,
    StatementOutcome,
};
pub use statement::{ReadPreference, RuleParser, StatementParser, StatementReadout};
