//! Reading a LinkedIn data export, locally.
//!
//! The person downloads their own data from LinkedIn (*Settings → Data
//! privacy → Get a copy of your data*) and gives Narrow the file: the
//! `.zip` LinkedIn sends, the folder it unpacks to, or one of its CSV
//! files. Nothing here talks to LinkedIn.
//!
//! Only career files are opened, found by name, and within them only the
//! columns listed in [`CATEGORIES`]; everything else in the archive
//! (messages, connections, invitations, contact details, ads, searches, …)
//! is never read. A file Narrow cannot read truthfully fails the whole
//! import with a clear message: nothing is half-imported.
//!
//! The result is a [`jobhunt_profile::ParsedResume`] (every text verbatim
//! from the export) and the text Narrow keeps as the source document:
//! the rows it used, rendered one per line, so every claim's snippet can
//! be traced back to them.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use jobhunt_core::text::clean_line;
use jobhunt_profile::{
    DocumentId, DocumentKind, ParsedEducation, ParsedExperience, ParsedOther, ParsedProject,
    ParsedResume, ParsedSkillLine, PartialDate, SourceDocument, SpokenLanguage,
};
use sha2::{Digest, Sha256};

/// The parser's name, as recorded on the document.
pub const PARSER: &str = "linkedin-export/1";

/// The largest career file read (LinkedIn's are kilobytes).
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A career file of the export: its name, and the columns without which
/// Narrow does not recognize it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Category {
    pub file: &'static str,
    /// What it is, for people ("positions").
    pub what: &'static str,
    pub required: &'static [&'static str],
}

/// Every file Narrow reads from an export. Nothing else is opened.
pub const CATEGORIES: [Category; 7] = [
    Category {
        file: "Profile.csv",
        what: "profile",
        required: &["Headline"],
    },
    Category {
        file: "Positions.csv",
        what: "positions",
        required: &["Company Name", "Title"],
    },
    Category {
        file: "Education.csv",
        what: "education",
        required: &["School Name"],
    },
    Category {
        file: "Skills.csv",
        what: "skills",
        required: &["Name"],
    },
    Category {
        file: "Certifications.csv",
        what: "certifications",
        required: &["Name", "Authority"],
    },
    Category {
        file: "Projects.csv",
        what: "projects",
        required: &["Title"],
    },
    Category {
        file: "Languages.csv",
        what: "languages",
        required: &["Name"],
    },
];

/// Why an export could not be read. Messages name files and columns,
/// never their contents.
#[derive(Debug, thiserror::Error)]
pub enum LinkedinError {
    #[error("could not read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "{0} is not a LinkedIn data export Narrow can read: it has none of {files}. \
         Download yours from LinkedIn (Settings → Data privacy → Get a copy of your data) \
         and give Narrow the .zip, its folder, or one of those CSV files",
        files = CATEGORIES.iter().map(|c| c.file).collect::<Vec<_>>().join(", ")
    )]
    NotAnExport(String),
    #[error("{name} is not a readable ZIP archive: {detail}")]
    Zip { name: String, detail: String },
    #[error("{file} is not valid CSV (line {line}): {detail}")]
    Csv {
        file: String,
        line: u64,
        detail: String,
    },
    #[error(
        "{file} does not have the columns Narrow expects ({}); LinkedIn may have changed its export. Nothing was imported",
        missing.join(", ")
    )]
    MissingColumns { file: String, missing: Vec<String> },
    #[error("{file} is larger than Narrow reads from an export ({} MB)", MAX_FILE_BYTES / 1024 / 1024)]
    TooLarge { file: String },
}

/// A LinkedIn data export, read.
#[derive(Debug, Clone)]
pub struct LinkedinExport {
    /// The file (or folder) name it was read from.
    pub file_name: Option<String>,
    pub parsed: ParsedResume,
    /// The rows Narrow used, as text: the source document.
    pub text: String,
    /// SHA-256 of `text`: the same career data gives the same hash, even
    /// when the archive around it differs.
    pub sha256: String,
    /// Career files read and their rows (`("Positions.csv", 4)`).
    pub read: Vec<(String, usize)>,
    /// Other files in the export, never opened.
    pub unopened: usize,
}

