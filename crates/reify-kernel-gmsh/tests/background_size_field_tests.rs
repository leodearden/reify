//! Integration tests for [`reify_kernel_gmsh::BackgroundSizeField`] — the
//! serialiser that carries a per-volume-vertex size field into gmsh as a
//! `"SS"` (scalar-on-tetrahedron) post-view buffer.
//!
//! Deliberately NOT gated on `cfg(has_gmsh)`. The type is plain data with no
//! FFI of its own, and both arms of `refine_volume_with_size_field` take it as
//! a parameter, so it must compile and be testable in stub builds too.
//!
//! The layout assertion below is the load-bearing one: `gmshViewAddListData`
//! returns `ierr = 0` for a wrongly-grouped buffer and produces a
//! plausible-but-wrong mesh, so a byte-exact check here is the only thing that
//! distinguishes the two.

use reify_ir::{ElementOrderTag, GeometryError, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::BackgroundSizeField;

/// Doubles per `"SS"` element: 4 x-coords, 4 y, 4 z, 4 values.
const SS_STRIDE: usize = 16;

fn tet_vm(vertices: Vec<f32>, indices: Vec<u32>, order: ElementOrderTag) -> VolumeMesh {
    VolumeMesh {
        vertices,
        connectivity: VolumeConnectivity::Tet { indices, order },
        normals: None,
        boundary: None,
    }
}

/// Single P1 tet with corners chosen so every coordinate is distinct and
/// recognisable in a flat buffer dump.
fn one_p1_tet_vm() -> VolumeMesh {
    tet_vm(
        vec![
            0.0_f32, 0.0, 0.0, // 0
            2.0, 0.0, 0.0, // 1
            0.0, 3.0, 0.0, // 2
            0.0, 0.0, 5.0, // 3
        ],
        vec![0, 1, 2, 3],
        ElementOrderTag::P1,
    )
}

/// Coarse 6-tet (Kuhn) decomposition of the unit cube over its 8 corners —
/// the same decomposition `reify-solver-elastic`'s suites seed from.
fn kuhn_6tet_unit_cube_vm() -> VolumeMesh {
    tet_vm(
        vec![
            0.0_f32, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ],
        vec![
            0, 1, 2, 6, //
            0, 1, 5, 6, //
            0, 3, 2, 6, //
            0, 3, 7, 6, //
            0, 4, 5, 6, //
            0, 4, 7, 6, //
        ],
        ElementOrderTag::P1,
    )
}

fn expect_err(result: Result<BackgroundSizeField, GeometryError>, what: &str) -> String {
    match result {
        Ok(_) => panic!("{what}: expected an error, got Ok"),
        Err(GeometryError::OperationFailed(msg)) => msg,
        Err(other) => panic!("{what}: expected OperationFailed, got {other:?}"),
    }
}

// --- (a) shape -------------------------------------------------------------

#[test]
fn from_tet_mesh_emits_sixteen_doubles_per_tet() {
    let vm = kuhn_6tet_unit_cube_vm();
    let n_tets = vm.tet_indices().expect("tet mesh").len() / 4;
    assert_eq!(n_tets, 6, "fixture sanity");

    let sizes = vec![0.25_f64; 8];
    let field = BackgroundSizeField::from_tet_mesh(&vm, &sizes).expect("valid field");

    assert_eq!(field.element_count(), n_tets);
    assert_eq!(field.list_data().len(), SS_STRIDE * n_tets);
}

// --- (b) layout ------------------------------------------------------------

/// The silent-scramble guard. Per gmshc.h:3200-3204 one `"SS"` element is
/// `[x0,x1,x2,x3, y0,y1,y2,y3, z0,z1,z2,z3, v0,v1,v2,v3]` — grouped by AXIS.
/// The ASCII `.pos` "parsed" format groups per POINT instead; feeding gmsh
/// that grouping scrambles the field geometry and still returns `ierr = 0`.
#[test]
fn list_data_groups_coordinates_by_axis_not_by_point() {
    let vm = one_p1_tet_vm();
    let sizes = vec![0.1_f64, 0.2, 0.3, 0.4];

    let field = BackgroundSizeField::from_tet_mesh(&vm, &sizes).expect("valid field");

    #[rustfmt::skip]
    let expected: Vec<f64> = vec![
        0.0, 2.0, 0.0, 0.0, // x0..x3
        0.0, 0.0, 3.0, 0.0, // y0..y3
        0.0, 0.0, 0.0, 5.0, // z0..z3
        0.1, 0.2, 0.3, 0.4, // v0..v3
    ];
    assert_eq!(
        field.list_data(),
        expected.as_slice(),
        "\"SS\" element must be grouped by axis, not by point"
    );
}

// --- (c) P2 stride ---------------------------------------------------------

/// gmsh's `"SS"` primitive is a LINEAR tet: a P2 element contributes exactly
/// one 16-double record built from its first four (corner) node indices. The
/// six edge midpoints carry no sizing information the corners do not.
///
/// The failure this pins is dividing the index buffer by 4 instead of by the
/// element stride, which would read 10 P2 indices as 2.5 "tets".
#[test]
fn p2_tet_emits_one_element_from_its_first_four_nodes() {
    let vm = tet_vm(
        vec![
            // 4 corners
            0.0_f32, 0.0, 0.0, //
            2.0, 0.0, 0.0, //
            0.0, 3.0, 0.0, //
            0.0, 0.0, 5.0, //
            // 6 edge midpoints
            1.0, 0.0, 0.0, //
            1.0, 1.5, 0.0, //
            0.0, 1.5, 0.0, //
            0.0, 0.0, 2.5, //
            1.0, 0.0, 2.5, //
            0.0, 1.5, 2.5, //
        ],
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        ElementOrderTag::P2,
    );
    assert_eq!(vm.nodes_per_element(), 10, "fixture sanity");

    // Midpoint sizes are deliberately the smallest values in the slice: if the
    // implementation read all ten nodes they would show up in the buffer.
    let sizes = vec![0.1_f64, 0.2, 0.3, 0.4, 0.01, 0.01, 0.01, 0.01, 0.01, 0.01];

    let field = BackgroundSizeField::from_tet_mesh(&vm, &sizes).expect("valid field");

    assert_eq!(field.element_count(), 1, "one P2 tet is one \"SS\" element");
    #[rustfmt::skip]
    let expected: Vec<f64> = vec![
        0.0, 2.0, 0.0, 0.0,
        0.0, 0.0, 3.0, 0.0,
        0.0, 0.0, 0.0, 5.0,
        0.1, 0.2, 0.3, 0.4,
    ];
    assert_eq!(field.list_data(), expected.as_slice());
}

// --- (d) orphaned vertices -------------------------------------------------

/// `project_per_element_sizes_to_vertices` leaves `f64::INFINITY` at vertices
/// no element references. Because the buffer is built by walking TETS, such a
/// vertex is structurally unreachable — no filter is needed and none can be
/// forgotten. Assert on the EMITTED values, never on the input slice.
#[test]
fn vertices_no_tet_references_cannot_reach_the_buffer() {
    let mut vm = one_p1_tet_vm();
    // A fifth vertex that no element indexes.
    vm.vertices.extend_from_slice(&[9.0_f32, 9.0, 9.0]);

    let sizes = vec![0.1_f64, 0.2, 0.3, 0.4, f64::INFINITY];

    let field = BackgroundSizeField::from_tet_mesh(&vm, &sizes)
        .expect("an orphaned vertex must not fail construction");

    assert_eq!(field.element_count(), 1);
    assert!(
        field.list_data().iter().all(|v| v.is_finite()),
        "orphan INFINITY leaked into the buffer: {:?}",
        field.list_data()
    );
    assert!(
        !field.list_data().contains(&9.0),
        "orphan coordinate leaked into the buffer"
    );
    assert_eq!(
        field.max_size(),
        0.4,
        "max_size must be the max over EMITTED sizes, not over the input slice"
    );
}

// --- (e) validation --------------------------------------------------------

#[test]
fn size_slice_length_must_match_the_vertex_count() {
    let vm = one_p1_tet_vm(); // 4 vertices
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&vm, &[0.1, 0.2, 0.3]),
        "short size slice",
    );
    assert!(
        msg.contains('3') && msg.contains('4'),
        "message must report both counts, got: {msg}"
    );
}

