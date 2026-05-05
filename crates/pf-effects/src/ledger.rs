// SPDX-License-Identifier: MIT
//! Append-only effect ledger with HMAC chaining.

use chrono::{DateTime, Utc};
use ring::hmac;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;

/// How "dangerous" a tool call's side-effect is, for replay policy purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SideEffectClass {
    /// Pure function — safe to replay from cached result.
    Pure,
    /// Idempotent under the same `idempotency_key` (POST-with-key, PUT, …).
    Idempotent,
    /// Genuinely irreversible (sent email, charged card, dropped table).
    Irreversible,
    /// Network-only read (e.g. GET) — cacheable but stale-aware.
    NetworkOnly,
}

/// Per-session HMAC key. Wraps an opaque byte slice; debug-prints as
/// `SessionSecret(<redacted>)` so it never accidentally lands in logs.
#[derive(Clone)]
pub struct SessionSecret(Vec<u8>);

impl SessionSecret {
    /// Wrap an existing byte slice (e.g. from a hardware HSM or env var).
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Generate a fresh 32-byte secret using `ring::rand`.
    pub fn generate() -> pf_core::Result<Self> {
        use ring::rand::SecureRandom;
        let mut buf = [0u8; 32];
        ring::rand::SystemRandom::new()
            .fill(&mut buf)
            .map_err(|_| pf_core::Error::Integrity("RNG failed".into()))?;
        Ok(Self(buf.to_vec()))
    }

    fn key(&self) -> hmac::Key {
        hmac::Key::new(hmac::HMAC_SHA256, &self.0)
    }
}

impl std::fmt::Debug for SessionSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionSecret(<{} bytes redacted>)", self.0.len())
    }
}

/// A single ledger entry. Wire format `effects.entry.v1`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Wall-clock timestamp at the moment the tool call was issued.
    #[serde(rename = "ts")]
    pub timestamp: DateTime<Utc>,
    /// Tool identifier as registered with the [`crate::ToolProxy`].
    pub tool_id: String,
    /// SHA-256 of the canonical-JSON-serialized tool args.
    pub args_hash: Digest256,
    /// Per-call idempotency key. ULID-shaped for sortability; the proxy mints
    /// these and persists them so a re-issued call after restore reuses the
    /// key and is therefore safe under `Idempotent` semantics.
    pub idempotency_key: String,
    /// SHA-256 of the canonical-JSON-serialized tool result.
    pub result_hash: Digest256,
    /// Tool author's declared side-effect class.
    pub side_effect_class: SideEffectClass,
    /// HMAC chain: `HMAC(session_secret, prev_entry_hash || this_entry_minus_hmac)`.
    /// Hex-encoded.
    pub session_hmac: String,
}

impl LedgerEntry {
    /// SHA-256 of the canonical-JSON serialization of this entry **with
    /// `session_hmac = ""`**. Used both for chaining and for storage CAS.
    pub fn entry_hash_without_hmac(&self) -> pf_core::Result<Digest256> {
        let mut clone = self.clone();
        clone.session_hmac.clear();
        let bytes = serde_json::to_vec(&clone)?;
        Ok(Digest256::of(&bytes))
    }
}

/// An append-only ledger held in memory. Persistent storage is the
/// caller's responsibility: call [`Ledger::serialize`] to get a CAS-ready
/// blob, [`Ledger::deserialize`] to restore.
#[derive(Clone, Debug)]
pub struct Ledger {
    secret: SessionSecret,
    entries: Vec<LedgerEntry>,
}

impl Ledger {
    /// Open a fresh ledger with the given session secret.
    #[must_use]
    pub fn new(secret: SessionSecret) -> Self {
        Self {
            secret,
            entries: Vec::new(),
        }
    }

