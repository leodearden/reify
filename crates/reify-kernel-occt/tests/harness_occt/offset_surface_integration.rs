//! Integration tests for `GeometryOp::OffsetSurface` via the public
//! OcctKernel API.
//!
//! RED step-3 (task #4192): verifies the kernel execute arm produces a
//! `BRepKind::Face` handle that preserves surface area (rigid translation of
//! a planar face) and lands at the expected +Z offset, and rejects a
//! zero-distance offset.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Parse the JSON-encoded bounding box string returned by
/// `GeometryQuery::BoundingBox` into (zmin, zmax).
fn parse_bbox_z(v: &Value) -> (f64, f64) {
    let s = match v {
        Value::String(s) => s,
        other => panic!("expected Value::String, got {:?}", other),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(s).unwrap_or_else(|e| panic!("failed to parse {:?} as JSON: {e}", s));
    let zmin = parsed["zmin"].as_f64().expect("missing zmin");
    let zmax = parsed["zmax"].as_f64().expect("missing zmax");
    (zmin, zmax)
}

/// `offset_surface(rectangle(20mm,10mm), 2mm)`: the offset face must (1) be
/// a `BRepKind::Face`, (2) preserve surface area within 2% (rigid
/// translation of a planar face), and (3) land at z ≈ +0.002 m (offset
/// along the +Z normal), within 0.5mm.
#[test]
fn offset_surface_preserves_area_and_lands_at_positive_z() {
    let mut kernel = OcctKernel::new();
    let rect_h = kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.020),
            height: Value::Real(0.010),
        })
        .expect("RectangleProfile execute should succeed");

    let off_h = kernel
        .execute(&GeometryOp::OffsetSurface {
            target: rect_h.id,
            distance: Value::Real(0.002),
        })
        .expect("OffsetSurface execute should succeed");
    assert_eq!(
        off_h.repr,
        Some(BRepKind::Face),
        "OffsetSurface must produce BRepKind::Face"
    );

    let a_in = kernel
        .query(&GeometryQuery::SurfaceArea(rect_h.id))
        .expect("input SurfaceArea query should succeed")
        .as_f64()
        .expect("SurfaceArea should be numeric");
    let a_out = kernel
        .query(&GeometryQuery::SurfaceArea(off_h.id))
        .expect("offset SurfaceArea query should succeed")
        .as_f64()
        .expect("SurfaceArea should be numeric");
    let rel_err = (a_out - a_in).abs() / a_in;
    assert!(
        rel_err <= 0.02,
        "offset surface area should be within 2% of input: input={a_in:.6e}, offset={a_out:.6e} (rel_err={rel_err:.4})"
    );

    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(off_h.id))
        .expect("bbox query should succeed");
    let (zmin, zmax) = parse_bbox_z(&bbox);
    let tol = 0.0005; // 0.5mm
    assert!(
        (zmin - 0.002).abs() <= tol,
        "offset surface zmin should be ≈ 0.002 m, got {zmin}"
    );
    assert!(
        (zmax - 0.002).abs() <= tol,
        "offset surface zmax should be ≈ 0.002 m, got {zmax}"
    );
}

/// A zero-distance offset is degenerate and must return
/// `GeometryError::OperationFailed`, not a silent no-op.
#[test]
fn offset_surface_zero_distance_returns_err() {
    let mut kernel = OcctKernel::new();
    let rect_h = kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.020),
            height: Value::Real(0.010),
        })
        .expect("RectangleProfile execute should succeed");

    let result = kernel.execute(&GeometryOp::OffsetSurface {
        target: rect_h.id,
        distance: Value::Real(0.0),
    });
    assert!(
        result.is_err(),
        "zero-distance OffsetSurface should return Err, got Ok"
    );
}
