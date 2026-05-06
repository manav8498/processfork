// SPDX-License-Identifier: MIT
//! Live CDP client. Gated by the `cdp-live` feature.
//!
//! Two-stage protocol:
//!
//! 1. **HTTP discovery** — `GET <endpoint>/json` lists every open
//!    page with its `webSocketDebuggerUrl`.
//! 2. **WebSocket per-page** — connect to each page's debuggerUrl
//!    and send a stream of `{"id": N, "method": "...", "params": ...}`
//!    JSON-RPC requests; we use `Page.captureSnapshot`,
//!    `DOMStorage.getDOMStorageItems` (×2 for local/session), and
//!    `Network.getCookies`.

use std::sync::Arc;

use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use futures_util::{SinkExt, StreamExt};
use pf_core::cas::BlobStore;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::PageSnapshot;

/// Connect-and-capture client for one Chromium debugger endpoint.
pub struct CdpClient {
    /// HTTP base, e.g. `http://127.0.0.1:9222`.
    endpoint: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct TargetInfo {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(default)]
    title: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_debugger_url: String,
}

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

impl CdpClient {
    /// Build a client pointed at a CDP HTTP endpoint
    /// (e.g. `http://127.0.0.1:9222`).
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            http: reqwest::Client::builder()
                .user_agent(concat!("processfork/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
        }
    }

    /// List every page-style target via `GET /json`.
    async fn list_targets(&self) -> pf_core::Result<Vec<TargetInfo>> {
        let url = format!("{}/json", self.endpoint);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| pf_core::Error::Integrity(format!("CDP /json: {e}")))?;
        let targets: Vec<TargetInfo> = resp
            .json()
            .await
            .map_err(|e| pf_core::Error::Integrity(format!("CDP /json decode: {e}")))?;
        // Only "page" targets are relevant — service workers, browser,
        // and shared workers don't have an MHTML representation.
        Ok(targets.into_iter().filter(|t| t.kind == "page").collect())
    }

    /// Capture every page. Returns one `PageSnapshot` per open tab.
    pub async fn capture(&self, blobs: Arc<dyn BlobStore>) -> pf_core::Result<Vec<PageSnapshot>> {
        let targets = self.list_targets().await?;
        let mut out = Vec::with_capacity(targets.len());
        for target in targets {
            let snap = self.capture_one(target, blobs.clone()).await?;
            out.push(snap);
        }
        Ok(out)
    }

    /// Capture one page: open WS, run the four RPC methods, store
    /// MHTML + cookies as content-addressed blobs.
    async fn capture_one(
        &self,
        target: TargetInfo,
        blobs: Arc<dyn BlobStore>,
    ) -> pf_core::Result<PageSnapshot> {
        let (ws, _resp) = tokio_tungstenite::connect_async(&target.ws_debugger_url)
            .await
            .map_err(|e| pf_core::Error::Integrity(format!("CDP WS connect: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        let mut next_id: u64 = 0;

        // Enable the domains we need before issuing their methods.
        rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "Page.enable",
            json!({}),
        )
        .await?;
        rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "DOMStorage.enable",
            json!({}),
        )
        .await?;
        rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "Network.enable",
            json!({}),
        )
        .await?;

        // 1. Layout metrics (viewport + scroll + dpr).
        let lm = rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "Page.getLayoutMetrics",
            json!({}),
        )
        .await?;
        let layout = &lm["layoutViewport"];
        let visual = &lm["visualViewport"];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let viewport_width = visual["clientWidth"].as_f64().unwrap_or(0.0) as u32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let viewport_height = visual["clientHeight"].as_f64().unwrap_or(0.0) as u32;
        let scroll_x = layout["pageX"].as_f64().unwrap_or(0.0);
        let scroll_y = layout["pageY"].as_f64().unwrap_or(0.0);
        let device_pixel_ratio = visual["scale"].as_f64().unwrap_or(1.0);

        // 2. MHTML snapshot.
        let mhtml = rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "Page.captureSnapshot",
            json!({"format": "mhtml"}),
        )
        .await?;
        let mhtml_data = mhtml["data"].as_str().unwrap_or("");
        let mhtml_digest = blobs.put(mhtml_data.as_bytes())?;

        // 3. localStorage + sessionStorage.
        let origin = origin_from_url(&target.url);
        let local_storage =
            fetch_storage(&mut sink, &mut stream, &mut next_id, &origin, true).await?;
        let session_storage =
            fetch_storage(&mut sink, &mut stream, &mut next_id, &origin, false).await?;

        // 4. Cookies — keep CDP's JSON shape opaque.
        let cookies = rpc(
            &mut sink,
            &mut stream,
            &mut next_id,
            "Network.getCookies",
            json!({}),
        )
        .await?;
        let cookies_bytes = serde_json::to_vec(&cookies["cookies"])
            .map_err(|e| pf_core::Error::Integrity(format!("CDP cookie encode: {e}")))?;
        let cookies_digest = blobs.put(&cookies_bytes)?;

        Ok(PageSnapshot {
            target_id: target.id,
            url: target.url,
            title: target.title,
            viewport_width,
            viewport_height,
            scroll_x,
            scroll_y,
            device_pixel_ratio,
            mhtml_digest,
            local_storage,
            session_storage,
            cookies_digest,
        })
    }
}

