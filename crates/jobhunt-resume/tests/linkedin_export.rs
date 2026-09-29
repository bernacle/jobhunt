//! Reading LinkedIn data exports: the fictional export under
//! `tests/fixtures/linkedin_export/`, zipped at run time, and broken or
//! foreign files. No real person's data.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jobhunt_resume::{LinkedinError, LinkedinExport};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/linkedin_export")
}

/// The fixture folder as LinkedIn's `.zip`, optionally under a folder.
fn zipped(prefix: &str, only: Option<&[&str]>) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut entries: Vec<_> = std::fs::read_dir(fixture())
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if only.is_some_and(|keep| !keep.contains(&name.as_str())) {
                continue;
            }
            zip.start_file(format!("{prefix}{name}"), options).unwrap();
            zip.write_all(&std::fs::read(entry.path()).unwrap())
                .unwrap();
        }
        zip.finish().unwrap();
    }
    out.into_inner()
}

fn assert_no_private_data(export: &LinkedinExport) {
    let everything = format!("{}\n{:?}", export.text, export.parsed);
    assert!(
        !everything.contains("SENTINEL"),
        "private or unread data leaked: {everything}"
    );
    for private in ["Riley", "1990-01-01", "@sentinel", "sentinel@example.com"] {
        assert!(!everything.contains(private), "{private} was read");
    }
}

#[test]
fn reads_the_career_files_of_an_export_folder() {
    let export = LinkedinExport::read(&fixture()).unwrap();
    assert_eq!(
        export.read,
        vec![
            ("Profile.csv".to_owned(), 1),
            ("Positions.csv".to_owned(), 3),
            ("Education.csv".to_owned(), 1),
            ("Skills.csv".to_owned(), 4),
            ("Certifications.csv".to_owned(), 1),
            ("Projects.csv".to_owned(), 1),
            ("Languages.csv".to_owned(), 2),
        ]
    );
    assert_eq!(export.unopened, 7, "messages, connections, … never opened");
    assert_no_private_data(&export);

    let p = &export.parsed;
    assert_eq!(
        p.basics.headline.as_deref(),
        Some("Backend engineer building payment systems")
    );
    assert_eq!(
        p.basics.summary.as_deref(),
        Some("I build reliable payment systems. Mostly Rust and Go.")
    );
    assert_eq!(p.basics.location.as_deref(), Some("Lisbon, Portugal"));
    assert_eq!(p.basics.name, None, "names are not read");
    assert!(p.basics.contacts.is_empty());
    assert_eq!(p.basics.languages.len(), 2);

    let northwind = &p.experiences[0];
    assert_eq!(northwind.company.as_deref(), Some("Northwind Labs"));
    assert_eq!(northwind.title.as_deref(), Some("Senior Software Engineer"));
    assert_eq!(northwind.start.unwrap().to_string(), "2021-03");
    assert!(
        northwind.current && northwind.end.is_none(),
        "empty Finished On: current"
    );
    assert_eq!(
        northwind.header,
        "Senior Software Engineer · Northwind Labs · Mar 2021 – Present · Lisbon, Portugal"
    );
    assert_eq!(
        northwind.bullets,
        vec![
            "Built the settlement pipeline in Rust.",
            "Led the migration from batch to streaming settlement with Kafka.",
            "Mentored four engineers.",
        ]
    );
    let contoso = &p.experiences[1];
    assert_eq!(contoso.end.unwrap().to_string(), "2021-02");
    assert!(!contoso.current);
    assert!(p.experiences[2].bullets.is_empty());

    assert_eq!(p.education[0].institution, "Example State University");
    assert_eq!(
        p.education[0].degree.as_deref(),
        Some("B.Sc. Computer Science")
    );
    assert_eq!(p.skills.len(), 4);
    assert_eq!(p.skills[3].skills, vec!["Distributed Systems"]);
    assert_eq!(p.projects[0].name, "lox");
    assert_eq!(
        p.projects[0].url.as_deref(),
        Some("https://github.com/rileyx/lox")
    );
    let others: Vec<(&str, &str)> = p
        .other
        .iter()
        .map(|o| (o.section.as_str(), o.line.as_str()))
        .collect();
    assert_eq!(
        others,
        vec![
            ("Websites", "https://riley.example.dev"),
            ("Websites", "https://blog.example.org"),
            (
                "Certifications",
                "Certified Kafka Developer — Example Institute (Jan 2024)"
            ),
        ]
    );
    // The document is what was used, section by section.
    assert!(export.text.starts_with(
        "LinkedIn data export: career files only (Profile, Positions, Education, Skills, Certifications, Projects, Languages)"
    ));
    assert!(
        export
            .text
            .contains("\n[Positions]\nSenior Software Engineer · Northwind Labs")
    );
    let doc = export.source_document(chrono::Utc::now());
    assert_eq!(doc.kind, jobhunt_profile::DocumentKind::Linkedin);
    assert_eq!(doc.sha256.len(), 64);
    assert_eq!(
        doc.parser,
        "linkedin-export/1;categories=Profile,Positions,Education,Skills,Certifications,Projects,Languages"
    );
}

