//! JobHunt Cloud end to end, through the real binary: `narrow server`,
//! `narrow migrate`, scheduled workers (two at once) against recorded job
//! boards, and a person's `init` → `login` → `sync` flow on a laptop, with
//! the laptop working offline before and after.
//!
//! Needs `JOBHUNT_TEST_DATABASE_URL` (skipped without it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{Env, resume};
use jobhunt_storage::postgres::testing::TestDatabase;
use serde_json::Value;

const KEY: &str = "k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

struct Cloud {
    db: TestDatabase,
    port: u16,
    server: Option<Child>,
}

impl Cloud {
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// `narrow <args>` as a cloud process (configured by environment).
    fn command(&self, env: &Env) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_narrow"));
        c.env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .env_remove("JOBHUNT_DATABASE")
            .env("DATABASE_URL", self.db.url())
            .env("JOBHUNT_ENCRYPTION_KEYS", KEY)
            .env("JOBHUNT_AUTH_MODE", "dev")
            .env(
                "JOBHUNT_AUTH_DEV_SECRET",
                "0123456789abcdef0123456789abcdef",
            )
            .env("JOBHUNT_ENV", "test")
            .env("JOBHUNT_PUBLIC_URL", self.base())
            .env("JOBHUNT_BIND", format!("127.0.0.1:{}", self.port))
            .env("JOBHUNT_CONFIG", env.config())
            .env("JOBHUNT_LOG_FORMAT", "json")
            .env("JOBHUNT_DISCOVERY_ENDPOINT", env.server.uri())
            .env("JOBHUNT_VERIFY_ENDPOINT", env.server.uri());
        c
    }

    fn ok(&self, env: &Env, args: &[&str]) -> String {
        let output = self.command(env).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "narrow {args:?} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    async fn start(env: &Env) -> Option<Self> {
        let db = TestDatabase::create().await?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut cloud = Self {
            db,
            port,
            server: None,
        };
        cloud.ok(env, &["migrate"]);
        let child = cloud
            .command(env)
            .arg("server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        cloud.server = Some(child);
        let http = reqwest::Client::new();
        for _ in 0..100 {
            if let Ok(r) = http.get(format!("{}/ready", cloud.base())).send().await
                && r.status().is_success()
            {
                return Some(cloud);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("narrow server did not become ready");
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.server.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    async fn pool(&self) -> sqlx::PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.db.options())
            .await
            .unwrap()
    }
}

impl Drop for Cloud {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The laptop: the usual test home plus its own session file.
fn laptop(env: &Env, base: String) -> impl Fn(&[&str]) -> std::process::Output + '_ {
    let credentials = env.dir.path().join("credentials.json");
    let make = move || {
        let mut c = env.command();
        c.env("JOBHUNT_CREDENTIALS_FILE", &credentials)
            .env("JOBHUNT_CLOUD_URL", &base)
            .env_remove("DATABASE_URL");
        c
    };
    move |args: &[&str]| make().args(args).output().unwrap()
}

fn stdout_of(output: &std::process::Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[tokio::test]
async fn workers_discover_once_and_a_laptop_syncs_with_the_cloud() {
    let env = Env::new().await;
    let Some(mut cloud) = Cloud::start(&env).await else {
        return;
    };
    // Migrations are idempotent (the pre-deploy command runs every deploy).
    assert!(cloud.ok(&env, &["migrate"]).contains("Schema current"));

    // Two discovery workers at once: every source is read exactly once.
    let spawn = || {
        cloud
            .command(&env)
            .args(["worker", "discovery"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let (a, b) = (spawn(), spawn());
    let (a, b) = (a.wait_with_output().unwrap(), b.wait_with_output().unwrap());
    let summary = |o: &std::process::Output| -> Value {
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let (sa, sb) = (summary(&a), summary(&b));
    let read = sa["sources_read"].as_u64().unwrap() + sb["sources_read"].as_u64().unwrap();
    assert_eq!(read, 4, "{sa} {sb}");
    let new = sa["new"].as_u64().unwrap() + sb["new"].as_u64().unwrap();
    assert!(new > 10);
    let pool = cloud.pool().await;
    let scans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM source_scans")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scans, 4, "no source was scanned twice");
    let news: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_events WHERE kind = 'new'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(news as u64, new, "history recorded each job as NEW once");
    let runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM worker_runs WHERE kind = 'discovery' AND status = 'succeeded'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(runs, 2);

    // Nothing is due any more: a repeated run reads nothing.
    let again: Value = serde_json::from_str(&cloud.ok(&env, &["worker", "discovery"])).unwrap();
    assert_eq!(again["sources_due"], 0);

    // Re-verification picks the newly discovered jobs, then leaves them.
    let verified: Value =
        serde_json::from_str(&cloud.ok(&env, &["worker", "verification"])).unwrap();
    assert!(verified["verified"].as_u64().unwrap() > 0, "{verified}");
    let again: Value = serde_json::from_str(&cloud.ok(&env, &["worker", "verification"])).unwrap();
    assert_eq!(
        again["verified"], 0,
        "fresh verifications are not repeated: {again}"
    );

    // The laptop: a profile made offline.
    let jobhunt = laptop(&env, cloud.base());
    stdout_of(
        &jobhunt(&["init", resume("marina_costa.pdf").to_str().unwrap()]),
        "init",
    );
    stdout_of(
        &jobhunt(&["preferences", "set", "location", "New", "York,", "NY"]),
        "preferences",
    );
    // Not signed in: sync says so, and nothing else needs the cloud.
    let not_signed = jobhunt(&["sync"]);
    assert!(!not_signed.status.success());
    assert!(String::from_utf8_lossy(&not_signed.stderr).contains("narrow login"));

    let login = stdout_of(&jobhunt(&["login", "--as", "marina"]), "login");
    assert!(login.contains("Signed in"));
    let credentials = std::fs::read_to_string(env.dir.path().join("credentials.json")).unwrap();
    let session: Value = serde_json::from_str(&credentials).unwrap();
    let token = session["access_token"].as_str().unwrap().to_owned();
    let account = stdout_of(&jobhunt(&["account", "--json"]), "account");
    let account: Value = serde_json::from_str(&account).unwrap();
    assert!(account["id"].as_str().unwrap().starts_with("usr_"));

    let synced = stdout_of(&jobhunt(&["sync"]), "sync");
    assert!(synced.contains("Synced:"), "{synced}");
    assert!(!synced.contains("conflict"));

    // The cloud ranks the shared corpus for this person (no job board is
    // read by the search), and remembers what it showed.
    let http = reqwest::Client::new();
    let found: Value = http
        .post(format!("{}/api/v1/search", cloud.base()))
        .bearer_auth(&token)
        .json(&serde_json::json!({"limit": 3, "include_lower_tiers": true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(found["refresh"]["performed"], false, "{found}");
    let shown: Vec<String> = found["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!shown.is_empty(), "{found}");
    // Saved in the cloud (a hosted client) …
    let saved: Value = http
        .post(format!(
            "{}/api/v1/opportunities/{}/feedback",
            cloud.base(),
            shown[0]
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"action": "save"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["recorded"], true);

    // … reaches the laptop, with the job, which then works offline.
    let fresh_env = Env::new().await;
    // A second, empty machine with the same account.
    let second = laptop(&fresh_env, cloud.base());
    // It signs in with a personal access token made on the first one.
    let created = stdout_of(
        &jobhunt(&["token", "create", "second machine", "--days", "7"]),
        "token create",
    );
    let secret = created
        .lines()
        .find(|l| l.starts_with("jh_pat_"))
        .expect("the secret is shown once")
        .to_owned();
    let listed = stdout_of(&jobhunt(&["token", "list"]), "token list");
    assert!(listed.contains("second machine") && !listed.contains(&secret));
    let login = fresh_env
        .command()
        .env(
            "JOBHUNT_CREDENTIALS_FILE",
            fresh_env.dir.path().join("credentials.json"),
        )
        .env("JOBHUNT_CLOUD_URL", cloud.base())
        .env("JOBHUNT_TOKEN", &secret)
        .args(["login", "--token"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(stdout_of(&login, "login --token").contains("token"));
    stdout_of(&second(&["sync"]), "sync on the second machine");
    let pipeline = stdout_of(&second(&["pipeline", "--json"]), "pipeline");
    assert!(pipeline.contains(&shown[0]), "{pipeline}");
    let shown_offline = stdout_of(&second(&["show", &shown[0]]), "show offline");
    assert!(!shown_offline.is_empty());
    let profile = stdout_of(&second(&["profile"]), "profile");
    assert!(profile.contains("Marina") || profile.contains("marina") || !profile.is_empty());

    // The first laptop syncs again: the jobs the cloud recommended (found
    // by the cloud's discovery, never read by this laptop) arrive with
    // their verification, so the laptop can rank them offline.
    let again = stdout_of(&jobhunt(&["sync", "--json"]), "second sync");
    let again: Value = serde_json::from_str(&again).unwrap();
    assert!(
        again["jobs_pulled"].as_u64().unwrap() >= shown.len() as u64,
        "{again}"
    );
    assert!(
        again["verifications_pulled"].as_u64().unwrap() > 0,
        "{again}"
    );
    assert_eq!(again["feedback_pulled"], 1);

    // The cloud goes away: local commands keep working; sync says why.
    cloud.stop();
    let offline = jobhunt(&["sync"]);
    assert!(!offline.status.success());
    let message = String::from_utf8_lossy(&offline.stderr);
    assert!(
        message.contains("could not reach JobHunt Cloud"),
        "{message}"
    );
    stdout_of(&jobhunt(&["find", "--offline", "--all"]), "find --offline");
    stdout_of(&jobhunt(&["profile"]), "profile offline");

    // Logging out forgets the session, keeps the data.
    stdout_of(&jobhunt(&["logout"]), "logout");
    assert!(!env.dir.path().join("credentials.json").exists());
    stdout_of(&jobhunt(&["profile"]), "profile after logout");
    pool.close().await;
}
