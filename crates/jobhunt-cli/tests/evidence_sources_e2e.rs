//! LinkedIn export and GitHub evidence end to end: the `narrow` binary
//! against a temporary database, the fictional `ana_lima.md` resume, a
//! fictional LinkedIn export folder, and a local server speaking GitHub's
//! REST API (`JOBHUNT_GITHUB_ENDPOINT`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn resume() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../jobhunt-resume/tests/fixtures/ana_lima.md")
}

struct Env {
    dir: tempfile::TempDir,
    github: String,
}

impl Env {
    fn new(github: &MockServer) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        Self {
            dir,
            github: github.uri(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_narrow"))
            .arg("--config")
            .arg(self.path("config.toml"))
            .arg("--database")
            .arg(self.path("jobhunt.db"))
            .args(args)
            .env_remove("JOBHUNT_LOG")
            .env_remove("RUST_LOG")
            .env_remove("GITHUB_TOKEN")
            .env("JOBHUNT_GITHUB_ENDPOINT", &self.github)
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

    fn fails(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(!output.status.success(), "narrow {args:?} should fail");
        String::from_utf8(output.stderr).unwrap()
    }

    /// A LinkedIn export folder: Ana's current position (also on her
    /// resume), one only LinkedIn has, skills, and a private file.
    fn linkedin_export(&self, extra_position: &str) -> PathBuf {
        let dir = self.path("Basic_LinkedInDataExport");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Positions.csv"),
            format!(
                "Company Name,Title,Description,Location,Started On,Finished On\n\
                 Acme Payments,Staff Software Engineer,\"Led the redesign of the settlement service, cutting batch time by 60%.\",Remote,Jan 2021,\n\
                 {extra_position}\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("Skills.csv"), "Name\nGo\nTerraform\n").unwrap();
        std::fs::write(
            dir.join("messages.csv"),
            "FROM,CONTENT\nSomeone,SENTINEL-PRIVATE-MESSAGE\n",
        )
        .unwrap();
        dir
    }
}

async fn github() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/analima"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "login": "analima",
            "html_url": "https://github.com/analima",
            "type": "User",
            "public_repos": 2,
            "created_at": "2014-05-01T00:00:00Z"
        })))
        .mount(&server)
        .await;
    let repo = |id: u64, name: &str, fork: bool| {
        json!({
            "id": id,
            "name": name,
            "full_name": format!("analima/{name}"),
            "owner": { "login": "analima" },
            "html_url": format!("https://github.com/analima/{name}"),
            "description": "A linter for double-entry ledgers",
            "fork": fork,
            "archived": false,
            "is_template": false,
            "private": false,
            "size": 300,
            "stargazers_count": 300,
            "forks_count": 12,
            "language": "Go",
            "topics": ["accounting", "linter"],
            "created_at": "2022-02-01T00:00:00Z",
            "pushed_at": "2026-09-01T00:00:00Z"
        })
    };
    Mock::given(method("GET"))
        .and(path("/users/analima/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            repo(10, "ledgerlint", false),
            repo(11, "kubernetes", true)
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/analima/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/someone-else"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "login": "someone-else",
            "html_url": "https://github.com/someone-else",
            "type": "User",
            "public_repos": 0
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/someone-else/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/someone-else/orgs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_linkedin_and_github_build_one_profile() {
    let server = github().await;
    let env = Env::new(&server);
    env.ok(&["init", resume().to_str().unwrap()]);

    // LinkedIn: one position the resume has, one it does not.
    let export = env.linkedin_export("Initech,Backend Engineer,,Remote,Jan 2015,May 2016");
    let out = env.ok(&["profile", "import-linkedin", export.to_str().unwrap()]);
    assert!(
        out.contains("Imported LinkedIn export Basic_LinkedInDataExport"),
        "{out}"
    );
    assert!(
        out.contains("Read: positions (2 rows), skills (2 rows)"),
        "{out}"
    );
    assert!(
        out.contains("Not used: 1 other file in the export, never opened"),
        "{out}"
    );
    assert!(
        out.contains(
            "experiences: 1 new, 1 already in your profile, now also backed by this source"
        ),
        "{out}"
    );
    assert!(!out.contains("SENTINEL"));
    // Idempotent.
    let again = env.ok(&["profile", "import-linkedin", export.to_str().unwrap()]);
    assert!(
        again.contains("Re-imported (nothing changed) LinkedIn export"),
        "{again}"
    );
    assert!(!again.contains("new"), "{again}");

    let profile = env.ok(&["profile"]);
    assert_eq!(
        profile
            .matches("Acme Payments — Staff Software Engineer")
            .count(),
        1,
        "{profile}"
    );
    assert!(profile.contains("in: resume, LinkedIn export"), "{profile}");
    assert!(profile.contains("Initech — Backend Engineer"), "{profile}");
    assert!(
        profile.contains("from Basic_LinkedInDataExport"),
        "{profile}"
    );

    // GitHub: the account the resume links, forks left out.
    let out = env.ok(&["profile", "import-github"]);
    assert!(out.contains("Imported GitHub github.com/analima"), "{out}");
    assert!(
        out.contains("Read: 1 public repository you own, main languages: Go"),
        "{out}"
    );
    assert!(out.contains("Not used: 1 fork"), "{out}");
    assert!(out.contains("claims need your review"), "{out}");
    let again = env.ok(&["profile", "import-github", "https://github.com/analima"]);
    assert!(
        again.contains("Re-imported (nothing changed) GitHub"),
        "{again}"
    );
    // The resume's ledgerlint project and the repository are one project.
    let profile = env.ok(&["profile"]);
    assert_eq!(profile.matches("ledgerlint").count(), 1, "{profile}");
    assert!(profile.contains("in: resume, GitHub"), "{profile}");
    // Its inference waits for review, with the repository behind it.
    let review = env.ok(&["claims", "review", "--all"]);
    assert!(
        review.contains("Recent hands-on Go work in public GitHub repositories"),
        "{review}"
    );
    assert!(review.contains("GitHub: “analima/ledgerlint"), "{review}");

    // Another account is refused, not silently merged.
    let err = env.fails(&["profile", "import-github", "someone-else"]);
    assert!(err.contains("remove-source github"), "{err}");

    // A file that is not an export changes nothing.
    let export_now = || {
        env.ok(&["profile", "export"])
            .lines()
            .filter(|l| !l.contains("\"exported_at\""))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let before = export_now();
    std::fs::write(env.path("notes.txt"), "not a LinkedIn export").unwrap();
    let err = env.fails(&[
        "profile",
        "import-linkedin",
        env.path("notes.txt").to_str().unwrap(),
    ]);
    assert!(
        err.contains("is not a LinkedIn data export Narrow can read"),
        "{err}"
    );
    assert_eq!(export_now(), before);

    // Removing LinkedIn: Initech goes, Acme stays (the resume has it).
    let out = env.ok(&["profile", "remove-source", "linkedin"]);
    assert!(
        out.contains("Removed your LinkedIn export from your profile"),
        "{out}"
    );
    let profile = env.ok(&["profile"]);
    assert!(!profile.contains("Initech"), "{profile}");
    assert_eq!(
        profile
            .matches("Acme Payments — Staff Software Engineer")
            .count(),
        1
    );
    assert!(
        !profile.contains("in: resume, LinkedIn export"),
        "{profile}"
    );
    let err = env.fails(&["profile", "remove-source", "linkedin"]);
    assert!(err.contains("no imported source matches"), "{err}");

    // Removing GitHub, then importing another account works.
    env.ok(&["profile", "remove-source", "github"]);
    let out = env.ok(&["profile", "import-github", "someone-else"]);
    assert!(out.contains("github.com/someone-else"), "{out}");
    assert!(
        out.contains("your resume links github.com/analima, not github.com/someone-else"),
        "{out}"
    );
    let history = env.ok(&["profile", "history"]);
    for kind in ["github_imported", "source_removed", "linkedin_imported"] {
        assert!(
            history.contains(kind) || history.contains(&kind.replace('_', " ")),
            "{kind}: {history}"
        );
    }
}
