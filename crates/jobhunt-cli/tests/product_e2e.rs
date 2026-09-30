//! The terminal product, offline: first run, the shortlist and when it
//! reads the network, short ids, the whole-product export and import, and
//! `narrow doctor`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use serde_json::Value;

use common::{Env, requests, resume};

fn has(out: &str, expected: &[&str]) {
    for e in expected {
        assert!(out.contains(e), "missing {e:?} in:\n{out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn first_run_then_a_fast_repeated_shortlist() {
    let env = Env::new().await;

    // Nothing yet: no profile, no jobs. `find` reads the boards (they were
    // never read) and lists what it found, saying how to start.
    let out = env.ok(&["find"]);
    has(
        &out,
        &[
            "No profile found, so these are not personalized. Start with:\n  narrow init resume.pdf",
            "Checked 4 sources",
        ],
    );
    let stderr = env.fails(&["why", "opp_00000000"]);
    assert!(stderr.contains("no stored opportunity"), "{stderr}");
    assert!(env.fails(&["rank"]).contains("No profile found"));

    // The first-use flow.
    let out = env.ok(&["init", resume("marina_costa.pdf").to_str().unwrap()]);
    has(&out, &["Imported marina_costa.pdf"]);
    env.ok(&["preferences", "set", "location", "New", "York,", "NY"]);
    env.ok(&["preferences", "set", "authorized-in", "United", "States"]);
    let out = env.ok(&[
        "preferences",
        "add",
        "I want backend or infrastructure roles, at least USD 180k",
    ]);
    has(
        &out,
        &[
            "want   backend roles",
            "must   at least USD 180,000 per year",
        ],
    );

    // The stored jobs are fresh: no discovery request, only verification
    // of the best candidates.
    let before = requests(&env.server).await;
    let output = env.run(&["find"]);
    assert!(output.status.success());
    let out = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    has(
        &out,
        &[
            "Checked 11 open jobs",
            "passed basic eligibility",
            "looked plausible",
            "worth reviewing",
            "Member of Technical Staff - Systems — Modal   Worth reviewing",
            "✓ Verified open just now · ✓ Eligible",
            "Why this may be worth your time",
            "Used stored jobs (every source read just now",
            "--refresh reads the job boards now",
        ],
    );
    assert!(!out.contains('%'), "{out}");
    assert!(stderr.contains("Verifying the best candidates"), "{stderr}");
    assert!(!stderr.contains("Refreshing"), "{stderr}");
    // Greenhouse's listing is only read by discovery (its jobs verify
    // through their own endpoint): it was not read again.
    let received = env.server.received_requests().await.unwrap();
    assert!(
        received[before..]
            .iter()
            .all(|r| r.url.path() != "/v1/boards/figma/jobs"),
        "no listing was re-read"
    );
    let shown = out
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .count();
    assert!((1..=5).contains(&shown), "a handful of results: {shown}");

    // Asking again right away touches nothing at all.
    let before = requests(&env.server).await;
    let again = env.ok(&["find"]);
    assert_eq!(requests(&env.server).await, before, "no network");
    assert!(!again.contains("Verified 1 listing"), "{again}");

    // Short ids, as printed, work everywhere.
    let short = again
        .split_whitespace()
        .find(|w| w.starts_with("opp_") && w.len() == 12)
        .unwrap()
        .to_owned();
    has(
        &env.ok(&["why", &short]),
        &["Why it may be worth your time"],
    );
    has(&env.ok(&["save", &short]), &["Status: saved"]);

    // --refresh always reads; --offline never does; --raw is the inventory.
    let before = requests(&env.server).await;
    let out = env.ok(&["find", "--refresh"]);
    assert!(requests(&env.server).await > before);
    has(
        &out,
        &["Checked 4 sources in", "11 open jobs (0 new, 0 updated)"],
    );
    let before = requests(&env.server).await;
    let out = env.ok(&["find", "--offline", "backend"]);
    assert_eq!(requests(&env.server).await, before);
    has(
        &out,
        &["Offline: used stored jobs without refreshing or verifying them."],
    );
    let raw = env.ok(&["find", "--raw", "--offline", "-n", "3"]);
    has(&raw, &["Showing 3 of 11 matching jobs.", "ashby:"]);
    let json: Value = serde_json::from_str(&env.ok(&["find", "--offline", "--json"])).unwrap();
    assert!(json["results"].as_array().unwrap().len() <= 5);
    assert_eq!(json["funnel"]["checked"], 11);
    assert_eq!(json["not_shown"]["in_pipeline"], 0, "saved is not applied");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_board_does_not_hide_stored_jobs() {
    let env = Env::new().await;
    env.with_profile();
    env.ok(&["find", "--refresh"]);
    // Every refresh is due, and the boards are unreachable.
    let config = std::fs::read_to_string(env.config()).unwrap();
    std::fs::write(
        env.config(),
        config.replace("[discovery]\n", "[discovery]\nrefresh_after_hours = 0\n"),
    )
    .unwrap();
    let output = env
        .command()
        .env("JOBHUNT_DISCOVERY_ENDPOINT", "http://127.0.0.1:9")
        .args(["find", "--all"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("could not reach any source"), "{stderr}");
    assert!(stderr.contains("using stored jobs"), "{stderr}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Checked 11 open jobs"));
    // Asked for explicitly, the failure is an error.
    let output = env
        .command()
        .env("JOBHUNT_DISCOVERY_ENDPOINT", "http://127.0.0.1:9")
        .args(["find", "--refresh"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not reach any source"));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_jobs_yet_says_how_to_start() {
    let env = Env::new().await;
    env.with_profile();
    let stderr = env.fails(&["find", "--offline"]);
    assert!(
        stderr.contains("No discovered opportunities yet. Run:\n  narrow find --refresh"),
        "{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn export_and_import_move_everything_that_is_yours() {
    let env = Env::new().await;
    env.with_profile();
    env.ok(&[
        "preferences",
        "add",
        "I want backend or infrastructure roles",
    ]);
    let found: Value =
        serde_json::from_str(&env.ok(&["find", "--refresh", "--all", "-n", "25", "--json"]))
            .unwrap();
    let id = |title: &str| {
        found["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["title"] == title)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let security = id("Security Engineer, Cloud");
    let systems = id("Member of Technical Staff - Systems");
    let forward = id("Forward Deployed Engineer - ML");
    env.ok(&["save", &security, "--reason", "interesting infra problem"]);
    env.ok(&["applied", &systems]);
    env.ok(&[
        "reject",
        &forward,
        "--reason",
        "customer-facing, too corporate",
    ]);
    env.ok(&["show", &security]); // a "looked at" mark: not exported
    let claim = env.ok(&["claims", "--state", "review"]);
    let claim = claim
        .split_whitespace()
        .find(|w| w.starts_with("clm_"))
        .unwrap()
        .to_owned();
    env.ok(&["claims", "confirm", &claim]);

    let file = env.dir.path().join("state.json");
    env.ok(&["export", "-o", file.to_str().unwrap()]);
    let state: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(state["format"], "jobhunt.state");
    assert_eq!(state["version"], 1);
    assert_eq!(state["profile"]["format"], "jobhunt.profile");
    assert_eq!(state["feedback"].as_array().unwrap().len(), 3);
    assert_eq!(
        state["jobs"].as_array().unwrap().len(),
        3,
        "only the jobs acted on"
    );
    let text = state.to_string();
    assert!(!text.contains("\"seen\""));
    assert!(text.contains("customer-facing, too corporate"));
    // Stdout export is the same document.
    let stdout: Value = serde_json::from_str(&env.ok(&["export"])).unwrap();
    assert_eq!(stdout["feedback"], state["feedback"]);
    let pipeline = env.ok(&["pipeline", "--all"]);
    let taste = env.ok(&["taste"]);

    // A new machine: an empty database gets everything back.
    let other = Env::new().await;
    let out = other.ok(&["import", file.to_str().unwrap()]);
    has(
        &out,
        &[
            "Imported your profile, 3 pieces of feedback (0 already here) on 3 opportunities",
            "3 job records (0 already here)",
        ],
    );
    assert_eq!(other.ok(&["pipeline", "--all"]), pipeline);
    assert_eq!(other.ok(&["taste"]), taste);
    let claims = other.ok(&["claims", "show", &claim]);
    assert!(claims.contains("confirmed"), "{claims}");
    // Importing again changes nothing; a profile is never replaced
    // silently.
    let stderr = other.fails(&["import", file.to_str().unwrap()]);
    assert!(stderr.contains("--replace"), "{stderr}");
    let out = other.ok(&["import", file.to_str().unwrap(), "--replace"]);
    has(
        &out,
        &[
            "0 pieces of feedback (3 already here)",
            "0 job records (3 already here)",
        ],
    );
    assert_eq!(other.ok(&["pipeline", "--all"]), pipeline);
    // Discovery on the new machine finds the same jobs by the same ids, and
    // the feedback stays attached.
    other.ok(&["find", "--refresh", "--raw"]);
    let json: Value =
        serde_json::from_str(&other.ok(&["find", "--offline", "--all", "-n", "25", "--json"]))
            .unwrap();
    assert_eq!(json["not_shown"]["rejected"], 1);
    assert_eq!(json["not_shown"]["in_pipeline"], 1);

    // The profile file stays what it was, and the two are not confused.
    let profile = env.dir.path().join("profile.json");
    env.ok(&["profile", "export", "-o", profile.to_str().unwrap()]);
    let stderr = other.fails(&["import", profile.to_str().unwrap()]);
    assert!(stderr.contains("narrow profile import"), "{stderr}");
    let broken = env.dir.path().join("broken.json");
    std::fs::write(&broken, text.replace("\"version\":1", "\"version\":99")).unwrap();
    assert!(
        other
            .fails(&["import", broken.to_str().unwrap()])
            .contains("version 99")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn doctor_reports_the_setup_and_the_mcp_command() {
    let env = Env::new().await;
    let out = env.ok(&["doctor"]);
    has(
        &out,
        &[
            "Config: ",
            "config.toml",
            "Database: ",
            "migrations applied",
            "Profile: none yet (narrow init resume.pdf)",
            "Sources: 4 configured; 4 sources were never read",
            "MCP: `narrow mcp` serves this profile and database over stdio.",
            "args:    [\"mcp\",\"--config\"",
        ],
    );
    env.with_profile();
    env.ok(&["find", "--refresh", "--raw"]);
    let out = env.ok(&["doctor"]);
    has(
        &out,
        &[
            "Profile: 4 experiences, 77 claims",
            "Jobs: 11 stored jobs (11 open opportunities)",
            "Sources: 4 configured; every source read just now",
        ],
    );
}