impl LinkedinExport {
    /// Reads an export from a path: the `.zip`, the folder it unpacks to,
    /// or one CSV file of it.
    pub fn read(path: &Path) -> Result<Self, LinkedinError> {
        let io = |source| LinkedinError::Io {
            path: path.to_path_buf(),
            source,
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        if path.is_dir() {
            let mut files: Vec<(String, PathBuf)> = Vec::new();
            collect_dir(path, 0, &mut files).map_err(io)?;
            let mut found: Vec<(Category, String)> = Vec::new();
            let mut unopened = 0;
            for (base, full) in &files {
                match category_named(base) {
                    Some(c) if !found.iter().any(|(f, _)| f.file == c.file) => {
                        let meta = std::fs::metadata(full).map_err(io)?;
                        if meta.len() > MAX_FILE_BYTES {
                            return Err(LinkedinError::TooLarge {
                                file: c.file.into(),
                            });
                        }
                        let bytes = std::fs::read(full).map_err(io)?;
                        found.push((c, decode(&bytes)));
                    }
                    _ => unopened += 1,
                }
            }
            return Self::from_files(name.unwrap_or_else(|| "the folder".into()), found, unopened);
        }
        let file = std::fs::File::open(path).map_err(io)?;
        let mut head = [0u8; 4];
        let is_zip = {
            let mut probe = &file;
            probe.read(&mut head).map_err(io)? == 4 && head == *b"PK\x03\x04"
        };
        if is_zip {
            let file = std::fs::File::open(path).map_err(io)?;
            Self::from_zip(file, name)
        } else {
            let meta = std::fs::metadata(path).map_err(io)?;
            if meta.len() > MAX_FILE_BYTES {
                return Err(LinkedinError::TooLarge {
                    file: name.unwrap_or_default(),
                });
            }
            let bytes = std::fs::read(path).map_err(io)?;
            Self::from_bytes(&bytes, name)
        }
    }

    /// Reads an export from bytes: a `.zip` or one CSV file of it.
    pub fn from_bytes(bytes: &[u8], file_name: Option<String>) -> Result<Self, LinkedinError> {
        if bytes.starts_with(b"PK\x03\x04") {
            return Self::from_zip(std::io::Cursor::new(bytes), file_name);
        }
        let label = file_name.clone().unwrap_or_else(|| "the file".into());
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(LinkedinError::TooLarge { file: label });
        }
        if std::str::from_utf8(bytes).is_err() {
            return Err(LinkedinError::NotAnExport(label));
        }
        let text = decode(bytes);
        let category = file_name
            .as_deref()
            .and_then(|n| category_named(base_name(n)))
            .or_else(|| category_by_header(&text))
            .ok_or_else(|| LinkedinError::NotAnExport(label.clone()))?;
        let mut export = Self::from_files(label, vec![(category, text)], 0)?;
        export.file_name = file_name;
        Ok(export)
    }

