// SPDX-License-Identifier: MIT
//! Content-addressed blob store.
//!
//! Two implementations ship in v1:
//! - [`FsBlobStore`]: sharded on-disk store, zstd-19 compressed at rest,
//!   atomic write via temp + rename. Default backing for `~/.processfork`.
//! - [`MemBlobStore`]: in-memory; used for unit tests and the `--ephemeral`
//!   CLI flag.
//!
//! Both implement [`BlobStore`].

use crate::digest::Digest256;
use crate::error::{Error, Result};

use dashmap::DashMap;
use parking_lot::Mutex;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Default zstd compression level. Spec §4.2 mandates 19 (high ratio,
/// reasonable encode CPU at our blob sizes).
pub const ZSTD_LEVEL: i32 = 19;

/// A read/write content-addressed store.
///
/// Implementations MUST guarantee that `get(put(x)?)?` round-trips to bytes
/// identical to `x`, and that the returned digest equals
/// [`Digest256::of`](crate::digest::Digest256::of) of `x`.
pub trait BlobStore: Send + Sync {
    /// Insert `bytes`, return its content digest. Idempotent: storing the
    /// same payload twice produces the same digest and is a no-op the second
    /// time.
    fn put(&self, bytes: &[u8]) -> Result<Digest256>;

    /// Retrieve the bytes for a previously-stored digest.
    fn get(&self, digest: &Digest256) -> Result<Vec<u8>>;

    /// Cheap existence check.
    fn contains(&self, digest: &Digest256) -> Result<bool>;

    /// Total physical bytes stored on disk (compressed). Implementations may
    /// approximate; used for `pf status` and the storage-efficiency
    /// microbenchmark.
    fn physical_bytes(&self) -> Result<u64>;
}

// ------------------------- FsBlobStore -------------------------

/// On-disk content-addressed store, sharded by digest prefix.
///
/// Layout (rooted at `<root>/blobs/sha256/`):
///
/// ```text
/// blobs/sha256/
///   ab/
///     ab12cd34…ff.zst        (zstd-19 compressed payload)
///   cd/
///     cd56ef78…00.zst
/// ```
///
/// The two-byte shard prefix keeps directory entry counts <65 536 even at
/// large stores. Atomic write is `temp file in same dir → fsync → rename`.
///
/// Thread-safe (concurrent `put`/`get` ok). On a race two writers may both
/// produce the same temp file path — we add a per-process counter to dodge.
#[derive(Debug)]
pub struct FsBlobStore {
    root: PathBuf,
    /// Per-process tempfile counter to avoid intra-process write races.
    counter: Mutex<u64>,
}

impl FsBlobStore {
    /// Open (or create) an on-disk store rooted at `root`.
    ///
    /// Creates `root/blobs/sha256/` if missing. Idempotent.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let blobs = root.join("blobs").join("sha256");
        fs::create_dir_all(&blobs)?;
        Ok(Self {
            root,
            counter: Mutex::new(0),
        })
    }

    /// The root directory passed to [`Self::open`].
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve the on-disk path for a given digest, creating the shard
    /// subdirectory if it does not exist.
    fn path_for(&self, digest: &Digest256) -> Result<PathBuf> {
        let hex = digest.hex();
        let shard = &hex[..2];
        let dir = self.root.join("blobs").join("sha256").join(shard);
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("{hex}.zst")))
    }

    /// Walk the entire store and sum compressed file sizes.
    fn walk_size(dir: &Path) -> Result<u64> {
        let mut total = 0u64;
        if !dir.exists() {
            return Ok(0);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                total = total.saturating_add(Self::walk_size(&entry.path())?);
            } else if ty.is_file() {
                total = total.saturating_add(entry.metadata()?.len());
            }
        }
        Ok(total)
    }
}

impl BlobStore for FsBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<Digest256> {
        let digest = Digest256::of(bytes);
        let final_path = self.path_for(&digest)?;

        // Idempotency: identical content → same digest → same path.
        if final_path.exists() {
            return Ok(digest);
        }

        // Compose a unique temp path in the SAME directory so rename is atomic
        // (POSIX guarantee). Counter avoids races between concurrent writers
        // in the same process; PID disambiguates between concurrent processes.
        let counter = {
            let mut g = self.counter.lock();
            *g = g.wrapping_add(1);
            *g
        };
        let pid = std::process::id();
        let tmp_path = final_path.with_extension(format!("tmp.{pid}.{counter}"));

        // Stream-encode with zstd-19. We allocate a single Vec because our
        // largest blob (a KV-cache page) is well under 64 MiB; switch to
        // streaming if we ever exceed that.
        let compressed = zstd::encode_all(bytes, ZSTD_LEVEL)?;

