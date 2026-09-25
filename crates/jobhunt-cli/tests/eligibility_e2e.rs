//! Eligibility end to end: saved real Ashby and Greenhouse responses are
//! discovered into a temporary database through a local mock server, then
//! the `jobhunt` binary checks them against preferences set on the command
//! line.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use jobhunt_core::SourceKey;
use jobhunt_jobs::{Discovery, JobSource};
use jobhunt_sources::{
    AshbyBoard, AshbySource, GreenhouseBoard, GreenhouseSource, HttpClient, HttpSettings,
};
use jobhunt_storage::SqliteJobStore;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(family: &str, name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../jobhunt-sources/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

async fn discover(db: &Path) {
    let server = MockServer::start().await;
    for (route, body) in [
        (
            "/posting-api/job-board/linear",
            fixture("ashby", "linear.json"),
        ),
        ("/posting-api/job-board/ramp", fixture("ashby", "ramp.json")),
        ("/v1/boards/figma/jobs", fixture("greenhouse", "figma.json")),
    ] {
        Mock::given(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }
    let http = HttpClient::new(HttpSettings {
        max_retries: 0,
        timeout: Duration::from_secs(5),
        ..HttpSettings::default()
    })
    .unwrap();
    let ashby = |board: &str, company: &str| {
        Box::new(
            AshbySource::with_api_base(
                SourceKey::new("ashby", board).unwrap(),
                AshbyBoard {
                    board: board.to_owned(),
                    company: Some(company.to_owned()),
                },
                http.clone(),
                &server.uri(),
            )
            .unwrap(),
        ) as Box<JobSource>
    };
    let figma = Box::new(
        GreenhouseSource::with_api_base(
            SourceKey::new("greenhouse", "figma").unwrap(),
            GreenhouseBoard {
                board: "figma".into(),
                company: Some("Figma".into()),
            },
            http.clone(),
            &server.uri(),
        )
        .unwrap(),
    ) as Box<JobSource>;
    let sources = vec![ashby("linear", "Linear"), ashby("ramp", "Ramp"), figma];
    let store = SqliteJobStore::open(db).await.unwrap();
    let report = Discovery::new(&store).run(&sources).await.unwrap();
    assert_eq!(report.succeeded(), 3);
    store.close().await;
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        discover(&dir.path().join("jobhunt.db")).await;
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_jobhunt"))
            .arg("--config")
            .arg(self.path("config.toml"))
            .arg("--database")
            .arg(self.path("jobhunt.db"))
            .args(args)
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "jobhunt {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

fn job_id(text: &str) -> String {
    let at = text.find("job_").expect("a job id in the output");
    text[at..]
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn checks_jobs_against_the_profile() {
    let env = Env::new().await;

    // Without a profile there is nothing to check against.
    let out = env.ok(&["find", "--offline", "-n", "50"]);
    assert!(!out.contains("ELIGIBLE"), "{out}");
    let output = env.run(&["find", "--offline", "--eligible"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("need a career profile"));

    env.ok(&["preferences", "set", "location", "Berlin,", "Germany"]);
    env.ok(&[
        "preferences",
        "set",
        "work-mode",
        "remote",
        "--stance",
        "require",
    ]);

    let out = env.ok(&["find", "--offline", "-n", "50"]);
    for expected in [
        "✓ ELIGIBLE: Germany is within the listed Europe region",
        "✗ INELIGIBLE: The listing limits remote work to Canada or the United States; you live in Germany",
        "✗ INELIGIBLE: This option is hybrid; you require remote work",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    let eligible = env.ok(&["find", "--offline", "--eligible", "-n", "50"]);
    assert!(eligible.contains("Germany is within"), "{eligible}");
    assert!(!eligible.contains("✗"), "{eligible}");
    assert!(!eligible.contains("UNCLEAR"), "{eligible}");

    // One job in full: every answer with the posting's words, from what is
    // stored (not verified yet, so not recommended).
    let listed = env.ok(&["find", "--offline", "Fullstack"]);
    let id = job_id(&listed);
    let out = env.ok(&["check", &id]);
    for expected in [
        "Senior / Staff Fullstack Engineer — Linear",
        "? First-party Ashby listing: not verified yet",
        "Remote: Europe",
        "ELIGIBLE",
        "Not trusted enough to recommend: the listing has not been verified",
        "✓ Germany is within the listed Europe region",
        "“Europe” (ashby:linear locations)",
        "your location: Berlin, Germany (preference)",
        "✓ The position is remote, as you require",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    let out = env.ok(&["show", &id]);
    assert!(out.contains("Eligibility:"), "{out}");
    assert!(out.contains(&format!("jobhunt check {id}")), "{out}");

    // Living elsewhere changes the answer, with the reason.
    env.ok(&["preferences", "set", "location", "São Paulo, Brazil"]);
    let out = env.ok(&["check", &id]);
    assert!(out.contains("INELIGIBLE"), "{out}");
    assert!(
        out.contains("The listing limits remote work to Europe; you live in Brazil"),
        "{out}"
    );
    // North America is in the description but not the listing: unclear,
    // with both statements.
    env.ok(&["preferences", "set", "location", "Toronto"]);
    let out = env.ok(&["check", &id]);
    assert!(out.contains("UNCLEAR"), "{out}");
    assert!(
        out.contains("The location fields exclude Canada but the description includes it"),
        "{out}"
    );
    assert!(
        out.contains("“This role is open to candidates based in North America and Europe.” (ashby:linear description)"),
        "{out}"
    );
}
