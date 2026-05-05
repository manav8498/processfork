// SPDX-License-Identifier: MIT
//! Shared fixtures for the Criterion benches.

#![allow(missing_docs)]

use std::sync::Arc;

use pf_core::fixture::{
    FixtureCacheCapture, FixtureEffectsCapture, FixtureModelCapture, FixtureSpec,
    FixtureTraceCapture, FixtureWorldCapture,
};
use pf_core::manifest::AgentInfo;
use pf_core::snapshot::Snapshotter;

pub fn make_snapshotter(spec: FixtureSpec) -> Snapshotter {
    let agent = AgentInfo {
        kind: "bench".into(),
        version: "0".into(),
        fingerprint: format!("seed-{}", spec.seed),
    };
    Snapshotter::new(
        agent,
        Arc::new(FixtureModelCapture(spec.clone())),
        Arc::new(FixtureCacheCapture(spec.clone())),
        Arc::new(FixtureWorldCapture(spec.clone())),
        Arc::new(FixtureEffectsCapture(spec.clone())),
        Arc::new(FixtureTraceCapture(spec)),
    )
}
