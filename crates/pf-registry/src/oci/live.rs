// SPDX-License-Identifier: MIT
//! Live OCI Distribution Spec v2 implementation. Gated by `oci-live`.

use async_trait::async_trait;
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;
use serde::{Deserialize, Serialize};

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError, transitive_blob_digests};
use crate::sign::{ManifestSignature, sign_manifest, verify_manifest};

const MEDIATYPE_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const MEDIATYPE_PF_CONFIG: &str = "application/vnd.processfork.image.v1+json";
const MEDIATYPE_PF_LAYER: &str = "application/vnd.processfork.layer.v1+zstd";
const MEDIATYPE_PF_SIG: &str = "application/vnd.processfork.signature.v1+json";

/// OCI Distribution Spec registry. Built via [`OciRegistry::new`].
///
/// Auth bag honoured keys:
///   - `OCI_USERNAME` / `OCI_PASSWORD` for HTTP Basic
///   - `OCI_BEARER` for Authorization: Bearer (e.g. ghcr push tokens)
#[derive(Debug)]
pub struct OciRegistry {
    auth: std::collections::BTreeMap<String, String>,
    /// `manifest.json.sig` symmetric key (mirrors FileRegistry).
    sign_key: Option<String>,
    client: reqwest::Client,
}

impl OciRegistry {
    pub fn new(auth: std::collections::BTreeMap<String, String>) -> Self {
        let sign_key = std::env::var("PF_OCI_REG_SIGN_KEY").ok();
        Self {
            auth,
            sign_key,
            client: reqwest::Client::builder()
                .user_agent(concat!("processfork/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Override the signing-key string (otherwise read from
    /// `PF_OCI_REG_SIGN_KEY`).
    #[must_use]
    pub fn with_sign_key(mut self, key: impl Into<String>) -> Self {
        self.sign_key = Some(key.into());
        self
    }

    /// Resolve `(base, repo, reference)` from an [`ImageRef::Oci`].
    /// `reference` is the tag (defaults to `latest`).
    pub(super) fn resolve(target: &ImageRef) -> Result<(String, &str, &str), RegistryError> {
        match target {
            ImageRef::Oci {
                host,
                port,
                repo,
                tag,
            } => {
                // Default to https on port 443; fall back to http on
                // localhost / 127.0.0.1 / dotted-quad reachable hosts so
                // the in-test wiremock and `registry:2` runs work.
                let scheme = if host == "localhost"
                    || host == "127.0.0.1"
                    || host.starts_with("127.")
                    || port.is_some_and(|p| p != 443)
                {
                    "http"
                } else {
                    "https"
                };
                let port_part = port.map_or(String::new(), |p| format!(":{p}"));
                let base = format!("{scheme}://{host}{port_part}");
                Ok((base, repo, tag.as_deref().unwrap_or("latest")))
            }
            other => Err(RegistryError::Backend(format!(
                "OciRegistry called with non-oci ref {other:?}"
            ))),
        }
    }

    /// Build the request with the right auth headers attached.
    fn auth_request(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = self.auth.get("OCI_BEARER") {
            req = req.header("Authorization", format!("Bearer {token}"));
        } else if let (Some(user), Some(pass)) =
            (self.auth.get("OCI_USERNAME"), self.auth.get("OCI_PASSWORD"))
        {
            req = req.basic_auth(user, Some(pass));
        }
        req
    }

    /// HEAD a blob (cheap "does it exist" check). 200 ⇒ already present
    /// on the registry; 404 ⇒ needs upload.
    async fn blob_exists(
        &self,
        base: &str,
        repo: &str,
        d: &Digest256,
    ) -> Result<bool, RegistryError> {
        let url = format!("{base}/v2/{repo}/blobs/sha256:{}", d.hex());
        let resp = self
            .auth_request(self.client.head(&url))
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI HEAD blob: {e}")))?;
        Ok(resp.status().is_success())
    }

    /// Push one blob using the monolithic two-step upload
    /// (POST upload → PUT bytes+digest). One round trip per blob.
    async fn push_blob(
        &self,
        base: &str,
        repo: &str,
        d: &Digest256,
        bytes: &[u8],
    ) -> Result<(), RegistryError> {
        // Skip if already present — content-addressed.
        if self.blob_exists(base, repo, d).await? {
            return Ok(());
        }
        // Step 1: initiate upload.
        let init_url = format!("{base}/v2/{repo}/blobs/uploads/");
        let resp = self
            .auth_request(self.client.post(&init_url))
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI start upload: {e}")))?;
        if resp.status().as_u16() != 202 {
            return Err(RegistryError::Backend(format!(
                "OCI start upload: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let location = resp
            .headers()
            .get("Location")
            .ok_or_else(|| RegistryError::Backend("OCI upload: no Location header".into()))?
            .to_str()
            .map_err(|e| RegistryError::Backend(format!("OCI Location: {e}")))?
            .to_owned();

        // Location may be relative or absolute. If relative, prefix with
        // base.
        let upload_url = if location.starts_with("http://") || location.starts_with("https://") {
            location
        } else {
            format!("{base}{location}")
        };

        // Step 2: PUT the bytes plus the ?digest=... query param.
        let sep = if upload_url.contains('?') { '&' } else { '?' };
        let put_url = format!("{upload_url}{sep}digest=sha256:{}", d.hex());
        let resp = self
            .auth_request(
                self.client
                    .put(&put_url)
                    .header("Content-Type", "application/octet-stream")
                    .body(bytes.to_owned()),
            )
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI PUT blob: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "OCI PUT blob {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// PUT the OCI manifest.
    async fn push_manifest(
        &self,
        base: &str,
        repo: &str,
        reference: &str,
        manifest: &OciManifest,
    ) -> Result<(), RegistryError> {
        let url = format!("{base}/v2/{repo}/manifests/{reference}");
        let body = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("OCI manifest serialize: {e}")))?;
        let resp = self
            .auth_request(
                self.client
                    .put(&url)
                    .header("Content-Type", MEDIATYPE_OCI_MANIFEST)
                    .body(body),
            )
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI PUT manifest: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "OCI PUT manifest {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    async fn fetch_manifest(
        &self,
        base: &str,
        repo: &str,
        reference: &str,
    ) -> Result<OciManifest, RegistryError> {
        let url = format!("{base}/v2/{repo}/manifests/{reference}");
        let resp = self
            .auth_request(
                self.client
                    .get(&url)
                    .header("Accept", MEDIATYPE_OCI_MANIFEST),
            )
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI GET manifest: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "OCI GET manifest {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        resp.json::<OciManifest>()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI manifest decode: {e}")))
    }

    async fn fetch_blob(
        &self,
        base: &str,
        repo: &str,
        d: &Digest256,
    ) -> Result<Vec<u8>, RegistryError> {
        let url = format!("{base}/v2/{repo}/blobs/sha256:{}", d.hex());
        let resp = self
            .auth_request(self.client.get(&url))
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI GET blob: {e}")))?;
        if !resp.status().is_success() {
            return Err(RegistryError::Backend(format!(
                "OCI GET blob {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI GET blob body: {e}")))?
            .to_vec();
        // Verify digest.
        let observed = Digest256::of(&bytes);
        if &observed != d {
            return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
                "OCI blob {d} hashes to {observed}"
            ))));
        }
        Ok(bytes)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OciManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "mediaType")]
    media_type: String,
    config: OciDescriptor,
    layers: Vec<OciDescriptor>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OciDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    size: u64,
    digest: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    annotations: std::collections::BTreeMap<String, String>,
}