/// One CDP request/response pair. Skips intervening events.
async fn rpc(
    sink: &mut WsSink,
    stream: &mut WsStream,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> pf_core::Result<Value> {
    *next_id += 1;
    let id = *next_id;
    let req = json!({"id": id, "method": method, "params": params});
    let req_text = serde_json::to_string(&req)
        .map_err(|e| pf_core::Error::Integrity(format!("CDP encode: {e}")))?;
    sink.send(Message::Text(req_text))
        .await
        .map_err(|e| pf_core::Error::Integrity(format!("CDP WS send: {e}")))?;
    loop {
        let msg = stream
            .next()
            .await
            .ok_or_else(|| pf_core::Error::Integrity("CDP WS closed".into()))?
            .map_err(|e| pf_core::Error::Integrity(format!("CDP WS recv: {e}")))?;
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => String::from_utf8(b)
                .map_err(|e| pf_core::Error::Integrity(format!("CDP WS utf8: {e}")))?,
            Message::Close(_) => return Err(pf_core::Error::Integrity("CDP WS closed".into())),
            // ping/pong/frame — read more.
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        };
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| pf_core::Error::Integrity(format!("CDP decode: {e}")))?;
        if v.get("id").and_then(Value::as_u64) == Some(id) {
            if let Some(err) = v.get("error") {
                return Err(pf_core::Error::Integrity(format!(
                    "CDP {method} error: {err}"
                )));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
        // Unrelated event; loop and read more.
    }
}

async fn fetch_storage(
    sink: &mut WsSink,
    stream: &mut WsStream,
    next_id: &mut u64,
    origin: &str,
    is_local: bool,
) -> pf_core::Result<std::collections::BTreeMap<String, String>> {
    let storage_id = json!({"securityOrigin": origin, "isLocalStorage": is_local});
    let resp = rpc(
        sink,
        stream,
        next_id,
        "DOMStorage.getDOMStorageItems",
        json!({"storageId": storage_id}),
    )
    .await?;
    let mut out = std::collections::BTreeMap::new();
    if let Some(entries) = resp.get("entries").and_then(Value::as_array) {
        for kv in entries {
            if let Some(arr) = kv.as_array()
                && arr.len() == 2
            {
                out.insert(
                    arr[0].as_str().unwrap_or("").to_owned(),
                    arr[1].as_str().unwrap_or("").to_owned(),
                );
            }
        }
    }
    Ok(out)
}

fn origin_from_url(url: &str) -> String {
    if let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    {
        let host_end = rest.find('/').unwrap_or(rest.len());
        let host = &rest[..host_end];
        let scheme = if url.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        return format!("{scheme}://{host}");
    }
    url.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_extracts_scheme_and_host_from_https() {
        assert_eq!(
            origin_from_url("https://example.com/path?x=1"),
            "https://example.com"
        );
        assert_eq!(
            origin_from_url("http://localhost:8080/"),
            "http://localhost:8080"
        );
    }

    #[test]
    fn origin_passes_through_non_http_urls() {
        assert_eq!(origin_from_url("file:///x"), "file:///x");
    }
}
