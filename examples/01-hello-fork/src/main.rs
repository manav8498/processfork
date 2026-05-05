// SPDX-License-Identifier: MIT
//! examples/01-hello-fork — the canonical ProcessFork "hello world".
//!
//! Captures a synthetic four-layer snapshot, prints the resulting content-id
//! and timing, then captures the SAME fixture a second time to demonstrate
//! that the CAS deduplicates: storage growth between snapshots is bounded
//! by manifest JSON size, not by total payload size.
//!
//! Run with `cargo run -p example-01-hello-fork`.
//! Phase-1 acceptance gate: this example exits 0 with `snapshot took … ms`
//! < 500 ms on the build host.

use std::sync::Arc;
use std::time::Instant;

use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;
use pf_core::store::PfStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("┌─ ProcessFork example 01: hello-fork ─────────────────────");

    let scratch = tempfile::tempdir()?;
    let store = PfStore::open(scratch.path())?;
    println!("│ store    : {}", scratch.path().display());

    let spec = FixtureSpec::default();
    println!(
        "│ fixture  : {} cache pages × {} B  + {} fs files × {} B  + {} B model diff",
        spec.cache_pages,
        spec.page_bytes,
        spec.world_files,
        spec.world_file_bytes,
        spec.model_diff_bytes,
    );

    let agent = AgentInfo {
        kind: "synthetic".into(),
        version: "0.1.0".into(),
        fingerprint: "examples/01-hello-fork".into(),
    };
    let snapper = Snapshotter::new(
        agent,
        Arc::new(FixtureModelCapture(spec.clone())),
        Arc::new(FixtureCacheCapture(spec.clone())),
        Arc::new(FixtureWorldCapture(spec.clone())),
        Arc::new(FixtureEffectsCapture(spec.clone())),
        Arc::new(FixtureTraceCapture(spec.clone())),
    );

    // First snapshot — warms the zstd dictionary lazy-init.
    let _ = snapper.snapshot(&store, vec![])?;
    let bytes_after_first = store.blobs().physical_bytes()?;

    // Measured snapshot.
    let t0 = Instant::now();
    let cid = snapper.snapshot(&store, vec![])?;
    let elapsed = t0.elapsed();
    let bytes_after_second = store.blobs().physical_bytes()?;

    println!("├──────────────────────────────────────────────────────────");
    println!("│ snapshot : {cid}");
    println!("│ took     : {} ms", elapsed.as_millis());
    println!(
        "│ store    : {} B → {} B  (Δ {} B from CoW dedup of identical fixture)",
        bytes_after_first,
        bytes_after_second,
        bytes_after_second - bytes_after_first,
    );

    // Verify the manifest can be rehydrated.
    let m = store.get_manifest(&cid)?;
    println!(
        "│ manifest : agent={}@{}  schema=v{}  trace={}",
        m.agent.kind, m.agent.version, m.schema_version, m.trace.messages,
    );

    // Phase-1 budget assertion (mirrors the integration test).
    assert!(
        elapsed.as_millis() < 500,
        "Phase-1 budget breach: snapshot took {} ms (≤500 ms required)",
        elapsed.as_millis()
    );
    println!("└─ ✓ within Phase-1 budget (500 ms)");
    Ok(())
}
