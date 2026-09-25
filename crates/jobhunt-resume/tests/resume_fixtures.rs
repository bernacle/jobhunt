//! Reading and parsing the resume fixtures (see `fixtures/src/README.md`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use jobhunt_profile::{
    ContactKind, DocumentKind, EmploymentKind, ParsedExperience, ParsedResume, PartialDate, Period,
};
use jobhunt_resume::{DeterministicParser, ExtractError, ReadError, ResumeFile, ResumeParser};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read(name: &str) -> (ResumeFile, ParsedResume) {
    let file = ResumeFile::read(&fixture(name)).unwrap();
    let parsed = DeterministicParser.parse(&file.text);
    (file, parsed)
}

fn d(s: &str) -> Option<PartialDate> {
    Some(s.parse().unwrap())
}

fn roles(parsed: &ParsedResume) -> Vec<(Option<&str>, Option<&str>)> {
    parsed
        .experiences
        .iter()
        .map(|e| (e.company.as_deref(), e.title.as_deref()))
        .collect()
}

fn period(e: &ParsedExperience) -> Period {
    Period {
        start: e.start,
        end: e.end,
        current: e.current,
    }
}

#[test]
fn multi_page_chromium_pdf() {
    let (file, r) = read("marina_costa.pdf");
    assert_eq!(file.text.kind, DocumentKind::Pdf);
    assert_eq!(file.text.pages, 2);
    // The running header and both page numbers are gone from the text.
    assert_eq!(
        file.text.removed,
        ["Page 1 of 2", "Marina Costa · Resume", "Page 2 of 2"]
    );
    let text = file.text.text();
    assert!(!text.contains("Page 1"));
    assert!(
        text.contains("Mar 2022 – Present"),
        "kerning does not split words"
    );
    assert!(
        text.contains("personal finance app"),
        "ligatures are normalized"
    );

    assert_eq!(r.basics.name.as_deref(), Some("Marina Costa"));
    assert_eq!(
        r.basics.headline.as_deref(),
        Some("Senior Software Engineer · Backend & Platform")
    );
    assert_eq!(r.basics.location.as_deref(), Some("São Paulo, Brazil"));
    let kinds: Vec<ContactKind> = r.basics.contacts.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            ContactKind::Email,
            ContactKind::Phone,
            ContactKind::Github,
            ContactKind::Linkedin
        ]
    );
    assert!(
        r.basics
            .summary
            .as_deref()
            .unwrap()
            .ends_with("from architecture to on-call.")
    );
    assert_eq!(r.basics.languages.len(), 3);
    assert_eq!(r.basics.languages[1].level.as_deref(), Some("fluent"));

    // Two roles under one company line; an undated freelance role.
    assert_eq!(
        roles(&r),
        [
            (Some("Ledgerly"), Some("Senior Software Engineer")),
            (Some("Banco Horizonte"), Some("Software Engineer II")),
            (Some("Banco Horizonte"), Some("Software Engineer")),
            (None, Some("Full Stack Developer")),
        ]
    );
    let [ledgerly, bank2, bank1, freelance] = &r.experiences[..] else {
        panic!("four experiences");
    };
    // Current job.
    assert_eq!(
        (ledgerly.start, ledgerly.end, ledgerly.current),
        (d("2022-03"), None, true)
    );
    assert_eq!(ledgerly.location.as_deref(), Some("Remote"));
    assert_eq!(ledgerly.employment, Some(EmploymentKind::FullTime));
    // Overlapping dates are kept as written, not "fixed".
    assert_eq!((bank2.start, bank2.end), (d("2020-01"), d("2022-04")));
    assert!(period(ledgerly).overlaps(&period(bank2), "2026-09".parse().unwrap()));
    assert_eq!(bank2.location.as_deref(), Some("São Paulo, Brazil"));
    assert_eq!((bank1.start, bank1.end), (d("2018-06"), d("2019-12")));
    // Missing dates stay missing, with a note.
    assert_eq!((freelance.start, freelance.end), (None, None));
    assert_eq!(freelance.employment, Some(EmploymentKind::Freelance));
    assert_eq!(freelance.notes, ["no dates found"]);
    assert!(r.experiences.iter().all(|e| !e.ambiguous_header));

    // Bullets without bullet characters, rebuilt from wrapped lines, verbatim.
    assert_eq!(
        ledgerly.bullets,
        [
            "Built a multi-tenant organization layer that lets enterprise customers manage users, roles and API keys across 30+ subsidiaries.",
            "Designed a rules engine for Travel Rule compliance checks, reducing manual reviews by 40%.",
            "Integrated four payment providers (Stripe, Adyen, dLocal and PIX) behind a single settlement API.",
            "Owned architectural decisions for the event pipeline and led the migration from RabbitMQ to Kafka.",
            "Mentored three engineers and ran the backend hiring loop.",
        ]
    );
    assert_eq!(
        ledgerly.technologies,
        [
            "Rust",
            "TypeScript",
            "PostgreSQL",
            "Kafka",
            "Kubernetes",
            "AWS"
        ]
    );
    assert_eq!(bank2.bullets.len(), 3);
    assert_eq!(bank1.bullets.len(), 2);
    // A role that starts on one page and continues on the next.
    assert_eq!(freelance.bullets.len(), 2);

    let projects: Vec<(&str, Option<PartialDate>, Option<PartialDate>, bool)> = r
        .projects
        .iter()
        .map(|p| (p.name.as_str(), p.start, p.end, p.current))
        .collect();
    assert_eq!(
        projects,
        [
            ("tinyqueue", d("2021"), None, true),
            ("Budgetly", d("2019"), d("2019"), false)
        ]
    );
    assert_eq!(r.education.len(), 1);
    let edu = &r.education[0];
    assert_eq!(edu.institution, "Universidade de São Paulo");
    assert_eq!(edu.degree.as_deref(), Some("B.Sc."));
    assert_eq!(edu.field.as_deref(), Some("Computer Science"));
    assert_eq!((edu.start, edu.end), (d("2014"), d("2018")));

    let categories: Vec<Option<&str>> = r.skills.iter().map(|s| s.category.as_deref()).collect();
    assert_eq!(
        categories,
        [
            Some("Languages"),
            Some("Infrastructure"),
            Some("Data"),
            Some("Frontend")
        ]
    );
    assert_eq!(
        r.skills[0].skills,
        ["Rust", "TypeScript", "Go", "Python", "SQL"]
    );
    assert!(r.ignored.is_empty(), "{:?}", r.ignored);
    assert!(r.notes.is_empty(), "{:?}", r.notes);
}

