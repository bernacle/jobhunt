//! The career profile end to end: the `narrow` binary against a temporary
//! database and the resume fixtures, and the same pipeline through the
//! libraries (resume file → parser → profile service → SQLite).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{TimeZone, Utc};
use jobhunt_jobs::{JobQuery, JobRepository};
use jobhunt_profile::{ClaimKind, ProfileRepository, ProfileService, Standing, Verification};
use jobhunt_resume::{DeterministicParser, ResumeFile, ResumeParser};
use jobhunt_storage::SqliteJobStore;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jobhunt-resume/tests/fixtures")
        .join(name)
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Runs `narrow` with this environment's config and database.
    fn run_with(&self, database: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_narrow"))
            .arg("--config")
            .arg(self.path("config.toml"))
            .arg("--database")
            .arg(self.path(database))
            .args(args)
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with("jobhunt.db", args)
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "narrow {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn fails(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(!output.status.success(), "narrow {args:?} should fail");
        String::from_utf8(output.stderr).unwrap()
    }
}

fn first_claim_id(text: &str) -> String {
    let at = text.find("clm_").expect("a claim id in the output");
    text[at..at + 12].to_owned()
}

#[test]
fn cli_flow_from_resume_to_export() {
    let env = Env::new();
    let v1 = fixture("marina_costa.pdf");
    let v2 = fixture("marina_costa_v2.pdf");

    // Nothing yet.
    let err = env.fails(&["profile"]);
    assert!(err.contains("narrow init"), "{err}");

    // Import.
    let out = env.ok(&["init", v1.to_str().unwrap()]);
    for expected in [
        "Imported marina_costa.pdf (2 pages)",
        "ignored 3 lines repeated on every page",
        "Ledgerly — Senior Software Engineer · Mar 2022 – Present · Remote",
        "Banco Horizonte — Software Engineer II · Jan 2020 – Apr 2022",
        "Banco Horizonte — Software Engineer · Jun 2018 – Dec 2019",
        "Full Stack Developer (freelance) · dates unknown",
        "tinyqueue, Budgetly",
        "B.Sc. in Computer Science, Universidade de São Paulo",
        "Preferences\n  none yet",
        "directly supported",
        "need review",
        "Full Stack Developer: no dates found",
        "narrow claims review",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }

    // Inspect.
    let out = env.ok(&["profile"]);
    for expected in [
        "Marina Costa — Senior Software Engineer · Backend & Platform",
        "Experience",
        "Rust, TypeScript, PostgreSQL, Kafka, Kubernetes, AWS",
        "used in your work:",
        "listed only: ",
        "Domains",
        "payments",
        "Role signals",
        "backend",
        "Evidence",
        "Missing or uncertain",
        "no compensation set",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }

    // Preferences in the user's words.
    let out = env.ok(&[
        "preferences",
        "add",
        "I want small product teams and at least $120k. Avoid pure SRE roles.",
    ]);
    assert!(
        out.contains("at least 120,000 per year (currency unknown)"),
        "“$” alone is not read as USD:\n{out}"
    );
    assert!(out.contains("can mean USD, CAD, AUD"), "{out}");
    assert!(out.contains("avoid  sre roles"), "{out}");
    let out = env.ok(&["profile"]);
    assert!(out.contains("has no currency"), "{out}");
    let out = env.ok(&[
        "preferences",
        "set",
        "compensation",
        "--minimum",
        "120k",
        "--currency",
        "USD",
    ]);
    assert!(
        out.contains("replaces: required at least 120,000 per year (currency unknown)"),
        "{out}"
    );
    let out = env.ok(&["profile"]);
    assert!(!out.contains("has no currency"), "{out}");
    let out = env.ok(&["preferences"]);
    assert!(out.contains("“I want small product teams and at least $120k. Avoid pure SRE roles.”"));
    assert!(out.contains("from “Avoid pure SRE roles”"), "{out}");
    let out = env.ok(&[
        "preferences",
        "set",
        "compensation",
        "--target",
        "150k",
        "--currency",
        "usd",
    ]);
    assert!(out.contains("target USD 150,000 per year"), "{out}");

    // Claims: list, review, show, confirm, reject.
    let out = env.ok(&["claims"]);
    assert!(out.contains("✓") && out.contains("?"), "{out}");
    let review = env.ok(&["claims", "review"]);
    assert!(review.contains("need review"), "{review}");
    assert!(review.contains("why: "), "{review}");
    assert!(review.contains("resume: “"), "{review}");
    let to_confirm = first_claim_id(&review);
    let out = env.ok(&["claims", "confirm", &to_confirm]);
    assert!(out.contains("Confirmed 1 claim"), "{out}");
    let shown = env.ok(&["claim", "show", &to_confirm]);
    assert!(shown.contains("confirmed on"), "{shown}");
    assert!(
        shown.contains("Source        marina_costa.pdf, Experience"),
        "{shown}"
    );
    let rules = env.ok(&["claims", "--kind", "accomplishment"]);
    let rules_line = rules
        .lines()
        .find(|l| l.contains("Designed a rules engine"))
        .unwrap();
    let rules_id = first_claim_id(rules_line);
    env.ok(&["claims", "confirm", &rules_id]);
    let seniority = env.ok(&["claims", "--kind", "ownership"]);
    let senior_id = first_claim_id(
        seniority
            .lines()
            .find(|l| l.contains("Senior-level role at Ledgerly"))
            .unwrap(),
    );
    let out = env.ok(&["claims", "reject", &senior_id, "--reason", "not senior yet"]);
    assert!(out.contains("Rejected 1 claim"), "{out}");
    let rejected = env.ok(&["claims", "--state", "rejected"]);
    assert!(
        rejected.contains("Senior-level role at Ledgerly"),
        "{rejected}"
    );

    // A manual correction.
    let listing = env.ok(&["profile"]);
    let bank_line = listing
        .lines()
        .skip_while(|l| !l.contains("Software Engineer II"))
        .nth(1)
        .unwrap();
    let bank_id = &bank_line[bank_line.find("exp_").unwrap()..][..12];
    let out = env.ok(&[
        "profile",
        "edit",
        "experience",
        bank_id,
        "--location",
        "Hybrid, São Paulo",
    ]);
    assert!(out.contains("Your edits are kept"), "{out}");

    // Export.
    let export_path = env.path("profile.json");
    env.ok(&["profile", "export", "-o", export_path.to_str().unwrap()]);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
    assert_eq!(json["format"], "jobhunt.profile");
    assert_eq!(json["version"], 1);
    for key in [
        "profile",
        "documents",
        "experiences",
        "projects",
        "education",
        "skills",
        "claims",
        "preferences",
        "statements",
    ] {
        assert!(!json[key].is_null(), "export has {key}");
    }
    let states: Vec<&str> = json["claims"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["verification"].as_str().unwrap())
        .collect();
    assert_eq!(states.iter().filter(|s| **s == "confirmed").count(), 2);
    assert_eq!(states.iter().filter(|s| **s == "rejected").count(), 1);
    let stdout_export = env.ok(&["profile", "export"]);
    assert!(stdout_export.trim_start().starts_with('{'));

    // Re-import a changed resume: decisions and edits survive.
    let out = env.ok(&["init", v2.to_str().unwrap()]);
    for expected in [
        "Re-imported marina_costa_v2.pdf",
        "Ledgerly — Staff Software Engineer · Mar 2022 – Present",
        "Nimbus Labs — Software Engineering Intern · Jan 2017 – Dec 2017",
        "Changes since the last import",
        "experiences: 1 new, 1 updated, 2 unchanged, 1 no longer in the resume",
        // The rejected seniority claim went stale with the old title.
        "kept your decisions: 1 confirmed, 0 rejected",
        "kept your edits on 1 record",
        "1 claim you confirmed left the resume",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    let out = env.ok(&["profile"]);
    assert!(
        out.contains("Hybrid, São Paulo"),
        "the edit won over the resume:\n{out}"
    );
    assert!(out.contains("not in your latest resume"), "{out}");
    let rejected = env.ok(&["claims", "--state", "rejected"]);
    assert!(
        rejected.contains("Senior-level role at Ledgerly"),
        "still rejected:\n{rejected}"
    );
    let stale = env.ok(&["claims", "--state", "stale"]);
    assert!(stale.contains("reducing manual reviews by 40%"), "{stale}");
    let new_rules = env.ok(&["claims", "--kind", "accomplishment"]);
    let new_id = first_claim_id(new_rules.lines().find(|l| l.contains("by 55%")).unwrap());
    let shown = env.ok(&["claims", "show", &new_id]);
    assert!(shown.contains("Replaces"), "{shown}");
    assert!(shown.contains("not reviewed"), "{shown}");

    // Import the export into a fresh database: the same profile.
    let output = env.run_with(
        "other.db",
        &["profile", "import", export_path.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = env.run_with("other.db", &["claims", "--state", "rejected"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Senior-level role at Ledgerly"));
    // Importing over an existing profile needs --replace.
    let err = env.fails(&["profile", "import", export_path.to_str().unwrap()]);
    assert!(err.contains("--replace"), "{err}");

    // A broken file changes nothing.
    let broken = env.path("broken.json");
    std::fs::write(
        &broken,
        std::fs::read_to_string(&export_path).unwrap().replacen(
            "\"version\": 1",
            "\"version\": 99",
            1,
        ),
    )
    .unwrap();
    let before = env.ok(&["profile", "export"]);
    let err = env.fails(&["profile", "import", "--replace", broken.to_str().unwrap()]);
    assert!(err.contains("version 99 is not supported"), "{err}");
    assert!(err.contains("your profile is unchanged"), "{err}");
    let after = env.ok(&["profile", "export"]);
    let strip = |s: &str| {
        s.lines()
            .filter(|l| !l.contains("exported_at"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&before), strip(&after));

    // History records every change.
    let history = env.ok(&["profile", "history", "-n", "50"]);
    for kind in [
        "resume_imported",
        "statement_added",
        "claim_confirmed",
        "claim_rejected",
        "record_edited",
        "preference_set",
    ] {
        assert!(history.contains(kind), "missing {kind} in:\n{history}");
    }
}

#[test]
fn unreadable_resumes_fail_with_a_reason() {
    let env = Env::new();
    let err = env.fails(&["init", fixture("blank.pdf").to_str().unwrap()]);
    assert!(err.contains("no text could be extracted"), "{err}");
    let err = env.fails(&["init", fixture("truncated.pdf").to_str().unwrap()]);
    assert!(err.contains("damaged or truncated"), "{err}");
    let err = env.fails(&["init", env.path("missing.pdf").to_str().unwrap()]);
    assert!(err.contains("could not read"), "{err}");
    // Nothing was stored.
    assert!(env.fails(&["profile"]).contains("narrow init"));
}

#[tokio::test]
async fn markdown_reimport_through_the_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteJobStore::open(&dir.path().join("jobhunt.db"))
        .await
        .unwrap();
    let service = ProfileService::new(&store);
    let parser = DeterministicParser;
    let import = |name: &'static str, minute: u32| {
        let service = &service;
        async move {
            let now = Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap();
            let file = ResumeFile::read(&fixture(name)).unwrap();
            let parsed = parser.parse(&file.text);
            service
                .import_resume(file.source_document(parser.name(), now), &parsed, now)
                .await
                .unwrap()
        }
    };

    let (first, data) = import("ana_lima.md", 1).await;
    assert!(first.first_import);
    assert_eq!(data.experiences.len(), 3);
    let globex = data
        .experiences
        .iter()
        .find(|e| e.company.as_deref() == Some("Globex"))
        .unwrap()
        .id;
    let pix = data
        .claims
        .iter()
        .find(|c| c.text.starts_with("Built the PIX integration"))
        .unwrap()
        .id;
    let certification = data
        .claims
        .iter()
        .find(|c| c.kind == ClaimKind::Other)
        .unwrap();
    assert!(
        certification
            .text
            .starts_with("Certifications: AWS Certified")
    );
    service
        .decide_claims(
            &[pix.to_string()],
            Verification::Confirmed,
            None,
            // Between the imports, on the same clock as they are.
            Utc.with_ymd_and_hms(2026, 9, 25, 12, 1, 30).unwrap(),
        )
        .await
        .unwrap();

    // The same file again: nothing new.
    let (again, _) = import("ana_lima.md", 2).await;
    assert!(again.same_file);
    assert_eq!((again.claims.added, again.claims.stale), (0, 0));

    // The corrected resume.
    let (report, data) = import("ana_lima_v2.md", 3).await;
    assert_eq!(report.experiences.added, 0, "{report:?}");
    let globex_after = data.experience(globex).unwrap();
    assert_eq!(
        globex_after.title.as_deref(),
        Some("Backend Engineer"),
        "corrected title, same record"
    );
    let pix_after = data.claim(pix).unwrap();
    assert!(pix_after.stale_since.is_some());
    assert_eq!(pix_after.verification, Verification::Confirmed);
    assert!(matches!(data.standing(pix_after), Standing::NeedsReview(_)));
    let reworded = data
        .claims
        .iter()
        .find(|c| c.text.contains("from four hours to 90 minutes"))
        .unwrap();
    assert!(reworded.supersedes.is_some());
    assert!(
        data.claims
            .iter()
            .any(|c| c.text == "Ran the on-call rotation for payments.")
    );
    assert!(data.skills.iter().any(|s| s.name == "Rust"));
    assert_eq!(data.documents.len(), 2);

    // Stored as it was computed, and the jobs side of the database works.
    let stored = store.load_profile(data.id()).await.unwrap().unwrap();
    assert_eq!(stored.claims.len(), data.claims.len());
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 0);
}
