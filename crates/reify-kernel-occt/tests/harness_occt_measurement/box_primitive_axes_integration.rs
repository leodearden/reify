//! `GeometryOp::Box`'s three size slots drive the X, Y and Z extents of an
//! origin-centred solid, measured on the real OCCT kernel.
//!
//! Each slot gets a distinct value, so any transposition of the slots on the way
//! to `BRepPrimAPI_MakeBox` changes a measured span and fails.

#![cfg(has_occt)]

use crate::common::bbox_of;
use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// `BRepBndLib` pads the box it reports by at most this much for metre-scale shapes.
const TOL: f64 = 1e-6;

fn assert_close(axis: &str, what: &str, actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < TOL,
        "{axis} {what}: expected {expected}, got {actual}"
    );
}

#[test]
fn box_size_slots_drive_the_x_y_z_extents() {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::length(0.01),
            size_y: Value::length(0.02),
            size_z: Value::length(0.03),
        })
        .expect("box creation should succeed");

    let bb = bbox_of(kernel.query(&GeometryQuery::BoundingBox(h.id)));
    let (span_x, span_y, span_z) = bb.spans();

    assert_close("X", "span", span_x, 0.01);
    assert_close("Y", "span", span_y, 0.02);
    assert_close("Z", "span", span_z, 0.03);

    assert_close("X", "min", bb.xmin, -0.005);
    assert_close("Y", "min", bb.ymin, -0.010);
    assert_close("Z", "min", bb.zmin, -0.015);
}
