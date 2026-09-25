//! Verification end to end, offline: a resume is imported, saved real
//! Ashby, Greenhouse and Lever responses are discovered into a temporary
//! database through a local mock server, and the `jobhunt` binary verifies
//! jobs against mock versions of their authoritative endpoints
//! (`JOBHUNT_VERIFY_ENDPOINT`), then shows, checks and filters them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use jobhunt_core::SourceKey;
use jobhunt_jobs::{Discovery, JobSource};
use jobhunt_sources::{
    AshbyBoard, AshbySource, GreenhouseBoard, GreenhouseSource, HttpClient, HttpSettings,
    LeverSite, LeverSource,
};
use jobhunt_storage::SqliteJobStore;
use wiremock::matchers::{path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn fixture(family: &str, name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../jobhunt-sources/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn resume() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../jobhunt-resume/tests/fixtures/marina_costa.pdf")
}

/// What the mock sources publish.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Board {
    /// The saved responses.
    Live,
    /// Every job taken down.
    Empty,
    /// Every endpoint failing.
    Down,
}

/// A mock of every endpoint discovery and verification use.
async fn sources(board: Board) -> MockServer {
    let server = MockServer::start().await;
    if board == Board::Down {
        Mock::given(path_regex(".*"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        return server;
    }
    let (linear, figma, spotify) = match board {
        Board::Live => (
            fixture("ashby", "linear.json"),
            fixture("greenhouse", "figma.json"),
            fixture("lever", "spotify.json"),
        ),
        _ => (
            r#"{"jobs":[]}"#.to_owned(),
            r#"{"jobs":[],"meta":{"total":0}}"#.to_owned(),
            "[]".to_owned(),
        ),
    };
    Mock::given(path("/posting-api/job-board/linear"))
        .respond_with(ResponseTemplate::new(200).set_body_string(linear))
        .mount(&server)
        .await;
    Mock::given(path("/v1/boards/figma/jobs"))
        .respond_with(ResponseTemplate::new(200).set_body_string(figma.clone()))
        .mount(&server)
        .await;
    Mock::given(path("/v0/postings/spotify"))
        .respond_with(ResponseTemplate::new(200).set_body_string(spotify.clone()))
        .mount(&server)
        .await;
    // Single jobs, looked up in the board.
    let figma_jobs: serde_json::Value = serde_json::from_str(&figma).unwrap();
    Mock::given(path_regex(r"^/v1/boards/figma/jobs/\d+$"))
        .respond_with(move |req: &Request| {
            let id = req
                .url
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            match figma_jobs["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|j| j["id"].as_u64().is_some_and(|n| n.to_string() == id))
            {
                Some(job) => ResponseTemplate::new(200).set_body_json(job),
                None => ResponseTemplate::new(404)
                    .set_body_string(r#"{"status":404,"error":"Job not found"}"#),
            }
        })
        .mount(&server)
        .await;
    let spotify_jobs: serde_json::Value = serde_json::from_str(&spotify).unwrap();
    Mock::given(path_regex(r"^/v0/postings/spotify/[0-9a-f-]+$"))
        .respond_with(move |req: &Request| {
            let id = req
                .url
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            match spotify_jobs
                .as_array()
                .unwrap()
                .iter()
                .find(|j| j["id"].as_str() == Some(id.as_str()))
            {
                Some(job) => ResponseTemplate::new(200).set_body_json(job),
                None => ResponseTemplate::new(404)
                    .set_body_string(r#"{"ok":false,"error":"Document not found"}"#),
            }
        })
        .mount(&server)
        .await;
    // Hosted pages with the application forms.
    let open = board == Board::Live;
    Mock::given(path_regex(r"^/(figma/jobs/\d+|spotify/[0-9a-f-]+/apply)$"))
        .respond_with(move |_: &Request| {
            ResponseTemplate::new(if open { 200 } else { 404 }).set_body_string("<form></form>")
        })
        .mount(&server)
        .await;
    server
}

async fn discover(db: &Path, server: &MockServer) {
    let http = HttpClient::new(HttpSettings {
        max_retries: 0,
        timeout: Duration::from_secs(5),
        ..HttpSettings::default()
    })
    .unwrap();
    let sources: Vec<Box<JobSource>> = vec![
        Box::new(
            AshbySource::with_api_base(
                SourceKey::new("ashby", "linear").unwrap(),
                AshbyBoard {
                    board: "linear".into(),
                    company: Some("Linear".into()),
                },
                http.clone(),
                &server.uri(),
            )
            .unwrap(),
        ),
        Box::new(
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
        ),
        Box::new(
            LeverSource::with_api_base(
                SourceKey::new("lever", "spotify").unwrap(),
                LeverSite {
                    site: "spotify".into(),
                    company: Some("Spotify".into()),
                    region: jobhunt_sources::LeverRegion::Global,
                },
                http,
                &server.uri(),
            )
            .unwrap(),
        ),
    ];
    let store = SqliteJobStore::open(db).await.unwrap();
    let report = Discovery::new(&store).run(&sources).await.unwrap();
    assert_eq!(report.succeeded(), 3);
    store.close().await;
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

    fn db(&self) -> PathBuf {
        self.dir.path().join("jobhunt.db")
    }

    fn run(&self, endpoint: Option<&MockServer>, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jobhunt"));
        command
            .arg("--config")
            .arg(self.dir.path().join("config.toml"))
            .arg("--database")
            .arg(self.db())
            .args(args)
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .env_remove("JOBHUNT_VERIFY_ENDPOINT");
        if let Some(server) = endpoint {
            command.env("JOBHUNT_VERIFY_ENDPOINT", server.uri());
        }
        command.output().unwrap()
    }

    fn ok(&self, endpoint: Option<&MockServer>, args: &[&str]) -> String {
        let output = self.run(endpoint, args);
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

/// The id of the listed job with exactly this title.
fn job_titled(text: &str, title: &str) -> String {
    let at = text
        .find(&format!(". {title}\n"))
        .unwrap_or_else(|| panic!("no job titled {title:?} in:\n{text}"));
    job_id(&text[at..])
}

fn has(out: &str, expected: &[&str]) {
    for e in expected {
        assert!(out.contains(e), "missing {e:?} in:\n{out}");
    }
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

#[tokio::test(flavor = "multi_thread")]
async fn init_verify_show_check_and_find() {
    let env = Env::new();
    env.ok(None, &["init", resume().to_str().unwrap()]);
    let live = sources(Board::Live).await;
    discover(&env.db(), &live).await;
    let linear = job_id(&env.ok(None, &["find", "--offline", "Fullstack"]));

    // Before any verification, show says so.
    let out = env.ok(None, &["show", &linear]);
    has(
        &out,
        &["Verification:", "not verified yet", "Never verified"],
    );

    // Verify: the Ashby board is read once, and the answer is explained.
    let before = requests(&live).await;
    let out = env.ok(Some(&live), &["verify", &linear]);
    assert_eq!(
        requests(&live).await,
        before + 1,
        "one request for one Ashby job"
    );
    has(
        &out,
        &[
            "Senior / Staff Fullstack Engineer — Linear",
            "Verification",
            "✓ First-party Ashby listing active",
            "✓ Application path active (published by the board's API)",
            "Verified just now",
            "Location",
            "Remote: Europe",
            "You: São Paulo, Brazil (from your resume)",
            "Employment",
            "Visa sponsorship: not stated",
            "Time zone: flexible hours",
            "Compensation",
            "Eligibility",
            "INELIGIBLE",
            "Why",
            "The listing limits remote work to Europe; you live in Brazil",
            "Conflicting information",
            "not legal advice",
        ],
    );

    // Persisted: show displays it without fetching.
    let before = requests(&live).await;
    let out = env.ok(None, &["show", &linear]);
    assert_eq!(requests(&live).await, before);
    has(
        &out,
        &[
            "✓ First-party Ashby listing active",
            "Verified just now",
            "✗ INELIGIBLE: The listing limits remote work to Europe",
            &format!("jobhunt check {linear}"),
        ],
    );

    // A profile change is a new decision; a recent verification is reused.
    env.ok(None, &["preferences", "set", "location", "Berlin, Germany"]);
    let before = requests(&live).await;
    let output = env.run(Some(&live), &["verify", &linear]);
    assert!(output.status.success());
    assert_eq!(requests(&live).await, before, "reused, not fetched again");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force asks again"));
    let out = String::from_utf8(output.stdout).unwrap();
    has(
        &out,
        &["ELIGIBLE", "Germany is within the listed Europe region"],
    );
    assert!(!out.contains("INELIGIBLE"), "{out}");

    // Details: provenance and evidence.
    let out = env.ok(Some(&live), &["verify", "--force", "--details", &linear]);
    has(
        &out,
        &[
            "ashby_board_api",
            "→ ats_api:",
            "“Europe” (ashby:linear locations)",
            "your location: Berlin, Germany (preference)",
            "“This role is open to candidates based in North America and Europe.” (ashby:linear description)",
        ],
    );
    // `check` is the same report from what is stored.
    let before = requests(&live).await;
    let out = env.ok(None, &["check", &linear]);
    assert_eq!(requests(&live).await, before);
    has(&out, &["ELIGIBLE", "“Europe” (ashby:linear locations)"]);

    // Greenhouse: the single-job endpoint and the hosted application page.
    let figma = job_titled(
        &env.ok(
            None,
            &[
                "find",
                "--offline",
                "-n",
                "50",
                "Account",
                "Executive",
                "Enterprise",
            ],
        ),
        "Account Executive, Enterprise",
    );
    let out = env.ok(Some(&live), &["verify", &figma]);
    has(
        &out,
        &[
            "✓ First-party Greenhouse listing active",
            "✓ Application path active (checked: HTTP 200)",
            "Limited to: the United States",
        ],
    );
    // Lever.
    let spotify = job_id(&env.ok(None, &["find", "--offline", "Audiobook"]));
    let out = env.ok(Some(&live), &["verify", &spotify]);
    has(
        &out,
        &[
            "✓ First-party Lever listing active",
            "✓ Application path active",
        ],
    );

    // The sources fail: the last success is kept, and the job is not
    // presented as verified.
    let down = sources(Board::Down).await;
    let out = env.ok(Some(&down), &["verify", "--force", &figma]);
    has(
        &out,
        &[
            "! Could not verify",
            "HTTP 503",
            "Last attempt just now failed; last successfully verified just now",
            "Not trusted enough to recommend",
        ],
    );

    // The job is taken down: verified closed, with its history kept.
    let gone = sources(Board::Empty).await;
    let out = env.ok(Some(&gone), &["verify", "--force", &linear]);
    has(
        &out,
        &[
            "✗ First-party Ashby listing closed",
            "missing from the board's complete listing",
            "Not trusted enough to recommend: the listing is closed",
        ],
    );
    let out = env.ok(Some(&gone), &["verify", "--force", &figma]);
    has(
        &out,
        &[
            "✗ First-party Greenhouse listing closed",
            "Greenhouse answered 404",
        ],
    );
    let out = env.ok(None, &["show", &linear]);
    has(
        &out,
        &[
            "✗ First-party Ashby listing closed",
            "Not trusted enough to recommend: the listing is closed",
            // Eligibility is a separate answer.
            "✓ ELIGIBLE: Germany is within the listed Europe region",
        ],
    );

    // find filters on the new decisions.
    let eligible = env.ok(None, &["find", "--offline", "--eligible", "-n", "100"]);
    assert!(!eligible.contains("INELIGIBLE"), "{eligible}");
    assert!(!eligible.contains("UNCLEAR"), "{eligible}");
    let possible = env.ok(None, &["find", "--offline", "--possible", "-n", "100"]);
    assert!(!possible.contains("INELIGIBLE"), "{possible}");
    let all = env.ok(None, &["find", "--offline", "-n", "100"]);
    has(&all, &["✗ INELIGIBLE", "? UNCLEAR"]);
}
