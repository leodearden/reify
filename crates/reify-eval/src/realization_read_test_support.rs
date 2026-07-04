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
use reify_ir::{ElementOrderTag, GeometryHandleId, GeometryKernel, ReprKind, VolumeConnectivity, VolumeMesh};
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
        connectivity: VolumeConnectivity::Tet {
            indices: vec![0, 1, 2, 3],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// Canonical single-P1-hex (task 4986, C-2) [`VolumeMesh`] fixture: a unit
/// cube in canonical Hughes/Gmsh hex8 order — bottom face (z=0) CCW, then
/// top face (z=1) in the same cyclic order (matches
/// `reify_solver_elastic::assembly::hex::element_stiffness_hex_p1`'s
/// documented node ordering).
pub(crate) fn make_hex_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            1.0, 1.0, 0.0, // v2
            0.0, 1.0, 0.0, // v3
            0.0, 0.0, 1.0, // v4
            1.0, 0.0, 1.0, // v5
            1.0, 1.0, 1.0, // v6
            0.0, 1.0, 1.0, // v7
        ],
        connectivity: VolumeConnectivity::Hex {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
        normals: None,
        boundary: None,
    }
}

/// Canonical single-P1-wedge (task 4986, C-2) [`VolumeMesh`] fixture: a unit
/// triangular prism in canonical Gmsh PRI6 order — bottom triangle (z=0) in
/// barycentric order, then top triangle (z=1) in the same cyclic order
/// (matches
/// `reify_solver_elastic::assembly::wedge::element_stiffness_wedge_p1`'s
/// documented node ordering).
pub(crate) fn make_wedge_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
            0.0, 0.0, 1.0, // v3
            1.0, 0.0, 1.0, // v4
            0.0, 1.0, 1.0, // v5
        ],
        connectivity: VolumeConnectivity::Wedge {
            indices: vec![0, 1, 2, 3, 4, 5],
        },
        normals: None,
        boundary: None,
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
            input_cone_hash: None,
        },
    );
    engine.realization_handles.insert(node_id, handle);
}
