//! Application-level encryption of private values stored in Postgres.
//!
//! Every profile record, feedback reason, profile history detail,
//! eligibility decision and ranking is sealed with AES-256-GCM (the
//! RustCrypto `aes-gcm` implementation; nothing here is home-grown
//! cryptography) before it reaches the database, so a database dump, a
//! backup or a misdirected query shows ciphertext, not resumes.
//!
//! * Keys come from configuration (`JOBHUNT_ENCRYPTION_KEYS`), never from
//!   code: a comma-separated list of `<key id>:<base64 of 32 bytes>`. The
//!   first key encrypts; every listed key decrypts. Rotating means putting
//!   a new key first, redeploying, and running `jobhunt admin reencrypt`
//!   before removing the old one.
//! * A sealed value is `0x01 | id length | key id | 96-bit random nonce |
//!   ciphertext and tag`, so it names the key it needs.
//! * The associated data binds a value to where it belongs (table, account,
//!   record id): a ciphertext copied into another account's row, or onto
//!   another record, fails to decrypt instead of being read as theirs.

use std::collections::HashMap;
use std::fmt;

use aes_gcm::aead::{Aead, Generate, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;

const FORMAT: u8 = 1;
const NONCE: usize = 12;

/// Why a keyring could not be built, or a value not opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("no encryption key configured (JOBHUNT_ENCRYPTION_KEYS)")]
    NoKeys,
    #[error("invalid encryption key entry {entry}: {reason}")]
    InvalidKey { entry: usize, reason: String },
    #[error("encryption key {0:?} is listed twice")]
    Duplicate(String),
    #[error("value was sealed with key {0:?}, which is not configured")]
    UnknownKey(String),
    #[error("sealed value is malformed")]
    Malformed,
    #[error("sealed value does not open with its key and context (tampered or misplaced)")]
    Unauthentic,
    #[error("encryption failed")]
    Seal,
}