#[test]
fn a_non_finite_size_on_a_referenced_vertex_is_an_error() {
    let vm = one_p1_tet_vm();
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&vm, &[0.1, f64::INFINITY, 0.3, 0.4]),
        "non-finite size",
    );
    assert!(
        msg.contains('1'),
        "message must name the offending vertex index, got: {msg}"
    );
}

#[test]
fn a_non_positive_size_on_a_referenced_vertex_is_an_error() {
    let vm = one_p1_tet_vm();
    for (idx, bad) in [(2_usize, 0.0_f64), (3, -0.5)] {
        let mut sizes = vec![0.1_f64, 0.2, 0.3, 0.4];
        sizes[idx] = bad;
        let msg = expect_err(
            BackgroundSizeField::from_tet_mesh(&vm, &sizes),
            "non-positive size",
        );
        assert!(
            msg.contains(&idx.to_string()),
            "message must name the offending vertex index {idx}, got: {msg}"
        );
    }
}

#[test]
fn a_tet_index_out_of_range_is_an_error_not_a_panic() {
    let vm = tet_vm(
        vec![
            0.0_f32, 0.0, 0.0, //
            1.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, //
            0.0, 0.0, 1.0,
        ],
        vec![0, 1, 2, 7],
        ElementOrderTag::P1,
    );
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&vm, &[0.1, 0.2, 0.3, 0.4]),
        "out-of-range index",
    );
    assert!(
        msg.contains('7'),
        "message must name the offending index, got: {msg}"
    );
}