#[test]
fn word_positioned_pdf_without_space_characters() {
    let (file, r) = read("positioned_words.pdf");
    assert_eq!(file.text.pages, 2);
    assert_eq!(
        file.text.removed,
        ["Ana Lima — page 1 of 2", "Ana Lima — page 2 of 2"],
        "a footer that differs only by its page number is still a footer"
    );
    assert_eq!(r.basics.name.as_deref(), Some("Ana Lima"));
    assert_eq!(r.basics.headline.as_deref(), Some("Backend Engineer"));
    assert_eq!(
        roles(&r),
        [
            (Some("Acme Payments"), Some("Staff Software Engineer")),
            (Some("Globex"), Some("Backend Developer")),
        ]
    );
    let acme = &r.experiences[0];
    assert_eq!((acme.start, acme.current), (d("2021-01"), true));
    assert_eq!(
        acme.bullets,
        [
            "Led the redesign of the settlement service, cutting batch time by 60%.",
            "Mentored five engineers and introduced design reviews for every service that handles money movement.",
        ]
    );
    assert_eq!(
        (r.experiences[1].start, r.experiences[1].end),
        (d("2015"), d("2020"))
    );
    assert_eq!(r.education[0].institution, "University of Porto");
    assert_eq!(r.skills[0].skills, ["Go", "Python", "SQL"]);
}

