//! Integration tests for the extrude_symmetric operation via the public
//! OcctKernel API.
//!
//! These tests exercise GeometryOp::ExtrudeSymmetric through
//! OcctKernel::execute(), verifying both the centroid-alignment invariant
//! (the extruded solid's centroid matches the profile's centroid in the
//! extrusion direction) and the standard degeneracy error paths
//! (zero / NaN / infinity distance).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Helper: create a kernel with a 1m × 1m × 1m Box profile centered at
/// the origin. We build the Box and translate it so its centroid is at
/// (0, 0, 0) — this lets extrude_symmetric's centroid-preservation
/// invariant be tested against a known profile centroid.
fn kernel_with_centered_box_profile() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        })
        .expect("Box creation should succeed");
    let centered = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: -0.5,
            dy: -0.5,
            dz: -0.5,
        })
        .expect("Translate should succeed");
    (kernel, centered.id)
}

/// Parse the JSON-encoded centroid string returned by
/// `GeometryQuery::Centroid` into (x, y, z).
fn parse_centroid(s: &str) -> (f64, f64, f64) {
    // Expected format: {"x":<f>,"y":<f>,"z":<f>}
    let inner = s.trim_start_matches('{').trim_end_matches('}');
    let mut x = f64::NAN;
    let mut y = f64::NAN;
    let mut z = f64::NAN;
    for pair in inner.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts
            .next()
            .unwrap()
            .trim_matches('"')
            .trim_matches('{')
            .trim();
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "x" => x = val,
            "y" => y = val,
            "z" => z = val,
            _ => {}
        }
    }
    (x, y, z)
}

/// The core centroid-alignment invariant: after ExtrudeSymmetric along
/// +z by distance d, the resulting solid's centroid in z equals the
/// profile's centroid in z (profile centered at z=0 → result centroid
/// z ≈ 0).
#[test]
fn extrude_symmetric_centroid_at_z_zero() {
    let (mut kernel, profile_id) = kernel_with_centered_box_profile();

    // Sanity check: the centered profile's centroid is at (0,0,0).
    let profile_centroid = kernel
        .query(&GeometryQuery::Centroid(profile_id))
        .expect("profile centroid query should succeed");
    let (px, py, pz) = match profile_centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };
    assert!(
        px.abs() < 1e-9 && py.abs() < 1e-9 && pz.abs() < 1e-9,
        "profile centroid should be at origin, got ({}, {}, {})",
        px,
        py,
        pz
    );

    // Extrude symmetrically by 0.02 m along +z (distance/2 each way).
    let result = kernel
        .execute(&GeometryOp::ExtrudeSymmetric {
            profile: profile_id,
            distance: Value::Real(0.02),
        })
        .expect("ExtrudeSymmetric should succeed");

    let centroid = kernel
        .query(&GeometryQuery::Centroid(result.id))
        .expect("centroid query should succeed");
    let (_cx, _cy, cz) = match centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };
    assert!(
        cz.abs() < 1e-9,
        "ExtrudeSymmetric result centroid z should align with profile \
         centroid z (0), got z={}",
        cz
    );
}

#[test]
fn extrude_symmetric_zero_distance_returns_error() {
    let (mut kernel, profile_id) = kernel_with_centered_box_profile();

    let result = kernel.execute(&GeometryOp::ExtrudeSymmetric {
        profile: profile_id,
        distance: Value::Real(0.0),
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("zero"),
                "expected error message containing 'zero', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for zero distance, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
fn extrude_symmetric_non_finite_distance_returns_error_nan() {
    let (mut kernel, profile_id) = kernel_with_centered_box_profile();

    let result = kernel.execute(&GeometryOp::ExtrudeSymmetric {
        profile: profile_id,
        distance: Value::Real(f64::NAN),
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite"),
                "expected error message containing 'finite', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for NaN distance, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
fn extrude_symmetric_non_finite_distance_returns_error_infinity() {
    let (mut kernel, profile_id) = kernel_with_centered_box_profile();

    let result = kernel.execute(&GeometryOp::ExtrudeSymmetric {
        profile: profile_id,
        distance: Value::Real(f64::INFINITY),
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite"),
                "expected error message containing 'finite', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for Infinity distance, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}