/// An index buffer that is not a whole number of elements is malformed, and
/// the trailing partial element must be reported rather than dropped: walking
/// whole elements only would build a field over a DIFFERENT mesh than the one
/// the caller handed in, with no error to say so.
#[test]
fn a_ragged_index_buffer_is_an_error_not_a_truncation() {
    let p1 = {
        let mut vm = one_p1_tet_vm();
        if let VolumeConnectivity::Tet { indices, .. } = &mut vm.connectivity {
            indices.extend_from_slice(&[0, 1, 2]);
        }
        vm
    };
    let p2 = tet_vm(
        vec![0.0_f32; 30],
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3],
        ElementOrderTag::P2,
    );

    for (vm, index_count, stride) in [(p1, 7_usize, 4_usize), (p2, 14, 10)] {
        assert_eq!(vm.nodes_per_element(), stride, "fixture sanity");
        let sizes = vec![0.1_f64; vm.vertices.len() / 3];
        let msg = expect_err(
            BackgroundSizeField::from_tet_mesh(&vm, &sizes),
            "ragged index buffer",
        );
        assert!(
            msg.contains(&index_count.to_string()) && msg.contains(&stride.to_string()),
            "message must report the index count {index_count} and the stride {stride}, \
             got: {msg}"
        );
    }
}

#[test]
fn non_tet_connectivity_is_an_error() {
    let hex = VolumeMesh {
        vertices: vec![0.0_f32; 24],
        connectivity: VolumeConnectivity::Hex {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
        normals: None,
        boundary: None,
    };
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&hex, &[0.1_f64; 8]),
        "hex connectivity",
    );
    assert!(
        msg.to_lowercase().contains("tet"),
        "message must say a tet mesh is required, got: {msg}"
    );

    let wedge = VolumeMesh {
        vertices: vec![0.0_f32; 18],
        connectivity: VolumeConnectivity::Wedge {
            indices: vec![0, 1, 2, 3, 4, 5],
        },
        normals: None,
        boundary: None,
    };
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&wedge, &[0.1_f64; 6]),
        "wedge connectivity",
    );
    assert!(
        msg.to_lowercase().contains("tet"),
        "message must say a tet mesh is required, got: {msg}"
    );
}

#[test]
fn an_empty_tet_mesh_is_an_error() {
    let vm = tet_vm(vec![0.0_f32; 12], vec![], ElementOrderTag::P1);
    let msg = expect_err(
        BackgroundSizeField::from_tet_mesh(&vm, &[0.1_f64; 4]),
        "no elements",
    );
    assert!(
        !msg.is_empty(),
        "an empty field would silently disable sizing; it must be rejected"
    );
}
