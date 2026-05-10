//! Pin the 2D plane-surface mesher (`mesh_plane_2d`) added by T2987.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! Two parallel test surfaces:
//! - `#[cfg(has_gmsh)]` — real FFI smoke tests asserting the meshed unit
//!   square produces a triangle (or quad-recombined) buffer with the
//!   expected stride and in-bounds indices.
//! - `#[cfg(not(has_gmsh))]` — the stub arm returns
//!   `GeometryError::OperationFailed` with "Gmsh not available" in the
//!   message.
//!
//! These run via `cargo test -p reify-kernel-gmsh --test mesh_plane_2d_tests`
//! in both build modes; the cfg gates pick the right arm.

use reify_kernel_gmsh::mesh_profile_2d::mesh_plane_2d;

/// Triangle path: `recombine=false` on a unit square produces a triangle
/// mesh with a non-empty, stride-3 index buffer, an even-length flat XY
/// vertex buffer, and every index in-bounds.
///
/// `mesh_plane_2d` acquires `init::GMSH_LOCK` internally — tests must NOT
/// hold the lock externally or the inner acquisition would deadlock.
#[cfg(has_gmsh)]
#[test]
fn mesh_plane_2d_triangle_path_unit_square_round_trip() {
    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let holes: Vec<Vec<[f64; 2]>> = vec![];

    let result = mesh_plane_2d(&outer, &holes, Some(0.5), false, true)
        .expect("mesh_plane_2d failed on unit-square triangle path");

    // (c) vertices_xy is a flat XY buffer (stride 2).
    assert!(
        result.vertices_xy.len() % 2 == 0,
        "vertices_xy.len()={} not even (XY pairs expected)",
        result.vertices_xy.len(),
    );
    let n_verts = result.vertices_xy.len() / 2;
    assert!(n_verts > 0, "expected at least one vertex");

    // (a) triangle_indices is non-empty and stride-3.
    assert!(
        !result.triangle_indices.is_empty(),
        "triangle_indices is empty — recombine=false should produce triangles",
    );
    assert_eq!(
        result.triangle_indices.len() % 3,
        0,
        "triangle_indices.len()={} not divisible by 3",
        result.triangle_indices.len(),
    );

    // (b) quad_indices is empty (recombine=false).
    assert!(
        result.quad_indices.is_empty(),
        "quad_indices is non-empty (len={}) despite recombine=false",
        result.quad_indices.len(),
    );

    // (d) every triangle index in-bounds against vertices_xy / 2.
    for (i, &idx) in result.triangle_indices.iter().enumerate() {
        assert!(
            (idx as usize) < n_verts,
            "triangle_indices[{i}]={idx} out of bounds (n_verts={n_verts})",
        );
    }
}

/// Stub-build companion: the cfg(not(has_gmsh)) arm of `mesh_plane_2d`
/// returns `GeometryError::OperationFailed("…Gmsh not available…")`
/// regardless of input — pinning the documented stub-mode behaviour.
#[cfg(not(has_gmsh))]
#[test]
fn mesh_plane_2d_returns_gmsh_not_available_in_stub_build() {
    use reify_types::GeometryError;

    let outer: Vec<[f64; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let holes: Vec<Vec<[f64; 2]>> = vec![];

    let err = mesh_plane_2d(&outer, &holes, Some(0.5), false, true)
        .expect_err("mesh_plane_2d must return Err in stub builds");

    match err {
        GeometryError::OperationFailed(msg) => {
            assert!(
                msg.contains("Gmsh not available"),
                "stub error message must mention 'Gmsh not available', got: {msg}",
            );
        }
        other => panic!("expected OperationFailed, got {other:?}"),
    }
}
