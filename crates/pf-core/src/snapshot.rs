// SPDX-License-Identifier: MIT
//! Atomic four-layer snapshot orchestration.
//!
//! [`Snapshotter`] runs the four [`LayerCapture`] implementations
//! concurrently, collects their layer descriptors, assembles a
//! [`Manifest`](crate::manifest::Manifest), persists it via [`PfStore`], and
//! returns the resulting content-id. The whole orchestration is wall-clock
//! ≤500 ms for the synthetic 4-layer fixture (Phase-1 acceptance gate).

use crate::digest::Digest256;
use crate::error::Result;
use crate::manifest::{
    AgentInfo, CacheLayer, EffectsLayer, MEDIATYPE_V1, Manifest, ModelLayer, TraceLayer, WorldLayer,
};
use crate::store::PfStore;

use chrono::Utc;
use std::sync::Arc;
use std::thread;

/// One layer's contribution to a snapshot. Implementations capture their live
/// state, write blobs into the supplied store, and return a typed descriptor
/// suitable for [`Manifest`] embedding.
pub trait LayerCapture: Send + Sync {
    /// Strongly-typed descriptor enum naming the four layer kinds.
    fn kind(&self) -> LayerKind;
    /// Run the capture, returning a layer descriptor.
    fn capture(&self, store: &PfStore) -> Result<LayerDescriptor>;
}

/// Discriminator for the four layer kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// Model weights / adapters.
    Model,
    /// Paged KV cache.
    Cache,
    /// Filesystem / env / processes / DOM.
    World,
    /// Effect ledger.
    Effects,
    /// Reasoning trace (chat + tool messages).
    Trace,
}

/// Heterogeneous capture-result shape — one variant per layer kind.
#[derive(Clone, Debug)]
pub enum LayerDescriptor {
    /// Model layer descriptor.
    Model(ModelLayer),
    /// Cache layer descriptor.
    Cache(CacheLayer),
    /// World layer descriptor.
    World(WorldLayer),
    /// Effects layer descriptor.
    Effects(EffectsLayer),
    /// Trace layer descriptor.
    Trace(TraceLayer),
}

/// Atomic four-layer snapshot orchestrator.
///
/// Constructed with one [`LayerCapture`] per layer (model, cache, world,
/// effects, trace). Captures run concurrently on stdlib threads (not tokio:
/// each capture is CPU-bound by hashing/compression, not I/O-bound; threads
/// outperform a runtime).
///
/// ```no_run
/// use std::sync::Arc;
/// use pf_core::{
///     snapshot::{Snapshotter, LayerCapture, LayerDescriptor, LayerKind},
///     store::PfStore,
///     manifest::AgentInfo,
/// };
/// # struct StubModel; impl LayerCapture for StubModel {
/// #     fn kind(&self) -> LayerKind { LayerKind::Model }
/// #     fn capture(&self, _: &PfStore) -> pf_core::Result<LayerDescriptor> { unimplemented!() }
/// # }
/// # let store = PfStore::open("/tmp/_pf_doctest").unwrap();
/// # let model = Arc::new(StubModel);
/// # let cache = Arc::new(StubModel);
/// # let world = Arc::new(StubModel);
/// # let effects = Arc::new(StubModel);
/// # let trace = Arc::new(StubModel);
/// let agent = AgentInfo {
///     kind: "demo".into(), version: "0".into(), fingerprint: "abc".into(),
/// };
/// let snapper = Snapshotter::new(agent, model, cache, world, effects, trace);
/// // let cid = snapper.snapshot(&store, vec![]).unwrap();
/// ```
pub struct Snapshotter {
    agent: AgentInfo,
    model: Arc<dyn LayerCapture>,
    cache: Arc<dyn LayerCapture>,
    world: Arc<dyn LayerCapture>,
    effects: Arc<dyn LayerCapture>,
    trace: Arc<dyn LayerCapture>,
}

impl Snapshotter {
    /// Construct a snapshotter with one capture per layer.
    pub fn new(
        agent: AgentInfo,
        model: Arc<dyn LayerCapture>,
        cache: Arc<dyn LayerCapture>,
        world: Arc<dyn LayerCapture>,
        effects: Arc<dyn LayerCapture>,
        trace: Arc<dyn LayerCapture>,
    ) -> Self {
        Self {
            agent,
            model,
            cache,
            world,
            effects,
            trace,
        }
    }

    /// Run all five captures concurrently, assemble the manifest, persist it
    /// via `store`, and return its content-id. `parents` becomes the
    /// manifest's `parents` field (use `vec![]` for a root snapshot).
    pub fn snapshot(&self, store: &PfStore, parents: Vec<Digest256>) -> Result<Digest256> {
        // Spawn one OS thread per layer. We own the captures behind Arcs,
        // and `PfStore` is `Send + Sync` because its inner `BlobStore` is.
        // We re-create per-thread `&PfStore` borrows via raw pointer scopes
        // through `thread::scope`.
        let descriptors: Result<Vec<LayerDescriptor>> = thread::scope(|scope| {
            let model_cap = self.model.clone();
            let cache_cap = self.cache.clone();
            let world_cap = self.world.clone();
            let effects_cap = self.effects.clone();
            let trace_cap = self.trace.clone();

            let h_model = scope.spawn(move || model_cap.capture(store));
            let h_cache = scope.spawn(move || cache_cap.capture(store));
            let h_world = scope.spawn(move || world_cap.capture(store));
            let h_effects = scope.spawn(move || effects_cap.capture(store));
            let h_trace = scope.spawn(move || trace_cap.capture(store));

            let mut out = Vec::with_capacity(5);
            for handle in [h_model, h_cache, h_world, h_effects, h_trace] {
                out.push(handle.join().expect("capture thread panicked")?);
            }
            Ok(out)
        });
        let descriptors = descriptors?;

        // Stitch descriptors into the typed manifest layers. Order of
        // descriptors matches the spawn order above.
        let mut model = None;
        let mut cache = None;
        let mut world = None;
        let mut effects = None;
        let mut trace = None;
        for d in descriptors {
            match d {
                LayerDescriptor::Model(x) => model = Some(x),
                LayerDescriptor::Cache(x) => cache = Some(x),
                LayerDescriptor::World(x) => world = Some(x),
                LayerDescriptor::Effects(x) => effects = Some(x),
                LayerDescriptor::Trace(x) => trace = Some(x),
            }
        }

        let manifest = Manifest {
            schema_version: 1,
            media_type: MEDIATYPE_V1.to_owned(),
            agent: self.agent.clone(),
            model: model.expect("model capture must produce ModelLayer"),
            cache: cache.expect("cache capture must produce CacheLayer"),
            world: world.expect("world capture must produce WorldLayer"),
            effects: effects.expect("effects capture must produce EffectsLayer"),
            trace: trace.expect("trace capture must produce TraceLayer"),
            created_at: Utc::now(),
            parents,
        };

        store.put_manifest(&manifest)
    }
}
