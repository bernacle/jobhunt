//! Deterministic identifiers.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::hex;

/// A deterministic 128-bit identifier.
///
/// The same `(namespace, parts)` input always yields the same identifier, on
/// every machine and across runs, which is what lets repeated discovery of the
/// same record land on the same row instead of creating duplicates.
///
/// Each input field is length-prefixed before hashing, so `["ab", "c"]` and
/// `["a", "bc"]` produce different identifiers. The namespace should include a
/// version (for example `"jobhunt.job.v1"`) so the derivation can evolve
/// deliberately.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StableId([u8; 16]);

impl StableId {
    pub fn derive(namespace: &str, parts: &[&str]) -> Self {
        let mut hasher = Sha256::new();
        write_field(&mut hasher, namespace);
        for part in parts {
            write_field(&mut hasher, part);
        }
        let digest = hasher.finalize();
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Lowercase, 32-character hex representation.
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }
}

fn write_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StableId({})", self.to_hex())
    }
}

impl FromStr for StableId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        hex::decode::<16>(s)
            .map(Self)
            .ok_or_else(|| ParseIdError(s.to_owned()))
    }
}

/// The string is not a 32-character hex identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a valid identifier")]
pub struct ParseIdError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        let a = StableId::derive("test.v1", &["ashby", "linear", "123"]);
        let b = StableId::derive("test.v1", &["ashby", "linear", "123"]);
        assert_eq!(a, b);
    }

    #[test]
    fn known_value_is_pinned() {
        // Pinned so an accidental change to the derivation (which would
        // orphan every stored row) fails loudly. Value computed independently:
        // sha256 over length-prefixed (u64 BE) fields, first 16 bytes.
        let id = StableId::derive("test.v1", &["ashby", "linear", "123"]);
        assert_eq!(id.to_hex(), "5bd57e160e8d9b717329359daa008607");
    }

    #[test]
    fn distinguishes_namespace_and_field_boundaries() {
        let base = StableId::derive("test.v1", &["ab", "c"]);
        assert_ne!(base, StableId::derive("test.v1", &["a", "bc"]));
        assert_ne!(base, StableId::derive("test.v2", &["ab", "c"]));
        assert_ne!(base, StableId::derive("test.v1", &["abc"]));
    }

    #[test]
    fn hex_round_trip() {
        let id = StableId::derive("test.v1", &["x"]);
        let parsed: StableId = id.to_hex().parse().unwrap();
        assert_eq!(parsed, id);
        assert_eq!(id.to_string().len(), 32);
        assert!("xyz".parse::<StableId>().is_err());
    }
}