        // Write + fsync + atomic rename.
        {
            let mut f = fs::File::create(&tmp_path)?;
            f.write_all(&compressed)?;
            f.sync_all()?;
        }
        match fs::rename(&tmp_path, &final_path) {
            Ok(()) => Ok(digest),
            Err(e) => {
                // Best-effort cleanup of the temp file; ignore errors.
                let _ = fs::remove_file(&tmp_path);
                Err(e.into())
            }
        }
    }

    fn get(&self, digest: &Digest256) -> Result<Vec<u8>> {
        let path = self.path_for(digest)?;
        let mut f = fs::File::open(&path)?;
        let mut compressed = Vec::new();
        f.read_to_end(&mut compressed)?;
        let bytes = zstd::decode_all(compressed.as_slice())?;

        // Defence in depth: re-hash on read. If anything corrupted the file,
        // we surface it as `Error::Integrity` instead of returning bad bytes.
        let observed = Digest256::of(&bytes);
        if &observed != digest {
            return Err(Error::Integrity(format!(
                "blob {digest} on disk hashes to {observed}"
            )));
        }
        Ok(bytes)
    }

    fn contains(&self, digest: &Digest256) -> Result<bool> {
        Ok(self.path_for(digest)?.exists())
    }

    fn physical_bytes(&self) -> Result<u64> {
        Self::walk_size(&self.root.join("blobs"))
    }
}

// ------------------------- MemBlobStore -------------------------

/// In-memory CAS — used by unit tests and the (planned) `--ephemeral` CLI
/// flag. Stores raw bytes; no compression because we already paid for the
/// allocation.
#[derive(Debug, Default)]
pub struct MemBlobStore {
    inner: DashMap<Digest256, Vec<u8>>,
}

impl MemBlobStore {
    /// Construct an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlobStore for MemBlobStore {
    fn put(&self, bytes: &[u8]) -> Result<Digest256> {
        let d = Digest256::of(bytes);
        self.inner
            .entry(d.clone())
            .or_insert_with(|| bytes.to_vec());
        Ok(d)
    }

    fn get(&self, digest: &Digest256) -> Result<Vec<u8>> {
        self.inner
            .get(digest)
            .map(|r| r.value().clone())
            .ok_or_else(|| Error::Integrity(format!("not in mem store: {digest}")))
    }

    fn contains(&self, digest: &Digest256) -> Result<bool> {
        Ok(self.inner.contains_key(digest))
    }

    fn physical_bytes(&self) -> Result<u64> {
        Ok(self
            .inner
            .iter()
            .map(|kv| kv.value().len() as u64)
            .sum::<u64>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fs_round_trip_byte_identical() {
        let dir = TempDir::new().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let payload = b"the quick brown fox".to_vec();
        let d = store.put(&payload).unwrap();
        assert_eq!(d, Digest256::of(&payload));
        assert!(store.contains(&d).unwrap());
        let back = store.get(&d).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn fs_dedupes_identical_writes() {
        let dir = TempDir::new().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let payload = vec![0xABu8; 1024];
        let d1 = store.put(&payload).unwrap();
        let size_after_first = store.physical_bytes().unwrap();
        let d2 = store.put(&payload).unwrap();
        let size_after_second = store.physical_bytes().unwrap();
        assert_eq!(d1, d2);
        assert_eq!(
            size_after_first, size_after_second,
            "second put must be a no-op"
        );
    }

    #[test]
    fn fs_detects_corruption() {
        let dir = TempDir::new().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let d = store.put(b"original").unwrap();
        let path = store.path_for(&d).unwrap();
        // Overwrite with garbage; the on-read re-hash must catch it.
        fs::write(&path, b"\x28\xb5\x2f\xfd\x00garbage").unwrap();
        let err = store.get(&d).unwrap_err();
        assert!(matches!(err, Error::Integrity(_)) || matches!(err, Error::Io(_)));
    }

    #[test]
    fn mem_round_trip() {
        let store = MemBlobStore::new();
        let d = store.put(b"hello").unwrap();
        assert_eq!(store.get(&d).unwrap(), b"hello".to_vec());
        assert!(store.contains(&d).unwrap());
    }

    #[test]
    fn fs_sharding_uses_first_two_hex_chars() {
        let dir = TempDir::new().unwrap();
        let store = FsBlobStore::open(dir.path()).unwrap();
        let d = store.put(b"sharding test").unwrap();
        let path = store.path_for(&d).unwrap();
        let parent = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(parent, &d.hex()[..2]);
    }
}
