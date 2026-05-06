// SPDX-License-Identifier: MIT
//! Live S3 implementation. Gated by the `s3-live` feature.
//!
//! Uses the official `aws-sdk-s3` SDK so we get correct SigV4 signing,
//! the standard credential provider chain, multi-region support, and
//! R2/MinIO compatibility for free.

use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    Client, Config,
    config::{Region, SharedCredentialsProvider},
    primitives::ByteStream,
};
use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use pf_core::manifest::Manifest;

use crate::image_ref::ImageRef;
use crate::registry::{LayerSet, Registry, RegistryError, transitive_blob_digests};
use crate::sign::{ManifestSignature, sign_manifest, verify_manifest};

/// S3-compatible registry. Built via [`S3Registry::new`].
#[derive(Debug)]
pub struct S3Registry {
    client: Client,
    /// `manifest.json.sig` symmetric key (mirrors FileRegistry).
    sign_key: Option<String>,
}

impl S3Registry {
    pub fn new(auth: std::collections::BTreeMap<String, String>) -> Self {
        let region = auth
            .get("AWS_REGION")
            .cloned()
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_owned());

        let mut builder = Config::builder()
            .region(Region::new(region))
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .force_path_style(true); // R2/MinIO want path-style; AWS handles it too.

        // Explicit creds from the auth bag override the chain.
        if let (Some(ak), Some(sk)) = (
            auth.get("AWS_ACCESS_KEY_ID"),
            auth.get("AWS_SECRET_ACCESS_KEY"),
        ) {
            let creds = Credentials::new(ak, sk, None, None, "pf-registry");
            builder = builder.credentials_provider(SharedCredentialsProvider::new(creds));
        }

        // Custom endpoint (R2 / MinIO / wiremock) overrides the AWS default.
        if let Some(endpoint) = auth
            .get("AWS_ENDPOINT_URL")
            .cloned()
            .or_else(|| std::env::var("AWS_ENDPOINT_URL").ok())
        {
            builder = builder.endpoint_url(endpoint);
        }

        let sign_key = std::env::var("PF_S3_REG_SIGN_KEY").ok();
        Self {
            client: Client::from_conf(builder.build()),
            sign_key,
        }
    }

    /// Override the signing-key string (otherwise read from
    /// `PF_S3_REG_SIGN_KEY`).
    #[must_use]
    pub fn with_sign_key(mut self, key: impl Into<String>) -> Self {
        self.sign_key = Some(key.into());
        self
    }

    /// Resolve `(bucket, key_prefix)` from an [`ImageRef::S3`].
    /// `prefix` may be empty; we never start a key with `/` because
    /// some S3-compatible servers (notably MinIO 2021+) reject it.
    pub(super) fn resolve(target: &ImageRef) -> Result<(&str, &str), RegistryError> {
        match target {
            ImageRef::S3 { bucket, prefix } => Ok((bucket, prefix.trim_end_matches('/'))),
            other => Err(RegistryError::Backend(format!(
                "S3Registry called with non-s3 ref {other:?}"
            ))),
        }
    }

    fn key(prefix: &str, leaf: &str) -> String {
        if prefix.is_empty() {
            leaf.to_owned()
        } else {
            format!("{prefix}/{leaf}")
        }
    }

    fn blob_key(prefix: &str, d: &Digest256) -> String {
        let hex = d.hex();
        let leaf = format!("blobs/sha256/{}/{hex}.zst", &hex[..2]);
        Self::key(prefix, &leaf)
    }

    /// Put one object via the S3 SDK. Errors map to `Backend`.
    async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        body: Vec<u8>,
    ) -> Result<(), RegistryError> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(body))
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("S3 PutObject {key}: {e}")))?;
        Ok(())
    }

    async fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>, RegistryError> {
        let resp = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| RegistryError::Backend(format!("S3 GetObject {key}: {e}")))?;
        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| RegistryError::Backend(format!("S3 body collect {key}: {e}")))?
            .into_bytes()
            .to_vec();
        Ok(bytes)
    }

    /// HEAD the manifest key to answer `exists()`.
    async fn head_manifest(&self, bucket: &str, key: &str) -> Result<bool, RegistryError> {
        match self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let s = format!("{e}");
                if s.contains("NotFound") || s.contains("404") {
                    Ok(false)
                } else {
                    Err(RegistryError::Backend(format!("S3 HeadObject {key}: {e}")))
                }
            }
        }
    }

    /// List all blob keys under the prefix.
    async fn list_blob_keys(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<String>, RegistryError> {
        let blobs_prefix = Self::key(prefix, "blobs/sha256/");
        let mut keys = Vec::new();
        let mut continuation: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(&blobs_prefix);
            if let Some(token) = &continuation {
                req = req.continuation_token(token);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| RegistryError::Backend(format!("S3 ListObjectsV2: {e}")))?;
            for obj in resp.contents() {
                if let Some(k) = obj.key() {
                    if std::path::Path::new(k)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
                    {
                        keys.push(k.to_owned());
                    }
                }
            }
            if resp.is_truncated().unwrap_or(false) {
                continuation = resp.next_continuation_token().map(str::to_owned);
            } else {
                break;
            }
        }
        Ok(keys)
    }
}

