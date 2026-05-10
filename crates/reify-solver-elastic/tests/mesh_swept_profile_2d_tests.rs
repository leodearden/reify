//! Integration tests for [`reify_solver_elastic::mesher::mesh_swept_profile_2d`].
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! These tests round-trip a polygonal `ProfileBoundary` through the
//! orchestrator into a typed `Mesh2dReport`. They gate the libgmsh-
//! dependent assertions at runtime on `reify_kernel_gmsh::GMSH_AVAILABLE`
//! — the orchestrator itself isn't `cfg(has_gmsh)`-aware (and we don't
//! plumb a build.rs cfg through the crate per the task plan), so a stub
//! build simply asserts the `GmshUnavailable` error path.

use reify_kernel_gmsh::GMSH_AVAILABLE;
use reify_solver_elastic::mesher::{
    mesh_swept_profile_2d, Mesh2d, Mesh2dError, Mesh2dOptions, ProfileBoundary, SweepElementTarget,
};

fn unit_square_boundary() -> ProfileBoundary {
    ProfileBoundary {
        outer: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        holes: vec![],
    }
}

/// `SweepElementTarget::WedgeOnly` on a unit square produces a
/// triangle mesh with `recombine_attempted=false` and vacuously
/// `recombine_quality_ok=true`. All vertices land within the unit
/// square; the index buffer is non-empty stride-3.
#[test]
fn mesh_swept_profile_2d_wedge_target_unit_square_returns_triangles() {
    let boundary = unit_square_boundary();
    let mut options = Mesh2dOptions::default();
    // Force determinism so the test result is reproducible across runs.
    options.deterministic = true;
    // Use an explicit mesh size so auto-size derivation doesn't kick in
    // (auto-size for a unit-square outer ring is 1.0, which on its own
    // can yield a single triangle pair — still valid, but the explicit
    // 0.5 keeps the test aligned with the kernel smoke test).
    options.mesh_size = Some(0.5);

    let result = mesh_swept_profile_2d(&boundary, SweepElementTarget::WedgeOnly, &options);

    if !GMSH_AVAILABLE {
        // Stub build: the underlying mesh_plane_2d returns
        // GeometryError::OperationFailed("Gmsh not available"), which the
        // orchestrator maps to Mesh2dError::GmshUnavailable.
        match result {
            Err(Mesh2dError::GmshUnavailable) => {}
            other => panic!(
                "stub build: expected Err(GmshUnavailable), got {other:?}",
            ),
        }
        return;
    }

    let report = result.expect("WedgeOnly mesh_swept_profile_2d failed");

    // (b) recombine_attempted is false for WedgeOnly.
    assert!(
        !report.recombine_attempted,
        "WedgeOnly must NOT attempt recombination",
    );
    // (c) recombine_quality_ok is vacuously true (no quads to check).
    assert!(
        report.recombine_quality_ok,
        "WedgeOnly recombine_quality_ok must be true (vacuous — no quads)",
    );

    // (a, d, e, f) Triangle variant with valid stride-3 indices, even
    // vertex buffer length, and every vertex inside the unit square.
    match report.mesh {
        Mesh2d::Triangle { vertices, indices } => {
            assert!(
                indices.len() > 0,
                "WedgeOnly triangle indices must be non-empty",
            );
            assert_eq!(
                indices.len() % 3,
                0,
                "WedgeOnly triangle indices must be stride-3",
            );
            assert_eq!(
                vertices.len() % 2,
                0,
                "vertices buffer (flat XY) must be even-length",
            );
            let n_verts = vertices.len() / 2;
            for &idx in &indices {
                assert!(
                    (idx as usize) < n_verts,
                    "triangle index {idx} out of bounds (n_verts={n_verts})",
                );
            }
            // Every vertex within the unit-square bounds (allowing a
            // tiny epsilon for gmsh's f64→f32 readback round-trip).
            for chunk in vertices.chunks_exact(2) {
                let (x, y) = (chunk[0], chunk[1]);
                assert!(
                    x >= -1e-5 && x <= 1.0 + 1e-5 && y >= -1e-5 && y <= 1.0 + 1e-5,
                    "vertex ({x}, {y}) outside unit square",
                );
            }
        }
        Mesh2d::Quad { .. } => panic!("WedgeOnly must return Triangle, not Quad"),
    }
}