#[test]
fn a_zip_reads_the_same_as_its_folder_wherever_the_files_are() {
    let folder = LinkedinExport::read(&fixture()).unwrap();
    let flat = LinkedinExport::from_bytes(
        &zipped("", None),
        Some("Basic_LinkedInDataExport_09-01-2026.zip".into()),
    )
    .unwrap();
    let nested =
        LinkedinExport::from_bytes(&zipped("Complete_LinkedInDataExport/", None), None).unwrap();
    assert_eq!(flat.parsed, folder.parsed);
    assert_eq!(nested.parsed, folder.parsed);
    assert_eq!(
        flat.sha256, folder.sha256,
        "same career data, same document"
    );
    assert_eq!(nested.sha256, folder.sha256);
    assert_eq!(flat.unopened, 7);
    assert_eq!(
        flat.file_name.as_deref(),
        Some("Basic_LinkedInDataExport_09-01-2026.zip")
    );
    assert_no_private_data(&flat);

    // From a path, the archive is read without loading it whole.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export.zip");
    std::fs::write(&path, zipped("", None)).unwrap();
    let from_path = LinkedinExport::read(&path).unwrap();
    assert_eq!(from_path.parsed, folder.parsed);

    // Unrelated files in a newer download do not change the document.
    let fewer = LinkedinExport::from_bytes(
        &zipped(
            "",
            Some(&[
                "Profile.csv",
                "Positions.csv",
                "Education.csv",
                "Skills.csv",
                "Certifications.csv",
                "Projects.csv",
                "Languages.csv",
            ]),
        ),
        None,
    )
    .unwrap();
    assert_eq!(fewer.sha256, folder.sha256);
    assert_eq!(fewer.unopened, 0);
}

#[test]
fn categories_that_are_missing_are_simply_absent() {
    let only = LinkedinExport::from_bytes(&zipped("", Some(&["Positions.csv"])), None).unwrap();
    assert_eq!(only.read, vec![("Positions.csv".to_owned(), 3)]);
    assert!(only.parsed.skills.is_empty());
    assert!(only.parsed.education.is_empty());
    assert!(only.parsed.other.is_empty());
    assert_eq!(only.parsed.basics.headline, None);
    assert!(
        only.parsed.notes.is_empty(),
        "absence is not a problem to report"
    );
}

#[test]
fn one_csv_file_is_enough_and_is_recognized_by_name_or_header() {
    let positions = std::fs::read(fixture().join("Positions.csv")).unwrap();
    let by_name = LinkedinExport::from_bytes(&positions, Some("Positions.csv".into())).unwrap();
    assert_eq!(by_name.parsed.experiences.len(), 3);
    let by_header =
        LinkedinExport::from_bytes(&positions, Some("positions (1).csv".into())).unwrap();
    assert_eq!(by_header.parsed, by_name.parsed);
    // "Name" alone could be skills or languages: not guessed.
    let skills = std::fs::read(fixture().join("Skills.csv")).unwrap();
    assert!(matches!(
        LinkedinExport::from_bytes(&skills, Some("export.csv".into())),
        Err(LinkedinError::NotAnExport(_))
    ));
    assert_eq!(
        LinkedinExport::from_bytes(&skills, Some("Skills.csv".into()))
            .unwrap()
            .parsed
            .skills
            .len(),
        4
    );
}

