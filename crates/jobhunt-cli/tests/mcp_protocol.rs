//! `jobhunt mcp` at the protocol level: the binary is started as a child
//! process, exactly as an MCP client starts it, and spoken to over
//! stdin/stdout with newline-delimited JSON-RPC. Every line it writes to
//! stdout must be a protocol message, whatever it logs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::process::Stdio;

use chrono::Utc;
use jobhunt_core::{CanonicalUrl, Provenance, SourceKey};
use jobhunt_jobs::JobPosting;
use jobhunt_jobs::verification::VerificationRepository;
use jobhunt_storage::SqliteJobStore;
use serde_json::{Value, json};

use common::{Env, McpClient};

const READ_TOOLS: [&str; 5] = [
    "get_job",
    "get_pipeline",
    "get_profile",
    "get_taste",
    "prepare_application_context",
];
const MUTATING_TOOLS: [&str; 5] = [
    "mark_applied",
    "record_feedback",
    "reject_job",
    "save_job",
    "update_preferences",
];
const NETWORK_TOOLS: [&str; 3] = ["get_feed", "search_jobs", "verify_job"];

#[tokio::test(flavor = "multi_thread")]
async fn handshake_tools_errors_and_clean_shutdown_with_logs_on_stderr() {
    let env = Env::new().await;
    // The noisiest logging there is: none of it may reach stdout.
    let mut mcp = McpClient::start(&env, &["-vvv", "--log-format", "json"]).await;

    let init = mcp.initialize().await;
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "jobhunt");
    assert!(init["capabilities"]["tools"].is_object());
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("search_jobs")
    );

    // A ping is answered.
    let pong = mcp.request("ping", json!({})).await;
    assert_eq!(pong["result"], json!({}));

    // Every tool, typed and described, with its read/write nature declared.
    let tools = mcp.request("tools/list", json!({})).await;
    let tools = tools["result"]["tools"].as_array().unwrap().clone();
    let names: BTreeSet<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let expected: BTreeSet<&str> = READ_TOOLS
        .iter()
        .chain(&MUTATING_TOOLS)
        .chain(&NETWORK_TOOLS)
        .copied()
        .collect();
    assert_eq!(names, expected);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            tool["description"].as_str().unwrap().len() > 80,
            "{name} needs a real description"
        );
        let input = &tool["inputSchema"];
        assert_eq!(input["type"], "object", "{name}");
        if input["properties"]
            .as_object()
            .is_some_and(|p| !p.is_empty())
        {
            assert_eq!(
                input["additionalProperties"], false,
                "{name} must reject unknown arguments"
            );
        }
        assert_eq!(
            tool["outputSchema"]["type"], "object",
            "{name} returns structured content"
        );
        let hints = &tool["annotations"];
        let read_only = READ_TOOLS.contains(&name);
        assert_eq!(hints["readOnlyHint"], read_only, "{name}");
        if !read_only {
            assert_eq!(hints["destructiveHint"], false, "{name}");
            assert_eq!(hints["idempotentHint"], true, "{name}");
        }
        assert_eq!(
            hints["openWorldHint"],
            NETWORK_TOOLS.contains(&name),
            "{name}"
        );
    }
    // Only standard JSON Schema: no tool-specific numeric formats.
    let schemas = serde_json::to_string(&tools).unwrap();
    for format in ["\"uint", "\"int64", "\"double"] {
        assert!(
            !schemas.contains(&format!("\"format\":{format}")),
            "{format}"
        );
    }
    let search = tools.iter().find(|t| t["name"] == "search_jobs").unwrap();
    assert_eq!(
        search["inputSchema"]["$defs"]["RefreshInput"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["const"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["auto", "always", "never"]
    );

    // Actionable errors, each with its own code.
    let error = mcp.call_error("get_profile", json!({})).await;
    assert_eq!(error["code"], "no_profile");
    assert!(error["message"].as_str().unwrap().contains("jobhunt init"));
    assert!(error["hint"].is_string());
    let error = mcp.call_error("search_jobs", json!({})).await;
    assert_eq!(error["code"], "no_profile");
    let error = mcp
        .call_error(
            "get_job",
            json!({"id": "opp_00000000000000000000000000000000"}),
        )
        .await;
    assert_eq!(error["code"], "unknown_opportunity");
    let error = mcp.call_error("save_job", json!({"id": "banana"})).await;
    assert_eq!(error["code"], "invalid_arguments");
    let error = mcp
        .call_error(
            "update_preferences",
            json!({"set": [{"kind": "company", "company": "unicorn"}]}),
        )
        .await;
    assert_eq!(error["code"], "invalid_preference");
    assert!(error["message"].as_str().unwrap().contains("founder-led"));
    let error = mcp.call_error("update_preferences", json!({})).await;
    assert_eq!(error["code"], "invalid_preference");

    // Arguments that don't match the schema are rejected before any use
    // case runs, as tool errors the model can correct (the MCP
    // specification's input-validation rule), naming the problem.
    for (tool, arguments, problem) in [
        ("search_jobs", json!({"limit": "five"}), "expected u8"),
        (
            "search_jobs",
            json!({"refresh": "sometimes"}),
            "unknown variant",
        ),
        ("get_job", json!({}), "missing field `id`"),
        (
            "reject_job",
            json!({"id": "opp_1234", "why": "extra field"}),
            "unknown field `why`",
        ),
    ] {
        let response = mcp
            .request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await;
        let result = &response["result"];
        assert_eq!(result["isError"], true, "{tool}: {response}");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains(problem), "{tool}: {text}");
    }
    let limit = mcp.call_error("search_jobs", json!({"limit": 200})).await;
    assert_eq!(limit["code"], "invalid_arguments");
    let unknown = mcp
        .request(
            "tools/call",
            json!({"name": "delete_everything", "arguments": {}}),
        )
        .await;
    assert!(unknown.get("error").is_some() || unknown["result"]["isError"] == true);

    // The client disconnects: the server exits cleanly, and everything it
    // wrote to stdout was protocol (checked line by line by the client).
    let stderr = mcp.stderr.clone();
    let transcript = mcp.close().await;
    assert!(transcript.len() >= 15);
    let logs = std::fs::read_to_string(stderr).unwrap();
    assert!(
        logs.lines().filter(|l| l.contains("\"level\"")).count() > 5,
        "the logs went to stderr:\n{logs}"
    );
    assert!(!logs.contains("\u{1b}["), "no color codes in a log file");
}

