//! Tests for the auto mesh-size derivation from the smallest geometric feature.
//!
//! Per the v0.3 FEA PRD, deriving `mesh_size` from the overall geometry
//! tolerance (or bounding-box diagonal) under-resolves thin features by
//! 5–10×. The feature-based heuristic uses the smallest triangle edge in the
//! repaired surface mesh — that is, the surface-mesh-level approximation of
//! the smallest geometric feature dimension. The function honours a caller's
//! explicit `MeshingOptions.mesh_size` override at the dispatcher level; this
//! function returns only the auto-derived default.

use reify_kernel_gmsh::auto_size::{auto_mesh_size_from_features, AutoSizeConfig};
use reify_types::Mesh;

/// With the default multiplier (1.0), the returned size equals the smallest
/// triangle-edge length in the surface mesh.
#[test]
fn auto_mesh_size_from_smallest_edge_with_default_multiplier() {
    // A single triangle with edge lengths 0.5, 1.0, sqrt(1.25) ≈ 1.118.
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            0.5, 0.0, 0.0, // v1 — edge v0-v1 length = 0.5 (the shortest)
            0.0, 1.0, 0.0, // v2 — edge v0-v2 length = 1.0; edge v1-v2 length ≈ 1.118
        ],
        indices: vec![0, 1, 2],
        normals: None,
    };
    let cfg = AutoSizeConfig::default();
    let size = auto_mesh_size_from_features(&mesh, cfg);
    assert!(
        (size - 0.5).abs() < 1e-9,
        "expected size ≈ 0.5 (shortest edge × 1.0 default multiplier); got {}",
        size
    );
}

/// A multiplier of 0.5 halves the auto-derived size — finer mesh, more
/// elements per feature, used when the caller wants tighter resolution
/// without specifying an absolute mesh_size.
#[test]
fn multiplier_scales_size_proportionally() {
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, //
            0.5, 0.0, 0.0, // edge length 0.5
            0.0, 1.0, 0.0, //
        ],
        indices: vec![0, 1, 2],
        normals: None,
    };
    let cfg = AutoSizeConfig {
        feature_multiplier: 0.5,
    };
    let size = auto_mesh_size_from_features(&mesh, cfg);
    assert!(
        (size - 0.25).abs() < 1e-9,
        "expected size ≈ 0.25 (0.5 × 0.5 multiplier); got {}",
        size
    );
}

/// A mesh with a 1m bounding box but a 1mm sliver edge somewhere returns a
/// size derived from the 1mm edge — NOT from the bounding-box diagonal. This
/// is the scenario the PRD calls out: bounding-box-derived defaults
/// under-resolve thin features by 5–10×.
#[test]
fn does_not_use_overall_bounding_box() {
    // Triangle 0: large, edges ~1.0m. Triangle 1: tiny, with a 1mm edge.
    let mesh = Mesh {
        vertices: vec![
            // Triangle 0 — big
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            // Triangle 1 — tiny edge somewhere in the same body
            5.0, 5.0, 5.0,
            5.001, 5.0, 5.0, // 1mm edge with v3 (1e-3)
            5.0, 5.001, 5.0, // 1mm edge with v3
        ],
        indices: vec![0, 1, 2, 3, 4, 5],
        normals: None,
    };
    let cfg = AutoSizeConfig::default();
    let size = auto_mesh_size_from_features(&mesh, cfg);
    // Tolerance: Mesh::vertices is Vec<f32>, so values like 5.001 - 5.0 lose
    // ~1e-7 of absolute precision in the f32 round-trip. 1e-6 absolute is
    // safely above that floor while still 1000× tighter than the 1m bbox
    // diagonal we're proving the heuristic does NOT use.
    assert!(
        (size - 1e-3).abs() < 1e-6,
        "expected size ≈ 1e-3 (the 1mm sliver edge), NOT the 1m bbox diagonal; got {}",
        size
    );
}
