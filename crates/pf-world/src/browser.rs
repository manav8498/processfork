// SPDX-License-Identifier: MIT
//! Browser DOM capture via Chrome DevTools Protocol.
//!
//! Closes the §M2 World layer 'browser DOM via CDP for
//! Playwright/Puppeteer sandboxes' deliverable. Connects to a
//! Chromium instance launched with `--remote-debugging-port=<port>`
//! and snapshots every open page:
//!
//! - URL, title, viewport, scroll position, devicePixelRatio
//! - Page.captureSnapshot (MHTML) — the entire serialised page
//! - DOMStorage.getDOMStorageItems for localStorage AND sessionStorage
//! - Network.getCookies (subject to env-scrub regex if the operator wants)
//!
//! On restore we emit instructions for re-spawning Chromium with the
//! same flags and CDP-loading each MHTML; auto-spawn is operator-led
//! since headless flags vary widely.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(feature = "cdp-live")]
mod live;
#[cfg(feature = "cdp-live")]
pub use live::CdpClient;

/// Wire format of a captured browser session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum BrowserBlob {
    /// Real CDP capture from a Chromium instance.
    #[serde(rename = "browser.cdp.v1")]
    Cdp {
        /// CDP endpoint we connected to (e.g. http://127.0.0.1:9222).
        endpoint: String,
        /// One per open tab/page.
        pages: Vec<PageSnapshot>,
    },
    /// Placeholder for hosts where CDP capture is not available
    /// (no chromium running, no `cdp-live` feature compiled).
    #[serde(rename = "browser.unsupported.v1")]
    Unsupported { reason: String },
}

/// Snapshot of a single browser page/tab.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageSnapshot {
    /// CDP page id (a UUID-like string).
    pub target_id: String,
    pub url: String,
    pub title: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub device_pixel_ratio: f64,
    /// Digest of the MHTML body (so duplicates dedup like every other
    /// blob in the .pfimg).
    pub mhtml_digest: Digest256,
    /// localStorage entries.
    pub local_storage: std::collections::BTreeMap<String, String>,
    /// sessionStorage entries.
    pub session_storage: std::collections::BTreeMap<String, String>,
    /// Cookies as a JSON-array of CDP CookieParam objects (we keep the
    /// shape opaque to avoid refighting the Cookie schema for every
    /// CDP minor revision).
    pub cookies_digest: Digest256,
}

/// Captures the (optional) attached-browser DOM.
///
/// Construct with [`BrowserCapture::new`] passing the CDP HTTP endpoint
/// (typically `http://127.0.0.1:9222`). Build the chromium with
/// `--remote-debugging-port=9222 --headless=new` (or omit `--headless`
/// for a visible browser). Then call [`BrowserCapture::capture`].
pub struct BrowserCapture {
    endpoint: Option<String>,
}

impl BrowserCapture {
    /// Construct from an explicit endpoint. `None` = capture is skipped
    /// and the resulting blob is `BrowserBlob::Unsupported`.
    #[must_use]
    pub fn new(endpoint: Option<String>) -> Self {
        Self { endpoint }
    }

    /// Construct from `$PF_BROWSER_CDP` env var. Same Unsupported
    /// fall-through if the env var is unset/empty.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("PF_BROWSER_CDP")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    /// Capture the current browser state. Always returns a digest
    /// pointing at the on-disk JSON serialization of [`BrowserBlob`].
    pub async fn capture(&self, blobs: &Arc<dyn BlobStore>) -> pf_core::Result<Digest256> {
        let blob = match (&self.endpoint, cfg!(feature = "cdp-live")) {
            (Some(endpoint), true) => self.capture_cdp(endpoint, blobs).await?,
            (Some(_endpoint), false) => BrowserBlob::Unsupported {
                reason: "pf-world built without `cdp-live` feature".into(),
            },
            (None, _) => BrowserBlob::Unsupported {
                reason:
                    "no CDP endpoint configured (set PF_BROWSER_CDP or pass to BrowserCapture::new)"
                        .into(),
            },
        };
        blobs.put(&serde_json::to_vec(&blob)?)
    }

    #[cfg(feature = "cdp-live")]
    async fn capture_cdp(
        &self,
        endpoint: &str,
        blobs: &Arc<dyn BlobStore>,
    ) -> pf_core::Result<BrowserBlob> {
        let client = live::CdpClient::new(endpoint.to_owned());
        match client.capture(blobs.clone()).await {
            Ok(pages) => Ok(BrowserBlob::Cdp {
                endpoint: endpoint.to_owned(),
                pages,
            }),
            Err(e) => {
                tracing::warn!(?e, "CDP capture failed; emitting Unsupported placeholder");
                Ok(BrowserBlob::Unsupported {
                    reason: format!("CDP capture failed: {e}"),
                })
            }
        }
    }

    #[cfg(not(feature = "cdp-live"))]
    #[allow(clippy::unused_async, clippy::unused_self)]
    async fn capture_cdp(
        &self,
        _endpoint: &str,
        _blobs: &Arc<dyn BlobStore>,
    ) -> pf_core::Result<BrowserBlob> {
        unreachable!("capture_cdp only called when cdp-live is on")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    #[tokio::test]
    async fn browser_capture_no_endpoint_returns_unsupported() {
        let blobs: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new());
        let cap = BrowserCapture::new(None);
        let digest = cap.capture(&blobs).await.unwrap();
        let bytes = blobs.get(&digest).unwrap();
        let blob: BrowserBlob = serde_json::from_slice(&bytes).unwrap();
        match blob {
            BrowserBlob::Unsupported { reason } => {
                assert!(reason.contains("CDP endpoint"), "{reason}");
            }
            BrowserBlob::Cdp { .. } => panic!("expected Unsupported, got Cdp"),
        }
    }

    #[test]
    fn page_snapshot_round_trips_through_json() {
        let p = PageSnapshot {
            target_id: "T-1".into(),
            url: "https://example.com".into(),
            title: "Example".into(),
            viewport_width: 1280,
            viewport_height: 800,
            scroll_x: 0.0,
            scroll_y: 42.5,
            device_pixel_ratio: 2.0,
            mhtml_digest: Digest256::of(b"mhtml"),
            local_storage: [("k".to_owned(), "v".to_owned())].into(),
            session_storage: std::collections::BTreeMap::default(),
            cookies_digest: Digest256::of(b"[]"),
        };
        let s = serde_json::to_string(&p).unwrap();
        let p2: PageSnapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(p.url, p2.url);
        assert_eq!(p.viewport_width, p2.viewport_width);
        assert_eq!(p.local_storage.get("k").map(String::as_str), Some("v"));
    }

    // Note: from_env() is exercised end-to-end by the operator-side
    // CDP integration test (operator sets PF_BROWSER_CDP before
    // running pf snapshot). We deliberately don't test it here
    // because env mutation is `unsafe` since Rust 1.85 and pf-world
    // has `#![deny(unsafe_code)]`.
}
