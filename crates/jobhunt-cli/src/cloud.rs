//! JobHunt Cloud from this machine: `jobhunt login`, `logout`, `account`,
//! `sync` and `token`.
//!
//! Only these commands use the network to reach the cloud; every other
//! command works on the local database, online or not. When the cloud
//! cannot be reached, these fail with `cloud_unavailable` and change
//! nothing locally.

use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use anyhow::Context;
use chrono::Utc;
use jobhunt_app::sync::Side;
use jobhunt_app::{AppError, Paths, Remote, SyncReport};
use jobhunt_cloud::client::{CloudClient, DeviceFlow, server_url};
use jobhunt_profile::entities::{EntityKey, EntityKind};

use crate::config::LoadedConfig;
use crate::credentials::{Session, SessionKind, Vault};
use crate::local::{finish, print_json};

/// The session store for this machine.
pub fn vault() -> anyhow::Result<Vault> {
    let paths = Paths::platform();
    Vault::locate(paths.as_ref().map(|p| p.data_dir.as_path()))
}

#[derive(Debug, clap::Args)]
pub struct LoginArgs {
    /// The JobHunt Cloud server (default: `[cloud] server` in the config,
    /// or JOBHUNT_CLOUD_URL).
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,
    /// Sign in with a personal access token read from standard input (or
    /// JOBHUNT_TOKEN), instead of in the browser.
    #[arg(long)]
    pub token: bool,
    /// Development servers only: the name to sign in as.
    #[arg(long = "as", value_name = "NAME")]
    pub dev_user: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct LogoutArgs {
    /// Also sign out every other device and revoke every personal access
    /// token of the account.
    #[arg(long)]
    pub everywhere: bool,
}

#[derive(Debug, clap::Args)]
pub struct AccountArgs {
    /// Print the account as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct SyncArgs {
    /// Only show where sync stands (no network).
    #[arg(long, conflicts_with = "keep")]
    pub status: bool,
    /// Resolve conflicts by keeping this side (all, or `--record` ones);
    /// applies to conflicts found during this sync too.
    #[arg(long, value_enum, value_name = "SIDE")]
    pub keep: Option<KeepArg>,
    /// Records (`clm_…`, `exp_…`, …) `--keep` applies to.
    #[arg(long = "record", value_name = "ID", requires = "keep")]
    pub records: Vec<String>,
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum KeepArg {
    Local,
    Cloud,
}

#[derive(Debug, clap::Args)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub command: TokenCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum TokenCommand {
    /// Create a personal access token (for an MCP client or a script). Its
    /// secret is shown once.
    Create {
        /// What it is for ("Claude Desktop").
        name: String,
        /// Days until it expires (1 to 365; default 90).
        #[arg(long)]
        days: Option<u32>,
    },
    /// Your personal access tokens (never their secrets).
    List,
    /// Revoke a token.
    Revoke {
        /// `tok_…`.
        id: String,
    },
}

fn configured_server(loaded: &LoadedConfig, explicit: Option<&str>) -> anyhow::Result<String> {
    explicit
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("JOBHUNT_CLOUD_URL")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(|| loaded.config.cloud.server.clone())
        .context(
            "which JobHunt Cloud server? Pass --server <URL>, or set `server` under [cloud] in \
             the config file",
        )
}

/// A client for the signed-in session, refreshing its access token when
/// it is about to expire.
pub async fn signed_in(vault: &Vault) -> anyhow::Result<(CloudClient, Session)> {
    let mut session = vault.load()?.ok_or_else(|| {
        AppError::Unauthenticated("not signed in to JobHunt Cloud; run `jobhunt login`".into())
    })?;
    let now = Utc::now();
    if session.expiring(now) {
        match (
            &session.refresh_token,
            &session.token_endpoint,
            &session.client_id,
        ) {
            (Some(refresh), Some(endpoint), Some(client_id)) => {
                let flow = DeviceFlow::for_session(endpoint, client_id)?;
                let tokens = flow.refresh(refresh).await?;
                session.access_token = tokens.access_token;
                session.access_expires_at = tokens
                    .expires_in
                    .map(|s| now + chrono::Duration::seconds(i64::try_from(s).unwrap_or(0)));
                if tokens.refresh_token.is_some() {
                    session.refresh_token = tokens.refresh_token;
                }
                vault.save(&session)?;
            }
            _ => {
                return Err(AppError::Unauthenticated(
                    "the JobHunt Cloud session expired; run `jobhunt login`".into(),
                )
                .into());
            }
        }
    }
    let client = CloudClient::new(server_url(&session.server)?)?.with_token(&session.access_token);
    Ok((client, session))
}

fn read_token() -> anyhow::Result<String> {
    if let Ok(t) = std::env::var("JOBHUNT_TOKEN")
        && !t.trim().is_empty()
    {
        return Ok(t.trim().to_owned());
    }
    if std::io::stdin().is_terminal() {
        eprint!("Paste the personal access token (jh_pat_…) and press Enter: ");
        let _ = std::io::stderr().flush();
    }
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("could not read the token from standard input")?;
    let token = text.trim().to_owned();
    anyhow::ensure!(!token.is_empty(), "no token given");
    Ok(token)
}

pub async fn login(args: LoginArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let server = server_url(&configured_server(loaded, args.server.as_deref())?)?;
    let anonymous = CloudClient::new(server.clone())?;
    let auth = anonymous.auth_config().await?;
    let now = Utc::now();
    let mut session = if args.token {
        Session {
            server: server.to_string(),
            user_id: String::new(),
            kind: SessionKind::Token,
            access_token: read_token()?,
            access_expires_at: None,
            refresh_token: None,
            token_endpoint: None,
            revocation_endpoint: None,
            client_id: None,
        }
    } else if auth.mode == "dev" {
        let name = args
            .dev_user
            .or_else(|| std::env::var("USER").ok())
            .context("development server: pass --as <NAME>")?;
        let token = anonymous.dev_token(&name).await?;
        Session {
            server: server.to_string(),
            user_id: String::new(),
            kind: SessionKind::Dev,
            access_token: token.access_token,
            access_expires_at: Some(
                now + chrono::Duration::seconds(i64::try_from(token.expires_in).unwrap_or(0)),
            ),
            refresh_token: None,
            token_endpoint: None,
            revocation_endpoint: None,
            client_id: None,
        }
    } else {
        let flow = DeviceFlow::from_config(&auth)?;
        let code = flow.start().await?;
        let mut err = anstream::stderr().lock();
        writeln!(err, "To sign in, open this page and confirm the code:")?;
        writeln!(
            err,
            "  {}",
            code.verification_uri_complete
                .as_deref()
                .unwrap_or(&code.verification_uri)
        )?;
        writeln!(err, "  code: {}", code.user_code)?;
        writeln!(err, "Waiting for you to confirm…")?;
        drop(err);
        let tokens = flow.wait(&code).await?;
        Session {
            server: server.to_string(),
            user_id: String::new(),
            kind: SessionKind::Oidc,
            access_token: tokens.access_token,
            access_expires_at: tokens
                .expires_in
                .map(|s| now + chrono::Duration::seconds(i64::try_from(s).unwrap_or(0))),
            refresh_token: tokens.refresh_token,
            token_endpoint: auth.token_endpoint.clone(),
            revocation_endpoint: auth.revocation_endpoint.clone(),
            client_id: auth.cli_client_id.clone(),
        }
    };
    // The server knows the account; this also creates it on first use.
    let account = anonymous
        .clone()
        .with_token(&session.access_token)
        .account()
        .await?;
    session.user_id = account.id.clone();
    let vault = vault()?;
    vault.save(&session)?;
    let mut out = anstream::stdout().lock();
    finish(
        writeln!(
            out,
            "Signed in to {server} as {} ({}).\nRun `jobhunt sync` to sync this machine's \
             profile, preferences and feedback.",
            account.id, account.authenticated_with
        ),
        "the sign-in summary",
    )
}

pub async fn logout(args: LogoutArgs) -> anyhow::Result<ExitCode> {
    let vault = vault()?;
    let Some(session) = vault.load()? else {
        println!("Not signed in.");
        return Ok(ExitCode::SUCCESS);
    };
    if args.everywhere {
        let (client, _) = signed_in(&vault).await?;
        client.logout_everywhere().await?;
    }
    if let (Some(endpoint), Some(refresh), Some(client_id)) = (
        &session.revocation_endpoint,
        &session.refresh_token,
        &session.client_id,
    ) {
        let flow = DeviceFlow::for_session(
            session.token_endpoint.as_deref().unwrap_or_default(),
            client_id,
        )?;
        if let Err(e) = flow.revoke(endpoint, refresh).await {
            tracing::warn!(error = %e, "could not revoke the refresh token at the provider");
        }
    }
    vault.delete()?;
    println!(
        "Signed out of {}{}. Local data is unchanged.",
        session.server,
        if args.everywhere {
            " on every device"
        } else {
            ""
        }
    );
    Ok(ExitCode::SUCCESS)
}

pub async fn account(args: AccountArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let vault = vault()?;
    let (client, session) = signed_in(&vault).await?;
    let account = client.account().await?;
    if args.json {
        return print_json(&account);
    }
    let app = crate::local::open(loaded).await?;
    let status = app.sync_status().await;
    app.close().await;
    let status = status?;
    let mut out = anstream::stdout().lock();
    let c = &account.cloud;
    let result = (|| -> std::io::Result<()> {
        writeln!(out, "Account:   {}", account.id)?;
        writeln!(out, "Server:    {}", session.server)?;
        writeln!(
            out,
            "Signed in: {} ({:?})",
            account.authenticated_with, session.kind
        )?;
        for i in &account.identities {
            writeln!(
                out,
                "Identity:  {} (since {})",
                i.issuer,
                i.created_at.date_naive()
            )?;
        }
        writeln!(
            out,
            "Cloud:     profile revision {}, {} feedback, last sync {}",
            c.profile_revision,
            c.feedback,
            c.last_sync_at
                .map_or_else(|| "never".into(), |t| t.to_rfc3339())
        )?;
        writeln!(
            out,
            "This machine: last sync {}, {} records synced, {} feedback not yet synced, {} conflicts",
            status
                .account
                .as_ref()
                .and_then(|a| a.last_sync_at)
                .map_or_else(|| "never".into(), |t| t.to_rfc3339()),
            status.synced_records,
            status.unsynced_feedback,
            status.conflicts.len()
        )
    })();
    finish(result, "the account")
}

fn parse_record(id: &str) -> anyhow::Result<EntityKey> {
    let kind = match id.split_once('_').map(|(p, _)| p) {
        Some("clm") => EntityKind::Claim,
        Some("exp") => EntityKind::Experience,
        Some("proj") => EntityKind::Project,
        Some("edu") => EntityKind::Education,
        Some("skill") => EntityKind::Skill,
        Some("pref") => EntityKind::Preference,
        Some("stmt") => EntityKind::Statement,
        Some("doc") => EntityKind::Document,
        Some("prof") => EntityKind::Profile,
        _ => anyhow::bail!("{id:?} is not a profile record id (clm_…, exp_…, pref_…, …)"),
    };
    Ok(EntityKey::new(kind, id))
}

fn print_report(out: &mut impl Write, report: &SyncReport) -> std::io::Result<()> {
    writeln!(
        out,
        "Synced: {} records from the cloud, {} sent, {} merged; feedback {} received, {} sent; \
         {} jobs and {} verifications received.",
        report.applied_locally,
        report.pushed,
        report.merged,
        report.feedback_pulled,
        report.feedback_pushed,
        report.jobs_pulled,
        report.verifications_pulled
    )?;
    if report.conflicts.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "\n{} conflict(s) need you (nothing was overwritten; this machine keeps its version):",
        report.conflicts.len()
    )?;
    for c in &report.conflicts {
        writeln!(out, "  {} {}: {}", c.kind, c.id, c.reason)?;
        writeln!(out, "      here:  {}", c.local)?;
        writeln!(out, "      cloud: {}", c.cloud)?;
    }
    writeln!(
        out,
        "\nKeep one side with `jobhunt sync --keep local` or `--keep cloud` \
         (add `--record <id>` for one record)."
    )
}

