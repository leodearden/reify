//! Pin the surface-mesh-I/O FFI surface against libgmsh 4.15.2.
//!
//! All tests in this binary route through [`init::ensure_initialized`]
//! (the `OnceLock<()>`-guarded init path that production code uses). The
//! direct-`ffi::initialize`/`ffi::finalize` lifecycle test lives in its
//! own binary at `tests/ffi_lifecycle_test.rs` so it cannot share
//! process state with these tests; mixing the two paths in one binary
//! would couple test ordering to gmsh's process-wide initialisation
//! semantics (see that file's module doc for the failure modes).
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds (no `/opt/reify-deps`) the file is empty and the test binary
//! contains zero tests — preserving the all-OK posture of `cargo test
//! -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::ffi;
use reify_kernel_gmsh::init;

/// Round-trip a single triangle through the gmsh model API: add a discrete
/// surface entity, push 3 nodes + 1 triangle into it, then read them back
/// and assert the values match.
///
/// Pins the surface-mesh I/O FFI surface that `mesh_to_volume` builds on:
/// `gmshModelAdd`, `gmshModelAddDiscreteEntity`, `gmshModelMeshAddNodes`,
/// `gmshModelMeshAddElements`, `gmshModelMeshGetNodes`,
/// `gmshModelMeshGetElementsByType`. A regression on any of these would
/// surface here as a coordinate / tag mismatch rather than as a confusing
/// 3D-meshing failure inside `mesh_to_volume`.
#[test]
fn gmsh_add_and_read_mesh_nodes_and_triangles_round_trip() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    init::ensure_initialized();
    ffi::clear().expect("ffi::clear failed");

    ffi::model_add("rt").expect("ffi::model_add failed");
    let surf_tag =
        ffi::add_discrete_entity(2, &[]).expect("ffi::add_discrete_entity(dim=2) failed");

    let in_node_tags: [u64; 3] = [1, 2, 3];
    let in_coords: [f64; 9] = [
        0.0, 0.0, 0.0, // node 1
        1.0, 0.0, 0.0, // node 2
        0.0, 1.0, 0.0, // node 3
    ];
    ffi::add_nodes_2d(surf_tag, &in_node_tags, &in_coords).expect("ffi::add_nodes_2d failed");

    let in_tri_tags: [u64; 1] = [1];
    let in_tri_node_tags: [u64; 3] = [1, 2, 3];
    ffi::add_elements_2d(
        surf_tag,
        2, // gmsh element-type 2 = 3-node triangle
        &in_tri_tags,
        &in_tri_node_tags,
    )
    .expect("ffi::add_elements_2d failed");

    let (out_node_tags, out_coords) = ffi::get_nodes_all().expect("ffi::get_nodes_all failed");
    assert_eq!(
        out_node_tags.len(),
        3,
        "expected 3 node tags after add_nodes_2d, got {}",
        out_node_tags.len(),
    );
    assert_eq!(
        out_coords.len(),
        9,
        "expected 9 coords (3 nodes × 3) after add_nodes_2d, got {}",
        out_coords.len(),
    );
    // Build a sorted (tag → coords) mapping so the assertion is index-order
    // independent (gmsh does not promise to return tags in insertion order).
    let mut paired: Vec<(u64, [f64; 3])> = out_node_tags
        .iter()
        .copied()
        .zip(out_coords.chunks_exact(3))
        .map(|(t, c)| (t, [c[0], c[1], c[2]]))
        .collect();
    paired.sort_by_key(|(t, _)| *t);
    for (i, (tag, coord)) in paired.iter().enumerate() {
        assert_eq!(*tag, in_node_tags[i], "node tag mismatch at slot {i}");
        let expected = [in_coords[3 * i], in_coords[3 * i + 1], in_coords[3 * i + 2]];
        for k in 0..3 {
            assert!(
                (coord[k] - expected[k]).abs() < 1e-9,
                "coord mismatch at node tag {tag} component {k}: got {} expected {}",
                coord[k],
                expected[k],
            );
        }
    }

    let (out_elem_tags, out_elem_node_tags) =
        ffi::get_elements_by_type(2).expect("ffi::get_elements_by_type(2) failed");
    assert_eq!(
        out_elem_tags.len(),
        1,
        "expected 1 triangle tag after add_elements_2d, got {}",
        out_elem_tags.len(),
    );
    assert_eq!(
        out_elem_node_tags.len(),
        3,
        "expected 3 node tags (1 triangle × 3 nodes) after add_elements_2d, got {}",
        out_elem_node_tags.len(),
    );
    assert_eq!(
        out_elem_node_tags.as_slice(),
        &in_tri_node_tags[..],
        "triangle node tags must round-trip exactly (gmsh preserves connectivity order)",
    );

    ffi::clear().expect("ffi::clear failed (cleanup)");
}

