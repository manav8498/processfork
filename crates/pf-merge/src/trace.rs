// SPDX-License-Identifier: MIT
//! Trace-layer merge: extract a "lessons learned" patch from B and append
//! it to A as a system message.

use pf_core::cas::BlobStore;
use pf_core::digest::Digest256;
use serde::{Deserialize, Serialize};

/// Pluggable summarizer interface. The default impl ([`StubSummarizer`])
/// emits a deterministic concatenation of B's last `n` messages — enough
/// to test the merge plumbing without an API key.
pub trait Summarizer: Send + Sync {
    /// Given the trace messages of branch `b` (in causal order) and of the
    /// common ancestor `x`, return at most `max_chars` of a "lessons
    /// learned" string suitable for injection as a system message.
    ///
    /// Implementations MUST be deterministic given the same inputs (the
    /// merge protocol depends on it for short-circuit caching).
    fn summarize(
        &self,
        b: &[TraceMessage],
        x: &[TraceMessage],
        max_chars: usize,
    ) -> pf_core::Result<String>;
}

/// One message in a trace blob (`trace.messages.v1` format — matches the
/// fixture in `pf-core::fixture` and the wire format used by the SDK
/// adapters).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceMessage {
    /// Speaker role: `"system"`, `"user"`, `"assistant"`, `"tool"`.
    pub role: String,
    /// Message content (UTF-8).
    pub content: String,
}

/// What [`merge_trace`] returns alongside the new trace digest.
#[derive(Clone, Debug)]
pub struct TraceMergeOutcome {
    /// Digest of the merged trace blob (A's messages + injected system
    /// patch).
    pub merged_trace: Digest256,
    /// The system message we injected. Surfaced for logging in the engine
    /// report.
    pub injected_summary: String,
    /// How many tokens (rough char-÷-4 estimate) need to be re-prefilled
    /// at the cache layer when this trace is checked out. Used for the
    /// `agent_docs/merge-protocol.md` UX line.
    pub re_prefill_token_estimate: u32,
}

/// Read three trace blobs (A, B, X), summarize B's divergence, and emit a
/// new trace blob = `A.messages + [system: <summary>]`. Returns
/// [`TraceMergeOutcome`].
///
/// `summarizer` is supplied by the caller; tests pass [`StubSummarizer`].
pub fn merge_trace(
    blobs: &dyn BlobStore,
    a: &Digest256,
    b: &Digest256,
    x: &Digest256,
    summarizer: &dyn Summarizer,
) -> pf_core::Result<TraceMergeOutcome> {
    let a_msgs = read_trace(blobs, a)?;
    let b_msgs = read_trace(blobs, b)?;
    let x_msgs = read_trace(blobs, x)?;
    let summary = summarizer.summarize(&b_msgs, &x_msgs, 1024)?;

    let mut merged = a_msgs;
    let injected = TraceMessage {
        role: "system".into(),
        content: format!("[merged from sibling] {summary}"),
    };
    merged.push(injected.clone());

    let bytes = encode_trace(&merged)?;
    let cid = blobs.put(&bytes)?;

    // Cheap char-÷-4 token estimate (matches Anthropic's quick-rule).
    #[allow(clippy::cast_possible_truncation)]
    let re_prefill_token_estimate = (injected.content.len() / 4) as u32;

    Ok(TraceMergeOutcome {
        merged_trace: cid,
        injected_summary: injected.content,
        re_prefill_token_estimate,
    })
}

fn read_trace(blobs: &dyn BlobStore, digest: &Digest256) -> pf_core::Result<Vec<TraceMessage>> {
    let bytes = blobs.get(digest)?;
    let mut out = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let m: TraceMessage = serde_json::from_slice(line)?;
        out.push(m);
    }
    Ok(out)
}

fn encode_trace(msgs: &[TraceMessage]) -> pf_core::Result<Vec<u8>> {
    let mut out = Vec::new();
    for m in msgs {
        out.extend_from_slice(&serde_json::to_vec(m)?);
        out.push(b'\n');
    }
    Ok(out)
}