pub async fn sync(args: SyncArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let app = crate::local::open(loaded).await?;
    let result = run_sync(&app, &args).await;
    app.close().await;
    result
}

async fn run_sync(app: &jobhunt_app::App, args: &SyncArgs) -> anyhow::Result<ExitCode> {
    if args.status {
        let status = app.sync_status().await?;
        let mut out = anstream::stdout().lock();
        let text = match &status.account {
            None => "This machine has never synced with JobHunt Cloud.".to_owned(),
            Some(a) => format!(
                "Syncs with {} as {}; last sync {}; {} records synced, {} feedback not yet \
                 synced, {} conflicts.",
                a.server,
                a.user_id,
                a.last_sync_at
                    .map_or_else(|| "never".into(), |t| t.to_rfc3339()),
                status.synced_records,
                status.unsynced_feedback,
                status.conflicts.len()
            ),
        };
        return finish(writeln!(out, "{text}"), "the sync status");
    }
    let side = args.keep.map(|k| match k {
        KeepArg::Local => Side::Local,
        KeepArg::Cloud => Side::Cloud,
    });
    let now = Utc::now();
    if let Some(side) = side {
        let keys = args
            .records
            .iter()
            .map(|r| parse_record(r))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let resolved = app.resolve_sync_conflicts(&keys, side, now).await?;
        if resolved > 0 {
            eprintln!("Resolved {resolved} conflict(s).");
        }
    }
    let vault = vault()?;
    let (client, session) = signed_in(&vault).await?;
    let remote = Remote {
        server: session.server.clone(),
        user_id: session.user_id.clone(),
    };
    // With --record, only those records were resolved; don't auto-resolve
    // others found now.
    let prefer = if args.records.is_empty() { side } else { None };
    let report = app.sync(&client, &remote, prefer, now).await?;
    if args.json {
        return print_json(&report);
    }
    let mut out = anstream::stdout().lock();
    finish(print_report(&mut out, &report), "the sync report")
}