/// Smoke test the five new built-in-CAD FFI bindings added for the 2D
/// profile-mesher pipeline (T2987, PRD docs/prds/v0_3/hex-wedge-meshing.md
/// task #6): `geo_add_point`, `geo_add_line`, `geo_add_curve_loop`,
/// `geo_add_plane_surface`, `mesh_set_recombine`.
///
/// Builds a unit-square plane surface from 4 points / 4 lines / 1 loop / 1
/// surface and asserts each call returns a positive Gmsh tag (or
/// `Ok(())` for the void-returning `mesh_set_recombine`). A regression on
/// any of these would surface here as a non-positive tag or an error from
/// the wrapper rather than as a confusing 2D-meshing failure inside
/// `mesh_plane_2d`.
#[test]
fn geo_add_point_line_curve_loop_plane_surface_and_set_recombine_round_trip() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    init::ensure_initialized();
    ffi::clear().expect("ffi::clear failed");

    ffi::model_add("smoke_2987").expect("ffi::model_add failed");

    // (a) geo_add_point — two distinct points with positive tags.
    let p1 = ffi::geo_add_point(0.0, 0.0, 0.0, 0.0).expect("ffi::geo_add_point(0,0,0) failed");
    let p2 = ffi::geo_add_point(1.0, 0.0, 0.0, 0.0).expect("ffi::geo_add_point(1,0,0) failed");
    assert!(p1 > 0, "geo_add_point returned non-positive tag {p1}");
    assert!(p2 > 0, "geo_add_point returned non-positive tag {p2}");
    assert_ne!(p1, p2, "geo_add_point returned the same tag twice: {p1}");

    let p3 = ffi::geo_add_point(1.0, 1.0, 0.0, 0.0).expect("ffi::geo_add_point(1,1,0) failed");
    let p4 = ffi::geo_add_point(0.0, 1.0, 0.0, 0.0).expect("ffi::geo_add_point(0,1,0) failed");

    // (b) geo_add_line — four lines forming a unit-square loop.
    let l1 = ffi::geo_add_line(p1, p2).expect("ffi::geo_add_line(p1,p2) failed");
    let l2 = ffi::geo_add_line(p2, p3).expect("ffi::geo_add_line(p2,p3) failed");
    let l3 = ffi::geo_add_line(p3, p4).expect("ffi::geo_add_line(p3,p4) failed");
    let l4 = ffi::geo_add_line(p4, p1).expect("ffi::geo_add_line(p4,p1) failed");
    assert!(
        l1 > 0 && l2 > 0 && l3 > 0 && l4 > 0,
        "non-positive line tag(s)"
    );

    // (c) geo_add_curve_loop — one closed loop from the four lines.
    let loop_tag =
        ffi::geo_add_curve_loop(&[l1, l2, l3, l4]).expect("ffi::geo_add_curve_loop failed");
    assert!(
        loop_tag > 0,
        "geo_add_curve_loop returned non-positive tag {loop_tag}"
    );

    // (d) geo_add_plane_surface — plane surface bounded by the loop.
    let surf_tag =
        ffi::geo_add_plane_surface(&[loop_tag]).expect("ffi::geo_add_plane_surface failed");
    assert!(
        surf_tag > 0,
        "geo_add_plane_surface returned non-positive tag {surf_tag}"
    );

    // Synchronise the built-in CAD into the gmsh model so the surface
    // becomes a real model entity. Without this, the next call hits
    // "Surface N does not exist" — `gmshModelMeshSetRecombine` resolves
    // its (dim, tag) against the synchronised model, not the built-in CAD.
    ffi::geo_synchronize().expect("ffi::geo_synchronize failed");

    // (e) mesh_set_recombine — scopes recombination to this surface. The
    // 45.0 angle is the per-corner deviation tolerance Gmsh uses to decide
    // whether two triangles can be merged into a quad.
    ffi::mesh_set_recombine(2, surf_tag, 45.0).expect("ffi::mesh_set_recombine failed");

    ffi::clear().expect("ffi::clear failed (cleanup)");
}
