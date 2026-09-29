//! `narrow doctor`: is everything where it should be, and how does an MCP
//! client start this JobHunt?

use std::io::{self, Write};
use std::process::ExitCode;

use jobhunt_app::RefreshReason;
use jobhunt_app::doctor::Diagnostics;

use crate::config::LoadedConfig;
use crate::local::{finish, with_app};
use crate::rank_render::{GOOD, OPEN};
use crate::render::{DIM, plural};

pub async fn run(loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    with_app!(loaded, |app| {
        let d = app.doctor(jobhunt_app::now()).await?;
        let cloud = cloud_lines(&app).await;
        finish(
            write(&mut anstream::stdout().lock(), &d, loaded)
                .and_then(|()| write_cloud(&mut anstream::stdout().lock(), &cloud)),
            "the report",
        )
    })
}

/// The JobHunt Cloud section: where the session is kept and whether there
/// is one (never its tokens), and where sync stands. No network.
async fn cloud_lines(app: &jobhunt_app::App) -> Vec<(bool, String)> {
    let mut lines = Vec::new();
    match crate::cloud::vault() {
        Ok(vault) => {
            match vault.load() {
                Ok(Some(session)) => lines.push((
                    true,
                    format!(
                        "Cloud: signed in to {} as {} ({:?}){}",
                        session.server,
                        session.user_id,
                        session.kind,
                        if session.expiring(chrono::Utc::now()) && session.refresh_token.is_none() {
                            "; the session expired, run `narrow login`"
                        } else {
                            ""
                        }
                    ),
                )),
                Ok(None) => lines.push((
                    true,
                    "Cloud: not signed in (optional; `narrow login` to sync)".into(),
                )),
                Err(e) => lines.push((false, format!("Cloud: {e}"))),
            }
            lines.push((
                true,
                format!("Cloud session stored in {}", vault.describe()),
            ));
        }
        Err(e) => lines.push((false, format!("Cloud: {e}"))),
    }
    // In a cloud environment (the service's variables are set), what the
    // cloud processes would find: present, missing or invalid, never values.
    if [
        "DATABASE_URL",
        "JOBHUNT_OIDC_ISSUER",
        "JOBHUNT_ENCRYPTION_KEYS",
    ]
    .iter()
    .any(|v| std::env::var_os(v).is_some())
    {
        let cloud = crate::serve::cloud_config();
        for setting in cloud.report() {
            let good = !setting.required
                || !(setting.status.starts_with("missing")
                    || setting.status.starts_with("invalid"));
            lines.push((
                good,
                format!("Cloud config {}: {}", setting.name, setting.status),
            ));
        }
        for problem in cloud.problems(jobhunt_cloud::Role::Server) {
            lines.push((false, format!("Cloud config: {problem}")));
        }
    }
    if let Ok(status) = app.sync_status().await
        && let Some(account) = &status.account
    {
        lines.push((
            status.conflicts.is_empty(),
            format!(
                "Sync: last {} with {}; {} feedback to send; {} conflicts",
                account
                    .last_sync_at
                    .map_or_else(|| "never".into(), |t| t.to_rfc3339()),
                account.server,
                status.unsynced_feedback,
                status.conflicts.len()
            ),
        ));
    }
    lines
}

fn write_cloud(out: &mut impl Write, lines: &[(bool, String)]) -> io::Result<()> {
    writeln!(out)?;
    for (good, text) in lines {
        ok(out, *good, text)?;
    }
    out.flush()
}

fn ok(out: &mut impl Write, good: bool, text: &str) -> io::Result<()> {
    if good {
        writeln!(out, "{GOOD}✓{GOOD:#} {text}")
    } else {
        writeln!(out, "{OPEN}!{OPEN:#} {text}")
    }
}

fn write(out: &mut impl Write, d: &Diagnostics, loaded: &LoadedConfig) -> io::Result<()> {
    let config = match (&d.config_file, &d.default_config_file) {
        (Some(file), _) => file.display().to_string(),
        (None, Some(default)) => {
            format!("none (defaults; would be read from {})", default.display())
        }
        (None, None) => "none (defaults)".to_owned(),
    };
    ok(out, true, &format!("Config: {config}"))?;
    ok(out, true, &format!("Database: {}", d.database.display()))?;
    let s = &d.stats;
    ok(
        out,
        s.migrations_applied >= s.migrations_known,
        &format!(
            "Schema: {} of {} migrations applied",
            s.migrations_applied, s.migrations_known
        ),
    )?;
    match &d.profile {
        Some(p) => ok(
            out,
            p.experiences > 0 || p.preferences > 0,
            &format!(
                "Profile: {}, {} ({} usable, {} to review), {}",
                plural(p.experiences as u64, "experience", "experiences"),
                plural(p.claims as u64, "claim", "claims"),
                p.usable_claims,
                p.needs_review,
                plural(p.preferences as u64, "preference", "preferences"),
            ),
        )?,
        None => ok(out, false, "Profile: none yet (narrow init resume.pdf)")?,
    }
    ok(
        out,
        s.open_jobs > 0,
        &format!(
            "Jobs: {} ({} open opportunities), {}, {}",
            plural(s.jobs, "stored job", "stored jobs"),
            s.open_opportunities,
            plural(s.verifications, "verification", "verifications"),
            plural(s.feedback, "piece of feedback", "pieces of feedback"),
        ),
    )?;
    let now = jobhunt_app::now();
    let fresh = matches!(d.freshness, RefreshReason::Fresh { .. });
    ok(
        out,
        fresh,
        &format!(
            "Sources: {} configured{}; {}",
            d.sources,
            if d.careers_pages > 0 {
                format!(
                    " plus {}",
                    plural(d.careers_pages as u64, "careers page", "careers pages")
                )
            } else {
                String::new()
            },
            d.freshness.describe(now)
        ),
    )?;
    writeln!(out)?;
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "narrow".to_owned());
    let mut args = vec!["mcp".to_owned()];
    if let Some(file) = &loaded.file {
        args.push("--config".into());
        args.push(file.display().to_string());
    }
    args.push("--database".into());
    args.push(d.database.display().to_string());
    writeln!(
        out,
        "MCP: `narrow mcp` serves this profile and database over stdio. For a client:"
    )?;
    writeln!(out, "  command: {exe}")?;
    writeln!(
        out,
        "  args:    {}",
        serde_json::to_string(&args).unwrap_or_default()
    )?;
    writeln!(
        out,
        "{DIM}No AI provider or account is needed; see the README for client configuration.{DIM:#}"
    )?;
    out.flush()
}
