//! Integration tests for the extrude_symmetric operation via the public
//! OcctKernel API.
//!
//! These tests exercise GeometryOp::ExtrudeSymmetric through
//! OcctKernel::execute(), verifying both the symmetry invariant
//! (the extruded shape spans +/- distance/2 about the profile's plane,
//! i.e. the z-centroid of the result aligns with the profile's z-centroid)
//! and the standard degeneracy error paths (zero / NaN / infinity distance).
//!
//! The centroid-alignment invariant is verified via a BoundingBox query on
//! a wire profile at z=0 extruded along Z: the resulting shell's z-extent
//! must be [-distance/2, +distance/2]. This exercises the public API only
//! — a Face-based centroid check would need private FFI access (OcctKernel
//! does not expose face primitives) and is covered by in-crate unit tests
//! in lib.rs.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Helper: create a kernel with a horizontal line-segment wire at z=0 as
/// profile. Extruding this along Z produces a vertical rectangular shell
/// whose z-extent mirrors the extrusion distance. We use this (rather than
/// a Box) because `make_prism` requires an Edge/Wire/Face — not a Solid.
fn kernel_with_line_profile_at_z0() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let wire = kernel
        .execute(&GeometryOp::LineSegment {
            x1: -0.5,
            y1: 0.0,
            z1: 0.0,
            x2: 0.5,
            y2: 0.0,
            z2: 0.0,
        })
        .expect("LineSegment creation should succeed");
    (kernel, wire.id)
}

/// Parse the JSON-encoded bounding box string returned by
/// `GeometryQuery::BoundingBox` into (zmin, zmax). (We only need Z here.)
fn parse_bbox_z(s: &str) -> (f64, f64) {
    // Expected format:
    // {"xmin":<f>,"ymin":<f>,"zmin":<f>,"xmax":<f>,"ymax":<f>,"zmax":<f>}
    let mut zmin = f64::NAN;
    let mut zmax = f64::NAN;
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "zmin" => zmin = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (zmin, zmax)
}

/// The core symmetry invariant: after ExtrudeSymmetric along +Z by
/// distance d on a profile at z=0, the resulting shape's z-extent is
/// [-d/2, +d/2]. This is equivalent to "result centroid z aligns with
/// profile centroid z (=0)" for translation-symmetric profiles.
#[test]
fn extrude_symmetric_centroid_at_z_zero() {
    let (mut kernel, profile_id) = kernel_with_line_profile_at_z0();

    // Sanity check: the profile's z-extent is essentially [0, 0] (flat at
    // z=0). OCCT BRepBndLib applies a tiny positive pad to every bounding
    // box (typically ≤1e-6 for a 1m-scale shape), so we use a matching
    // tolerance.
    let bbox_tol = 1e-6_f64;
    let profile_bbox = kernel
        .query(&GeometryQuery::BoundingBox(profile_id))
        .expect("profile bbox query should succeed");
    let (p_zmin, p_zmax) = match profile_bbox {
        Value::String(s) => parse_bbox_z(&s),
        other => panic!("expected bbox String, got {:?}", other),
    };
    assert!(
        p_zmin.abs() < bbox_tol && p_zmax.abs() < bbox_tol,
        "profile z-extent should be ≈ [0, 0], got [{}, {}]",
        p_zmin,
        p_zmax
    );

    // Extrude symmetrically by 0.02 m along +z (distance/2 each way).
    let distance = 0.02_f64;
    let result = kernel
        .execute(&GeometryOp::ExtrudeSymmetric {
            profile: profile_id,
            distance: Value::Real(distance),
        })
        .expect("ExtrudeSymmetric should succeed");

    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(result.id))
        .expect("bbox query should succeed");
    let (zmin, zmax) = match bbox {
        Value::String(s) => parse_bbox_z(&s),
        other => panic!("expected bbox String, got {:?}", other),
    };
    let half = distance / 2.0;
    assert!(
        (zmin - (-half)).abs() < bbox_tol,
        "ExtrudeSymmetric zmin should be ≈ -distance/2 = {}, got {}",
        -half,
        zmin
    );
    assert!(
        (zmax - half).abs() < bbox_tol,
        "ExtrudeSymmetric zmax should be ≈ +distance/2 = {}, got {}",
        half,
        zmax
    );
    // And centroid (midpoint) aligns with profile's z-centroid (0).
    // bbox padding is symmetric so cancels in the midpoint → assertion can
    // be tighter than bbox_tol.
    let center = (zmin + zmax) / 2.0;
    assert!(
        center.abs() < 1e-9,
        "ExtrudeSymmetric result z-centroid should align with profile \
         centroid z (0), got center_z={} (zmin={}, zmax={})",
        center,
        zmin,
        zmax
    );
}

#[test]
fn extrude_symmetric_zero_distance_returns_error() {
    let (mut kernel, profile_id) = kernel_with_line_profile_at_z0();

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
    let (mut kernel, profile_id) = kernel_with_line_profile_at_z0();

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
    let (mut kernel, profile_id) = kernel_with_line_profile_at_z0();

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
