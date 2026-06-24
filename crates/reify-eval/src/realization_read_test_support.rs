//! Shared test fixtures for realization-read γ tests.
//!
//! Three helpers that were verbatim-duplicated in `realization_read_gamma` and
//! `realization_content::tests` live here so a future `RealizationNodeData`-
//! field or kernel-injection-seam change updates exactly one place.
//!
//! This module is declared `#[cfg(test)]` in `lib.rs`; all items are
//! `pub(crate)` so sibling test modules can import them.

use std::collections::BTreeMap;

use reify_core::{ContentHash, KernelId, RealizationNodeId};
use reify_ir::{ElementOrderTag, GeometryHandleId, GeometryKernel, ReprKind, VolumeMesh};
use reify_test_support::mocks::MockConstraintChecker;

use crate::Engine;
use crate::graph::{EvaluationGraph, RealizationNodeData};

/// Build an `Engine` with a single geometry kernel injected under `name` (the
/// producing-kernel registry name) and an empty capability registry — the γ
/// projection resolves kernels from `geometry_kernels` keyed by
/// `produced_kernel`, not from the dispatch registry.
pub(crate) fn engine_with_kernel(name: &str, kernel: Box<dyn GeometryKernel>) -> Engine {
    let mut kernels: BTreeMap<String, Box<dyn GeometryKernel>> = BTreeMap::new();
    kernels.insert(name.to_string(), kernel);
    Engine::with_test_kernels_and_registry(
        Box::new(MockConstraintChecker::new()),
        kernels,
        BTreeMap::new(),
        Some(name.to_string()),
    )
}

/// Canonical single-P1-tet [`VolumeMesh`] fixture (matches the content-arm
/// fixture in `realization_content`).
pub(crate) fn make_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
            0.0, 0.0, 1.0, // v3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

/// Seed a kernel-backed realization: insert the `RealizationNodeData` with
/// `produced_kernel` set AND register the engine-side `realization_handles`
/// entry, so the γ projection can resolve `(kernel, handle)`.
pub(crate) fn seed_kernel_realization(
    engine: &mut Engine,
    graph: &mut EvaluationGraph,
    node_id: RealizationNodeId,
    content_hash: ContentHash,
    produced_repr: ReprKind,
    produced_kernel: KernelId,
    handle: GeometryHandleId,
) {
    graph.realizations.insert(
        node_id.clone(),
        RealizationNodeData {
            id: node_id.clone(),
            operations: vec![],
            content_hash,
            produced_repr,
            geometry_cell: None,
            produced_kernel: Some(produced_kernel),
        },
    );
    engine.realization_handles.insert(node_id, handle);
}
