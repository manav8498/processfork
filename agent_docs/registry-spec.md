# Registry spec

A registry stores `.pfimg` artifacts and lets users push/pull them. Four
backends, all behind the `pf-registry::Registry` trait:

```rust
#[async_trait]
pub trait Registry: Send + Sync {
    async fn push(&self, manifest: &Manifest, blobs: &dyn BlobStore) -> Result<()>;
    async fn pull(&self, image_ref: &ImageRef) -> Result<(Manifest, Vec<(Digest256, Vec<u8>)>)>;
    async fn exists(&self, image_ref: &ImageRef) -> Result<bool>;
}
```

## Hugging Face Hub

URL: `hf://<user>/<repo>[:tag]`.

- One HF "model repo" = one ProcessFork image.
- Manifest stored at `manifest.json` in the repo root.
- Blobs stored under `blobs/sha256/<aa>/<aabbcc…>`.
- Cosign signature stored at `manifest.json.sig`.
- Auth via `HF_TOKEN` env var.

## S3-compatible

URL: `s3://<bucket>/<prefix>`. Works with AWS S3, Cloudflare R2, MinIO, etc.

- One image = one prefix containing `manifest.json` + signature + blobs/.
- Auth via standard AWS env vars or instance profile.
- Multipart upload for blobs >100 MB.

## IPFS (feature-flag `ipfs`)

URL: `ipfs://<CID>`.

- Manifest pinned as a single CID.
- Blobs pinned individually; manifest holds the CID list.
- Auth via the local IPFS daemon (`http://127.0.0.1:5001` by default).

## Local OCI registry

URL: `oci://<host>:<port>/<repo>[:tag]`. Standard OCI Distribution API.

- For air-gapped use; works with Harbor, distribution/distribution, Zot.
- Auth via OCI_USERNAME / OCI_PASSWORD or docker-config.

## Cosign signing

On `push`, the manifest is signed with `cosign sign-blob`. The `--key` flag
selects the signing key; default is keyless (Sigstore Fulcio / Rekor).

On `pull`, the signature is verified before any blob is decompressed. Failed
verification rejects the pull with exit code 4.
