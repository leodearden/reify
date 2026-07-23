//! Integration tests for curve constructor operations via the public OcctKernel API.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_kernel_occt::RUST_GUARD_MARKER;
use reify_ir::{BRepKind, GeometryError, GeometryOp};

// --- LineSegment ---

#[test]
fn line_segment_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::LineSegment {
        x1: 0.0,
        y1: 0.0,
        z1: 0.0,
        x2: 1.0,
        y2: 0.0,
        z2: 0.0,
    });
    let handle = result.expect("line_segment should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

#[test]
fn line_segment_coincident_points_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::LineSegment {
        x1: 1.0,
        y1: 2.0,
        z1: 3.0,
        x2: 1.0,
        y2: 2.0,
        z2: 3.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains(RUST_GUARD_MARKER),
                "expected Rust-guard error (containing {RUST_GUARD_MARKER:?} marker), got: {msg}"
            );
        }
        Ok(_) => panic!("expected error for coincident points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- Arc ---

#[test]
fn arc_quarter_circle_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Arc {
        center: [0.0, 0.0, 0.0],
        radius: 1.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::FRAC_PI_2,
        axis: [0.0, 0.0, 1.0],
    });
    let handle = result.expect("arc should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

#[test]
fn arc_zero_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Arc {
        center: [0.0, 0.0, 0.0],
        radius: 0.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::FRAC_PI_2,
        axis: [0.0, 0.0, 1.0],
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for zero radius, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- Helix ---

#[test]
fn helix_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Helix {
        radius: 5.0,
        pitch: 2.0,
        height: 10.0,
    });
    let handle = result.expect("helix should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

// --- InterpCurve ---

#[test]
fn interp_curve_through_4_points_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::InterpCurve {
        points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 1.0, 0.0],
        ],
    });
    let handle = result.expect("interp_curve should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

#[test]
fn interp_curve_fewer_than_2_points_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::InterpCurve {
        points: vec![[0.0, 0.0, 0.0]],
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for fewer than 2 points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- BezierCurve ---

#[test]
fn bezier_curve_4_control_points_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::BezierCurve {
        control_points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 0.0],
            [3.0, 2.0, 0.0],
            [4.0, 0.0, 0.0],
        ],
    });
    let handle = result.expect("bezier_curve should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

#[test]
fn bezier_curve_fewer_than_2_points_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::BezierCurve {
        control_points: vec![[0.0, 0.0, 0.0]],
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for fewer than 2 control points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- NurbsCurve ---

#[test]
fn nurbs_curve_valid_creates_wire() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::NurbsCurve {
        control_points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 1.0, 0.0],
        ],
        weights: vec![1.0, 1.0, 1.0, 1.0],
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        degree: 3,
    });
    let handle = result.expect("nurbs_curve should succeed");
    assert_eq!(handle.repr, Some(BRepKind::Wire));
}

#[test]
fn nurbs_curve_mismatched_weights_returns_error() {
    let mut kernel = OcctKernel::new();
    // 4 control points but only 2 weights — must return an error, not UB
    let result = kernel.execute(&GeometryOp::NurbsCurve {
        control_points: vec![
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 1.0, 0.0],
        ],
        weights: vec![1.0, 1.0], // only 2 instead of 4
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        degree: 3,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("weights count must equal control points count"),
                "expected weights mismatch message, got: {}",
                msg,
            );
        }
        Ok(_) => panic!("expected error for mismatched weights/control_points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}