    fn from_zip<R: Read + Seek>(
        reader: R,
        file_name: Option<String>,
    ) -> Result<Self, LinkedinError> {
        let label = file_name.clone().unwrap_or_else(|| "the archive".into());
        let zip_error = |e: zip::result::ZipError| LinkedinError::Zip {
            name: label.clone(),
            detail: e.to_string(),
        };
        let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
        // Names first (the central directory): only career files are opened.
        let mut wanted: Vec<(usize, Category, usize)> = Vec::new();
        let mut unopened = 0;
        for i in 0..archive.len() {
            let Some(name) = archive.name_for_index(i).map(str::to_owned) else {
                continue;
            };
            if name.ends_with('/') {
                continue;
            }
            let depth = name.matches('/').count();
            match category_named(base_name(&name)) {
                Some(c) => match wanted.iter_mut().find(|(_, w, _)| w.file == c.file) {
                    // The shallowest copy wins (the export's root).
                    Some(slot) if depth < slot.2 => *slot = (i, c, depth),
                    Some(_) => unopened += 1,
                    None => wanted.push((i, c, depth)),
                },
                None => unopened += 1,
            }
        }
        let mut found = Vec::new();
        for (i, category, _) in wanted {
            let entry = archive.by_index(i).map_err(zip_error)?;
            if entry.size() > MAX_FILE_BYTES {
                return Err(LinkedinError::TooLarge {
                    file: category.file.into(),
                });
            }
            let mut bytes = Vec::new();
            entry
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| LinkedinError::Zip {
                    name: label.clone(),
                    detail: format!("{}: {e}", category.file),
                })?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(LinkedinError::TooLarge {
                    file: category.file.into(),
                });
            }
            found.push((category, decode(&bytes)));
        }
        let mut export = Self::from_files(label, found, unopened)?;
        export.file_name = file_name;
        Ok(export)
    }

    fn from_files(
        label: String,
        mut files: Vec<(Category, String)>,
        unopened: usize,
    ) -> Result<Self, LinkedinError> {
        if files.is_empty() {
            return Err(LinkedinError::NotAnExport(label));
        }
        // Always the same order, whatever the archive's.
        files.sort_by_key(|(c, _)| CATEGORIES.iter().position(|x| x.file == c.file));
        let mut builder = Builder::default();
        let mut read = Vec::new();
        for (category, text) in &files {
            let table = Table::parse(category, text)?;
            read.push((category.file.to_owned(), table.rows.len()));
            builder.add(category, &table);
        }
        let text = builder.text();
        let sha256 = Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            file_name: Some(label),
            parsed: builder.parsed,
            text,
            sha256,
            read,
            unopened,
        })
    }

    /// The document the profile keeps (its id is set by the profile).
    pub fn source_document(&self, now: DateTime<Utc>) -> SourceDocument {
        SourceDocument {
            id: DocumentId::derive(&["linkedin", &self.sha256]),
            kind: DocumentKind::Linkedin,
            file_name: self.file_name.clone(),
            sha256: self.sha256.clone(),
            pages: None,
            text: self.text.clone(),
            parser: format!(
                "{PARSER};categories={}",
                self.read
                    .iter()
                    .map(|(file, _)| file.trim_end_matches(".csv"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            first_imported_at: now,
            last_imported_at: now,
        }
    }
}

/// Files in a folder and one level of subfolders (LinkedIn's Complete
/// export has a few).
fn collect_dir(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if depth < 1 {
                collect_dir(&path, depth + 1, out)?;
            }
        } else {
            out.push((entry.file_name().to_string_lossy().into_owned(), path));
        }
    }
    Ok(())
}

fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

fn category_named(name: &str) -> Option<Category> {
    CATEGORIES
        .into_iter()
        .find(|c| c.file.eq_ignore_ascii_case(name.trim()))
}

/// A CSV given without its LinkedIn name: recognized by its header, when
/// exactly one category matches.
fn category_by_header(text: &str) -> Option<Category> {
    let headers = header_candidates(text);
    let mut matching: Vec<Category> = CATEGORIES
        .into_iter()
        .filter(|c| headers.iter().any(|h| has_columns(h, c.required)))
        .collect();
    // The most specific wins (Positions' columns include Projects' "Title");
    // a tie is ambiguous (Skills and Languages both have only "Name").
    matching.sort_by_key(|c| std::cmp::Reverse(c.required.len()));
    match matching.as_slice() {
        [one] => Some(*one),
        [a, b, ..] if a.required.len() > b.required.len() => Some(*a),
        _ => None,
    }
}

