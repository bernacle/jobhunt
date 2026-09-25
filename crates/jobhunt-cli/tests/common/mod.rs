//! Shared by the local-product tests: mock job boards serving saved real
//! responses, a temporary JobHunt home whose config lists them, the
//! `jobhunt` binary pointed at both, and a minimal MCP client that speaks
//! newline-delimited JSON-RPC to `jobhunt mcp` over its stdin/stdout,
//! exactly as a desktop MCP client does, and checks that every line the
//! server writes to stdout is a protocol message.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use wiremock::matchers::{path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub fn fixture(family: &str, name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../jobhunt-sources/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

pub fn resume(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../jobhunt-resume/tests/fixtures")
        .join(name)
}

pub const ASHBY: [(&str, &str); 3] = [("linear", "Linear"), ("ramp", "Ramp"), ("modal", "Modal")];

/// Every endpoint discovery and verification use, serving saved responses
/// of three Ashby boards and one Greenhouse board. Ashby verifies by
/// reading the board again; Greenhouse through its single-job endpoint and
/// hosted application page.
pub async fn job_boards() -> MockServer {
    let server = MockServer::start().await;
    for (board, _) in ASHBY {
        Mock::given(path(format!("/posting-api/job-board/{board}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture("ashby", &format!("{board}.json"))),
            )
            .mount(&server)
            .await;
    }
    let figma = fixture("greenhouse", "figma.json");
    Mock::given(path("/v1/boards/figma/jobs"))
        .respond_with(ResponseTemplate::new(200).set_body_string(figma.clone()))
        .mount(&server)
        .await;
    let jobs: Value = serde_json::from_str(&figma).unwrap();
    Mock::given(path_regex(r"^/v1/boards/figma/jobs/\d+$"))
        .respond_with(move |req: &Request| {
            let id = req
                .url
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            match jobs["jobs"]
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
    Mock::given(path_regex(r"^/figma/jobs/\d+$"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<form></form>"))
        .mount(&server)
        .await;
    server
}

/// Requests the mock received so far.
pub async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.map_or(0, |r| r.len())
}

/// A temporary JobHunt home: a config listing the mock's boards, and a
/// database file next to it.
pub struct Env {
    pub dir: tempfile::TempDir,
    pub server: MockServer,
}

impl Env {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = String::from("[discovery]\nmax_retries = 0\nrequest_timeout_secs = 5\n\n");
        for (board, company) in ASHBY {
            config.push_str(&format!(
                "[[sources.ashby]]\nboard = \"{board}\"\ncompany = \"{company}\"\n\n"
            ));
        }
        config.push_str("[[sources.greenhouse]]\nboard = \"figma\"\ncompany = \"Figma\"\n");
        std::fs::write(dir.path().join("config.toml"), config).unwrap();
        Self {
            dir,
            server: job_boards().await,
        }
    }

    pub fn db(&self) -> PathBuf {
        self.dir.path().join("jobhunt.db")
    }

    pub fn config(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    /// `jobhunt` with this home's config and database, pointed at the mock.
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jobhunt"));
        command
            .arg("--config")
            .arg(self.config())
            .arg("--database")
            .arg(self.db())
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .env("JOBHUNT_DISCOVERY_ENDPOINT", self.server.uri())
            .env("JOBHUNT_VERIFY_ENDPOINT", self.server.uri());
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    pub fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "jobhunt {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    pub fn fails(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(!output.status.success(), "jobhunt {args:?} should fail");
        String::from_utf8(output.stderr).unwrap()
    }

    /// The profile from the fixture resume, living in New York with US
    /// work authorization (so the mock's US jobs are eligible).
    pub fn with_profile(&self) {
        self.ok(&["init", resume("marina_costa.pdf").to_str().unwrap()]);
        self.ok(&["preferences", "set", "location", "New", "York,", "NY"]);
        self.ok(&["preferences", "set", "authorized-in", "United", "States"]);
    }

    pub async fn mcp(&self) -> McpClient {
        McpClient::start(self, &[]).await
    }
}

const TIMEOUT: Duration = Duration::from_secs(60);

/// A minimal MCP client over the server's stdin/stdout.
pub struct McpClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    /// Responses read while waiting for another one.
    pending: HashMap<u64, Value>,
    /// Every message the server wrote, in order.
    pub transcript: Vec<Value>,
    pub stderr: PathBuf,
}

impl McpClient {
    /// Starts `jobhunt [args] mcp`.
    pub async fn start(env: &Env, args: &[&str]) -> Self {
        let stderr = env.dir.path().join(format!(
            "mcp-{}.stderr",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut command = tokio::process::Command::from(env.command());
        command
            .args(args)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            stdin,
            stdout,
            next_id: 0,
            pending: HashMap::new(),
            transcript: Vec::new(),
            stderr,
        }
    }

    pub async fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        let mut line = serde_json::to_string(message).unwrap();
        line.push('\n');
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
    }

    /// Reads one line and checks it is a JSON-RPC 2.0 message: a response
    /// (with an id and a result or an error) or a notification.
    pub async fn read(&mut self) -> Option<Value> {
        let line = tokio::time::timeout(TIMEOUT, self.stdout.next_line())
            .await
            .expect("the server answered in time")
            .unwrap()?;
        let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| {
            panic!(
                "stdout carried something that is not JSON ({e}):\n{line}\nstderr:\n{}",
                self.stderr_text()
            )
        });
        assert_eq!(message["jsonrpc"], "2.0", "not JSON-RPC 2.0: {line}");
        let is_response = message.get("id").is_some()
            && (message.get("result").is_some() || message.get("error").is_some());
        let is_notification = message.get("method").is_some() && message.get("id").is_none();
        assert!(
            is_response || is_notification,
            "not a response or notification: {line}"
        );
        self.transcript.push(message.clone());
        Some(message)
    }

    /// Sends a request without waiting; returns its id.
    pub async fn submit(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        id
    }

    /// The response to request `id`.
    pub async fn response(&mut self, id: u64) -> Value {
        if let Some(message) = self.pending.remove(&id) {
            return message;
        }
        loop {
            let message = self
                .read()
                .await
                .unwrap_or_else(|| panic!("the server closed stdout before answering {id}"));
            let Some(got) = message.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if got == id {
                return message;
            }
            self.pending.insert(got, message);
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.submit(method, params).await;
        self.response(id).await
    }

    /// The MCP handshake. Returns the `initialize` result.
    pub async fn initialize(&mut self) -> Value {
        let response = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "jobhunt-tests", "version": "1.0"}
                }),
            )
            .await;
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        response["result"].clone()
    }

    /// Calls a tool that must succeed; returns its structured content.
    pub async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let response = self
            .request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await;
        let result = &response["result"];
        assert!(
            result["isError"] != true && response.get("error").is_none(),
            "{tool} failed: {response}\nserver stderr:\n{}",
            self.stderr_text()
        );
        let structured = result["structuredContent"].clone();
        assert!(
            structured.is_object(),
            "{tool} returned no structured content"
        );
        // The text content carries the same JSON, for clients that only
        // read text.
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, structured);
        structured
    }

    /// Calls a tool that must fail as a tool error; returns
    /// `{code, message, hint}`.
    pub async fn call_error(&mut self, tool: &str, arguments: Value) -> Value {
        let response = self
            .request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await;
        let result = &response["result"];
        assert_eq!(result["isError"], true, "{tool} should fail: {response}");
        let body: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        body["error"].clone()
    }

    pub fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// Closes stdin (the client disconnects) and waits for the server to
    /// exit cleanly, checking that nothing else reached stdout.
    pub async fn close(mut self) -> Vec<Value> {
        drop(self.stdin.take());
        while self.read().await.is_some() {}
        let status = tokio::time::timeout(TIMEOUT, self.child.wait())
            .await
            .expect("the server exited after stdin closed")
            .unwrap();
        assert!(
            status.success(),
            "the server exited with {status}:\n{}",
            self.stderr_text()
        );
        self.transcript
    }
}

/// The ids of search results.
pub fn result_ids(search: &Value) -> Vec<String> {
    search["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The search result titled `title`.
pub fn result<'a>(search: &'a Value, title: &str) -> &'a Value {
    search["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["title"] == title)
        .unwrap_or_else(|| panic!("no result titled {title:?} in {search:#}"))
}
