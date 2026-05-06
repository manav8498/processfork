// SPDX-License-Identifier: MIT
//! Live HF Hub implementation. Gated by the `hf-live` feature.

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;
use serde_json::json;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError, transitive_blob_digests};
use crate::sign::{ManifestSignature, sign_manifest, verify_manifest};

/// Default endpoint. Override via `HF_ENDPOINT` env var or
/// [`HfRegistry::with_endpoint`] to point at a HF mirror or in-test
/// `wiremock` server.
const DEFAULT_ENDPOINT: &str = "https://huggingface.co";

/// HF Hub registry. Built via [`HfRegistry::new`].
#[derive(Debug)]
pub struct HfRegistry {
    /// `Authorization: Bearer <token>` value. `None` ⇒ anonymous reads
    /// only; `push` returns `RegistryError::Backend` without a token.
    token: Option<String>,
    /// Endpoint base URL (e.g. `https://huggingface.co`). Configurable
    /// for in-test wiremock instances and HF mirrors.
    endpoint: String,
    /// `manifest.json.sig` symmetric key. Mirrors
    /// [`crate::file::FileRegistry`]; `None` ⇒ stub-key path used by
    /// the v1 self-signed mode.
    sign_key: Option<String>,
    /// Lazy reqwest client.
    client: reqwest::Client,
}

