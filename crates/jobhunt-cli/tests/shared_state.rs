//! The CLI and the MCP server are two interfaces to one local product: the
//! same configuration, the same SQLite database, the same use cases. These
//! tests mix them (while the server runs), compare what they answer, retry
//! mutations the way clients do, and run requests and processes
//! concurrently against one database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use jobhunt_profile::ProfileService;
use jobhunt_ranking::{FeedbackAction, FeedbackRepository};
use jobhunt_storage::SqliteJobStore;
use serde_json::{Value, json};

use common::{Env, result, result_ids};

const SECURITY: &str = "Security Engineer, Cloud";
const SYSTEMS: &str = "Member of Technical Staff - Systems";
const FORWARD: &str = "Forward Deployed Engineer - ML";
const MOBILE: &str = "Mobile Engineer, Android";

async fn feedback_events(env: &Env) -> Vec<jobhunt_ranking::FeedbackEvent> {
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let profile = ProfileService::new(&store).profile_id().to_string();
    let events = store.feedback(&profile).await.unwrap();
    store.close().await;
    events
        .into_iter()
        .filter(|e| e.action != FeedbackAction::Seen)
        .collect()
}

/// The ids of every result of a shortlist, by title.
fn ids_by_title(search: &Value) -> Vec<(String, String)> {
    search["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["title"].as_str().unwrap().to_owned(),
                r["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn id_of(search: &Value, title: &str) -> String {
    result(search, title)["id"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_and_mcp_share_one_database() {
    let env = Env::new().await;
    // 1. The profile, and a first search, from the terminal.
    env.with_profile();
    env.ok(&[
        "preferences",
        "add",
        "I want backend or infrastructure roles, at least USD 180k",
    ]);
    let listed = env.ok(&["find", "--refresh", "--all", "-n", "25"]);
    assert!(listed.contains("Checked 4 sources"), "{listed}");
    let found: Value =
        serde_json::from_str(&env.ok(&["find", "--offline", "--all", "-n", "25", "--json"]))
            .unwrap();
    let security = id_of(&found, SECURITY);
    let forward = id_of(&found, FORWARD);
    let mobile = id_of(&found, MOBILE);

    // 2. A save from an MCP client…
    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    let saved = mcp.call("save_job", json!({"id": security})).await;
    assert_eq!(saved["state"]["stage"], "saved");

    // 3. …is in the terminal's pipeline, while the server is running.
    let pipeline = env.ok(&["pipeline"]);
    assert!(
        pipeline.contains(&format!("Saved\n  {SECURITY} — Ramp")),
        "{pipeline}"
    );

    // 4. A rejection in the terminal, by short id…
    let short = &forward[..12];
    let out = env.ok(&["reject", short, "--reason", "too corporate"]);
    assert!(
        out.contains("Read as: avoid company: large companies"),
        "{out}"
    );

    // 5. …is what the MCP client's next search reflects.
    let search = mcp
        .call(
            "search_jobs",
            json!({"refresh": "never", "include_lower_tiers": true, "limit": 25}),
        )
        .await;
    let ids = result_ids(&search);
    assert!(!ids.contains(&forward), "rejected in the CLI");
    assert!(ids.contains(&security));
    assert_eq!(search["not_shown"]["rejected"], 1);
    assert_eq!(search["learning"]["feedback"], 2);
    let job = mcp.call("get_job", json!({"id": forward})).await;
    assert_eq!(job["pipeline"]["stage"], "rejected");
    assert_eq!(job["pipeline"]["feedback"][0]["reason"], "too corporate");
    let pipeline = mcp
        .call("get_pipeline", json!({"include_rejected": true}))
        .await;
    let stages: Vec<(&str, &str)> = pipeline["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["title"].as_str().unwrap(), e["stage"].as_str().unwrap()))
        .collect();
    assert_eq!(stages, [(SECURITY, "saved"), (FORWARD, "rejected")]);

    // The same question gets the same answer from both interfaces.
    let cli: Value =
        serde_json::from_str(&env.ok(&["find", "--offline", "--all", "-n", "25", "--json"]))
            .unwrap();
    assert_eq!(cli, search, "find --json and search_jobs agree");
    // (`show` records that the person looked at it, after answering;
    // `get_job` never does.)
    let mcp_job = mcp
        .call(
            "get_job",
            json!({"id": mobile, "include_sources": true, "full_description": true}),
        )
        .await;
    let cli: Value = serde_json::from_str(&env.ok(&["show", &mobile, "--json"])).unwrap();
    assert_eq!(cli, mcp_job, "show --json and get_job agree");
    let seen = mcp.call("get_job", json!({"id": mobile})).await;
    assert_eq!(seen["pipeline"]["stage"], "seen");
    let cli: Value = serde_json::from_str(&env.ok(&["pipeline", "--all", "--json"])).unwrap();
    assert_eq!(cli, pipeline, "pipeline --json and get_pipeline agree");

    // Rejecting with the same words through either interface leads to the
    // same domain state and the same reading.
    let systems = id_of(&found, SYSTEMS);
    let via_mcp = mcp
        .call(
            "reject_job",
            json!({"id": systems, "reason": "too corporate"}),
        )
        .await;
    let a = mcp.call("get_job", json!({"id": forward})).await;
    let b = mcp.call("get_job", json!({"id": systems})).await;
    for field in ["stage", "furthest", "sentiment"] {
        assert_eq!(a["pipeline"][field], b["pipeline"][field], "{field}");
    }
    assert_eq!(
        a["pipeline"]["feedback"][0]["reason"],
        b["pipeline"]["feedback"][0]["reason"]
    );
    assert_eq!(
        via_mcp["interpretation"]["read_as"],
        json!(["avoid company: large companies"])
    );
    let events = feedback_events(&env).await;
    let rejections: Vec<_> = events
        .iter()
        .filter(|e| e.action == FeedbackAction::Reject)
        .collect();
    assert_eq!(rejections.len(), 2);
    assert!(
        rejections
            .iter()
            .all(|e| e.reason.as_deref() == Some("too corporate"))
    );
    // Both are about opportunities: one event each, on the logical
    // opportunity, with the source record kept for provenance.
    assert_eq!(rejections[0].opportunity.to_string(), forward);
    assert_eq!(rejections[1].opportunity.to_string(), systems);

    // Verification agrees too (a fresh attempt each time).
    let cli: Value =
        serde_json::from_str(&env.ok(&["verify", &security, "--force", "--json"])).unwrap();
    let via_mcp = mcp
        .call("verify_job", json!({"id": security, "force": true}))
        .await;
    for field in [
        "listing",
        "application",
        "authority",
        "recommendable",
        "eligibility",
        "compensation",
    ] {
        let mut a = cli[field].clone();
        let mut b = via_mcp[field].clone();
        for v in [&mut a, &mut b] {
            if let Some(o) = v.as_object_mut() {
                o.remove("verified_at");
            }
        }
        assert_eq!(a, b, "{field}");
    }

    // Preferences set through MCP are the terminal's preferences.
    mcp.call(
        "update_preferences",
        json!({"set": [{"kind": "work_mode", "mode": "remote", "stance": "require"}]}),
    )
    .await;
    assert!(env.ok(&["preferences"]).contains("remote"));
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn retries_do_not_duplicate_state() {
    let env = Env::new().await;
    env.with_profile();
    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    let found = mcp
        .call(
            "search_jobs",
            json!({"refresh": "always", "include_lower_tiers": true, "limit": 25}),
        )
        .await;
    let security = id_of(&found, SECURITY);
    let forward = id_of(&found, FORWARD);
    let systems = id_of(&found, SYSTEMS);

    // Saving twice: one event; the retry says nothing changed.
    let first = mcp.call("save_job", json!({"id": security})).await;
    let again = mcp.call("save_job", json!({"id": security})).await;
    assert_eq!(
        (first["recorded"].clone(), again["recorded"].clone()),
        (json!(true), json!(false))
    );
    assert_eq!(first["feedback_id"], again["feedback_id"]);
    assert_eq!(again["state"]["stage"], "saved");
    assert_eq!(again["note"], "Already recorded: nothing changed.");

    // Applying twice.
    let first = mcp.call("mark_applied", json!({"id": systems})).await;
    let again = mcp.call("mark_applied", json!({"id": systems})).await;
    assert_eq!(first["recorded"], true);
    assert_eq!(again["recorded"], false);
    assert_eq!(again["state"]["stage"], "applied");

    // The same rejection twice; a new reason is new information.
    let reason = json!({"id": forward, "reason": "customer-facing"});
    assert_eq!(
        mcp.call("reject_job", reason.clone()).await["recorded"],
        true
    );
    let again = mcp.call("reject_job", reason).await;
    assert_eq!(again["recorded"], false);
    assert_eq!(again["taste_changed"], false);
    let more = mcp
        .call(
            "reject_job",
            json!({"id": forward, "reason": "too much travel"}),
        )
        .await;
    assert_eq!(more["recorded"], true);

    // The CLI treats a repeat the same way.
    let out = env.ok(&["save", &security]);
    assert!(out.starts_with("Already saved"), "{out}");
    let out = env.ok(&["applied", &systems]);
    assert!(out.starts_with("Already applied to"), "{out}");

    let events = feedback_events(&env).await;
    let count = |action| events.iter().filter(|e| e.action == action).count();
    assert_eq!(count(FeedbackAction::Save), 1);
    assert_eq!(count(FeedbackAction::Applied), 1);
    assert_eq!(count(FeedbackAction::Reject), 2);

    // Preference updates, retried.
    let update = json!({
        "statement": "I want backend roles, at least USD 150k",
        "set": [{"kind": "company", "company": "small-team"}]
    });
    let first = mcp.call("update_preferences", update.clone()).await;
    assert_eq!(first["unchanged"], false);
    let again = mcp.call("update_preferences", update).await;
    assert_eq!(again["unchanged"], true);
    assert_eq!(first["active"], again["active"]);
    let out = env.ok(&[
        "preferences",
        "add",
        "I want backend roles, at least USD 150k",
    ]);
    assert!(out.starts_with("Already saved"), "{out}");
    let out = env.ok(&["preferences", "set", "company", "small-team"]);
    assert!(out.starts_with("Already set"), "{out}");
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let data = ProfileService::new(&store).require().await.unwrap();
    store.close().await;
    assert_eq!(
        data.statements.len(),
        1,
        "one statement, however often it was sent"
    );

    // Concurrent identical saves from one client: still one event.
    let mobile = id_of(&found, MOBILE);
    let mut ids = Vec::new();
    for _ in 0..6 {
        ids.push(
            mcp.submit(
                "tools/call",
                json!({"name": "save_job", "arguments": {"id": mobile}}),
            )
            .await,
        );
    }
    let mut recorded = 0;
    for id in ids {
        let response = mcp.response(id).await;
        assert_eq!(response["result"]["isError"], false, "{response}");
        if response["result"]["structuredContent"]["recorded"] == true {
            recorded += 1;
        }
    }
    assert_eq!(recorded, 1);
    mcp.close().await;
    let events = feedback_events(&env).await;
    assert_eq!(
        events
            .iter()
            .filter(|e| e.action == FeedbackAction::Save && e.opportunity.to_string() == mobile)
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_and_processes_share_the_database_safely() {
    let env = Env::new().await;
    env.with_profile();
    let found: Value =
        serde_json::from_str(&env.ok(&["find", "--refresh", "--all", "-n", "25", "--json"]))
            .unwrap();
    let all = ids_by_title(&found);
    assert!(all.len() >= 8, "{all:?}");

    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    // Many requests in flight at once, reads and writes mixed…
    let mut pending = Vec::new();
    for (i, (_, id)) in all.iter().take(6).enumerate() {
        let (name, arguments) = match i % 3 {
            0 => ("save_job", json!({"id": id})),
            1 => ("reject_job", json!({"id": id, "reason": "not for me"})),
            _ => ("verify_job", json!({"id": id, "force": true})),
        };
        pending.push(
            mcp.submit("tools/call", json!({"name": name, "arguments": arguments}))
                .await,
        );
        pending.push(
            mcp.submit(
                "tools/call",
                json!({"name": "search_jobs", "arguments": {"refresh": "never"}}),
            )
            .await,
        );
    }
    pending.push(
        mcp.submit(
            "tools/call",
            json!({"name": "update_preferences", "arguments": {"statement": "I want remote work"}}),
        )
        .await,
    );
    // …while other processes use the same database from the terminal.
    let (_, cli_target) = &all[7];
    let handles: Vec<std::thread::JoinHandle<std::process::Output>> = [
        vec!["like".to_owned(), cli_target.clone()],
        vec![
            "preferences".into(),
            "add".into(),
            "I want backend roles".into(),
        ],
        vec!["find".into(), "--offline".into(), "--json".into()],
        vec!["pipeline".into()],
        vec!["doctor".into()],
    ]
    .into_iter()
    .map(|args| {
        let mut command = env.command();
        command.args(&args);
        std::thread::spawn(move || command.output().unwrap())
    })
    .collect();
    for id in pending {
        let response = mcp.response(id).await;
        assert!(
            response.get("error").is_none() && response["result"]["isError"] == false,
            "{response}\n{}",
            mcp.stderr_text()
        );
    }
    for handle in handles {
        let output = handle.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let pipeline = mcp
        .call("get_pipeline", json!({"include_rejected": true}))
        .await;
    assert_eq!(pipeline["entries"].as_array().unwrap().len(), 4);
    let prefs = mcp.call("get_profile", json!({})).await;
    let values: Vec<&str> = prefs["preferences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_str().unwrap())
        .collect();
    assert!(
        values.contains(&"remote work") && values.contains(&"backend roles"),
        "{values:?}"
    );
    mcp.close().await;
    let events = feedback_events(&env).await;
    assert_eq!(events.len(), 5, "4 from MCP, 1 from the CLI");
}

#[tokio::test(flavor = "multi_thread")]
async fn processes_opening_a_new_database_at_once_all_succeed() {
    let env = Env::new().await;
    // Several processes create and migrate the same new database file.
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let mut command = env.command();
            command.arg("doctor");
            std::thread::spawn(move || command.output().unwrap())
        })
        .collect();
    for handle in handles {
        let output = handle.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let out = String::from_utf8(output.stdout).unwrap();
        assert!(out.contains("migrations applied"), "{out}");
    }
}
