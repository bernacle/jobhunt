//! What a resume parser hands to the profile domain.
//!
//! Parsers (the deterministic one in `jobhunt-resume`, or an optional
//! AI-assisted one later) only *structure* the document: they find the
//! entries and keep the resume's own words. Deciding what those words
//! claim, and how far to trust them, is the domain's job
//! ([`crate::import`]). Every text field here is verbatim resume text
//! (with line wraps joined); fields a parser could not determine are
//! `None`, with a note saying so.

use crate::date::PartialDate;
use crate::model::{Contact, EmploymentKind, SpokenLanguage};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedResume {
    pub basics: ParsedBasics,
    pub experiences: Vec<ParsedExperience>,
    pub projects: Vec<ParsedProject>,
    pub education: Vec<ParsedEducation>,
    pub skills: Vec<ParsedSkillLine>,
    /// Lines of other recognized sections (certifications, awards, ...).
    pub other: Vec<ParsedOther>,
    /// Document-level doubts ("no experience section found").
    pub notes: Vec<String>,
    /// Lines that were not understood and were not used.
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedBasics {
    pub name: Option<String>,
    pub headline: Option<String>,
    pub location: Option<String>,
    pub summary: Option<String>,
    pub contacts: Vec<Contact>,
    pub languages: Vec<SpokenLanguage>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedExperience {
    pub company: Option<String>,
    pub title: Option<String>,
    pub employment: Option<EmploymentKind>,
    pub location: Option<String>,
    pub start: Option<PartialDate>,
    pub end: Option<PartialDate>,
    pub current: bool,
    /// A paragraph describing the role, when the resume has one apart from
    /// its bullets.
    pub summary: Option<String>,
    /// The entry's header lines, verbatim.
    pub header: String,
    /// One item per bullet or sentence, verbatim.
    pub bullets: Vec<String>,
    /// A "Tech: …" style line, verbatim, and the names it lists.
    pub tech_line: Option<String>,
    pub technologies: Vec<String>,
    /// The company and title could not be told apart with confidence.
    pub ambiguous_header: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedProject {
    pub name: String,
    pub description: Option<String>,
    pub role: Option<String>,
    pub url: Option<String>,
    pub start: Option<PartialDate>,
    pub end: Option<PartialDate>,
    pub current: bool,
    pub header: String,
    pub bullets: Vec<String>,
    pub tech_line: Option<String>,
    pub technologies: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedEducation {
    pub institution: String,
    pub degree: Option<String>,
    pub field: Option<String>,
    pub start: Option<PartialDate>,
    pub end: Option<PartialDate>,
    pub current: bool,
    pub header: String,
    pub notes: Vec<String>,
}

/// One line of a skills section: `Languages: Rust, TypeScript`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedSkillLine {
    pub category: Option<String>,
    pub skills: Vec<String>,
    /// The line, verbatim.
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedOther {
    /// The section heading as written ("Certifications").
    pub section: String,
    pub line: String,
}
