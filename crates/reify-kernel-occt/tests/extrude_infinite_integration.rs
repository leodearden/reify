//! Integration tests for the extrude_infinite operation via the public OcctKernel API.
//!
//! These tests exercise `GeometryOp::ExtrudeInfinite` through `OcctKernel::execute()`,
//! verifying:
//!   1. Valid profile + non-degenerate axis → Ok (solid handle returned).
//!   2. `intersection(extrude_infinite(circle, +Z), box_at_origin)` yields a finite,
//!      clipped solid with `z_min ≈ 0`, `z_max ≈ +half_depth` — proving unbounded→bounded.
//!   3. Zero/degenerate axis → `OperationFailed` (with a diagnostic about the axis).
//!
//! Mirrors the structure of `extrude_symmetric_integration.rs`.
//! Gated on `#![cfg(has_occt)]` so non-OCCT builds skip without linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Parse the full bounding box from the JSON string returned by
/// `GeometryQuery::BoundingBox`.
///
/// Expected format:
/// `{"xmin":<f>,"ymin":<f>,"zmin":<f>,"xmax":<f>,"ymax":<f>,"zmax":<f>}`
fn parse_bbox(s: &str) -> (f64, f64, f64, f64, f64, f64) {
    let (mut xmin, mut ymin, mut zmin) = (f64::NAN, f64::NAN, f64::NAN);
    let (mut xmax, mut ymax, mut zmax) = (f64::NAN, f64::NAN, f64::NAN);
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "xmin" => xmin = val,
            "ymin" => ymin = val,
            "zmin" => zmin = val,
            "xmax" => xmax = val,
            "ymax" => ymax = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (xmin, ymin, zmin, xmax, ymax, zmax)
}

/// Helper: create a kernel with a 5mm-radius `CircleProfile` (disk at z=0)
/// as the profile for `ExtrudeInfinite`.
fn kernel_with_circle_profile() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let profile = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(5.0e-3),
        })
        .expect("CircleProfile creation should succeed");
    (kernel, profile.id)
}

/// Test 1: `ExtrudeInfinite` on a planar disk profile with a non-degenerate axis
/// returns `Ok` (a valid solid handle).
///
/// RED until step-8 replaces the placeholder `OperationFailed` in execute().
#[test]
fn extrude_infinite_valid_profile_returns_ok() {
    let (mut kernel, profile_id) = kernel_with_circle_profile();

    let result = kernel.execute(&GeometryOp::ExtrudeInfinite {
        profile: profile_id,
        axis: [0.0, 0.0, 1.0],
        both: false,
    });

    assert!(
        result.is_ok(),
        "ExtrudeInfinite with valid axis should succeed, got: {:?}",
        result.err()
    );
}

/// Test 2: A semi-infinite prism in +Z (profile at z=0) intersected with a
/// 20mm×20mm×10mm box centred at origin yields a clipped solid:
/// - `z_min ≈ 0` (the profile base face),
/// - `z_max ≈ +5mm` (the upper face of the box, half of 10mm depth),
/// - `x/y` extents finite and within the circle-profile footprint (±5mm).
///
/// This proves the unbounded→bounded conversion path end-to-end.
///
/// RED until step-8 makes `ExtrudeInfinite` succeed.
#[test]
fn extrude_infinite_intersected_with_box_yields_finite_clipped_solid() {
    let (mut kernel, profile_id) = kernel_with_circle_profile();

    // Semi-infinite prism in +Z starting from the profile face at z=0.
    let prism_h = kernel
        .execute(&GeometryOp::ExtrudeInfinite {
            profile: profile_id,
            axis: [0.0, 0.0, 1.0],
            both: false,
        })
        .expect("ExtrudeInfinite with valid axis should succeed");

    // 20mm × 20mm × 10mm box centred at origin → Z spans [−5mm, +5mm].
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(20.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("Box creation should succeed");

    // Boolean intersection: prism ∩ box → region where z ∈ [0, +5mm].
    let intersection_h = kernel
        .execute(&GeometryOp::Intersection {
            left: prism_h.id,
            right: box_h.id,
        })
        .expect("Intersection of infinite prism with box should succeed");

    // Query and parse the bounding box.
    let bbox_val = kernel
        .query(&GeometryQuery::BoundingBox(intersection_h.id))
        .expect("BoundingBox query should succeed");
    let bbox_str = match bbox_val {
        Value::String(s) => s,
        other => panic!("expected BoundingBox to return a String, got {:?}", other),
    };
    let (xmin, ymin, zmin, xmax, ymax, zmax) = parse_bbox(&bbox_str);

    // OCCT adds a small positive padding to bounding boxes (~1e-7 m scale).
    let tol = 1.0e-6_f64;
    let half_depth = 5.0e-3_f64; // half of the 10mm box depth
    let radius_m = 5.0e-3_f64;   // circle profile radius

    assert!(
        zmin.abs() < tol,
        "z_min should be ≈ 0 (profile face at z=0), got {zmin}"
    );
    assert!(
        (zmax - half_depth).abs() < tol,
        "z_max should be ≈ +5mm (box upper face), got {zmax}"
    );
    assert!(
        xmin.is_finite() && ymin.is_finite() && xmax.is_finite() && ymax.is_finite(),
        "x/y bbox extents should all be finite, got \
         xmin={xmin} ymin={ymin} xmax={xmax} ymax={ymax}"
    );
    assert!(
        xmin >= -radius_m - tol && xmax <= radius_m + tol,
        "x extents should be within profile radius ±5mm, got [{xmin}, {xmax}]"
    );
    assert!(
        ymin >= -radius_m - tol && ymax <= radius_m + tol,
        "y extents should be within profile radius ±5mm, got [{ymin}, {ymax}]"
    );
}

/// Test 3: `ExtrudeInfinite` with a zero (degenerate) axis returns
/// `OperationFailed` with a diagnostic about the axis magnitude.
///
/// With the step-6 placeholder this fails because the error message
/// says "not yet wired" rather than describing the zero-axis condition.
#[test]
fn extrude_infinite_zero_axis_returns_error() {
    let (mut kernel, profile_id) = kernel_with_circle_profile();

    let result = kernel.execute(&GeometryOp::ExtrudeInfinite {
        profile: profile_id,
        axis: [0.0, 0.0, 0.0],
        both: false,
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("zero") || msg.contains("degenerate") || msg.contains("magnitude"),
                "expected error message about zero/degenerate axis, got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for zero axis, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}