/// Deterministic test summarizer — concatenates the last few B messages
/// after the X-divergence point. Used by every test in this crate; the
/// real Claude API call lives behind the `live-summarizer` feature flag.
#[derive(Default)]
pub struct StubSummarizer;

impl Summarizer for StubSummarizer {
    fn summarize(
        &self,
        b: &[TraceMessage],
        x: &[TraceMessage],
        max_chars: usize,
    ) -> pf_core::Result<String> {
        use std::fmt::Write;
        // "Divergence" = b messages beyond the prefix shared with x.
        let common = b.iter().zip(x.iter()).take_while(|(p, q)| p == q).count();
        let divergent: Vec<&TraceMessage> = b.iter().skip(common).collect();
        let mut joined = String::new();
        for m in divergent.iter().rev().take(4).rev() {
            if !joined.is_empty() {
                joined.push_str(" / ");
            }
            let _ = write!(joined, "[{}] {}", m.role, m.content);
        }
        if joined.len() > max_chars {
            joined.truncate(max_chars);
        }
        if joined.is_empty() {
            joined.push_str("(no divergence)");
        }
        Ok(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::cas::MemBlobStore;

    fn put_trace(blobs: &dyn BlobStore, msgs: &[(&str, &str)]) -> Digest256 {
        let v: Vec<TraceMessage> = msgs
            .iter()
            .map(|(r, c)| TraceMessage {
                role: (*r).into(),
                content: (*c).into(),
            })
            .collect();
        let bytes = encode_trace(&v).unwrap();
        blobs.put(&bytes).unwrap()
    }

    #[test]
    fn stub_summarizer_finds_divergence() {
        let s = StubSummarizer;
        let x = vec![TraceMessage {
            role: "user".into(),
            content: "hello".into(),
        }];
        let b = vec![
            TraceMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            TraceMessage {
                role: "assistant".into(),
                content: "I learned X".into(),
            },
        ];
        let summary = s.summarize(&b, &x, 1024).unwrap();
        assert!(summary.contains("I learned X"));
    }

    #[test]
    fn merge_trace_appends_system_message() {
        let blobs = MemBlobStore::new();
        let x = put_trace(&blobs, &[("user", "go")]);
        let a = put_trace(
            &blobs,
            &[("user", "go"), ("assistant", "ok, doing main thing")],
        );
        let b = put_trace(
            &blobs,
            &[("user", "go"), ("assistant", "ok, exploring sibling")],
        );
        let r = merge_trace(&blobs, &a, &b, &x, &StubSummarizer).unwrap();
        let merged = read_trace(&blobs, &r.merged_trace).unwrap();
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[2].role, "system");
        assert!(merged[2].content.contains("merged from sibling"));
        assert!(merged[2].content.contains("exploring sibling"));
    }

    #[test]
    fn merge_trace_is_deterministic_given_same_inputs() {
        let blobs = MemBlobStore::new();
        let x = put_trace(&blobs, &[("user", "go")]);
        let a = put_trace(&blobs, &[("user", "go"), ("assistant", "main")]);
        let b = put_trace(&blobs, &[("user", "go"), ("assistant", "sibling")]);
        let r1 = merge_trace(&blobs, &a, &b, &x, &StubSummarizer).unwrap();
        let r2 = merge_trace(&blobs, &a, &b, &x, &StubSummarizer).unwrap();
        assert_eq!(r1.merged_trace, r2.merged_trace);
    }

    #[test]
    fn merge_trace_handles_no_divergence() {
        let blobs = MemBlobStore::new();
        let same = put_trace(&blobs, &[("user", "go"), ("assistant", "ok")]);
        let r = merge_trace(&blobs, &same, &same, &same, &StubSummarizer).unwrap();
        assert!(r.injected_summary.contains("no divergence"));
    }
}
