//! Integration tests for `OcctProjector` — the `reify_mesh_morph::Projector`
//! impl backed by `OcctKernel` (task 3535; PRD `mesh-morphing-phase-2.md`
//! §3.4, §7.3).
//!
//! Fixture: a 10×10×10 box centred at the origin (`x∈[-5,5]`, `y∈[-5,5]`,
//! `z∈[-5,5]`). The BRepExtrema_DistShapeShape semantics are already pinned
//! by `closest_point_on_shape_integration.rs`; these tests verify that the
//! Projector trait methods are wired through to those primitives correctly.

#![cfg(all(has_occt, feature = "mesh-morph"))]

use reify_kernel_occt::{OcctKernel, OcctProjector};
use reify_mesh_morph::Projector;
use reify_types::{GeometryHandleId, GeometryOp, Value};

/// Build a kernel with a single 10×10×10 box centred at the origin
/// (`x∈[-5,5]`, `y∈[-5,5]`, `z∈[-5,5]`).
///
/// Returns `(kernel, box_id)`. Mirrors `box_kernel` in
/// `closest_point_on_shape_integration.rs` so projector assertions reference
/// the same BRepExtrema-distance invariants the existing FFI tests pin.
fn box_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed");
    (kernel, handle.id)
}

/// Every face of a centred 10×10×10 box is at distance 5 from the origin
/// (each face plane is `x = ±5`, `y = ±5`, or `z = ±5`; the perpendicular
/// foot from the origin lands within the `[-5, 5]²` face bounds). Test that
/// `OcctProjector::project_onto_face` returns a witness at distance ≈5 for
/// every face — face-id independent, robust to TopExp ordering changes.
#[test]
fn occt_projector_project_onto_face_returns_distance_5_witness_for_box_face() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed for a valid box");
    assert_eq!(faces.len(), 6, "box should have 6 faces");

    let projector = OcctProjector::new(&kernel);
    for &fid in &faces {
        match projector.project_onto_face(fid, [0.0, 0.0, 0.0]) {
            Ok([x, y, z]) => {
                let dist = (x * x + y * y + z * z).sqrt();
                assert!(
                    (dist - 5.0).abs() < 1e-6,
                    "every face plane of a centred 10×10×10 box is at distance 5 \
                     from the origin; got ({x}, {y}, {z}), dist={dist}"
                );
            }
            Err(e) => panic!("project_onto_face on a box face should succeed, got Err({e:?})"),
        }
    }
}

/// Every edge of a centred 10×10×10 box has its perpendicular foot from the
/// origin landing on a corner-adjacent point at distance `√(5² + 5²) = 5√2 ≈
/// 7.0710678`. Each box edge is parallel to one of the three axes; the
/// perpendicular foot from the origin onto e.g. the edge `(5, 5, t)` for
/// `t ∈ [-5, 5]` is the closest point on that edge, at `(5, 5, 0)` with
/// distance `√50 = 5√2`. Test that `OcctProjector::project_onto_edge` returns
/// such a witness for every edge — edge-id independent, TopExp-order robust.
#[test]
fn occt_projector_project_onto_edge_returns_distance_5_sqrt_2_witness_for_box_edge() {
    let (mut kernel, box_id) = box_kernel();
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges should succeed for a valid box");
    assert_eq!(edges.len(), 12, "box should have 12 edges");

    let expected = (50.0_f64).sqrt(); // 5√2
    let projector = OcctProjector::new(&kernel);
    for &eid in &edges {
        match projector.project_onto_edge(eid, [0.0, 0.0, 0.0]) {
            Ok([x, y, z]) => {
                let dist = (x * x + y * y + z * z).sqrt();
                assert!(
                    (dist - expected).abs() < 1e-6,
                    "every edge of a centred 10×10×10 box has its perpendicular foot \
                     from the origin at distance 5√2≈{expected}; got ({x}, {y}, {z}), \
                     dist={dist}"
                );
            }
            Err(e) => panic!("project_onto_edge on a box edge should succeed, got Err({e:?})"),
        }
    }
}

/// `OcctProjector::vertex_position` snaps to the vertex's exact stored
/// coordinates (PRD §3.4 "BRep_Tool::Pnt direct; no closest-point"). Pin a
/// non-origin location so a buggy "always-zero" impl can't pass.
#[test]
fn occt_projector_vertex_position_returns_exact_stored_coordinates() {
    let mut kernel = OcctKernel::new();
    let vertex_id = kernel.store_vertex_at_for_test(1.5, -2.5, 3.5);

    let projector = OcctProjector::new(&kernel);
    match projector.vertex_position(vertex_id) {
        Ok([x, y, z]) => {
            assert!((x - 1.5).abs() < 1e-9, "expected x≈1.5, got {x}");
            assert!((y - (-2.5)).abs() < 1e-9, "expected y≈-2.5, got {y}");
            assert!((z - 3.5).abs() < 1e-9, "expected z≈3.5, got {z}");
        }
        Err(e) => panic!("expected Ok([1.5, -2.5, 3.5]), got Err({e:?})"),
    }
}
