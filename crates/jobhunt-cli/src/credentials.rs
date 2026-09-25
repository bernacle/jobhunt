//! Where `jobhunt login` keeps the session.
//!
//! One session at a time: the server, the account, and the tokens to reach
//! it. On macOS and Windows it lives in the system keychain (Keychain,
//! Credential Manager). Elsewhere, and when `JOBHUNT_CREDENTIALS_FILE`
//! names a file, it is a JSON file readable only by its owner (mode 0600
//! in a 0700 directory), as `gh` and cloud CLIs do; its path is shown by
//! `jobhunt doctor`, its contents never are. `jobhunt logout` deletes it.

use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Overrides where the session is stored (a file; tests and headless use).
pub const CREDENTIALS_FILE_VAR: &str = "JOBHUNT_CREDENTIALS_FILE";

/// How the session authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Tokens from the identity provider (device sign-in).
    Oidc,
    /// A personal access token.
    Token,
    /// A development server's token.
    Dev,
}

/// A signed-in session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub server: String,
    pub user_id: String,
    pub kind: SessionKind,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// For refreshing and revoking without asking the server again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("server", &self.server)
            .field("user_id", &self.user_id)
            .field("kind", &self.kind)
            .field("access_expires_at", &self.access_expires_at)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// The access token expires within a minute.
    pub fn expiring(&self, now: DateTime<Utc>) -> bool {
        self.access_expires_at
            .is_some_and(|at| at <= now + chrono::Duration::seconds(60))
    }
}

/// Where the session is kept.
#[derive(Debug, Clone)]
pub enum Vault {
    File(PathBuf),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Keychain,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keychain_entry() -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new("jobhunt", "cloud-session").context("the system keychain is unavailable")
}

impl Vault {
    /// The vault for this machine.
    pub fn locate(data_dir: Option<&Path>) -> anyhow::Result<Self> {
        if let Ok(file) = std::env::var(CREDENTIALS_FILE_VAR)
            && !file.trim().is_empty()
        {
            return Ok(Self::File(PathBuf::from(file)));
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let _ = data_dir;
            Ok(Self::Keychain)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let dir = data_dir
                .context("no data directory for this platform; set JOBHUNT_CREDENTIALS_FILE")?;
            Ok(Self::File(dir.join("credentials.json")))
        }
    }

    /// Where, for `jobhunt doctor`.
    pub fn describe(&self) -> String {
        match self {
            Self::File(path) => format!("{} (owner-only file)", path.display()),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::Keychain => "the system keychain".into(),
        }
    }

    pub fn load(&self) -> anyhow::Result<Option<Session>> {
        let text = match self {
            Self::File(path) => match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => {
                    return Err(e).with_context(|| format!("could not read {}", path.display()));
                }
            },
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::Keychain => match keychain_entry()?.get_password() {
                Ok(text) => text,
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(e) => return Err(e).context("could not read the keychain"),
            },
        };
        serde_json::from_str(&text)
            .map(Some)
            .context("the stored session is unreadable; run `jobhunt login` again")
    }

    pub fn save(&self, session: &Session) -> anyhow::Result<()> {
        let text = serde_json::to_string(session).context("could not encode the session")?;
        match self {
            Self::File(path) => write_private(path, &text),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::Keychain => keychain_entry()?
                .set_password(&text)
                .context("could not write to the keychain"),
        }
    }

    pub fn delete(&self) -> anyhow::Result<bool> {
        match self {
            Self::File(path) => match std::fs::remove_file(path) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e).with_context(|| format!("could not delete {}", path.display())),
            },
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::Keychain => match keychain_entry()?.delete_credential() {
                Ok(()) => Ok(true),
                Err(keyring::Error::NoEntry) => Ok(false),
                Err(e) => Err(e).context("could not delete from the keychain"),
            },
        }
    }
}

/// Writes a file only its owner can read, atomically.
fn write_private(path: &Path, text: &str) -> anyhow::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let tmp = path.with_extension("tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("could not write {}", tmp.display()))?;
        std::io::Write::write_all(&mut file, text.as_bytes())
            .with_context(|| format!("could not write {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_round_trip_in_an_owner_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/credentials.json");
        let vault = Vault::File(path.clone());
        assert!(vault.load().unwrap().is_none());
        let session = Session {
            server: "https://api.example.com/".into(),
            user_id: "usr_0123".into(),
            kind: SessionKind::Oidc,
            access_token: "secret-access".into(),
            access_expires_at: Some(Utc::now()),
            refresh_token: Some("secret-refresh".into()),
            token_endpoint: None,
            revocation_endpoint: None,
            client_id: None,
        };
        vault.save(&session).unwrap();
        assert_eq!(vault.load().unwrap(), Some(session.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        assert!(!format!("{session:?}").contains("secret"));
        assert!(session.expiring(Utc::now()));
        assert!(vault.delete().unwrap());
        assert!(!vault.delete().unwrap());
    }
}
