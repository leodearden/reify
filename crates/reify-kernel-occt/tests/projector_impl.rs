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
