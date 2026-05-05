// SPDX-License-Identifier: MIT
//! URL-scheme parser for ProcessFork registry references.

use std::path::PathBuf;

/// One of the five supported registry URLs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageRef {
    /// `file:///abs/path` — local-FS-backed test registry.
    File { path: PathBuf },
    /// `hf://<user>/<repo>[:<tag>]` — Hugging Face Hub.
    Hf {
        user: String,
        repo: String,
        tag: Option<String>,
    },
    /// `s3://<bucket>/<prefix>` — AWS S3 / R2 / MinIO.
    S3 { bucket: String, prefix: String },
    /// `ipfs://<CID>` — IPFS pinned manifest.
    Ipfs { cid: String },
    /// `oci://<host>:<port>/<repo>[:<tag>]` — local OCI Distribution.
    Oci {
        host: String,
        port: Option<u16>,
        repo: String,
        tag: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ImageRefError {
    #[error("unrecognised URL scheme in {0:?} (expected file/hf/s3/ipfs/oci://)")]
    BadScheme(String),
    #[error("malformed {scheme} URL: {url:?} ({reason})")]
    Malformed {
        scheme: &'static str,
        url: String,
        reason: &'static str,
    },
}

impl ImageRef {
    /// Parse a URL into the typed enum.
    pub fn parse(url: &str) -> Result<Self, ImageRefError> {
        if let Some(rest) = url.strip_prefix("file://") {
            return Ok(Self::File {
                path: PathBuf::from(rest),
            });
        }
        if let Some(rest) = url.strip_prefix("hf://") {
            let (path, tag) = split_tag(rest);
            let parts: Vec<&str> = path.splitn(2, '/').collect();
            if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                return Err(ImageRefError::Malformed {
                    scheme: "hf",
                    url: url.to_owned(),
                    reason: "expected hf://<user>/<repo>[:<tag>]",
                });
            }
            return Ok(Self::Hf {
                user: parts[0].to_owned(),
                repo: parts[1].to_owned(),
                tag,
            });
        }
        if let Some(rest) = url.strip_prefix("s3://") {
            let parts: Vec<&str> = rest.splitn(2, '/').collect();
            if parts.is_empty() || parts[0].is_empty() {
                return Err(ImageRefError::Malformed {
                    scheme: "s3",
                    url: url.to_owned(),
                    reason: "expected s3://<bucket>/<prefix>",
                });
            }
            return Ok(Self::S3 {
                bucket: parts[0].to_owned(),
                prefix: parts.get(1).copied().unwrap_or("").to_owned(),
            });
        }
        if let Some(rest) = url.strip_prefix("ipfs://") {
            if rest.is_empty() {
                return Err(ImageRefError::Malformed {
                    scheme: "ipfs",
                    url: url.to_owned(),
                    reason: "expected ipfs://<CID>",
                });
            }
            return Ok(Self::Ipfs {
                cid: rest.to_owned(),
            });
        }
        if let Some(rest) = url.strip_prefix("oci://") {
            let (path, tag) = split_tag(rest);
            // Split `host[:port]/repo` — the slash separator delimits the repo.
            let (hostport, repo) = path.split_once('/').ok_or(ImageRefError::Malformed {
                scheme: "oci",
                url: url.to_owned(),
                reason: "expected oci://<host>[:<port>]/<repo>[:<tag>]",
            })?;
            let (host, port) = if let Some((h, p)) = hostport.split_once(':') {
                (
                    h.to_owned(),
                    Some(p.parse().map_err(|_| ImageRefError::Malformed {
                        scheme: "oci",
                        url: url.to_owned(),
                        reason: "port is not a u16",
                    })?),
                )
            } else {
                (hostport.to_owned(), None)
            };
            return Ok(Self::Oci {
                host,
                port,
                repo: repo.to_owned(),
                tag,
            });
        }
        Err(ImageRefError::BadScheme(url.to_owned()))
    }
}

/// Helper: split `prefix[:tag]` so a tag containing `/` (illegal in
/// HF / OCI tags) doesn't get accidentally captured.
fn split_tag(s: &str) -> (&str, Option<String>) {
    if let Some((path, tag)) = s.rsplit_once(':') {
        // Make sure we didn't split on a `:` that's part of a host:port —
        // HF / OCI tags can't contain `/`, so guard against that.
        if !tag.contains('/') {
            return (path, Some(tag.to_owned()));
        }
    }
    (s, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_scheme() {
        assert_eq!(
            ImageRef::parse("file:///tmp/store").unwrap(),
            ImageRef::File {
                path: PathBuf::from("/tmp/store")
            }
        );
    }

    #[test]
    fn hf_basic() {
        assert_eq!(
            ImageRef::parse("hf://alice/refactor-2026-05-05").unwrap(),
            ImageRef::Hf {
                user: "alice".into(),
                repo: "refactor-2026-05-05".into(),
                tag: None,
            }
        );
    }

    #[test]
    fn hf_with_tag() {
        assert_eq!(
            ImageRef::parse("hf://alice/foo:v1").unwrap(),
            ImageRef::Hf {
                user: "alice".into(),
                repo: "foo".into(),
                tag: Some("v1".into()),
            }
        );
    }

    #[test]
    fn s3_with_prefix() {
        assert_eq!(
            ImageRef::parse("s3://my-bucket/agents/refactor").unwrap(),
            ImageRef::S3 {
                bucket: "my-bucket".into(),
                prefix: "agents/refactor".into(),
            }
        );
    }

    #[test]
    fn ipfs_cid() {
        assert_eq!(
            ImageRef::parse("ipfs://bafyabc").unwrap(),
            ImageRef::Ipfs {
                cid: "bafyabc".into()
            }
        );
    }

    #[test]
    fn oci_host_port_repo_tag() {
        assert_eq!(
            ImageRef::parse("oci://harbor.local:8080/myorg/img:v3").unwrap(),
            ImageRef::Oci {
                host: "harbor.local".into(),
                port: Some(8080),
                repo: "myorg/img".into(),
                tag: Some("v3".into()),
            }
        );
    }

    #[test]
    fn bad_scheme_errors() {
        assert!(matches!(
            ImageRef::parse("ftp://x").unwrap_err(),
            ImageRefError::BadScheme(_)
        ));
    }

    #[test]
    fn hf_missing_repo() {
        assert!(ImageRef::parse("hf://alice").is_err());
    }
}
