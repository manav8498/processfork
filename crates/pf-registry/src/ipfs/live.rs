// SPDX-License-Identifier: MIT
//! Live IPFS implementation. Gated by the `ipfs-live` feature.

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError, transitive_blob_digests};
use crate::sign::{ManifestSignature, sign_manifest, verify_manifest};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:5001";

/// IPFS registry. Built via [`IpfsRegistry::new`].
#[derive(Debug)]
pub struct IpfsRegistry {
    endpoint: String,
    /// `manifest.json.sig` symmetric key (mirrors FileRegistry).
    sign_key: Option<String>,
    client: reqwest::Client,
}

impl IpfsRegistry {
    pub fn new(endpoint: String) -> Self {
        let endpoint = if endpoint.is_empty() {
            std::env::var("IPFS_API").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned())
        } else {
            endpoint
        };
        let sign_key = std::env::var("PF_IPFS_REG_SIGN_KEY").ok();
        Self {
            endpoint,
            sign_key,
            client: reqwest::Client::builder()
                .user_agent(concat!("processfork/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Override the signing-key string (otherwise read from
    /// `PF_IPFS_REG_SIGN_KEY`).
    #[must_use]
    pub fn with_sign_key(mut self, key: impl Into<String>) -> Self {
        self.sign_key = Some(key.into());
        self
    }

    /// Resolve `cid` from an [`ImageRef::Ipfs`].
    pub(super) fn resolve(target: &ImageRef) -> Result<&str, RegistryError> {
        match target {
            ImageRef::Ipfs { cid } => Ok(cid),
            other => Err(RegistryError::Backend(format!(
                "IpfsRegistry called with non-ipfs ref {other:?}"
            ))),
        }
    }

    /// `POST /api/v0/add` — upload a single byte buffer as a file,
    /// returns the CID. Multipart form with one part: `file` field.
    async fn add_file(&self, name: &str, bytes: Vec<u8>) -> Result<String, RegistryError> {
        let url = format!("{}/api/v0/add", self.endpoint);
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(name.to_owned())
            .mime_str("application/octet-stream")
            .map_err(|e| RegistryError::Backend(format!("IPFS multipart mime: {e}")))?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS add: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS add {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let body: AddResponse = resp
            .json()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS add decode: {e}")))?;
        Ok(body.hash)
    }

    /// `POST /api/v0/object/new?arg=unixfs-dir` — create an empty
    /// UnixFS directory; returns its CID.
    async fn new_dir(&self) -> Result<String, RegistryError> {
        let url = format!("{}/api/v0/object/new?arg=unixfs-dir", self.endpoint);
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS object/new: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS object/new {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let body: ObjectResponse = resp
            .json()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS object/new decode: {e}")))?;
        Ok(body.hash)
    }

    /// `POST /api/v0/object/patch/add-link?arg=<dir>&arg=<name>&arg=<cid>` —
    /// returns the new directory CID after the link is added.
    async fn add_link(
        &self,
        dir_cid: &str,
        name: &str,
        target_cid: &str,
    ) -> Result<String, RegistryError> {
        let url = format!(
            "{}/api/v0/object/patch/add-link?arg={}&arg={}&arg={}",
            self.endpoint,
            urlencode(dir_cid),
            urlencode(name),
            urlencode(target_cid),
        );
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS add-link: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS add-link {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let body: ObjectResponse = resp
            .json()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS add-link decode: {e}")))?;
        Ok(body.hash)
    }

    /// `POST /api/v0/pin/add?arg=<cid>` — pin a CID locally.
    async fn pin(&self, cid: &str) -> Result<(), RegistryError> {
        let url = format!("{}/api/v0/pin/add?arg={}", self.endpoint, urlencode(cid));
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS pin: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS pin {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// `POST /api/v0/ls?arg=<cid>` — list directory entries.
    async fn ls(&self, cid: &str) -> Result<Vec<LsEntry>, RegistryError> {
        let url = format!("{}/api/v0/ls?arg={}", self.endpoint, urlencode(cid));
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS ls: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS ls {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let body: LsResponse = resp
            .json()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS ls decode: {e}")))?;
        Ok(body.objects.into_iter().flat_map(|o| o.links).collect())
    }

    /// `POST /api/v0/cat?arg=<cid>` — fetch raw bytes.
    async fn cat(&self, cid: &str) -> Result<Vec<u8>, RegistryError> {
        let url = format!("{}/api/v0/cat?arg={}", self.endpoint, urlencode(cid));
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS cat: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "IPFS cat {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(resp
            .bytes()
            .await
            .map_err(|e| RegistryError::Backend(format!("IPFS cat body: {e}")))?
            .to_vec())
    }
}

#[derive(serde::Deserialize)]
struct AddResponse {
    #[serde(rename = "Hash")]
    hash: String,
}

#[derive(serde::Deserialize)]
struct ObjectResponse {
    #[serde(rename = "Hash")]
    hash: String,
}

#[derive(serde::Deserialize)]
struct LsResponse {
    #[serde(rename = "Objects")]
    objects: Vec<LsObject>,
}

#[derive(serde::Deserialize)]
struct LsObject {
    #[serde(rename = "Links")]
    links: Vec<LsEntry>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct LsEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Hash")]
    hash: String,
}

/// Minimal URL-encode for CID + path arguments. We restrict to the
/// safe set [A-Za-z0-9._/-]; everything else is %xx-encoded.
fn urlencode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = b.is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || b == b'/';
        if safe {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[async_trait]
impl Registry for IpfsRegistry {
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        let hint_cid = Self::resolve(target)?;

        // 1. Upload manifest + sig + every transitively-reachable blob,
        //    each as its own IPFS object. Collect (path, cid) pairs.
        let manifest_bytes = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("manifest serialize: {e}")))?;
        let sig = sign_manifest(&manifest_bytes, self.sign_key.as_deref());
        let sig_bytes = serde_json::to_vec(&sig)
            .map_err(|e| RegistryError::Backend(format!("sig serialize: {e}")))?;

        let mut entries: Vec<(String, String)> = Vec::new();
        entries.push((
            "manifest.json".to_owned(),
            self.add_file("manifest.json", manifest_bytes).await?,
        ));
        entries.push((
            "manifest.json.sig".to_owned(),
            self.add_file("manifest.json.sig", sig_bytes).await?,
        ));
        for digest in transitive_blob_digests(manifest, blobs)? {
            let raw = blobs.get(&digest)?;
            let compressed = zstd::encode_all(raw.as_slice(), 19)
                .map_err(|e| RegistryError::Backend(format!("zstd encode: {e}")))?;
            // Use a flat name so add-link doesn't need nested paths.
            // Prefix with "blob_" so we never collide with the
            // manifest/sig names.
            let leaf = format!("blob_{}.zst", digest.hex());
            let cid = self.add_file(&leaf, compressed).await?;
            entries.push((leaf, cid));
        }

        // 2. Build a UnixFS directory containing all of them.
        let mut dir_cid = self.new_dir().await?;
        for (name, cid) in &entries {
            dir_cid = self.add_link(&dir_cid, name, cid).await?;
        }

        // 3. Pin the final directory.
        self.pin(&dir_cid).await?;

        // 4. Surface the actual CID. The `cid` in target is informational;
        //    if it matches the daemon's result we leave a confirmation
        //    note, otherwise we log both for the operator.
        if hint_cid.is_empty() || hint_cid == dir_cid {
            tracing::info!(
                target = "pf-registry::ipfs",
                cid = %dir_cid,
                "IPFS push complete"
            );
        } else {
            tracing::warn!(
                target = "pf-registry::ipfs",
                cid = %dir_cid,
                hint = hint_cid,
                "IPFS push complete; resulting CID differs from hint"
            );
        }
        Ok(())
    }

    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError> {
        let dir_cid = Self::resolve(source)?;
        let entries = self.ls(dir_cid).await?;

        // 1. Find manifest + sig entries.
        let manifest_entry = entries
            .iter()
            .find(|e| e.name == "manifest.json")
            .ok_or_else(|| RegistryError::Backend("IPFS dir missing manifest.json".into()))?;
        let sig_entry = entries
            .iter()
            .find(|e| e.name == "manifest.json.sig")
            .ok_or_else(|| RegistryError::Backend("IPFS dir missing manifest.json.sig".into()))?;

        let manifest_bytes = self.cat(&manifest_entry.hash).await?;
        let sig_bytes = self.cat(&sig_entry.hash).await?;
        let sig: ManifestSignature = serde_json::from_slice(&sig_bytes)
            .map_err(|e| RegistryError::SignatureVerify(format!("parse sig: {e}")))?;
        verify_manifest(&manifest_bytes, &sig, self.sign_key.as_deref())
            .map_err(RegistryError::SignatureVerify)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("parse manifest: {e}")))?;

        // 2. Walk the remaining entries; each `blob_<hex>.zst` carries a
        //    compressed blob whose decompressed digest must match the
        //    encoded hex.
        let mut blobs = Vec::new();
        for entry in &entries {
            let Some(hex) = entry
                .name
                .strip_prefix("blob_")
                .and_then(|s| s.strip_suffix(".zst"))
            else {
                continue;
            };
            let digest = Digest256::parse(&format!("sha256:{hex}"))?;
            let compressed = self.cat(&entry.hash).await?;
            let bytes = zstd::decode_all(compressed.as_slice())
                .map_err(|e| RegistryError::Backend(format!("zstd decode {}: {e}", entry.name)))?;
            let observed = Digest256::of(&bytes);
            if observed != digest {
                return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
                    "IPFS blob {digest} hashes to {observed}"
                ))));
            }
            blobs.push((digest, bytes));
        }

        Ok(LayerSet { manifest, blobs })
    }

    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError> {
        let cid = Self::resolve(source)?;
        // `ls` succeeds for any pinned dir; if the CID isn't reachable
        // we get a 500 / 404 / timeout, all surfacing as Backend(...).
        match self.ls(cid).await {
            Ok(entries) => Ok(entries.iter().any(|e| e.name == "manifest.json")),
            Err(RegistryError::Backend(s)) if s.contains("404") => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_keeps_safe_set_passes_others_as_pct() {
        assert_eq!(urlencode("Qm123abc/leaf"), "Qm123abc/leaf");
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("a&b"), "a%26b");
    }

    #[test]
    fn resolve_returns_cid() {
        let r = ImageRef::Ipfs {
            cid: "Qm123".into(),
        };
        assert_eq!(IpfsRegistry::resolve(&r).unwrap(), "Qm123");
    }

    #[test]
    fn resolve_rejects_non_ipfs_ref() {
        let r = ImageRef::File {
            path: "/tmp/x".into(),
        };
        assert!(IpfsRegistry::resolve(&r).is_err());
    }

    #[test]
    fn empty_endpoint_falls_back_to_default() {
        let r = IpfsRegistry::new(String::new());
        assert!(r.endpoint == DEFAULT_ENDPOINT || r.endpoint.starts_with("http"));
    }
}