/// The configured keys.
#[derive(Clone)]
pub struct Keyring {
    active: String,
    keys: HashMap<String, Aes256Gcm>,
}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the key material.
        let mut ids: Vec<&String> = self.keys.keys().collect();
        ids.sort();
        f.debug_struct("Keyring")
            .field("active", &self.active)
            .field("keys", &ids)
            .finish()
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl Keyring {
    /// Parses `id:base64key[,id:base64key…]`; the first key is active.
    pub fn parse(spec: &str) -> Result<Self, CryptoError> {
        let mut active = None;
        let mut keys = HashMap::new();
        for (i, entry) in spec
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .enumerate()
        {
            let invalid = |reason: &str| CryptoError::InvalidKey {
                entry: i + 1,
                reason: reason.to_owned(),
            };
            let (id, material) = entry
                .split_once(':')
                .ok_or_else(|| invalid("expected <key id>:<base64 key>"))?;
            let id = id.trim();
            if !valid_id(id) {
                return Err(invalid(
                    "the key id must be 1-32 characters of a-z, 0-9, '-' or '_'",
                ));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(material.trim())
                .map_err(|_| invalid("the key is not valid base64"))?;
            if bytes.len() != 32 {
                return Err(invalid(
                    "the key must be 32 bytes (openssl rand -base64 32)",
                ));
            }
            let cipher = Aes256Gcm::new_from_slice(&bytes)
                .map_err(|_| invalid("the key must be 32 bytes (openssl rand -base64 32)"))?;
            if keys.insert(id.to_owned(), cipher).is_some() {
                return Err(CryptoError::Duplicate(id.to_owned()));
            }
            active.get_or_insert_with(|| id.to_owned());
        }
        Ok(Self {
            active: active.ok_or(CryptoError::NoKeys)?,
            keys,
        })
    }

    /// A keyring with one random key (tests, local development).
    pub fn ephemeral() -> Self {
        let key = Key::<Aes256Gcm>::generate();
        let spec = format!(
            "ephemeral:{}",
            base64::engine::general_purpose::STANDARD.encode(key)
        );
        match Self::parse(&spec) {
            Ok(k) => k,
            Err(_) => unreachable!("a generated key is always valid"),
        }
    }

    /// The id of the key new values are sealed with.
    pub fn active_key(&self) -> &str {
        &self.active
    }

    /// Ids of every configured key.
    pub fn key_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.keys.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Encrypts `plaintext` for `context` with the active key.
    pub fn seal(&self, context: &str, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let cipher = self.keys.get(&self.active).ok_or(CryptoError::NoKeys)?;
        let nonce = Nonce::generate();
        let sealed = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: context.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Seal)?;
        let id = self.active.as_bytes();
        let mut out = Vec::with_capacity(2 + id.len() + NONCE + sealed.len());
        out.push(FORMAT);
        out.push(u8::try_from(id.len()).map_err(|_| CryptoError::Seal)?);
        out.extend_from_slice(id);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// The id of the key a sealed value needs.
    pub fn key_of(sealed: &[u8]) -> Result<&str, CryptoError> {
        let (&format, rest) = sealed.split_first().ok_or(CryptoError::Malformed)?;
        let (&len, rest) = rest.split_first().ok_or(CryptoError::Malformed)?;
        if format != FORMAT || rest.len() < usize::from(len) {
            return Err(CryptoError::Malformed);
        }
        std::str::from_utf8(&rest[..usize::from(len)]).map_err(|_| CryptoError::Malformed)
    }

    /// Decrypts a value sealed for `context`.
    pub fn open(&self, context: &str, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let id = Self::key_of(sealed)?;
        let cipher = self
            .keys
            .get(id)
            .ok_or_else(|| CryptoError::UnknownKey(id.to_owned()))?;
        let rest = &sealed[2 + id.len()..];
        if rest.len() < NONCE + 16 {
            return Err(CryptoError::Malformed);
        }
        let (nonce, ciphertext) = rest.split_at(NONCE);
        cipher
            .decrypt(
                &Nonce::try_from(nonce).map_err(|_| CryptoError::Malformed)?,
                Payload {
                    msg: ciphertext,
                    aad: context.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Unauthentic)
    }

    /// Whether a sealed value uses the active key (rotation).
    pub fn is_current(&self, sealed: &[u8]) -> bool {
        Self::key_of(sealed).is_ok_and(|id| id == self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> String {
        base64::engine::general_purpose::STANDARD.encode([byte; 32])
    }

    #[test]
    fn seals_and_opens_with_context() {
        let ring = Keyring::parse(&format!("k1:{}", key(1))).unwrap();
        let sealed = ring.seal("claims|usr_a|clm_1", b"secret resume").unwrap();
        assert!(!sealed.windows(6).any(|w| w == b"secret"));
        assert_eq!(
            ring.open("claims|usr_a|clm_1", &sealed).unwrap(),
            b"secret resume"
        );
        assert_eq!(
            ring.open("claims|usr_b|clm_1", &sealed),
            Err(CryptoError::Unauthentic),
            "another account's context must not open it"
        );
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(
            ring.open("claims|usr_a|clm_1", &tampered),
            Err(CryptoError::Unauthentic)
        );
        // Random nonces: sealing twice differs.
        assert_ne!(
            sealed,
            ring.seal("claims|usr_a|clm_1", b"secret resume").unwrap()
        );
    }

    /// A value sealed by an earlier build (aes-gcm 0.10) must keep opening:
    /// the database holds values sealed by every release since.
    #[test]
    fn opens_values_sealed_by_earlier_releases() {
        let ring = Keyring::parse(&format!("k1:{}", key(7))).unwrap();
        let sealed: Vec<u8> = (0..SEALED_BY_AES_GCM_0_10.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&SEALED_BY_AES_GCM_0_10[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(Keyring::key_of(&sealed).unwrap(), "k1");
        assert_eq!(
            ring.open("claims|usr_a|clm_1", &sealed).unwrap(),
            b"secret resume"
        );
        assert_eq!(
            ring.open("claims|usr_b|clm_1", &sealed),
            Err(CryptoError::Unauthentic)
        );
    }

    /// `seal("claims|usr_a|clm_1", b"secret resume")` under key `k1` = 32
    /// bytes of 7, as written by aes-gcm 0.10.3.
    const SEALED_BY_AES_GCM_0_10: &str = "01026b3164624b9d181a254442ad5ff27e1ed805790d0425aedacf040eca551a6c758483289c39a6cd26b70de6";

    #[test]
    fn rotation_keeps_old_values_readable() {
        let old = Keyring::parse(&format!("k1:{}", key(1))).unwrap();
        let sealed = old.seal("ctx", b"v").unwrap();
        let rotated = Keyring::parse(&format!("k2:{}, k1:{}", key(2), key(1))).unwrap();
        assert_eq!(rotated.active_key(), "k2");
        assert!(!rotated.is_current(&sealed));
        assert_eq!(rotated.open("ctx", &sealed).unwrap(), b"v");
        let resealed = rotated.seal("ctx", b"v").unwrap();
        assert!(rotated.is_current(&resealed));
        let only_new = Keyring::parse(&format!("k2:{}", key(2))).unwrap();
        assert_eq!(
            only_new.open("ctx", &sealed),
            Err(CryptoError::UnknownKey("k1".into()))
        );
    }

    #[test]
    fn rejects_bad_configuration() {
        assert_eq!(Keyring::parse("").unwrap_err(), CryptoError::NoKeys);
        assert!(matches!(
            Keyring::parse("k1:short"),
            Err(CryptoError::InvalidKey { .. })
        ));
        assert!(matches!(
            Keyring::parse(&format!("Bad Id:{}", key(1))),
            Err(CryptoError::InvalidKey { .. })
        ));
        assert_eq!(
            Keyring::parse(&format!("k1:{},k1:{}", key(1), key(2))).unwrap_err(),
            CryptoError::Duplicate("k1".into())
        );
        let ring = Keyring::parse(&format!("k1:{}", key(1))).unwrap();
        assert!(!format!("{ring:?}").contains(&key(1)));
        assert_eq!(ring.open("ctx", b"\x01"), Err(CryptoError::Malformed));
    }
}
