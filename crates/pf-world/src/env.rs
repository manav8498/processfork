// SPDX-License-Identifier: MIT
//! Environment-variable + cwd capture, with regex redaction.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Wire format of a captured environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvSnapshot {
    /// Schema discriminator. Always `"env.v1"`.
    pub kind: String,
    /// Captured working directory at snapshot time.
    pub cwd: String,
    /// Captured env vars; keys are sorted (BTreeMap) so the digest is
    /// deterministic across hosts.
    pub vars: BTreeMap<String, String>,
}

/// Captures env vars + cwd, optionally redacting matching keys before sealing.
pub struct EnvCapture {
    scrub: Vec<Regex>,
}

impl EnvCapture {
    /// Construct a capturer with no redaction.
    #[must_use]
    pub fn new() -> Self {
        Self { scrub: Vec::new() }
    }

    /// Add a regex; any env-var key matching this regex will be replaced with
    /// `"<redacted>"` in the output. Common defaults: `(?i)(token|secret|key|password)`.
    ///
    /// # Errors
    /// Returns the underlying [`regex::Error`] when `pattern` does not parse.
    pub fn scrub(mut self, pattern: &str) -> Result<Self, regex::Error> {
        self.scrub.push(Regex::new(pattern)?);
        Ok(self)
    }

    /// Run the capture, store the resulting blob, and return its digest.
    pub fn capture(&self, blobs: &Arc<dyn BlobStore>) -> pf_core::Result<Digest256> {
        let cwd = match std::env::current_dir() {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(_) => String::new(),
        };
        let mut vars = BTreeMap::new();
        for (k, v) in std::env::vars() {
            let redacted = self.scrub.iter().any(|re| re.is_match(&k));
            vars.insert(k, if redacted { "<redacted>".into() } else { v });
        }
        let snap = EnvSnapshot {
            kind: "env.v1".into(),
            cwd,
            vars,
        };
        blobs.put(&serde_json::to_vec(&snap)?)
    }
}

impl Default for EnvCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(unsafe_code)] // std::env::set_var is unsafe since Rust 1.85
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;
    // SAFETY note: `std::env::set_var` is documented as racy with concurrent
    // reads in multithreaded code. We single-thread these tests with a mutex
    // so the env-var manipulation is sequential.
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn captures_vars_and_cwd() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: env mutation is serialized via ENV_LOCK.
        unsafe {
            std::env::set_var("PF_TEST_VAR_PLAIN", "value123");
        }
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cid = EnvCapture::new().capture(&blobs).unwrap();
        let snap: EnvSnapshot = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        assert_eq!(snap.kind, "env.v1");
        assert!(!snap.cwd.is_empty());
        assert_eq!(snap.vars.get("PF_TEST_VAR_PLAIN").unwrap(), "value123");
    }

    #[test]
    fn scrub_redacts_matching_keys() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: env mutation is serialized via ENV_LOCK.
        unsafe {
            std::env::set_var("PF_TEST_SECRET_TOKEN", "do-not-leak");
            std::env::set_var("PF_TEST_PUBLIC_INFO", "ok-to-share");
        }
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cap = EnvCapture::new().scrub("(?i)secret|token").unwrap();
        let cid = cap.capture(&blobs).unwrap();
        let snap: EnvSnapshot = serde_json::from_slice(&blobs.get(&cid).unwrap()).unwrap();
        assert_eq!(snap.vars.get("PF_TEST_SECRET_TOKEN").unwrap(), "<redacted>");
        assert_eq!(snap.vars.get("PF_TEST_PUBLIC_INFO").unwrap(), "ok-to-share");
    }

    #[test]
    fn vars_are_sorted_so_digest_is_deterministic() {
        let _g = ENV_LOCK.lock().unwrap();
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let c1 = EnvCapture::new().capture(&blobs).unwrap();
        let c2 = EnvCapture::new().capture(&blobs).unwrap();
        // Same env → same capture → same digest.
        assert_eq!(c1, c2);
    }
}