impl OciDescriptor {
    fn new(media_type: &str, bytes: &[u8], path_annotation: Option<&str>) -> Self {
        let digest = Digest256::of(bytes);
        let mut annotations = std::collections::BTreeMap::new();
        if let Some(p) = path_annotation {
            annotations.insert("org.processfork.path".to_owned(), p.to_owned());
        }
        Self {
            media_type: media_type.to_owned(),
            size: bytes.len() as u64,
            digest: format!("sha256:{}", digest.hex()),
            annotations,
        }
    }
}

#[async_trait]
impl Registry for OciRegistry {
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        let (base, repo, reference) = Self::resolve(target)?;

        // 1. The .pfimg manifest is the OCI config blob.
        let pf_manifest_bytes = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("manifest serialize: {e}")))?;
        let pf_manifest_digest = Digest256::of(&pf_manifest_bytes);
        self.push_blob(&base, repo, &pf_manifest_digest, &pf_manifest_bytes)
            .await?;

        let config_desc = OciDescriptor::new(MEDIATYPE_PF_CONFIG, &pf_manifest_bytes, None);

        // 2. Push the signature as the first layer (so it's
        //    discoverable without a full clone).
        let sig = sign_manifest(&pf_manifest_bytes, self.sign_key.as_deref());
        let sig_bytes = serde_json::to_vec(&sig)
            .map_err(|e| RegistryError::Backend(format!("sig serialize: {e}")))?;
        let sig_digest = Digest256::of(&sig_bytes);
        self.push_blob(&base, repo, &sig_digest, &sig_bytes).await?;
        let mut layers = vec![OciDescriptor::new(
            MEDIATYPE_PF_SIG,
            &sig_bytes,
            Some("manifest.json.sig"),
        )];

        // 3. Push every transitively-reachable blob (zstd-compressed).
        for digest in transitive_blob_digests(manifest, blobs)? {
            let raw = blobs.get(&digest)?;
            let compressed = zstd::encode_all(raw.as_slice(), 19)
                .map_err(|e| RegistryError::Backend(format!("zstd encode: {e}")))?;
            let layer_digest = Digest256::of(&compressed);
            self.push_blob(&base, repo, &layer_digest, &compressed)
                .await?;
            let path = format!("blobs/sha256/{}/{}.zst", &digest.hex()[..2], digest.hex());
            layers.push(OciDescriptor::new(
                MEDIATYPE_PF_LAYER,
                &compressed,
                Some(&path),
            ));
        }

        // 4. PUT the OCI manifest pointing at all of them.
        let oci_manifest = OciManifest {
            schema_version: 2,
            media_type: MEDIATYPE_OCI_MANIFEST.to_owned(),
            config: config_desc,
            layers,
        };
        self.push_manifest(&base, repo, reference, &oci_manifest)
            .await
    }

    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError> {
        let (base, repo, reference) = Self::resolve(source)?;

        // 1. Fetch the OCI manifest, which references the .pfimg
        //    manifest as its config blob and the sig + per-layer blobs
        //    as its layers.
        let oci = self.fetch_manifest(&base, repo, reference).await?;
        let config_digest = parse_oci_digest(&oci.config.digest)?;
        let pf_manifest_bytes = self.fetch_blob(&base, repo, &config_digest).await?;

        // 2. Find the signature layer (mediatype match) and verify.
        let mut sig_bytes = None;
        let mut blob_layers = Vec::new();
        for layer in oci.layers {
            let d = parse_oci_digest(&layer.digest)?;
            if layer.media_type == MEDIATYPE_PF_SIG {
                sig_bytes = Some(self.fetch_blob(&base, repo, &d).await?);
            } else if layer.media_type == MEDIATYPE_PF_LAYER {
                let raw_path = layer
                    .annotations
                    .get("org.processfork.path")
                    .cloned()
                    .unwrap_or_default();
                blob_layers.push((d, raw_path));
            }
        }
        let sig_bytes = sig_bytes.ok_or_else(|| {
            RegistryError::Backend("OCI manifest missing pf-signature layer".into())
        })?;
        let sig: ManifestSignature = serde_json::from_slice(&sig_bytes)
            .map_err(|e| RegistryError::SignatureVerify(format!("parse sig: {e}")))?;
        verify_manifest(&pf_manifest_bytes, &sig, self.sign_key.as_deref())
            .map_err(RegistryError::SignatureVerify)?;
        let pf_manifest: Manifest = serde_json::from_slice(&pf_manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("parse manifest: {e}")))?;

        // 3. Fetch + decompress every layer; verify the digest of the
        //    decompressed bytes matches the path annotation.
        let mut blobs = Vec::new();
        for (compressed_digest, path) in blob_layers {
            let compressed = self.fetch_blob(&base, repo, &compressed_digest).await?;
            let bytes = zstd::decode_all(compressed.as_slice())
                .map_err(|e| RegistryError::Backend(format!("zstd decode {path}: {e}")))?;
            // The annotation tells us the underlying digest.
            let logical_digest = if let Some(stripped) = path
                .strip_prefix("blobs/sha256/")
                .and_then(|p| p.strip_suffix(".zst"))
            {
                let hex_part = stripped.split_once('/').map_or(stripped, |s| s.1);
                Digest256::parse(&format!("sha256:{hex_part}"))?
            } else {
                Digest256::of(&bytes)
            };
            let observed = Digest256::of(&bytes);
            if observed != logical_digest {
                return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
                    "OCI layer {path} decompresses to {observed}, expected {logical_digest}"
                ))));
            }
            blobs.push((logical_digest, bytes));
        }

        Ok(LayerSet {
            manifest: pf_manifest,
            blobs,
        })
    }

    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError> {
        let (base, repo, reference) = Self::resolve(source)?;
        let url = format!("{base}/v2/{repo}/manifests/{reference}");
        let resp = self
            .auth_request(
                self.client
                    .head(&url)
                    .header("Accept", MEDIATYPE_OCI_MANIFEST),
            )
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("OCI head: {e}")))?;
        Ok(resp.status().is_success())
    }
}

