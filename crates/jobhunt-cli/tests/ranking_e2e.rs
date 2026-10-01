//! Ranking end to end, offline: a resume is imported, saved real Ashby
//! and Greenhouse responses are discovered into a temporary database
//! through a local mock server, some jobs are verified against it
//! (`JOBHUNT_VERIFY_ENDPOINT`), and the `narrow` binary ranks them, takes
//! feedback, learns from it, and explains itself.

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

fn resume() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../jobhunt-resume/tests/fixtures/marina_costa.pdf")
}

const BOARDS: [(&str, &str); 3] = [("linear", "Linear"), ("ramp", "Ramp"), ("modal", "Modal")];

/// Discovery and verification endpoints: Ashby verifies by reading the
/// board, so the board routes serve both.
async fn server() -> MockServer {
    let server = MockServer::start().await;
    for (board, _) in BOARDS {
        Mock::given(path(format!("/posting-api/job-board/{board}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture("ashby", &format!("{board}.json"))),
            )
            .mount(&server)
            .await;
    }
    Mock::given(path("/v1/boards/figma/jobs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("greenhouse", "figma.json")),
        )
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
    let mut sources: Vec<Box<JobSource>> = BOARDS
        .iter()
        .map(|(board, company)| {
            Box::new(
                AshbySource::with_api_base(
                    SourceKey::new("ashby", board).unwrap(),
                    AshbyBoard {
                        board: (*board).to_owned(),
                        company: Some((*company).to_owned()),
                    },
                    http.clone(),
                    &server.uri(),
                )
                .unwrap(),
            ) as Box<JobSource>
        })
        .collect();
    sources.push(Box::new(
        GreenhouseSource::with_api_base(
            SourceKey::new("greenhouse", "figma").unwrap(),
            GreenhouseBoard {
                board: "figma".into(),
                company: Some("Figma".into()),
            },
            http,
            &server.uri(),
        )
        .unwrap(),
    ));
    let store = SqliteJobStore::open(db).await.unwrap();
    let report = Discovery::new(&store).run(&sources).await.unwrap();
    assert_eq!(report.succeeded(), 4);
    store.close().await;
}

struct Env {
    dir: tempfile::TempDir,
    server: MockServer,
}

impl Env {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        let server = server().await;
        discover(&dir.path().join("jobhunt.db"), &server).await;
        Self { dir, server }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_narrow"))
            .arg("--config")
            .arg(self.dir.path().join("config.toml"))
            .arg("--database")
            .arg(self.dir.path().join("jobhunt.db"))
            .args(args)
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .env("JOBHUNT_VERIFY_ENDPOINT", self.server.uri())
            .output()
            .unwrap()
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

    /// The id of the stored job with exactly this title.
    fn id_of(&self, title: &str) -> String {
        let out = self.ok(&["find", "--raw", "--offline", "-n", "100"]);
        let at = out
            .find(&format!(". {title}\n"))
            .unwrap_or_else(|| panic!("no job titled {title:?} in:\n{out}"));
        let rest = &out[at..];
        let at = rest.find("job_").unwrap();
        rest[at..]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap()
            .to_owned()
    }
}

fn has(out: &str, expected: &[&str]) {
    for e in expected {
        assert!(out.contains(e), "missing {e:?} in:\n{out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ranks_learns_and_explains() {
    let env = Env::new().await;

    // Without a profile there is nothing to rank against: `find` lists the
    // inventory and says how to start; the stored-only shortlist can't.
    let out = env.ok(&["find", "--offline"]);
    has(
        &out,
        &["No profile found, so these are not personalized. Start with:\n  narrow init resume.pdf"],
    );
    let output = env.run(&["rank"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No profile found"));

    env.ok(&["init", resume().to_str().unwrap()]);
    env.ok(&["preferences", "set", "location", "New", "York,", "NY"]);
    env.ok(&["preferences", "set", "authorized-in", "United", "States"]);
    env.ok(&["preferences", "set", "role", "backend"]);
    env.ok(&["preferences", "set", "role", "infrastructure"]);
    env.ok(&["preferences", "set", "role", "sales", "--stance", "avoid"]);
    env.ok(&[
        "preferences",
        "set",
        "compensation",
        "--minimum",
        "180k",
        "--target",
        "250k",
        "--currency",
        "USD",
    ]);

    let security = env.id_of("Security Engineer, Cloud");
    let mobile = env.id_of("Mobile Engineer, Android");
    let account = env.id_of("Account Manager | Commercial");
    let systems = env.id_of("Member of Technical Staff - Systems");
    env.ok(&["verify", &security]);
    env.ok(&["verify", &systems]);

    // A small number of jobs, grouped by how far they can be trusted,
    // each with a short brief. No percentages.
    let out = env.ok(&["rank"]);
    has(
        &out,
        &[
            "Checked 11 open jobs",
            "passed basic eligibility",
            "3 are worth reviewing",
            // Only the work is known to fit what they said (backend or
            // infrastructure roles): worth reviewing, not strong fits.
            "Member of Technical Staff - Systems — Modal   Worth reviewing",
            "Security Engineer, Cloud — Ramp   Worth reviewing",
            "✓ Verified open just now · ✓ Eligible:",
            "Why this may be worth your time",
            "+ Touches infrastructure (the team: “Cloud”), close to the infrastructure work you want",
            "+ Senior level, matching your latest title",
            "maybe or low priority (--all)",
            "Offline: used stored jobs without refreshing or verifying them.",
            "Tiers are coarse on purpose",
        ],
    );
    assert!(!out.contains('%'), "{out}");
    // Pay meeting the minimum or target is a fact, never a reason.
    assert!(!out.contains("+ Meets your minimum"), "{out}");
    assert!(!out.contains("+ Reaches your target"), "{out}");
    assert!(!out.contains("Account Manager"), "{out}");
    assert_eq!(out.matches(" · narrow why opp_").count(), 2, "{out}");

    // Everything else is still inspectable, in its place.
    let all = env.ok(&["rank", "--all", "-n", "25"]);
    has(
        &all,
        &[
            "? Not verified yet",
            "? Eligibility unclear: Requires hybrid presence in London",
            "- Account Manager | Commercial: sales, while your experience is in engineering",
            "Account Manager | Commercial — Ramp   Low priority",
            "- Customer-facing engineering (solutions, forward-deployed), not product or platform engineering",
            " · narrow verify opp_",
            " · narrow check opp_",
        ],
    );
    let first_unverified = all.find("? Not verified yet").unwrap();
    assert!(all.find("Security Engineer, Cloud").unwrap() < first_unverified);

    // The brief: why, caveats, unknowns, and every signal with evidence.
    let out = env.ok(&["why", &security, "--details"]);
    has(
        &out,
        &[
            "Security Engineer, Cloud — Ramp",
            "WORTH REVIEWING",
            "Worth a look: some of it fits what you want, not enough to recommend it.",
            "infrastructure, security · AWS, Terraform",
            "USD 211,400 – 290,600 per year · hybrid",
            "Why it may be worth your time",
            "+ Touches infrastructure (the team: “Cloud”), close to the infrastructure work you want",
            "How the fit was assessed",
            "Fit plausible: the work 0.50, the rest 0.60 · fit-rules/2 · not reviewed by a model",
            "Practicality",
            "• Reaches your target of USD 250,000 per year at the top of the range",
            "Every signal",
            "Asks for AWS and Terraform, which you've used",
            "verified at ashby:ramp just now",
            "your minimum: at least USD 180,000 per year",
            // The day count depends on today's date (the fixture was
            // posted on 2026-04-07); the wording is what matters.
            "days ago, and still listed when verified",
            "it is not a match percentage",
        ],
    );
    assert!(
        out.contains("Posted 1") || out.contains("Posted 2"),
        "{out}"
    );
    let unverified = env.ok(&["why", &mobile]);
    has(
        &unverified,
        &[
            "MAYBE",
            "but verify it first: the listing has not been verified",
            "Things to check",
            "? Not verified yet: it may be closed or changed",
            "Every signal with its evidence: narrow why --details",
        ],
    );

    // Feedback, with reasons kept verbatim and read where possible.
    let out = env.ok(&["reject", &account, "--reason", "sales, too corporate"]);
    has(
        &out,
        &[
            "Rejected Account Manager | Commercial — Ramp",
            "Reason: “sales, too corporate”",
            "Read as: avoid role: sales; avoid company: large companies",
            "Status: rejected (was unseen)",
        ],
    );
    let out = env.ok(&["reject", &mobile, "--reason", "meh vibes"]);
    has(
        &out,
        &[
            "Reason: “meh vibes”",
            "Kept as written: JobHunt didn't recognize anything to learn from in it.",
            "Status: rejected (was seen)",
        ],
    );
    let out = env.ok(&["save", &security, "--reason", "interesting infra problem"]);
    has(
        &out,
        &[
            "Read as: prefer product: the product or problem (this job only); prefer domain: infrastructure",
            "Status: saved (was seen)",
        ],
    );
    env.ok(&["applied", &systems]);
    let out = env.ok(&["like", &systems, "--reason", "love tiny founder-led teams"]);
    has(&out, &["Status: applied, liked (was applied)"]);

    // What was learned, with its evidence; what couldn't be read, as written.
    let out = env.ok(&["taste"]);
    has(
        &out,
        &[
            "What you told JobHunt (this always wins)",
            "• wanted backend roles",
            "• unwanted sales roles",
            "• required at least USD 180,000 per year",
            "What JobHunt learned from your feedback (5 actions on 4 jobs)",
            "↓ Company: large companies — tentative",
            "for: “sales, too corporate” — rejected Account Manager | Commercial at Ramp",
            "↑ Company: founder-led companies — tentative",
            "↑ Domain: infrastructure — tentative",
            "Covered by what you said",
            "↓ Role: sales — you said “sales roles”, which your feedback agrees with",
            "About single jobs (not generalized)",
            "Kept as written (nothing recognized)",
            "• “meh vibes” — rejected Mobile Engineer, Android at Ramp",
            "with too little evidence to use yet (--all lists them)",
        ],
    );
    let all = env.ok(&["taste", "--all"]);
    has(
        &all,
        &[
            "Not enough evidence yet",
            "↓ Role: mobile — not enough evidence yet",
        ],
    );

    // The pipeline and the log.
    let out = env.ok(&["pipeline"]);
    has(
        &out,
        &[
            "Applied\n  Member of Technical Staff - Systems — Modal",
            "· liked ·",
            "“love tiny founder-led teams”",
            "Saved\n  Security Engineer, Cloud — Ramp",
        ],
    );
    assert!(!out.contains("Rejected"), "{out}");
    assert!(env.ok(&["pipeline", "--all"]).contains("Rejected\n"));
    let out = env.ok(&["feedback"]);
    assert_eq!(out.matches("  “").count(), 4, "{out}");
    assert!(!out.contains(" seen "), "looking is not feedback:\n{out}");
    let out = env.ok(&["feedback", &account]);
    has(
        &out,
        &["reject     Account Manager | Commercial — Ramp  “sales, too corporate”"],
    );

    // Rejected and applied jobs leave the recommendations; learned taste
    // shows up, attributed, in others.
    let out = env.ok(&["rank", "--all"]);
    assert!(!out.contains("Account Manager | Commercial"), "{out}");
    assert!(!out.contains("Member of Technical Staff"), "{out}");
    has(
        &out,
        &[
            "Security Engineer, Cloud — Ramp   Worth reviewing",
            "+ You saved it",
            "2 you rejected",
            "1 in your pipeline",
        ],
    );

    // show carries the verdict and the person's status.
    let out = env.ok(&["show", &security]);
    has(
        &out,
        &[
            "Fit:\n  Worth reviewing: Worth a look: some of it fits what you want",
            "Your status: saved since",
        ],
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pipeline_follows_each_step() {
    let env = Env::new().await;
    env.ok(&["preferences", "set", "location", "New", "York,", "NY"]);
    let systems = env.id_of("Member of Technical Staff - Systems");
    let out = env.ok(&["save", &systems]);
    has(
        &out,
        &[
            "Saved Member of Technical Staff - Systems — Modal",
            "Status: saved (was unseen)",
        ],
    );
    let out = env.ok(&["unsave", &systems]);
    has(&out, &["Status: seen (was saved)"]);
    env.ok(&["applied", &systems]);
    let out = env.ok(&["interview", &systems, "--reason", "great team"]);
    has(&out, &["Status: interviewing (was applied)"]);
    let out = env.ok(&["offer", &systems]);
    has(&out, &["Status: offer (was interviewing)"]);
    let out = env.ok(&["dislike", &systems, "--reason", "too much on-call"]);
    has(
        &out,
        &[
            "Read as: avoid work style: on-call duty",
            "Status: offer, disliked (was offer)",
        ],
    );
    let out = env.ok(&["pipeline"]);
    has(
        &out,
        &[
            "Offers\n  Member of Technical Staff - Systems — Modal",
            "· disliked ·",
        ],
    );
    // Rejecting after applying withdraws, and it stays inspectable.
    let out = env.ok(&["reject", &systems]);
    has(&out, &["Status: rejected, disliked (was offer, disliked)"]);
    assert!(
        env.ok(&["pipeline"])
            .starts_with("Nothing in your pipeline yet")
    );
    let out = env.ok(&["why", &systems]);
    has(
        &out,
        &[
            "NOT RECOMMENDED",
            "Not recommended: you rejected it.",
            "You rejected it",
        ],
    );
    // Unknown ids are errors, not feedback.
    let output = env.run(&["save", "job_00000000000000000000000000000000"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no stored opportunity or job has the id")
    );
}
