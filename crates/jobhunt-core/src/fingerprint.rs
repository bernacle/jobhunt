//! Content fingerprints used to detect whether a record changed between
//! observations.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::hex;

/// SHA-256 digest over a record's named content fields.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

/// Builds a [`Fingerprint`] from named fields.
///
/// Field names and values are length-prefixed and absent values are encoded
/// distinctly from empty ones, so the digest is unambiguous. Start with a
/// versioned schema tag (e.g. `"jobhunt.job.v1"`) and bump it whenever the set
/// of fingerprinted fields changes.
pub struct FingerprintBuilder(Sha256);

impl FingerprintBuilder {
    pub fn new(schema: &str) -> Self {
        let mut hasher = Sha256::new();
        write_bytes(&mut hasher, schema.as_bytes());
        Self(hasher)
    }

    pub fn field(mut self, name: &str, value: &str) -> Self {
        write_bytes(&mut self.0, name.as_bytes());
        self.0.update([1u8]);
        write_bytes(&mut self.0, value.as_bytes());
        self
    }

    pub fn optional(mut self, name: &str, value: Option<&str>) -> Self {
        match value {
            Some(value) => self.field(name, value),
            None => {
                write_bytes(&mut self.0, name.as_bytes());
                self.0.update([0u8]);
                self
            }
        }
    }

    pub fn finish(self) -> Fingerprint {
        Fingerprint(self.0.finalize().into())
    }
}

fn write_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(title: Option<&str>) -> Fingerprint {
        FingerprintBuilder::new("test.v1")
            .field("company", "Acme")
            .optional("title", title)
            .finish()
    }

    #[test]
    fn same_content_same_fingerprint() {
        assert_eq!(build(Some("Engineer")), build(Some("Engineer")));
    }

    #[test]
    fn changed_content_changes_fingerprint() {
        assert_ne!(build(Some("Engineer")), build(Some("Senior Engineer")));
    }

    #[test]
    fn absent_differs_from_empty() {
        assert_ne!(build(None), build(Some("")));
    }

    #[test]
    fn schema_tag_is_part_of_the_digest() {
        let a = FingerprintBuilder::new("v1").field("a", "b").finish();
        let b = FingerprintBuilder::new("v2").field("a", "b").finish();
        assert_ne!(a, b);
        assert_eq!(a.to_hex().len(), 64);
    }
}