impl HfRegistry {
    /// Build with a HF write token (forwarded as `Authorization: Bearer`).
    /// `None` keeps the registry read-only.
    pub fn new(token: Option<String>) -> Self {
        let endpoint = std::env::var("HF_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
        let sign_key = std::env::var("PF_HF_REG_SIGN_KEY").ok();
        Self {
            token,
            endpoint,
            sign_key,
            client: reqwest::Client::builder()
                .user_agent(concat!("processfork/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Override the endpoint. Used by integration tests pointing at a
    /// `wiremock` server.
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Override the signing-key string (otherwise read from
    /// `PF_HF_REG_SIGN_KEY`).
    #[must_use]
    pub fn with_sign_key(mut self, key: impl Into<String>) -> Self {
        self.sign_key = Some(key.into());
        self
    }

    /// Resolve `(user, repo, rev)` from an [`ImageRef::Hf`].
    pub(super) fn resolve(target: &ImageRef) -> Result<(&str, &str, &str), RegistryError> {
        match target {
            ImageRef::Hf { user, repo, tag } => Ok((user, repo, tag.as_deref().unwrap_or("main"))),
            other => Err(RegistryError::Backend(format!(
                "HfRegistry called with non-hf ref {other:?}"
            ))),
        }
    }

    /// Authorisation header. Returns `None` if no token (so anonymous
    /// reads still work).
    fn auth(&self) -> Option<(&'static str, String)> {
        self.token
            .as_deref()
            .map(|t| ("Authorization", format!("Bearer {t}")))
    }

    /// Ensure the dataset repo exists (idempotent — 409 from HF means
    /// "already there", which is success for our purposes).
    async fn ensure_repo(&self, user: &str, repo: &str) -> Result<(), RegistryError> {
        let url = format!("{}/api/repos/create", self.endpoint);
        let mut req = self.client.post(&url).json(&json!({
            "type": "dataset",
            "name": repo,
            "organization": user,
            "private": false,
        }));
        if let Some((k, v)) = self.auth() {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF create-repo: {e}")))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 409 {
            return Ok(());
        }
        Err(RegistryError::Backend(format!(
            "HF create-repo {status} {user}/{repo}: {}",
            resp.text().await.unwrap_or_default()
        )))
    }

    /// Build the body for one HF batched commit. Each tuple is
    /// `(in_repo_path, raw_bytes)`. Returns NDJSON with one
    /// `header` line followed by one `file` line per upload.
    pub(super) fn build_commit_body(summary: &str, files: &[(String, Vec<u8>)]) -> String {
        let mut body = String::new();
        body.push_str(
            &serde_json::to_string(&json!({
                "key": "header",
                "value": {
                    "summary": summary,
                    "description": "",
                },
            }))
            .expect("serde header"),
        );
        body.push('\n');
        for (path, content) in files {
            body.push_str(
                &serde_json::to_string(&json!({
                    "key": "file",
                    "value": {
                        "path": path,
                        "content": B64.encode(content),
                        "encoding": "base64",
                    },
                }))
                .expect("serde file"),
            );
            body.push('\n');
        }
        body
    }

    /// Send one commit covering every file in `files` to
    /// `<endpoint>/api/datasets/{user}/{repo}/commit/{rev}`.
    async fn commit_files(
        &self,
        user: &str,
        repo: &str,
        rev: &str,
        files: Vec<(String, Vec<u8>)>,
    ) -> Result<(), RegistryError> {
        let token = self.token.as_deref().ok_or_else(|| {
            RegistryError::Backend(
                "HF push requires HF_TOKEN; pass it via auth['HF_TOKEN'] or env".into(),
            )
        })?;
        let url = format!("{}/api/datasets/{user}/{repo}/commit/{rev}", self.endpoint);
        let summary = format!("pf push: {} files", files.len());
        let body = Self::build_commit_body(&summary, &files);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF commit: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(RegistryError::Backend(format!(
                "HF commit {status}: {}",
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// `GET /api/datasets/{user}/{repo}/tree/{rev}?recursive=true`
    async fn list_tree(
        &self,
        user: &str,
        repo: &str,
        rev: &str,
    ) -> Result<Vec<String>, RegistryError> {
        let url = format!(
            "{}/api/datasets/{user}/{repo}/tree/{rev}?recursive=true",
            self.endpoint
        );
        let mut req = self.client.get(&url);
        if let Some((k, v)) = self.auth() {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF tree: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "HF tree {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let entries: Vec<TreeEntry> = resp
            .json()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF tree decode: {e}")))?;
        Ok(entries
            .into_iter()
            .filter(|e| e.kind == "file")
            .map(|e| e.path)
            .collect())
    }

    /// `GET /datasets/{user}/{repo}/resolve/{rev}/{path}` → raw bytes.
    async fn fetch(
        &self,
        user: &str,
        repo: &str,
        rev: &str,
        path: &str,
    ) -> Result<Vec<u8>, RegistryError> {
        let url = format!(
            "{}/datasets/{user}/{repo}/resolve/{rev}/{path}",
            self.endpoint
        );
        let mut req = self.client.get(&url);
        if let Some((k, v)) = self.auth() {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF fetch {path}: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "HF fetch {path} {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| RegistryError::Backend(format!("HF body {path}: {e}")))
    }
}

#[derive(serde::Deserialize)]
struct TreeEntry {
    /// "file" or "directory"
    #[serde(rename = "type")]
    kind: String,
    path: String,
}

/// Map a `Digest256` to the in-repo blob path (sharded by first 2 hex chars).
pub(super) fn blob_path_in_repo(d: &Digest256) -> String {
    let hex = d.hex();
    format!("blobs/sha256/{}/{hex}.zst", &hex[..2])
}

#[async_trait]
impl Registry for HfRegistry {
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        let (user, repo, rev) = Self::resolve(target)?;
        self.ensure_repo(user, repo).await?;

        // 1. manifest.json + signature.
        let manifest_bytes = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("manifest serialize: {e}")))?;
        let sig = sign_manifest(&manifest_bytes, self.sign_key.as_deref());
        let sig_bytes = serde_json::to_vec(&sig)
            .map_err(|e| RegistryError::Backend(format!("sig serialize: {e}")))?;

        // 2. transitively-reachable blobs (zstd-compressed).
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        files.push(("manifest.json".to_owned(), manifest_bytes));
        files.push(("manifest.json.sig".to_owned(), sig_bytes));
        for digest in transitive_blob_digests(manifest, blobs)? {
            let raw = blobs.get(&digest)?;
            let compressed = zstd::encode_all(raw.as_slice(), 19)
                .map_err(|e| RegistryError::Backend(format!("zstd encode: {e}")))?;
            files.push((blob_path_in_repo(&digest), compressed));
        }

        // 3. one commit, all files.
        self.commit_files(user, repo, rev, files).await
    }

    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError> {
        let (user, repo, rev) = Self::resolve(source)?;

        // 1. manifest + sig + verify.
        let manifest_bytes = self.fetch(user, repo, rev, "manifest.json").await?;
        let sig_bytes = self.fetch(user, repo, rev, "manifest.json.sig").await?;
        let sig: ManifestSignature = serde_json::from_slice(&sig_bytes)
            .map_err(|e| RegistryError::SignatureVerify(format!("parse sig: {e}")))?;
        verify_manifest(&manifest_bytes, &sig, self.sign_key.as_deref())
            .map_err(RegistryError::SignatureVerify)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("parse manifest: {e}")))?;

        // 2. list every blob/* path the registry actually has, fetch
        //    them all, decompress, and verify each digest matches its
        //    filename (catches MITM tampering for free).
        let mut blobs: Vec<(Digest256, Vec<u8>)> = Vec::new();
        let tree = self.list_tree(user, repo, rev).await?;
        for path in tree {
            let Some(stripped) = path
                .strip_prefix("blobs/sha256/")
                .and_then(|p| p.strip_suffix(".zst"))
            else {
                continue;
            };
            // path is "blobs/sha256/<aa>/<full-hex>.zst"; strip the
            // shard segment and read the hex from the basename.
            let Some((_, hex)) = stripped.split_once('/') else {
                continue;
            };
            let Ok(digest) = Digest256::parse(&format!("sha256:{hex}")) else {
                continue;
            };
            let compressed = self.fetch(user, repo, rev, &path).await?;
            let bytes = zstd::decode_all(compressed.as_slice())
                .map_err(|e| RegistryError::Backend(format!("zstd decode {path}: {e}")))?;
            let observed = Digest256::of(&bytes);
            if observed != digest {
                return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
                    "HF blob {digest} hashes to {observed}"
                ))));
            }
            blobs.push((digest, bytes));
        }
        Ok(LayerSet { manifest, blobs })
    }

    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError> {
        let (user, repo, rev) = Self::resolve(source)?;
        let url = format!(
            "{}/datasets/{user}/{repo}/resolve/{rev}/manifest.json",
            self.endpoint
        );
        let mut req = self.client.head(&url);
        if let Some((k, v)) = self.auth() {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("HF head: {e}")))?;
        Ok(resp.status().is_success())
    }
}

// ---------- unit tests (no network) ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_commit_body_includes_header_and_each_file_as_ndjson() {
        let body = HfRegistry::build_commit_body(
            "summary line",
            &[
                ("manifest.json".into(), b"{}".to_vec()),
                ("blobs/sha256/aa/aa.zst".into(), b"\x00\x01".to_vec()),
            ],
        );
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 file lines");
        let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header["key"], "header");
        assert_eq!(header["value"]["summary"], "summary line");
        let f0: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(f0["key"], "file");
        assert_eq!(f0["value"]["path"], "manifest.json");
        assert_eq!(f0["value"]["encoding"], "base64");
        assert_eq!(f0["value"]["content"], B64.encode(b"{}"));
    }

    #[test]
    fn blob_path_is_sharded_by_first_two_hex_chars() {
        let d = Digest256::of(b"hello world");
        let p = blob_path_in_repo(&d);
        assert!(p.starts_with("blobs/sha256/"));
        assert!(
            std::path::Path::new(&p)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
        );
        let hex = d.hex();
        assert!(p.contains(&format!("/{}/", &hex[..2])));
        assert!(p.contains(hex));
    }

    #[test]
    fn resolve_uses_main_when_no_tag() {
        let r = ImageRef::Hf {
            user: "alice".into(),
            repo: "session".into(),
            tag: None,
        };
        let (u, repo, rev) = HfRegistry::resolve(&r).unwrap();
        assert_eq!((u, repo, rev), ("alice", "session", "main"));
    }

    #[test]
    fn resolve_uses_tag_when_present() {
        let r = ImageRef::Hf {
            user: "alice".into(),
            repo: "session".into(),
            tag: Some("v3".into()),
        };
        let (_, _, rev) = HfRegistry::resolve(&r).unwrap();
        assert_eq!(rev, "v3");
    }

    #[test]
    fn resolve_rejects_non_hf_ref() {
        let r = ImageRef::File {
            path: "/tmp/x".into(),
        };
        assert!(HfRegistry::resolve(&r).is_err());
    }
}
