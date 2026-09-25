//! Accounts: internal user ids, the identity-provider identities that map
//! to them, personal access tokens, and "log out everywhere".
//!
//! JobHunt keeps no passwords and no personal details about an account:
//! an identity is the `(issuer, subject)` pair of the provider's tokens.
//! Personal access tokens are random 256-bit secrets shown once; only
//! their SHA-256 digest is stored, so a database leak does not leak usable
//! tokens.

use std::fmt;
use std::str::FromStr;

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;
use base64::Engine;
use chrono::{DateTime, Utc};
use jobhunt_jobs::StorageError;
use sha2::{Digest, Sha256};
use sqlx::Row;

use super::{PgStore, corrupt, query_error};

/// The internal id of an account, `usr_<32 hex>`. Stable for the life of
/// the account and never derived from the identity provider's data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserId(String);

impl UserId {
    pub const PREFIX: &'static str = "usr_";

    /// A new random id.
    pub fn generate() -> Self {
        Self(format!("{}{}", Self::PREFIX, random_hex(16)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for UserId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.strip_prefix(Self::PREFIX).is_some_and(|hex| {
            hex.len() == 32
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!(
                "{s:?} is not a user id (usr_ followed by 32 hex digits)"
            ))
        }
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest stored for a token.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Prefix of personal access tokens, so they are recognizable (and
/// scannable by secret scanners).
pub const TOKEN_PREFIX: &str = "jh_pat_";

/// One identity-provider identity linked to an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityLink {
    pub issuer: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: DateTime<Utc>,
}

/// An account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: UserId,
    pub created_at: DateTime<Utc>,
    /// Tokens issued before this are refused.
    pub tokens_valid_after: Option<DateTime<Utc>>,
    pub identities: Vec<IdentityLink>,
    /// Whether this request created the account.
    pub created_now: bool,
}

/// A personal access token, without its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiToken {
    /// `tok_<16 hex>`.
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// A token just created: the only time its secret exists outside the
/// client.
#[derive(Debug, Clone)]
pub struct NewApiToken {
    pub token: ApiToken,
    pub secret: String,
}

/// Who a valid personal access token belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCheck {
    pub user: UserId,
    pub token_id: String,
}

fn parse_user(raw: &str) -> Result<UserId, StorageError> {
    raw.parse().map_err(|e: String| corrupt(raw, e))
}

impl PgStore {
    /// The account for an identity, created on first sign-in.
    pub async fn sign_in(
        &self,
        issuer: &str,
        subject: &str,
        now: DateTime<Utc>,
    ) -> Result<Account, StorageError> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        // Two first sign-ins racing: the identity's primary key decides.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("jobhunt.identity:{issuer}\n{subject}"))
            .execute(&mut *tx)
            .await
            .map_err(query_error("locking an identity"))?;
        let existing: Option<String> = sqlx::query_scalar(
            "UPDATE user_identities SET last_login_at = $3 WHERE issuer = $1 AND subject = $2 \
             RETURNING user_id",
        )
        .bind(issuer)
        .bind(subject)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(query_error("finding an identity"))?;
        let (user, created) = match existing {
            Some(raw) => (parse_user(&raw)?, false),
            None => {
                let user = UserId::generate();
                sqlx::query("INSERT INTO users (id, created_at) VALUES ($1, $2)")
                    .bind(user.as_str())
                    .bind(now)
                    .execute(&mut *tx)
                    .await
                    .map_err(query_error("creating an account"))?;
                sqlx::query("INSERT INTO user_state (user_id) VALUES ($1)")
                    .bind(user.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(query_error("creating an account"))?;
                sqlx::query(
                    "INSERT INTO user_identities (issuer, subject, user_id, created_at, \
                     last_login_at) VALUES ($1, $2, $3, $4, $4)",
                )
                .bind(issuer)
                .bind(subject)
                .bind(user.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(query_error("linking an identity"))?;
                (user, true)
            }
        };
        tx.commit()
            .await
            .map_err(query_error("committing a sign-in"))?;
        let mut account = self
            .account(&user)
            .await?
            .ok_or_else(|| corrupt(user.as_str(), "account vanished during sign-in"))?;
        account.created_now = created;
        Ok(account)
    }

    /// An account by id.
    pub async fn account(&self, user: &UserId) -> Result<Option<Account>, StorageError> {
        let Some(row) =
            sqlx::query("SELECT id, created_at, tokens_valid_after FROM users WHERE id = $1")
                .bind(user.as_str())
                .fetch_optional(self.pool())
                .await
                .map_err(query_error("loading an account"))?
        else {
            return Ok(None);
        };
        let identities = sqlx::query(
            "SELECT issuer, created_at, last_login_at FROM user_identities WHERE user_id = $1 \
             ORDER BY created_at",
        )
        .bind(user.as_str())
        .fetch_all(self.pool())
        .await
        .map_err(query_error("loading identities"))?
        .iter()
        .map(|r| {
            Ok(IdentityLink {
                issuer: r.try_get("issuer").map_err(|e| corrupt(user.as_str(), e))?,
                created_at: r
                    .try_get("created_at")
                    .map_err(|e| corrupt(user.as_str(), e))?,
                last_login_at: r
                    .try_get("last_login_at")
                    .map_err(|e| corrupt(user.as_str(), e))?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
        Ok(Some(Account {
            id: user.clone(),
            created_at: row
                .try_get("created_at")
                .map_err(|e| corrupt(user.as_str(), e))?,
            tokens_valid_after: row
                .try_get("tokens_valid_after")
                .map_err(|e| corrupt(user.as_str(), e))?,
            identities,
            created_now: false,
        }))
    }

    /// Refuses every token issued before `at` and revokes every personal
    /// access token ("log out everywhere").
    pub async fn revoke_sessions(
        &self,
        user: &UserId,
        at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let mut tx = self
            .pool()
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        sqlx::query("UPDATE users SET tokens_valid_after = $2 WHERE id = $1")
            .bind(user.as_str())
            .bind(at)
            .execute(&mut *tx)
            .await
            .map_err(query_error("revoking sessions"))?;
        sqlx::query(
            "UPDATE api_tokens SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user.as_str())
        .bind(at)
        .execute(&mut *tx)
        .await
        .map_err(query_error("revoking tokens"))?;
        tx.commit()
            .await
            .map_err(query_error("committing a revocation"))
    }

    /// Deletes an account and everything private to it. Shared jobs stay.
    pub async fn delete_account(&self, user: &UserId) -> Result<bool, StorageError> {
        let deleted = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.as_str())
            .execute(self.pool())
            .await
            .map_err(query_error("deleting an account"))?
            .rows_affected();
        Ok(deleted > 0)
    }

    /// Creates a personal access token.
    pub async fn create_api_token(
        &self,
        user: &UserId,
        name: &str,
        expires_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<NewApiToken, StorageError> {
        let secret = format!(
            "{TOKEN_PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>())
        );
        let token = ApiToken {
            id: format!("tok_{}", random_hex(8)),
            name: name.to_owned(),
            created_at: now,
            last_used_at: None,
            expires_at,
            revoked_at: None,
        };
        sqlx::query(
            "INSERT INTO api_tokens (id, user_id, name, token_hash, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&token.id)
        .bind(user.as_str())
        .bind(name)
        .bind(hash_token(&secret))
        .bind(now)
        .bind(expires_at)
        .execute(self.pool())
        .await
        .map_err(query_error("creating a token"))?;
        Ok(NewApiToken { token, secret })
    }

    /// Personal access tokens of an account, newest first.
    pub async fn api_tokens(&self, user: &UserId) -> Result<Vec<ApiToken>, StorageError> {
        sqlx::query(
            "SELECT id, name, created_at, last_used_at, expires_at, revoked_at FROM api_tokens \
             WHERE user_id = $1 ORDER BY created_at DESC, id",
        )
        .bind(user.as_str())
        .fetch_all(self.pool())
        .await
        .map_err(query_error("listing tokens"))?
        .iter()
        .map(|r| {
            let bad = |e: sqlx::Error| corrupt("<api_tokens>", e);
            Ok(ApiToken {
                id: r.try_get("id").map_err(bad)?,
                name: r.try_get("name").map_err(bad)?,
                created_at: r.try_get("created_at").map_err(bad)?,
                last_used_at: r.try_get("last_used_at").map_err(bad)?,
                expires_at: r.try_get("expires_at").map_err(bad)?,
                revoked_at: r.try_get("revoked_at").map_err(bad)?,
            })
        })
        .collect()
    }

    /// Revokes one of the account's tokens. Returns whether it existed.
    pub async fn revoke_api_token(
        &self,
        user: &UserId,
        token_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let changed = sqlx::query(
            "UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, $3) \
             WHERE user_id = $1 AND id = $2",
        )
        .bind(user.as_str())
        .bind(token_id)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(query_error("revoking a token"))?
        .rows_affected();
        Ok(changed > 0)
    }

    /// The account a personal access token belongs to, if it is valid now
    /// (not revoked, not expired). Records its use.
    pub async fn check_api_token(
        &self,
        secret: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<TokenCheck>, StorageError> {
        if !secret.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let row = sqlx::query(
            "UPDATE api_tokens SET last_used_at = $2 WHERE token_hash = $1 \
             AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > $2) \
             RETURNING id, user_id",
        )
        .bind(hash_token(secret))
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(query_error("checking a token"))?;
        row.map(|r| {
            let raw: String = r
                .try_get("user_id")
                .map_err(|e| corrupt("<api_tokens>", e))?;
            Ok(TokenCheck {
                user: parse_user(&raw)?,
                token_id: r.try_get("id").map_err(|e| corrupt("<api_tokens>", e))?,
            })
        })
        .transpose()
    }

    /// Number of accounts (diagnostics).
    pub async fn account_count(&self) -> Result<u64, StorageError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(self.pool())
            .await
            .map_err(query_error("counting accounts"))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_validated() {
        let id = UserId::generate();
        assert_eq!(id.as_str().parse::<UserId>(), Ok(id.clone()));
        assert!("usr_123".parse::<UserId>().is_err());
        assert!(
            "usr_ZZ000000000000000000000000000000"
                .parse::<UserId>()
                .is_err()
        );
        assert_ne!(UserId::generate(), id);
    }

    #[test]
    fn token_hashes_are_stable_and_opaque() {
        assert_eq!(hash_token("jh_pat_x"), hash_token("jh_pat_x"));
        assert_ne!(hash_token("jh_pat_x"), hash_token("jh_pat_y"));
        assert_eq!(hash_token("jh_pat_x").len(), 32);
    }
}
