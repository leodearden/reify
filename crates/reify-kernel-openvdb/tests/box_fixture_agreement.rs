//! Agreement between this crate's box fixture and the workspace-canonical one.
//!
//! `test_fixtures::axis_aligned_box` keeps its own corner literal, because the
//! module is reachable from the crate's plain library artifact (the
//! `test-fixtures` feature path) where `reify-test-support` is not linkable.
//! Its triangles come from the same index block as
//! `reify_test_support::mesh_fixtures::prismatic_box_mesh`, and that block is
//! outward-wound only under one corner order. Comparing the two builders at the
//! origin pins this crate's corner order to that contract.
//!
//! Pure fixture data, so no `cfg(has_openvdb)` gate: it runs whether or not the
//! OpenVDB library was found.

use reify_kernel_openvdb::test_fixtures::{axis_aligned_box, box_mesh, vertex_and_triangle_arrays};
use reify_test_support::mesh_fixtures::prismatic_box_mesh;

/// Every extent is exactly representable in f32, so the two builders must
/// agree bit for bit.
#[test]
fn axis_aligned_box_at_the_origin_is_the_canonical_prismatic_box() {
    for [lx, ly, lz] in [[1.0, 1.0, 1.0], [2.0, 0.5, 4.0]] {
        let local = axis_aligned_box([0.0; 3], [lx, ly, lz]);
        let canonical = prismatic_box_mesh(lx, ly, lz);

        let bits = |vertices: &[f32]| vertices.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(
            bits(&local.vertices),
            bits(&canonical.vertices),
            "vertices of a {lx} x {ly} x {lz} box"
        );
        assert_eq!(
            local.indices, canonical.indices,
            "indices of a {lx} x {ly} x {lz} box"
        );
        assert!(local.normals.is_none() && canonical.normals.is_none());
    }
}

#[test]
fn vertex_and_triangle_arrays_splits_the_flat_buffers() {
    let mesh = box_mesh(1.0, 1.0, 1.0);

    let (vertices, triangles) = vertex_and_triangle_arrays(&mesh);

    assert_eq!(vertices.len(), 8);
    assert_eq!(triangles.len(), 12);
    assert_eq!(vertices.concat(), mesh.vertices);
    assert_eq!(triangles.concat(), mesh.indices);
}