#[tokio::test(flavor = "multi_thread")]
async fn negotiates_the_protocol_version() {
    let env = Env::new().await;
    for (asked, answered) in [
        ("2025-06-18", "2025-06-18"),
        ("2025-03-26", "2025-03-26"),
        ("2024-11-05", "2024-11-05"),
    ] {
        let mut mcp = env.mcp().await;
        let response = mcp
            .request(
                "initialize",
                json!({
                    "protocolVersion": asked,
                    "capabilities": {},
                    "clientInfo": {"name": "old-client", "version": "0"}
                }),
            )
            .await;
        assert_eq!(response["result"]["protocolVersion"], answered);
        mcp.close().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_cannot_start_writes_nothing_to_stdout() {
    let env = Env::new().await;
    // The database path is a directory: the store can't open.
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_jobhunt"))
        .args(["--config", env.config().to_str().unwrap()])
        .args(["--database", env.dir.path().to_str().unwrap(), "mcp"])
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not open the job database"),
        "{stderr}"
    );
}

fn posting(native: &str) -> JobPosting {
    let url = CanonicalUrl::parse(&format!("https://jobs.example.com/acme/{native}")).unwrap();
    JobPosting {
        provenance: Provenance {
            source: SourceKey::new("ashby", "acme").unwrap(),
            source_record_id: Some(native.to_owned()),
            fetched_from: None,
        },
        url,
        apply_url: None,
        company: "Acme".into(),
        title: format!("Engineer {native}"),
        department: None,
        team: None,
        location: Some("Remote".into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: None,
        is_remote: Some(true),
        compensation: None,
        work_authorization: None,
        description_text: Some("A job.".into()),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    }
}

/// Two stored jobs whose ids share their first four hex digits.
async fn colliding_jobs(store: &SqliteJobStore) -> (String, String) {
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for n in 0.. {
        let p = posting(&n.to_string());
        let id = p.id().to_string();
        let prefix = id[..8].to_owned();
        if let Some(other) = seen.get(&prefix) {
            store
                .record_observation(&posting(other), Utc::now())
                .await
                .unwrap();
            store.record_observation(&p, Utc::now()).await.unwrap();
            return (prefix, n.to_string());
        }
        seen.insert(prefix, n.to_string());
    }
    unreachable!()
}

#[tokio::test(flavor = "multi_thread")]
async fn short_ids_resolve_and_ambiguous_ones_are_reported() {
    let env = Env::new().await;
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let (job_prefix, native) = colliding_jobs(&store).await;
    store.close().await;
    let id = posting(&native).id().to_string();
    let opportunity = format!("opp_{}", &id[4..]);

    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    let error = mcp.call_error("get_job", json!({"id": job_prefix})).await;
    assert_eq!(error["code"], "ambiguous_id");
    assert!(error["message"].as_str().unwrap().contains("job_"));
    let error = mcp
        .call_error(
            "get_job",
            json!({"id": format!("opp_{}", &job_prefix[4..])}),
        )
        .await;
    assert_eq!(error["code"], "ambiguous_id");
    // A longer prefix is unique; so is the full id.
    for input in [&id[..16], &opportunity[..16], &opportunity] {
        let job = mcp.call("get_job", json!({"id": input})).await;
        assert_eq!(job["id"], opportunity.as_str(), "{input}");
        assert_eq!(job["title"], format!("Engineer {native}"));
        // Without a profile there is no decision, and nothing personal.
        assert!(job.get("decision").is_none());
        assert_eq!(job["eligibility"], Value::Null);
    }
    mcp.close().await;

    // The CLI resolves the same short ids.
    assert!(
        env.fails(&["show", &job_prefix])
            .contains("matches more than one opportunity")
    );
    assert!(
        env.ok(&["show", &opportunity[..16]])
            .contains(&format!("Engineer {native}"))
    );
}
