//! Tests for the auto mesh-size derivation from the smallest geometric feature.
//!
//! Per the v0.3 FEA PRD, deriving `mesh_size` from the overall geometry
//! tolerance (or bounding-box diagonal) under-resolves thin features by
//! 5–10×. The feature-based heuristic uses the smallest triangle edge in the
//! repaired surface mesh — that is, the surface-mesh-level approximation of
//! the smallest geometric feature dimension. The function honours a caller's
//! explicit `MeshingOptions.mesh_size` override at the dispatcher level; this
//! function returns only the auto-derived default.

use reify_kernel_gmsh::auto_size::{AutoSizeConfig, AutoSizeError, auto_mesh_size_from_features};
use reify_ir::Mesh;

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
    let size = auto_mesh_size_from_features(&mesh, cfg).unwrap();
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
    let size = auto_mesh_size_from_features(&mesh, cfg).unwrap();
    assert!(
        (size - 0.25).abs() < 1e-9,
        "expected size ≈ 0.25 (0.5 × 0.5 multiplier); got {}",
        size
    );
}

/// An index that references a vertex slot beyond `vertices.len()/3` must
/// produce `Err(AutoSizeError::IndexOutOfBounds { .. })` rather than panicking
/// with an out-of-bounds slice access.
#[test]
fn out_of_bounds_index_returns_err() {
    // 3 vertices (9 floats) → n_vertices = 3. Index 99 is out of range.
    let mesh = reify_ir::Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
        ],
        indices: vec![0, 1, 99],
        normals: None,
    };
    let cfg = AutoSizeConfig::default();
    match auto_mesh_size_from_features(&mesh, cfg) {
        Err(AutoSizeError::IndexOutOfBounds { index, n_vertices }) => {
            assert_eq!(index, 99, "reported index must be 99");
            assert_eq!(n_vertices, 3, "reported n_vertices must be 3");
        }
        Ok(v) => panic!(
            "expected Err(AutoSizeError::IndexOutOfBounds) for index 99 \
             in a 3-vertex mesh, but got Ok({v})"
        ),
    }
}

/// When `mesh.indices` is empty (no triangles → no edges → no minimum), the
/// function must return `Ok(0.0)` per the early-return guard at the top of
/// `auto_mesh_size_from_features` that fires when `mesh.indices` is empty.
/// Callers treat a zero return as "auto-size unavailable" and fall back to a
/// configured default.
#[test]
fn empty_indices_returns_zero_fallback() {
    // Empty vertices is fine: the early-return fires on mesh.indices.is_empty()
    // before any vertex-range or edge-length logic runs.
    let mesh = Mesh {
        vertices: vec![],
        indices: vec![],
        normals: None,
    };
    let cfg = AutoSizeConfig::default();
    let result = auto_mesh_size_from_features(&mesh, cfg);
    let size = result.expect(
        "empty-indices mesh must return Ok(0.0), not Err — \
         the early-return short-circuit fires before validation or iteration",
    );
    assert_eq!(
        size, 0.0,
        "empty indices must yield exactly 0.0 fallback per documented contract; \
         callers treat 0.0 as 'auto-size unavailable' and fall back to a configured default"
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
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            // Triangle 1 — tiny edge somewhere in the same body
            5.0, 5.0, 5.0, 5.001, 5.0, 5.0, // 1mm edge with v3 (1e-3)
            5.0, 5.001, 5.0, // 1mm edge with v3
        ],
        indices: vec![0, 1, 2, 3, 4, 5],
        normals: None,
    };
    let cfg = AutoSizeConfig::default();
    let size = auto_mesh_size_from_features(&mesh, cfg).unwrap();
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
