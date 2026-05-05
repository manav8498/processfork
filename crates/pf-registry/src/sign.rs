// SPDX-License-Identifier: MIT
//! Manifest signing. v1 ships the wire format + a self-signed
//! HMAC-SHA256 mode. Sigstore Fulcio (keyless) is feature-gated for
//! v1.1.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Wire format for a manifest signature blob (`<manifest>.sig`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestSignature {
    /// Algorithm tag. v1 only emits `"hmac-sha256"`; v1.1 will add
    /// `"cosign-keyless"` and `"cosign-key"`.
    pub algorithm: String,
    /// SHA-256 of the canonical-JSON manifest bytes (the bytes we
    /// signed).
    pub manifest_digest: String,
    /// Signature value, hex-encoded.
    pub signature: String,
    /// Optional cosign certificate / Rekor entry (v1.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
}

const DEFAULT_KEY: &[u8] = b"processfork-default-self-sign-key.v1";

/// Sign the manifest bytes. `key` overrides the v1 default; `None`
/// uses [`DEFAULT_KEY`] (anyone can verify, but it's clear the
/// signature is self-signed).
pub fn sign_manifest(manifest_bytes: &[u8], key: Option<&str>) -> ManifestSignature {
    let manifest_digest = sha256_hex(manifest_bytes);
    let key_bytes = key.map_or(DEFAULT_KEY, str::as_bytes);
    let signature = hmac_sha256_hex(key_bytes, manifest_bytes);
    ManifestSignature {
        algorithm: "hmac-sha256".into(),
        manifest_digest,
        signature,
        bundle: None,
    }
}

/// Verify a signature against the manifest bytes.
///
/// # Errors
/// Returns a human-readable string when the signature is malformed,
/// uses an unknown algorithm, or doesn't validate.
pub fn verify_manifest(
    manifest_bytes: &[u8],
    sig: &ManifestSignature,
    key: Option<&str>,
) -> Result<(), String> {
    if sig.algorithm != "hmac-sha256" {
        return Err(format!(
            "unsupported signature algorithm {:?} (v1 supports only hmac-sha256; \
             cosign verifier lands in v1.1)",
            sig.algorithm
        ));
    }
    let observed = sha256_hex(manifest_bytes);
    if observed != sig.manifest_digest {
        return Err(format!(
            "manifest digest mismatch: bytes hash to {observed}, sig records {}",
            sig.manifest_digest
        ));
    }
    let key_bytes = key.map_or(DEFAULT_KEY, str::as_bytes);
    let expected = hmac_sha256_hex(key_bytes, manifest_bytes);
    // Constant-time compare via subtle would be nice; for v1 we use the
    // straight-line compare. The HMAC tag itself is only as confidential
    // as the key; for the default-key mode the signature is trivially
    // forgeable (and that's documented).
    if expected != sig.signature {
        return Err("HMAC tag does not validate".into());
    }
    Ok(())
}

fn sha256_hex(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    hex::encode(h.finalize())
}

fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String {
    use ring::hmac;
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    hex::encode(hmac::sign(&k, msg).as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_then_verify_round_trip() {
        let bytes = b"{\"hello\":\"world\"}";
        let sig = sign_manifest(bytes, None);
        verify_manifest(bytes, &sig, None).unwrap();
    }

    #[test]
    fn verify_fails_on_tampered_manifest() {
        let sig = sign_manifest(b"original", None);
        assert!(verify_manifest(b"tampered", &sig, None).is_err());
    }

    #[test]
    fn verify_fails_on_wrong_key() {
        let sig = sign_manifest(b"x", Some("key-a"));
        assert!(verify_manifest(b"x", &sig, Some("key-b")).is_err());
    }

    #[test]
    fn verify_rejects_unknown_algorithm() {
        let mut sig = sign_manifest(b"x", None);
        sig.algorithm = "future-cosine".into();
        assert!(verify_manifest(b"x", &sig, None).is_err());
    }
}