#[test]
fn markdown_resume() {
    let (file, r) = read("ana_lima.md");
    assert_eq!(file.text.kind, DocumentKind::Markdown);
    assert_eq!(r.basics.name.as_deref(), Some("Ana Lima"));
    assert_eq!(r.basics.headline.as_deref(), Some("Staff Engineer"));
    assert_eq!(
        r.basics.summary.as_deref(),
        Some("Backend engineer who likes payments, developer tooling and small teams.")
    );
    assert_eq!(
        roles(&r),
        [
            (Some("Acme Payments"), Some("Staff Software Engineer")),
            (Some("Acme Payments"), Some("Senior Software Engineer")),
            (Some("Globex"), Some("Backend Developer")),
        ]
    );
    assert_eq!(r.experiences[0].bullets.len(), 3);
    assert_eq!(r.experiences[0].location.as_deref(), Some("Remote"));
    assert_eq!(r.experiences[2].start, d("2016-06"));
    assert_eq!(r.projects[0].name, "ledgerlint");
    assert_eq!(r.projects[0].notes, ["no dates found"]);
    assert_eq!(r.other.len(), 1);
    assert_eq!(r.other[0].section, "Certifications");
    assert_eq!(r.skills.len(), 2);
}

#[test]
fn plain_text_resume_with_another_layout() {
    let (file, r) = read("plain.txt");
    assert_eq!(file.text.kind, DocumentKind::Text);
    assert_eq!(
        r.basics.name.as_deref(),
        Some("JOÃO PEREIRA"),
        "kept as written"
    );
    assert_eq!(r.basics.headline.as_deref(), Some("Full Stack Developer"));
    assert_eq!(
        roles(&r),
        [
            (Some("Fintechly"), Some("Software Engineer")),
            (Some("Lojas Aurora"), Some("Frontend Developer")),
        ]
    );
    let fintechly = &r.experiences[0];
    assert_eq!(fintechly.employment, Some(EmploymentKind::Contract));
    assert_eq!((fintechly.start, fintechly.current), (d("2022-04"), true));
    assert_eq!(fintechly.bullets.len(), 2, "{:?}", fintechly.bullets);
    assert!(fintechly.bullets[0].ends_with("by moving reports to background jobs."));
    let edu = &r.education[0];
    assert_eq!(edu.institution, "Universidade Federal do Rio Grande do Sul");
    assert_eq!(edu.degree.as_deref(), Some("Bachelor of Science"));
    assert_eq!(edu.field.as_deref(), Some("Computer Engineering"));
    assert_eq!(r.skills[0].skills.len(), 8);
}

#[test]
fn parsing_is_deterministic() {
    let (a_file, a) = read("marina_costa.pdf");
    let (b_file, b) = read("marina_costa.pdf");
    assert_eq!(a, b);
    assert_eq!(a_file.sha256, b_file.sha256);
    let now = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
    let doc = a_file.source_document("deterministic/1", now);
    assert_eq!(doc.sha256.len(), 64);
    assert_eq!(doc.pages, Some(2));
    assert_eq!(doc.file_name.as_deref(), Some("marina_costa.pdf"));
}

#[test]
fn unreadable_files_have_clear_errors() {
    let reason = |name: &str| match ResumeFile::read(&fixture(name)) {
        Err(ReadError::Extract { source, .. }) => source,
        other => panic!("expected an extraction error for {name}, got {other:?}"),
    };
    // A valid PDF with no text in it, as a scanned resume would be.
    let empty = reason("blank.pdf");
    assert!(
        matches!(empty, ExtractError::NoText { pages: 1 }),
        "{empty}"
    );
    assert!(empty.to_string().contains("OCR"));
    // A damaged file.
    assert!(matches!(
        reason("truncated.pdf"),
        ExtractError::Unreadable { .. }
    ));
    // Not a PDF at all.
    assert!(matches!(reason("not_a_pdf.pdf"), ExtractError::NotPdf));
    // A missing file.
    assert!(matches!(
        ResumeFile::read(&fixture("missing.pdf")),
        Err(ReadError::Io { .. })
    ));
}

#[test]
fn a_panicking_pdf_reader_becomes_an_error() {
    // Garbage after a PDF header: the reader either errors or panics on
    // it; either way the caller gets an error, not a crash.
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.extend(std::iter::repeat_n(b'\xff', 4096));
    bytes.extend_from_slice(b"\ntrailer << /Root 1 0 R >>\n%%EOF\n");
    let result = ResumeFile::from_bytes(&bytes, Some("resume.pdf".into()));
    assert!(
        matches!(result, Err(ExtractError::Unreadable { .. })),
        "{result:?}"
    );
}