/// Parse an OCI-style `"sha256:..."` digest into a [`Digest256`].
fn parse_oci_digest(s: &str) -> Result<Digest256, RegistryError> {
    Digest256::parse(s).map_err(RegistryError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_localhost_uses_http_and_default_tag() {
        let r = ImageRef::Oci {
            host: "localhost".into(),
            port: Some(5000),
            repo: "alice/sess".into(),
            tag: None,
        };
        let (base, repo, reference) = OciRegistry::resolve(&r).unwrap();
        assert_eq!(base, "http://localhost:5000");
        assert_eq!(repo, "alice/sess");
        assert_eq!(reference, "latest");
    }

    #[test]
    fn resolve_dns_host_uses_https() {
        let r = ImageRef::Oci {
            host: "ghcr.io".into(),
            port: None,
            repo: "manav8498/processfork".into(),
            tag: Some("v1".into()),
        };
        let (base, _, reference) = OciRegistry::resolve(&r).unwrap();
        assert_eq!(base, "https://ghcr.io");
        assert_eq!(reference, "v1");
    }

    #[test]
    fn descriptor_carries_path_annotation() {
        let bytes = b"hello";
        let d = OciDescriptor::new(MEDIATYPE_PF_LAYER, bytes, Some("manifest.json"));
        assert_eq!(d.media_type, MEDIATYPE_PF_LAYER);
        assert_eq!(d.size, 5);
        assert_eq!(
            d.annotations
                .get("org.processfork.path")
                .map(String::as_str),
            Some("manifest.json")
        );
        assert!(d.digest.starts_with("sha256:"));
    }

    #[test]
    fn parse_oci_digest_round_trip() {
        let bytes = b"x";
        let d = Digest256::of(bytes);
        let s = format!("sha256:{}", d.hex());
        assert_eq!(parse_oci_digest(&s).unwrap(), d);
    }
}