    /// Borrow the ledger entries in causal order.
    #[must_use]
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// Append a new entry and chain its HMAC. The caller supplies everything
    /// except `session_hmac`, which we compute here.
    pub fn append(
        &mut self,
        timestamp: DateTime<Utc>,
        tool_id: impl Into<String>,
        args_hash: Digest256,
        idempotency_key: impl Into<String>,
        result_hash: Digest256,
        side_effect_class: SideEffectClass,
    ) -> pf_core::Result<&LedgerEntry> {
        let mut entry = LedgerEntry {
            timestamp,
            tool_id: tool_id.into(),
            args_hash,
            idempotency_key: idempotency_key.into(),
            result_hash,
            side_effect_class,
            session_hmac: String::new(),
        };
        let prev = self
            .entries
            .last()
            .map(LedgerEntry::entry_hash_without_hmac)
            .transpose()?
            .map_or(String::new(), |d| d.hex().to_owned());
        let this = entry.entry_hash_without_hmac()?;
        let mut to_sign = Vec::with_capacity(prev.len() + this.hex().len());
        to_sign.extend_from_slice(prev.as_bytes());
        to_sign.extend_from_slice(this.hex().as_bytes());
        let tag = hmac::sign(&self.secret.key(), &to_sign);
        entry.session_hmac = hex::encode(tag.as_ref());
        self.entries.push(entry);
        Ok(self.entries.last().unwrap())
    }

    /// Verify the entire HMAC chain. Returns `Ok(())` if every entry verifies
    /// in order; otherwise the first bad-entry index in `Err`.
    pub fn verify(&self) -> pf_core::Result<()> {
        let mut prev_hash = String::new();
        for (ix, e) in self.entries.iter().enumerate() {
            let this = e.entry_hash_without_hmac()?;
            let mut to_sign = Vec::with_capacity(prev_hash.len() + this.hex().len());
            to_sign.extend_from_slice(prev_hash.as_bytes());
            to_sign.extend_from_slice(this.hex().as_bytes());
            let expected_tag = hex::decode(&e.session_hmac)
                .map_err(|err| pf_core::Error::Integrity(format!("entry {ix}: bad hex: {err}")))?;
            if hmac::verify(&self.secret.key(), &to_sign, &expected_tag).is_err() {
                return Err(pf_core::Error::Integrity(format!(
                    "ledger HMAC mismatch at entry index {ix}"
                )));
            }
            this.hex().clone_into(&mut prev_hash);
        }
        Ok(())
    }

    /// Serialize the ledger to a single JSONL blob and store via `blobs`.
    /// Note: the secret is NOT serialized — it must be re-supplied at
    /// deserialization time.
    pub fn serialize(&self, blobs: &dyn BlobStore) -> pf_core::Result<Digest256> {
        let mut out = Vec::new();
        for e in &self.entries {
            out.extend_from_slice(&serde_json::to_vec(e)?);
            out.push(b'\n');
        }
        // Prepend a header line for self-description.
        let mut blob = Vec::with_capacity(out.len() + 64);
        let header =
            serde_json::json!({"kind": "effects.ledger.v1", "entries": self.entries.len()});
        blob.extend_from_slice(&serde_json::to_vec(&header)?);
        blob.push(b'\n');
        blob.extend_from_slice(&out);
        blobs.put(&blob)
    }

    /// Restore a ledger from a previously-stored blob. The caller must supply
    /// the same `secret` that signed the chain; otherwise [`Self::verify`]
    /// will fail.
    pub fn deserialize(
        blobs: &dyn BlobStore,
        digest: &Digest256,
        secret: SessionSecret,
    ) -> pf_core::Result<Self> {
        let bytes = blobs.get(digest)?;
        let mut lines = bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty());
        let header = lines
            .next()
            .ok_or_else(|| pf_core::Error::Integrity("ledger blob has no header line".into()))?;
        let header_v: serde_json::Value = serde_json::from_slice(header)?;
        if header_v.get("kind").and_then(|v| v.as_str()) != Some("effects.ledger.v1") {
            return Err(pf_core::Error::Integrity(
                "not an effects.ledger.v1 blob".into(),
            ));
        }
        let mut entries = Vec::new();
        for line in lines {
            entries.push(serde_json::from_slice::<LedgerEntry>(line)?);
        }
        Ok(Self { secret, entries })
    }
}

/// Hash a serializable value via canonical-ish JSON (good enough — we depend
/// on `serde_json`'s stable field ordering + our types having no maps).
pub fn args_hash(args: &impl Serialize) -> pf_core::Result<Digest256> {
    Ok(Digest256::of(&serde_json::to_vec(args)?))
}

