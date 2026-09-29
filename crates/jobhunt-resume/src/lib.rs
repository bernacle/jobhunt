//! Reading resumes (and LinkedIn data exports) into JobHunt's profile
//! model, locally.
//!
//! * [`extract`](mod@extract): PDF, plain-text and Markdown files into lines of text
//!   (no browser, no network, no LLM).
//! * [`parse`]: the [`ResumeParser`] extension point and the built-in
//!   [`DeterministicParser`], which fills in a
//!   [`jobhunt_profile::ParsedResume`].
//! * [`linkedin`]: a LinkedIn data export the person downloaded (its
//!   career files only) into the same contract.
//!
//! The profile domain decides what the parsed resume *claims*; this crate
//! only structures the document and keeps its words.

pub mod dates;
pub mod extract;
pub mod linkedin;
pub mod parse;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use jobhunt_profile::{DocumentId, SourceDocument};
use sha2::{Digest, Sha256};

pub use extract::{ExtractError, ExtractedText, Line, extract};
pub use linkedin::{LinkedinError, LinkedinExport};
pub use parse::{DeterministicParser, ResumeParser};

/// A resume file, read and extracted.
#[derive(Debug, Clone)]
pub struct ResumeFile {
    pub file_name: Option<String>,
    /// SHA-256 of the file's bytes, hex.
    pub sha256: String,
    pub text: ExtractedText,
}

/// Why a resume file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("could not read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read the resume {}", path.display())]
    Extract {
        path: PathBuf,
        #[source]
        source: ExtractError,
    },
}

impl ResumeFile {
    /// Reads and extracts a resume file.
    pub fn read(path: &Path) -> Result<Self, ReadError> {
        let bytes = std::fs::read(path).map_err(|source| ReadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        Self::from_bytes(&bytes, name).map_err(|source| ReadError::Extract {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Extracts a resume from bytes; `file_name` decides text vs Markdown.
    pub fn from_bytes(bytes: &[u8], file_name: Option<String>) -> Result<Self, ExtractError> {
        let extension = file_name
            .as_deref()
            .and_then(|n| Path::new(n).extension())
            .map(|e| e.to_string_lossy().to_lowercase());
        let text = extract(bytes, extension.as_deref())?;
        let sha256 = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            file_name,
            sha256,
            text,
        })
    }

    /// The document record stored with the profile. Its id is assigned by
    /// the profile domain on import.
    pub fn source_document(&self, parser: &str, now: DateTime<Utc>) -> SourceDocument {
        SourceDocument {
            id: DocumentId::derive(&[&self.sha256]),
            kind: self.text.kind,
            file_name: self.file_name.clone(),
            sha256: self.sha256.clone(),
            pages: u32::try_from(self.text.pages).ok(),
            text: self.text.text(),
            parser: parser.to_owned(),
            first_imported_at: now,
            last_imported_at: now,
        }
    }
}