pub async fn token(args: TokenArgs) -> anyhow::Result<ExitCode> {
    let vault = vault()?;
    let (client, _) = signed_in(&vault).await?;
    let mut out = anstream::stdout().lock();
    match args.command {
        TokenCommand::Create { name, days } => {
            let created = client.create_token(&name, days).await?;
            let result = writeln!(
                out,
                "Created {} ({}), expires {}.\nSecret (shown only now; use it as a bearer token):\n{}",
                created.token.id,
                created.token.name,
                created
                    .token
                    .expires_at
                    .map_or_else(|| "never".into(), |t| t.date_naive().to_string()),
                created.secret
            );
            finish(result, "the token")
        }
        TokenCommand::List => {
            let list = client.tokens().await?;
            let mut result = Ok(());
            if list.tokens.is_empty() {
                result = writeln!(out, "No personal access tokens.");
            }
            for t in &list.tokens {
                let state = if t.revoked_at.is_some() {
                    "revoked"
                } else {
                    "active"
                };
                result = result.and(writeln!(
                    out,
                    "{}  {:<24} {state:<8} created {} last used {}",
                    t.id,
                    t.name,
                    t.created_at.date_naive(),
                    t.last_used_at
                        .map_or_else(|| "never".into(), |d| d.date_naive().to_string())
                ));
            }
            finish(result, "the tokens")
        }
        TokenCommand::Revoke { id } => {
            client.revoke_token(&id).await?;
            finish(writeln!(out, "Revoked {id}."), "the confirmation")
        }
    }
}
