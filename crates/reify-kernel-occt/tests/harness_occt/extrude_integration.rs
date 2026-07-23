//! Integration tests for the extrude operation via the public OcctKernel API.
//!
//! These tests exercise GeometryOp::Extrude through OcctKernel::execute(),
//! testing error-path validation (zero/NaN/Infinity distance).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, Value};

/// Helper: create a kernel with a Box profile to use as an extrude target.
fn kernel_with_box_profile() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(1.0),
        })
        .expect("Box creation should succeed");
    (kernel, box_h.id)
}

#[test]
fn extrude_zero_distance_returns_error() {
    let (mut kernel, profile_id) = kernel_with_box_profile();

    let result = kernel.execute(&GeometryOp::Extrude {
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
fn extrude_non_finite_distance_returns_error_nan() {
    let (mut kernel, profile_id) = kernel_with_box_profile();

    let result = kernel.execute(&GeometryOp::Extrude {
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
fn extrude_non_finite_distance_returns_error_infinity() {
    let (mut kernel, profile_id) = kernel_with_box_profile();

    let result = kernel.execute(&GeometryOp::Extrude {
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