#[async_trait]
impl Registry for S3Registry {
    async fn push(
        &self,
        target: &ImageRef,
        manifest: &Manifest,
        blobs: &dyn BlobStore,
    ) -> Result<(), RegistryError> {
        let (bucket, prefix) = Self::resolve(target)?;

        // 1. manifest + sig.
        let manifest_bytes = serde_json::to_vec(manifest)
            .map_err(|e| RegistryError::Backend(format!("manifest serialize: {e}")))?;
        let sig = sign_manifest(&manifest_bytes, self.sign_key.as_deref());
        let sig_bytes = serde_json::to_vec(&sig)
            .map_err(|e| RegistryError::Backend(format!("sig serialize: {e}")))?;

        self.put_object(bucket, &Self::key(prefix, "manifest.json"), manifest_bytes)
            .await?;
        self.put_object(bucket, &Self::key(prefix, "manifest.json.sig"), sig_bytes)
            .await?;

        // 2. transitively-reachable blobs.
        for digest in transitive_blob_digests(manifest, blobs)? {
            let raw = blobs.get(&digest)?;
            let compressed = zstd::encode_all(raw.as_slice(), 19)
                .map_err(|e| RegistryError::Backend(format!("zstd encode: {e}")))?;
            self.put_object(bucket, &Self::blob_key(prefix, &digest), compressed)
                .await?;
        }

        Ok(())
    }

    async fn pull(&self, source: &ImageRef) -> Result<LayerSet, RegistryError> {
        let (bucket, prefix) = Self::resolve(source)?;

        // 1. manifest + sig + verify.
        let manifest_bytes = self
            .get_object(bucket, &Self::key(prefix, "manifest.json"))
            .await?;
        let sig_bytes = self
            .get_object(bucket, &Self::key(prefix, "manifest.json.sig"))
            .await?;
        let sig: ManifestSignature = serde_json::from_slice(&sig_bytes)
            .map_err(|e| RegistryError::SignatureVerify(format!("parse sig: {e}")))?;
        verify_manifest(&manifest_bytes, &sig, self.sign_key.as_deref())
            .map_err(RegistryError::SignatureVerify)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| RegistryError::Backend(format!("parse manifest: {e}")))?;

        // 2. list every blob key the bucket has, fetch + decompress +
        //    verify each digest.
        let mut blobs = Vec::new();
        for key in self.list_blob_keys(bucket, prefix).await? {
            // key looks like "<prefix>/blobs/sha256/<aa>/<hex>.zst";
            // pull the hex out of the basename.
            let basename = key.rsplit('/').next().unwrap_or("");
            let Some(hex) = basename.strip_suffix(".zst") else {
                continue;
            };
            let Ok(digest) = Digest256::parse(&format!("sha256:{hex}")) else {
                continue;
            };
            let compressed = self.get_object(bucket, &key).await?;
            let bytes = zstd::decode_all(compressed.as_slice())
                .map_err(|e| RegistryError::Backend(format!("zstd decode {key}: {e}")))?;
            let observed = Digest256::of(&bytes);
            if observed != digest {
                return Err(RegistryError::Core(pf_core::Error::Integrity(format!(
                    "S3 blob {digest} hashes to {observed}"
                ))));
            }
            blobs.push((digest, bytes));
        }

        Ok(LayerSet { manifest, blobs })
    }

    async fn exists(&self, source: &ImageRef) -> Result<bool, RegistryError> {
        let (bucket, prefix) = Self::resolve(source)?;
        self.head_manifest(bucket, &Self::key(prefix, "manifest.json"))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_joins_prefix_and_leaf() {
        assert_eq!(
            S3Registry::key("agents/refactor", "manifest.json"),
            "agents/refactor/manifest.json"
        );
        assert_eq!(S3Registry::key("", "manifest.json"), "manifest.json");
    }

    #[test]
    fn blob_key_is_sharded_by_first_two_hex_chars() {
        let d = Digest256::of(b"hello world");
        let k = S3Registry::blob_key("p", &d);
        assert!(k.starts_with("p/blobs/sha256/"));
        assert!(
            std::path::Path::new(&k)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
        );
    }

    #[test]
    fn resolve_rejects_non_s3_ref() {
        let r = ImageRef::File {
            path: "/tmp/x".into(),
        };
        assert!(S3Registry::resolve(&r).is_err());
    }

    #[test]
    fn resolve_strips_trailing_slash_from_prefix() {
        let r = ImageRef::S3 {
            bucket: "b".into(),
            prefix: "a/b/".into(),
        };
        let (bucket, prefix) = S3Registry::resolve(&r).unwrap();
        assert_eq!(bucket, "b");
        assert_eq!(prefix, "a/b");
    }
}
