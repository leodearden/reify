//! Integration tests for the `mesh_volume` pipeline wrapper and its helpers.
//!
//! This file tests both the pure-Rust helpers (no `cfg(has_gmsh)` gate) and
//! the FFI-backed orchestrating wrapper (`#[cfg(has_gmsh)]` module).
//!
//! Pure-Rust helper tests run on every host — no libgmsh required.
//! Integration tests that call `mesh_surface_to_volume_with_diagnostics` are
//! inside the `with_libgmsh` module, gated with `#[cfg(has_gmsh)]`.

use reify_kernel_gmsh::mesh_volume::{apply_repair_if_requested, MeshSurfaceToVolumeReport};
use reify_kernel_gmsh::repair::RepairConfig;
use reify_types::Mesh;

// ---------------------------------------------------------------------------
// Helpers shared across multiple tests in this file
// ---------------------------------------------------------------------------

/// A sliver-laden mesh: one good equilateral-ish triangle + one sliver
/// triangle (near-collinear vertices). Mirrors the mesh constructed in
/// `repair_tests.rs::sliver_triangles_below_area_threshold_are_collapsed`.
fn sliver_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            // Triangle 0 corners (equilateral-ish, area ~0.43)
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.5, 0.866, 0.0, // v2
            // Triangle 1 corners (collinear-ish, area ~5e-9 — sliver)
            5.0, 0.0, 0.0, // v3
            6.0, 0.0, 0.0, // v4
            5.5, 1e-8, 0.0, // v5
        ],
        indices: vec![
            0, 1, 2, // good triangle
            3, 4, 5, // sliver
        ],
        normals: None,
    }
}

// ---------------------------------------------------------------------------
// apply_repair_if_requested — None passes through, Some delegates
// ---------------------------------------------------------------------------

/// `apply_repair_if_requested` with `None` must return the input unchanged:
/// identical vertices and indices (Cow::Borrowed semantics — no copy, no repair).
#[test]
fn apply_repair_if_requested_none_passes_input_through() {
    let mesh = sliver_mesh();
    let result = apply_repair_if_requested(&mesh, None);
    assert_eq!(
        result.vertices, mesh.vertices,
        "None must pass vertices through unchanged"
    );
    assert_eq!(
        result.indices, mesh.indices,
        "None must pass indices through unchanged"
    );
}

/// `apply_repair_if_requested` with `Some(cfg)` must delegate to
/// `repair_surface_mesh` and return the repaired mesh (fewer indices because
/// the sliver triangle was dropped).
#[test]
fn apply_repair_if_requested_some_delegates_to_repair_surface_mesh() {
    let mesh = sliver_mesh();
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-6,
        vertex_merge_epsilon: 1e-9,
    };
    let result = apply_repair_if_requested(&mesh, Some(cfg));
    assert!(
        result.indices.len() < mesh.indices.len(),
        "Some(cfg) must invoke repair and drop the sliver triangle; \
         before: {} indices, after: {} indices",
        mesh.indices.len(),
        result.indices.len()
    );
    // The sliver is dropped — only one triangle (3 indices) survives.
    assert_eq!(
        result.indices.len(),
        3,
        "exactly one triangle should survive; got {} indices",
        result.indices.len()
    );
}