#[test]
fn unsupported_and_malformed_files_fail_clearly_and_import_nothing() {
    // Not an export at all.
    let err =
        LinkedinExport::from_bytes(b"%PDF-1.7 a resume", Some("resume.pdf".into())).unwrap_err();
    assert!(matches!(err, LinkedinError::NotAnExport(_)), "{err}");
    assert!(err.to_string().contains("Positions.csv"), "{err}");
    // A zip with only private files: nothing Narrow reads.
    let private = zipped("", Some(&["messages.csv", "Connections.csv"]));
    let err = LinkedinExport::from_bytes(&private, Some("messages.zip".into())).unwrap_err();
    assert!(matches!(err, LinkedinError::NotAnExport(_)), "{err}");
    assert!(!err.to_string().contains("SENTINEL"));
    // A career file in a shape Narrow does not know.
    let changed = b"Organization,Role,Start\nNorthwind Labs,SENTINEL-ROW,2021\n";
    let err = LinkedinExport::from_bytes(changed, Some("Positions.csv".into())).unwrap_err();
    match &err {
        LinkedinError::MissingColumns { file, missing } => {
            assert_eq!(file, "Positions.csv");
            assert_eq!(missing, &["Company Name", "Title"]);
        }
        other => panic!("expected missing columns, got {other}"),
    }
    assert!(
        !err.to_string().contains("SENTINEL"),
        "contents never in errors"
    );
    // One bad file fails the whole export, even with good ones beside it.
    let mut zip_bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut zip_bytes);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("Skills.csv", options).unwrap();
        zip.write_all(b"Name\nRust\n").unwrap();
        zip.start_file("Positions.csv", options).unwrap();
        zip.write_all(changed).unwrap();
        zip.finish().unwrap();
    }
    assert!(matches!(
        LinkedinExport::from_bytes(&zip_bytes.into_inner(), None),
        Err(LinkedinError::MissingColumns { .. })
    ));
    // A damaged archive.
    let mut truncated = zipped("", None);
    truncated.truncate(truncated.len() / 2);
    let err = LinkedinExport::from_bytes(&truncated, Some("export.zip".into())).unwrap_err();
    assert!(matches!(err, LinkedinError::Zip { .. }), "{err}");
    // Not text.
    let err = LinkedinExport::from_bytes(&[0xff, 0xfe, 0x00, 0x41], Some("Skills.csv".into()))
        .unwrap_err();
    assert!(matches!(err, LinkedinError::NotAnExport(_)), "{err}");
    // A missing path.
    assert!(matches!(
        LinkedinExport::read(Path::new("/nonexistent/export.zip")),
        Err(LinkedinError::Io { .. })
    ));
}

#[test]
fn doubtful_rows_are_noted_not_guessed() {
    let csv = "Company Name,Title,Description,Location,Started On,Finished On\n\
               ,,,,,\n\
               Northwind Labs,,,,sometime,\n\
               ,Freelance developer,,,2019,2020\n";
    let export = LinkedinExport::from_bytes(csv.as_bytes(), Some("Positions.csv".into())).unwrap();
    // The empty row is dropped; the undated company kept, date unknown.
    assert_eq!(export.parsed.experiences.len(), 2);
    let northwind = &export.parsed.experiences[0];
    assert_eq!(northwind.start, None);
    assert_eq!(northwind.title, None);
    assert!(
        export
            .parsed
            .notes
            .iter()
            .any(|n| n == "Positions.csv line 3: a date was not understood; left unknown"),
        "{:?}",
        export.parsed.notes
    );
    assert_eq!(northwind.notes, vec!["no start date in the export"]);
}

/// Rough cost of a large export (a long career, many skills): printed with
/// `--nocapture`, bounded loosely so a pathological slowdown fails.
#[test]
fn a_large_export_reads_quickly() {
    let mut positions =
        String::from("Company Name,Title,Description,Location,Started On,Finished On\n");
    for i in 0..500 {
        positions.push_str(&format!(
            "Company {i},Engineer {i},\"Built system {i} in Rust.\nOperated it with Kubernetes.\",Remote,Jan {},Feb {}\n",
            1990 + i % 30,
            1991 + i % 30
        ));
    }
    let mut skills = String::from("Name\n");
    for i in 0..1000 {
        skills.push_str(&format!("Skill {i}\n"));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("Positions.csv", options).unwrap();
        zip.write_all(positions.as_bytes()).unwrap();
        zip.start_file("Skills.csv", options).unwrap();
        zip.write_all(skills.as_bytes()).unwrap();
        // A big file Narrow must not open (messages).
        zip.start_file("messages.csv", options).unwrap();
        zip.write_all(&vec![b'x'; 8 * 1024 * 1024]).unwrap();
        zip.finish().unwrap();
    }
    let bytes = out.into_inner();
    let started = Instant::now();
    let export = LinkedinExport::from_bytes(&bytes, None).unwrap();
    let elapsed = started.elapsed();
    println!(
        "LinkedIn export: {} positions, {} skills, {} KB zipped: read in {elapsed:?}",
        export.parsed.experiences.len(),
        export.parsed.skills.len(),
        bytes.len() / 1024
    );
    assert_eq!(export.parsed.experiences.len(), 500);
    assert!(elapsed.as_secs() < 10);
}