/// Mint a sortable idempotency key by combining a Unix timestamp prefix with
/// 80 bits of randomness, ULID-style.
pub fn mint_idempotency_key() -> pf_core::Result<String> {
    use ring::rand::SecureRandom;
    let mut rand = [0u8; 10];
    ring::rand::SystemRandom::new()
        .fill(&mut rand)
        .map_err(|_| pf_core::Error::Integrity("RNG failed".into()))?;
    let ts_ms = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(ts_ms.to_be_bytes());
    hasher.update(rand);
    let h = hasher.finalize();
    // 26-char base32-like string for ULID compatibility on length only.
    Ok(format!(
        "01J{:013}{}",
        ts_ms % 10_000_000_000_000,
        hex::encode(&h[..5])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    fn empty_ledger() -> Ledger {
        Ledger::new(SessionSecret::new(b"unit-test-secret".to_vec()))
    }

    fn fake_digest(byte: u8) -> Digest256 {
        Digest256::of(&[byte; 32])
    }

    #[test]
    fn appended_entry_has_hmac() {
        let mut l = empty_ledger();
        let e = l
            .append(
                Utc::now(),
                "send_email",
                fake_digest(1),
                "01JTEST".to_owned(),
                fake_digest(2),
                SideEffectClass::Irreversible,
            )
            .unwrap();
        assert!(!e.session_hmac.is_empty());
        assert_eq!(e.tool_id, "send_email");
    }

    #[test]
    fn verify_succeeds_on_clean_chain() {
        let mut l = empty_ledger();
        for i in 0..16u8 {
            l.append(
                Utc::now(),
                format!("tool_{i}"),
                fake_digest(i),
                format!("k{i}"),
                fake_digest(i ^ 0x55),
                if i % 5 == 0 {
                    SideEffectClass::Irreversible
                } else {
                    SideEffectClass::Pure
                },
            )
            .unwrap();
        }
        l.verify().unwrap();
    }

    #[test]
    fn verify_detects_tampering() {
        let mut l = empty_ledger();
        l.append(
            Utc::now(),
            "a",
            fake_digest(0),
            "k0",
            fake_digest(0),
            SideEffectClass::Pure,
        )
        .unwrap();
        l.append(
            Utc::now(),
            "b",
            fake_digest(1),
            "k1",
            fake_digest(1),
            SideEffectClass::Pure,
        )
        .unwrap();
        // Tamper with entry 0 *after* signing — chain must fail.
        l.entries[0].tool_id = "evil".into();
        assert!(l.verify().is_err());
    }

    #[test]
    fn round_trip_through_blob_store() {
        let blobs = MemBlobStore::new();
        let secret = SessionSecret::new(b"round-trip-secret".to_vec());
        let mut l = Ledger::new(secret.clone());
        for i in 0..4u8 {
            l.append(
                Utc::now(),
                format!("t{i}"),
                fake_digest(i),
                format!("k{i}"),
                fake_digest(i),
                SideEffectClass::Idempotent,
            )
            .unwrap();
        }
        let cid = l.serialize(&blobs).unwrap();
        let back = Ledger::deserialize(&blobs, &cid, secret).unwrap();
        assert_eq!(back.entries().len(), 4);
        back.verify().unwrap();
    }

    #[test]
    fn wrong_secret_fails_verification() {
        let blobs = MemBlobStore::new();
        let mut l = Ledger::new(SessionSecret::new(b"good".to_vec()));
        l.append(
            Utc::now(),
            "t",
            fake_digest(0),
            "k",
            fake_digest(1),
            SideEffectClass::Pure,
        )
        .unwrap();
        let cid = l.serialize(&blobs).unwrap();
        let back = Ledger::deserialize(&blobs, &cid, SessionSecret::new(b"evil".to_vec())).unwrap();
        assert!(back.verify().is_err());
    }

    #[test]
    fn idempotency_key_unique_within_loop() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..256 {
            let k = mint_idempotency_key().unwrap();
            assert!(seen.insert(k));
        }
    }

    #[test]
    fn secret_debug_does_not_leak() {
        let s = SessionSecret::new(b"shhh".to_vec());
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("shhh"));
        assert!(dbg.contains("redacted"));
    }
}