fn decode(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

fn normalize(column: &str) -> String {
    column.trim().trim_start_matches('\u{feff}').to_lowercase()
}

fn has_columns(header: &[String], required: &[&str]) -> bool {
    required
        .iter()
        .all(|r| header.iter().any(|h| *h == normalize(r)))
}

/// The first few records, normalized (a header may follow a short
/// preamble, as some LinkedIn files have).
fn header_candidates(text: &str) -> Vec<Vec<String>> {
    csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .take(5)
        .filter_map(Result::ok)
        .map(|r| r.iter().map(normalize).collect())
        .collect()
}

/// One CSV file: its header and rows.
struct Table {
    file: &'static str,
    header: Vec<String>,
    /// Data rows, each with the line of the file it starts on.
    rows: Vec<(u64, Vec<String>)>,
}

impl Table {
    fn parse(category: &Category, text: &str) -> Result<Self, LinkedinError> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut header: Option<Vec<String>> = None;
        let mut rows = Vec::new();
        let mut seen = 0;
        for record in reader.records() {
            let record = record.map_err(|e| LinkedinError::Csv {
                file: category.file.into(),
                line: e.position().map_or(0, csv::Position::line),
                detail: match e.kind() {
                    csv::ErrorKind::Utf8 { .. } => "not UTF-8 text".into(),
                    csv::ErrorKind::UnequalLengths { .. } => "rows of different lengths".into(),
                    _ => "unreadable record".into(),
                },
            })?;
            seen += 1;
            let cells: Vec<String> = record.iter().map(str::to_owned).collect();
            match &header {
                None => {
                    let normalized: Vec<String> = cells.iter().map(|c| normalize(c)).collect();
                    if has_columns(&normalized, category.required) {
                        header = Some(normalized);
                    } else if seen >= 5 {
                        break;
                    }
                }
                Some(_) => {
                    if cells.iter().any(|c| !c.trim().is_empty()) {
                        let line = record.position().map_or(0, csv::Position::line);
                        rows.push((line, cells));
                    }
                }
            }
        }
        let header = header.ok_or_else(|| {
            let first = header_candidates(text)
                .into_iter()
                .next()
                .unwrap_or_default();
            LinkedinError::MissingColumns {
                file: category.file.into(),
                missing: category
                    .required
                    .iter()
                    .filter(|r| !first.iter().any(|h| *h == normalize(r)))
                    .map(|r| (*r).to_owned())
                    .collect(),
            }
        })?;
        Ok(Self {
            file: category.file,
            header,
            rows,
        })
    }

    /// A cell by column name, cleaned; `None` when absent or empty.
    fn get(&self, row: &[String], column: &str) -> Option<String> {
        let at = self.header.iter().position(|h| *h == normalize(column))?;
        row.get(at).and_then(|v| clean_line(v))
    }

    /// A multi-line cell (a description), with its lines kept.
    fn block(&self, row: &[String], column: &str) -> Option<String> {
        let at = self.header.iter().position(|h| *h == normalize(column))?;
        let lines: Vec<String> = row.get(at)?.lines().filter_map(clean_line).collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Builds the parsed export and its text, category by category.
#[derive(Default)]
struct Builder {
    parsed: ParsedResume,
    sections: Vec<(String, Vec<String>)>,
}

impl Builder {
    fn section(&mut self, name: &str) -> &mut Vec<String> {
        if !self.sections.iter().any(|(n, _)| n == name) {
            self.sections.push((name.to_owned(), Vec::new()));
        }
        let at = self
            .sections
            .iter()
            .position(|(n, _)| n == name)
            .unwrap_or(0);
        &mut self.sections[at].1
    }

    fn note(&mut self, file: &str, line: u64, what: &str) {
        self.parsed
            .notes
            .push(format!("{file} line {line}: {what}"));
    }

    fn date(&mut self, file: &str, line: u64, raw: Option<&str>) -> Option<PartialDate> {
        let raw = raw?;
        match raw.parse::<PartialDate>() {
            Ok(date) => Some(date),
            Err(_) => {
                self.note(file, line, "a date was not understood; left unknown");
                None
            }
        }
    }

    fn add(&mut self, category: &Category, table: &Table) {
        match category.file {
            "Profile.csv" => self.profile(table),
            "Positions.csv" => self.positions(table),
            "Education.csv" => self.education(table),
            "Skills.csv" => self.skills(table),
            "Certifications.csv" => self.certifications(table),
            "Projects.csv" => self.projects(table),
            "Languages.csv" => self.languages(table),
            _ => {}
        }
    }

    fn profile(&mut self, table: &Table) {
        // One row. Names, birth date, address, zip code and messaging
        // handles are not read.
        let Some((_, row)) = table.rows.first() else {
            return;
        };
        let headline = table.get(row, "Headline");
        let summary = table.block(row, "Summary");
        let location = table.get(row, "Geo Location");
        let websites = table
            .get(row, "Websites")
            .map(|w| urls_in(&w))
            .unwrap_or_default();
        let lines = self.section("Profile");
        if let Some(h) = &headline {
            lines.push(format!("Headline: {h}"));
        }
        if let Some(s) = &summary {
            lines.push(format!("Summary: {}", s.replace('\n', " ")));
        }
        if let Some(l) = &location {
            lines.push(format!("Location: {l}"));
        }
        for url in &websites {
            lines.push(format!("Website: {url}"));
        }
        self.parsed.basics.headline = headline;
        self.parsed.basics.summary = summary.map(|s| s.replace('\n', " "));
        self.parsed.basics.location = location;
        for url in websites {
            self.parsed.other.push(ParsedOther {
                section: "Websites".into(),
                line: url,
            });
        }
    }

    fn positions(&mut self, table: &Table) {
        for (i, row) in &table.rows {
            let i = *i;
            let company = table.get(row, "Company Name");
            let title = table.get(row, "Title");
            if company.is_none() && title.is_none() {
                self.note(table.file, i, "no company or title; skipped");
                continue;
            }
            let started = table.get(row, "Started On");
            let finished = table.get(row, "Finished On");
            let location = table.get(row, "Location");
            let start = self.date(table.file, i, started.as_deref());
            let end = self.date(table.file, i, finished.as_deref());
            // LinkedIn leaves "Finished On" empty for a current position.
            let current = finished.is_none();
            let mut header = [title.as_deref(), company.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            header.push_str(&format!(
                " · {} – {}",
                started.as_deref().unwrap_or("?"),
                finished.as_deref().unwrap_or("Present")
            ));
            if let Some(l) = &location {
                header.push_str(&format!(" · {l}"));
            }
            let bullets = table
                .block(row, "Description")
                .map(|d| sentences(&d))
                .unwrap_or_default();
            let mut notes = Vec::new();
            if start.is_none() {
                notes.push("no start date in the export".to_owned());
            }
            let lines = self.section("Positions");
            lines.push(header.clone());
            lines.extend(bullets.iter().map(|b| format!("  {b}")));
            self.parsed.experiences.push(ParsedExperience {
                company,
                title,
                employment: None,
                location,
                start,
                end,
                current,
                summary: None,
                header,
                bullets,
                tech_line: None,
                technologies: Vec::new(),
                ambiguous_header: false,
                notes,
            });
        }
    }

    fn education(&mut self, table: &Table) {
        for (i, row) in &table.rows {
            let i = *i;
            let Some(institution) = table.get(row, "School Name") else {
                self.note(table.file, i, "no school; skipped");
                continue;
            };
            let degree = table.get(row, "Degree Name");
            let started = table.get(row, "Start Date");
            let finished = table.get(row, "End Date");
            let start = self.date(table.file, i, started.as_deref());
            let end = self.date(table.file, i, finished.as_deref());
            let mut header = institution.clone();
            if let Some(d) = &degree {
                header.push_str(&format!(" · {d}"));
            }
            if started.is_some() || finished.is_some() {
                header.push_str(&format!(
                    " · {} – {}",
                    started.as_deref().unwrap_or("?"),
                    finished.as_deref().unwrap_or("?")
                ));
            }
            self.section("Education").push(header.clone());
            self.parsed.education.push(ParsedEducation {
                institution,
                degree,
                field: None,
                start,
                end,
                current: false,
                header,
                notes: Vec::new(),
            });
        }
    }

    fn skills(&mut self, table: &Table) {
        for (_, row) in &table.rows {
            let Some(name) = table.get(row, "Name") else {
                continue;
            };
            self.section("Skills").push(name.clone());
            self.parsed.skills.push(ParsedSkillLine {
                category: None,
                skills: vec![name.clone()],
                line: name,
            });
        }
    }

    fn certifications(&mut self, table: &Table) {
        for (_, row) in &table.rows {
            let Some(name) = table.get(row, "Name") else {
                continue;
            };
            // Licence numbers and verification URLs are not read.
            let mut line = name;
            if let Some(authority) = table.get(row, "Authority") {
                line.push_str(&format!(" — {authority}"));
            }
            match (table.get(row, "Started On"), table.get(row, "Finished On")) {
                (Some(s), Some(f)) => line.push_str(&format!(" ({s} – {f})")),
                (Some(s), None) => line.push_str(&format!(" ({s})")),
                _ => {}
            }
            self.section("Certifications").push(line.clone());
            self.parsed.other.push(ParsedOther {
                section: "Certifications".into(),
                line,
            });
        }
    }

    fn projects(&mut self, table: &Table) {
        for (i, row) in &table.rows {
            let i = *i;
            let Some(name) = table.get(row, "Title") else {
                self.note(table.file, i, "no title; skipped");
                continue;
            };
            let started = table.get(row, "Started On");
            let finished = table.get(row, "Finished On");
            let start = self.date(table.file, i, started.as_deref());
            let end = self.date(table.file, i, finished.as_deref());
            let url = table.get(row, "Url").filter(|u| u.starts_with("http"));
            let mut header = name.clone();
            if started.is_some() || finished.is_some() {
                header.push_str(&format!(
                    " · {} – {}",
                    started.as_deref().unwrap_or("?"),
                    finished.as_deref().unwrap_or("Present")
                ));
            }
            if let Some(u) = &url {
                header.push_str(&format!(" · {u}"));
            }
            let bullets = table
                .block(row, "Description")
                .map(|d| sentences(&d))
                .unwrap_or_default();
            let lines = self.section("Projects");
            lines.push(header.clone());
            lines.extend(bullets.iter().map(|b| format!("  {b}")));
            self.parsed.projects.push(ParsedProject {
                name,
                key: None,
                description: None,
                role: None,
                url,
                start,
                end,
                current: started.is_some() && finished.is_none(),
                header,
                bullets,
                tech_line: None,
                technologies: Vec::new(),
                notes: Vec::new(),
            });
        }
    }

    fn languages(&mut self, table: &Table) {
        for (_, row) in &table.rows {
            let Some(name) = table.get(row, "Name") else {
                continue;
            };
            let level = table.get(row, "Proficiency");
            self.section("Languages").push(match &level {
                Some(l) => format!("{name} ({l})"),
                None => name.clone(),
            });
            self.parsed
                .basics
                .languages
                .push(SpokenLanguage { name, level });
        }
    }

    fn text(&self) -> String {
        let names: Vec<&str> = self.sections.iter().map(|(n, _)| n.as_str()).collect();
        let mut out = format!(
            "LinkedIn data export: career files only ({})\n",
            names.join(", ")
        );
        for (name, lines) in &self.sections {
            out.push_str(&format!("\n[{name}]\n"));
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }
}

/// URLs in LinkedIn's "Websites" cell (`[PERSONAL:https://…],[BLOG:…]`).
fn urls_in(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(at) = rest.find("http") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c == ']' || c == ',' || c.is_whitespace())
            .unwrap_or(tail.len());
        let url = &tail[..end];
        if (url.starts_with("https://") || url.starts_with("http://")) && url.len() > 8 {
            out.push(url.to_owned());
        }
        rest = &tail[end..];
    }
    out
}

/// A description's lines, bullet markers removed; a long paragraph is cut
/// into sentences. The words are kept as written.
fn sentences(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in block.lines() {
        let line = line
            .trim()
            .trim_start_matches(['•', '-', '*', '–', '·', '▪', '●'])
            .trim();
        if line.is_empty() {
            continue;
        }
        if line.chars().count() <= 240 {
            out.push(line.to_owned());
            continue;
        }
        let mut current = String::new();
        for word in line.split(' ') {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            if word.ends_with(['.', '!', '?']) && current.chars().count() > 20 {
                out.push(std::mem::take(&mut current));
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_websites_in_linkedins_format() {
        assert_eq!(
            urls_in("[PERSONAL:https://riley.example.dev],[BLOG:http://blog.example.org/x]"),
            vec!["https://riley.example.dev", "http://blog.example.org/x"]
        );
        assert!(urls_in("none").is_empty());
    }

    #[test]
    fn descriptions_become_verbatim_items() {
        assert_eq!(
            sentences("• Built the ledger.\n\n- Ran the on-call rotation"),
            vec!["Built the ledger.", "Ran the on-call rotation"]
        );
        let long = format!(
            "{} Then shipped it.",
            "Designed a very long thing. ".repeat(10)
        );
        assert!(sentences(&long).len() > 5);
    }

    #[test]
    fn a_csv_without_its_name_is_recognized_by_its_header() {
        let positions = "Company Name,Title,Description,Location,Started On,Finished On\n";
        assert_eq!(
            category_by_header(positions).map(|c| c.file),
            Some("Positions.csv")
        );
        assert_eq!(
            category_by_header("Name\nRust\n"),
            None,
            "ambiguous: skills or languages?"
        );
        assert_eq!(category_by_header("a,b\n1,2\n"), None);
    }
}
