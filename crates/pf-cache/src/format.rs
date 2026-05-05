// SPDX-License-Identifier: MIT
//! Wire format for the cache layer (`paged-batchinvariant-v1`).
//!
//! Mirrors `agent_docs/cache-layer.md` §"On-disk format" exactly. The page
//! manifest is serialized as JSON for human-debuggability; the per-page K/V
//! payloads are raw bytes (zstd-compressed at the [`pf_core::cas::FsBlobStore`]
//! layer, not double-compressed here).

use pf_core::digest::Digest256;
use serde::{Deserialize, Serialize};

/// Schema discriminator for the v1 layout.
pub const LAYOUT_V1: &str = "paged-batchinvariant-v1";

/// Numeric dtype of cache entries. Matches the engine-side dtype 1:1 — we
/// never convert here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dtype {
    /// IEEE bfloat16 — vLLM and SGLang default for Llama-class models.
    Bf16,
    /// IEEE binary16.
    F16,
    /// IEEE binary32 (single-precision; rare in production).
    F32,
    /// 8-bit FP, E4M3 layout.
    Fp8E4m3,
}

impl Dtype {
    /// Bytes per element.
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Bf16 | Self::F16 => 2,
            Self::F32 => 4,
            Self::Fp8E4m3 => 1,
        }
    }
}

/// Static metadata describing a paged KV cache. Identical across pages of
/// the same engine instance; embedded once in the [`PageManifest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheMeta {
    /// Tokens per page (vLLM default 16).
    pub page_size_tokens: u32,
    /// Number of transformer layers.
    pub n_layers: u32,
    /// Number of attention heads.
    pub n_heads: u32,
    /// Per-head dimension.
    pub head_dim: u32,
    /// Numeric dtype.
    pub dtype: Dtype,
}

impl CacheMeta {
    /// Bytes per K-page (or per V-page; they're the same shape).
    #[must_use]
    pub const fn page_bytes(&self) -> usize {
        (self.n_layers as usize)
            * (self.page_size_tokens as usize)
            * (self.n_heads as usize)
            * (self.head_dim as usize)
            * self.dtype.bytes()
    }
}

/// One physical page in the cache. K and V are content-addressed
/// independently so a fork that only mutates V (e.g. via a single-token
/// generation step) shares its K page with siblings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    /// Physical-page index inside the engine's page table.
    pub ix: u32,
    /// Digest of the K-tensor bytes for this page.
    pub k: Digest256,
    /// Digest of the V-tensor bytes for this page.
    pub v: Digest256,
}

/// One logical request (sequence) in the cache, mapping its token positions
/// onto a list of physical pages. Preserved across snapshot/restore so
/// prefix-sharing (vLLM PagedAttention, SGLang RadixAttention) survives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalSeq {
    /// Stable identifier for this sequence.
    pub id: String,
    /// Ordered list of physical-page indices the sequence occupies.
    pub page_ixs: Vec<u32>,
    /// How many of `page_size_tokens` slots in the LAST page are occupied.
    /// `0` means the last page is full and the next token starts a new page.
    pub fill_in_last_page: u32,
}

/// Top-level page manifest. Serialized as JSON; persisted as a single CAS
/// blob whose digest goes into the `.pfimg` manifest's `cache.manifest`
/// field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageManifest {
    /// Always [`LAYOUT_V1`] in this version.
    pub layout: String,
    /// Static metadata (page size, n_layers, dtype, etc.).
    #[serde(flatten)]
    pub meta: CacheMeta,
    /// Pages sorted by `ix` for deterministic manifest digests.
    pub pages: Vec<Page>,
    /// Logical sequences sorted by `id`.
    pub logical_seqs: Vec<LogicalSeq>,
}

impl PageManifest {
    /// Construct a fresh manifest with the v1 layout tag pre-set.
    #[must_use]
    pub fn new(meta: CacheMeta) -> Self {
        Self {
            layout: LAYOUT_V1.into(),
            meta,
            pages: Vec::new(),
            logical_seqs: Vec::new(),
        }
    }

    /// Sort pages by `ix` and seqs by `id` so the JSON serialization (and
    /// therefore the digest) is invariant w.r.t. iteration order.
    pub fn canonicalize(&mut self) {
        self.pages.sort_by_key(|p| p.ix);
        self.logical_seqs.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> CacheMeta {
        CacheMeta {
            page_size_tokens: 16,
            n_layers: 80,
            n_heads: 64,
            head_dim: 128,
            dtype: Dtype::Bf16,
        }
    }

    #[test]
    fn page_bytes_matches_spec() {
        // 80 layers × 16 tokens × 64 heads × 128 head_dim × 2 B (bf16) per page
        // = 20,971,520 bytes ≈ 20 MiB per K-page (and per V-page).
        assert_eq!(meta().page_bytes(), 80 * 16 * 64 * 128 * 2);
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let mut m = PageManifest::new(meta());
        let d = Digest256::of(b"x");
        m.pages.push(Page {
            ix: 1,
            k: d.clone(),
            v: d.clone(),
        });
        m.pages.push(Page {
            ix: 0,
            k: d.clone(),
            v: d.clone(),
        });
        m.logical_seqs.push(LogicalSeq {
            id: "seq-A".into(),
            page_ixs: vec![0, 1],
            fill_in_last_page: 7,
        });
        m.canonicalize();
        let s = serde_json::to_string(&m).unwrap();
        let back: PageManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.layout, LAYOUT_V1);
        assert_eq!(back.pages[0].ix, 0); // canonicalized order
        assert_eq!(back.meta.page_size_tokens, 16);
    }

    #[test]
    fn canonicalize_makes_digest_order_invariant() {
        let d = Digest256::of(b"x");
        let mut a = PageManifest::new(meta());
        a.pages.push(Page {
            ix: 0,
            k: d.clone(),
            v: d.clone(),
        });
        a.pages.push(Page {
            ix: 1,
            k: d.clone(),
            v: d.clone(),
        });
        let mut b = PageManifest::new(meta());
        b.pages.push(Page {
            ix: 1,
            k: d.clone(),
            v: d.clone(),
        });
        b.pages.push(Page {
            ix: 0,
            k: d.clone(),
            v: d,
        });
        a.canonicalize();
        b.canonicalize();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
    }

    #[test]
    fn dtype_bytes_correct() {
        assert_eq!(Dtype::Bf16.bytes(), 2);
        assert_eq!(Dtype::F16.bytes(), 2);
        assert_eq!(Dtype::F32.bytes(), 4);
        assert_eq!(Dtype::Fp8E4m3.bytes(), 1);
    }
}
