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
    ffi::add_nodes_2d(surf_tag, &in_node_tags, &in_coords)
        .expect("ffi::add_nodes_2d failed");

    let in_tri_tags: [u64; 1] = [1];
    let in_tri_node_tags: [u64; 3] = [1, 2, 3];
    ffi::add_elements_2d(
        surf_tag,
        2, // gmsh element-type 2 = 3-node triangle
        &in_tri_tags,
        &in_tri_node_tags,
    )
    .expect("ffi::add_elements_2d failed");

    let (out_node_tags, out_coords) =
        ffi::get_nodes_all().expect("ffi::get_nodes_all failed");
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
        let expected = [
            in_coords[3 * i],
            in_coords[3 * i + 1],
            in_coords[3 * i + 2],
        ];
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
