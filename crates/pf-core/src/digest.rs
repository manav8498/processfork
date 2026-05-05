// SPDX-License-Identifier: MIT
//! SHA-256 content digests, OCI-style.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::error::{Error, Result};

/// A SHA-256 content digest, formatted `sha256:<64-hex>` per OCI conventions.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Digest256(String);

impl Digest256 {
    /// Compute the digest of a byte slice.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hex = hex::encode(hasher.finalize());
        Self(format!("sha256:{hex}"))
    }

    /// Borrow the canonical `sha256:<hex>` string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrow just the 64-char hex part (without the `sha256:` prefix).
    #[must_use]
    pub fn hex(&self) -> &str {
        // safe: format is validated on construction
        &self.0["sha256:".len()..]
    }

    /// Parse a `sha256:<64-hex>` string. Errors on bad prefix or length.
    pub fn parse(s: &str) -> Result<Self> {
        let Some(rest) = s.strip_prefix("sha256:") else {
            return Err(Error::InvalidDigest(format!(
                "missing sha256: prefix in {s:?}"
            )));
        };
        if rest.len() != 64 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidDigest(format!("not 64 hex chars: {s:?}")));
        }
        Ok(Self(s.to_owned()))
    }
}

impl fmt::Display for Digest256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_known_sha256() {
        let d = Digest256::of(b"");
        assert_eq!(
            d.as_str(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn deterministic_across_calls() {
        let payload = b"processfork";
        assert_eq!(Digest256::of(payload), Digest256::of(payload));
    }

    #[test]
    fn parses_canonical_form() {
        let d = Digest256::of(b"hi");
        let parsed = Digest256::parse(d.as_str()).unwrap();
        assert_eq!(d, parsed);
        assert_eq!(parsed.hex().len(), 64);
    }

    #[test]
    fn rejects_bad_prefix_or_length() {
        assert!(Digest256::parse("md5:abc").is_err());
        assert!(Digest256::parse("sha256:short").is_err());
        assert!(Digest256::parse("sha256:zzzz").is_err());
    }
}
