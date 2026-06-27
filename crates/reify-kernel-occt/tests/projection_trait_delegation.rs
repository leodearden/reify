//! Integration tests for task 4744 (mesh-morph β): the two NEW
//! `GeometryKernel` trait methods `closest_point_on_shape(handle, [f64;3])` and
//! `vertex_point(handle)`, driven THROUGH a `&dyn GeometryKernel` bound to a
//! real `OcctKernelHandle`, must delegate to OCCT's inherent
//! `closest_point_on_shape` (BRepExtrema) / `vertex_point` (`BRep_Tool::Pnt`).
//!
//! `OcctKernelHandle` (not the lower-level `OcctKernel`) is the type that
//! implements `GeometryKernel`, so it is what the engine hands the morph seam
//! as `&dyn GeometryKernel`. This pins the cycle-free projection seam:
//! `reify-mesh-morph` holds only `&dyn GeometryKernel` (from `MorphRequest`) and
//! cannot name the OCCT kernel, so boundary-node projection onto the morphed
//! BRep MUST flow through the trait object — which means the OCCT kernel must
//! OVERRIDE the (default-Err) trait methods to reach its inherent
//! implementations.
//!
//! RED (step-3): `OcctKernelHandle` does not yet override the trait defaults, so
//! the trait-object calls hit the honest-absence `Err(QueryError::QueryFailed)`
//! default and the `.expect(...)` unwraps panic.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryKernel, GeometryOp, Value};
use reify_kernel_occt::OcctKernelHandle;

/// Build a handle containing one 10×10×10 box (centred at origin → spans
/// `[-5, 5]^3`), return the handle and the box's handle id.
fn box10() -> (OcctKernelHandle, GeometryHandleId) {
    let mut handle = OcctKernelHandle::spawn();
    let id = handle
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed")
        .id;
    (handle, id)
}

/// `vertex_point` through `&dyn GeometryKernel` returns the exact position of a
/// box corner — i.e. it delegates to the inherent `BRep_Tool::Pnt` accessor, not
/// the not-supported default. Every corner of the origin-centred 10mm box sits
/// at `(±5, ±5, ±5)`.
#[test]
fn vertex_point_trait_method_delegates_to_inherent() {
    let (handle, box_id) = box10();
    let verts = handle
        .extract_vertices(box_id)
        .expect("extract_vertices should succeed");
    assert!(!verts.is_empty(), "a box must have corner vertices");

    let dynk: &dyn GeometryKernel = &handle;
    let p = dynk.vertex_point(verts[0]).expect(
        "trait vertex_point must delegate to the inherent OcctKernel::vertex_point and succeed",
    );

    for (axis, c) in p.iter().enumerate() {
        assert!(
            (c.abs() - 5.0).abs() < 1e-6,
            "box corner axis {axis} should be ±5, got {p:?}",
        );
    }
}

/// `closest_point_on_shape` through `&dyn GeometryKernel` projects a far probe
/// onto the box surface — i.e. it delegates to the inherent BRepExtrema method
/// (the trait override forwards `[f64;3]` → `(px, py, pz)`). The nearest surface
/// point of `[100, 0, 0]` on the origin-centred 10mm box is the +X face at
/// `≈ [5, 0, 0]`.
#[test]
fn closest_point_on_shape_trait_method_delegates_to_inherent() {
    let (handle, box_id) = box10();

    let dynk: &dyn GeometryKernel = &handle;
    let p = dynk.closest_point_on_shape(box_id, [100.0, 0.0, 0.0]).expect(
        "trait closest_point_on_shape must delegate to the inherent OcctKernel method and succeed",
    );

    assert!((p[0] - 5.0).abs() < 1e-6, "expected x≈5, got {p:?}");
    assert!(p[1].abs() < 1e-6, "expected y≈0, got {p:?}");
    assert!(p[2].abs() < 1e-6, "expected z≈0, got {p:?}");
}
