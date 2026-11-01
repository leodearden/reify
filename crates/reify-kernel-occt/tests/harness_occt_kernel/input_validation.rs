//! `OcctKernel::execute` rejects non-finite, zero, negative or degenerate primitive and
//! transform parameters with a descriptive `GeometryError`.

#![cfg(has_occt)]

use crate::support::assert_operation_fails_with;
use reify_ir::{
    BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryOp, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE, Value,
};
use reify_kernel_occt::OcctKernel;

/// RED step-1 (task 4999): `GeometryOp::Surface` is a Mesh-repr terminal
/// anchor fed by a Voxel→Mesh conversion edge (PRD
/// docs/prds/v0_3/voxel-to-mesh-surfacing.md C-1) — it must never reach
/// `OcctKernel::execute()`. Mirrors the `GeometryOp::Split` defensive arm:
/// a permanent fail-loud `Err`, not a `todo!()` stub (Surface is never
/// occt-executed; reaching this arm would be a dispatcher bug).
#[test]
fn execute_surface_returns_operation_failed() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Surface {
        grid: GeometryHandleId(1),
        iso_level: 0.0,
        adaptive: false,
    });
    assert_operation_fails_with(result, "GeometryOp::Surface");
}

#[test]
fn execute_box_zero_dimension_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(0.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                "box-dimensions rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-width box"),
    }
}

#[test]
fn execute_box_negative_dimension_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(-5.0),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                "box-dimensions rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for negative-width box"),
    }
}

#[test]
fn execute_cylinder_zero_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cylinder {
        radius: Value::Real(0.0),
        height: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-radius cylinder"),
    }
}

#[test]
fn execute_cylinder_negative_height_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cylinder {
        radius: Value::Real(5.0),
        height: Value::Real(-1.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for negative-height cylinder"),
    }
}

#[test]
fn execute_sphere_zero_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Sphere {
        radius: Value::Real(0.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
                "sphere-radius rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-radius sphere"),
    }
}

#[test]
fn execute_sphere_negative_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Sphere {
        radius: Value::Real(-1.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
                "sphere-radius rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for negative-radius sphere"),
    }
}

#[test]
fn execute_fillet_zero_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    // Create a valid box first
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Fillet {
        target: box_h.id,
        edges: vec![],
        radius: Value::Real(0.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-radius fillet"),
    }
}

// --- NaN / Infinity parameter rejection tests (step-9) ---

#[test]
fn execute_box_nan_dimension_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(f64::NAN),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                "box-dimensions rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN-width box"),
    }
}

#[test]
fn execute_box_infinity_dimension_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(f64::INFINITY),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                "box-dimensions rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity-width box"),
    }
}

#[test]
fn execute_box_neg_infinity_dimension_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Box {
        size_x: Value::Real(f64::NEG_INFINITY),
        size_y: Value::Real(10.0),
        size_z: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE,
                "box-dimensions rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for neg-infinity-width box"),
    }
}

#[test]
fn execute_cylinder_nan_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cylinder {
        radius: Value::Real(f64::NAN),
        height: Value::Real(10.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN-radius cylinder"),
    }
}

#[test]
fn execute_cylinder_infinity_height_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cylinder {
        radius: Value::Real(5.0),
        height: Value::Real(f64::INFINITY),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity-height cylinder"),
    }
}

#[test]
fn execute_sphere_nan_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Sphere {
        radius: Value::Real(f64::NAN),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
                "sphere-radius rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN-radius sphere"),
    }
}

#[test]
fn execute_sphere_infinity_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Sphere {
        radius: Value::Real(f64::INFINITY),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert_eq!(
                msg, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE,
                "sphere-radius rejection must emit the byte-identical shared const; got {msg:?}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity-radius sphere"),
    }
}

#[test]
fn execute_fillet_nan_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Fillet {
        target: box_h.id,
        edges: vec![],
        radius: Value::Real(f64::NAN),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN-radius fillet"),
    }
}

#[test]
fn execute_fillet_infinity_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Fillet {
        target: box_h.id,
        edges: vec![],
        radius: Value::Real(f64::INFINITY),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity-radius fillet"),
    }
}

// --- Chamfer distance validation tests ---

#[test]
fn execute_chamfer_zero_distance_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Chamfer {
        target: box_h.id,
        edges: vec![],
        distance: Value::Real(0.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-distance chamfer"),
    }
}

#[test]
fn execute_chamfer_negative_distance_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Chamfer {
        target: box_h.id,
        edges: vec![],
        distance: Value::Real(-1.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for negative-distance chamfer"),
    }
}

#[test]
fn execute_chamfer_nan_distance_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Chamfer {
        target: box_h.id,
        edges: vec![],
        distance: Value::Real(f64::NAN),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN-distance chamfer"),
    }
}

#[test]
fn execute_chamfer_infinity_distance_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Chamfer {
        target: box_h.id,
        edges: vec![],
        distance: Value::Real(f64::INFINITY),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity-distance chamfer"),
    }
}

// --- fillet_with_history / chamfer_with_history direct-caller validation ---
// (v0.2 persistent-naming-v2, task 2655, step-3/step-7)
//
// These tests pin the DEFENSE-IN-DEPTH validation on the inherent
// `OcctKernel::fillet_with_history` / `chamfer_with_history` methods so
// that callers bypassing `GeometryOp::Fillet/Chamfer` get the same
// descriptive errors. The error message must mention the parameter name
// ("radius" / "distance") AND the op name ("fillet" / "chamfer") so call
// sites can identify which parameter caused the failure.

fn make_10mm_box_for_local_feature_test(kernel: &mut OcctKernel) -> GeometryHandle {
    kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0e-3),
            size_y: Value::Real(10.0e-3),
            size_z: Value::Real(10.0e-3),
        })
        .expect("10mm box should build")
}

#[test]
fn fillet_with_history_invalid_radius_returns_error() {
    let cases: &[(&str, f64)] = &[
        ("zero", 0.0),
        ("negative", -1.0e-3),
        ("NaN", f64::NAN),
        ("infinity", f64::INFINITY),
    ];
    for &(label, radius) in cases {
        let mut kernel = OcctKernel::new();
        let box_h = make_10mm_box_for_local_feature_test(&mut kernel);
        let result = kernel.fillet_with_history(box_h.id, radius);
        match &result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("fillet") && msg.contains("radius"),
                    "fillet {label}: error should mention 'fillet' and 'radius', got: {msg}"
                );
            }
            Err(other) => panic!("fillet {label}: expected OperationFailed, got {:?}", other),
            Ok(_) => {
                panic!("fillet {label}: expected error for {label}-radius fillet_with_history")
            }
        }
    }
}

#[test]
fn chamfer_with_history_invalid_distance_returns_error() {
    let cases: &[(&str, f64)] = &[
        ("zero", 0.0),
        ("negative", -1.0e-3),
        ("NaN", f64::NAN),
        ("infinity", f64::INFINITY),
    ];
    for &(label, distance) in cases {
        let mut kernel = OcctKernel::new();
        let box_h = make_10mm_box_for_local_feature_test(&mut kernel);
        let result = kernel.chamfer_with_history(box_h.id, distance);
        match &result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("chamfer") && msg.contains("distance"),
                    "chamfer {label}: error should mention 'chamfer' and 'distance', got: {msg}"
                );
            }
            Err(other) => panic!("chamfer {label}: expected OperationFailed, got {:?}", other),
            Ok(_) => panic!(
                "chamfer {label}: expected error for {label}-distance chamfer_with_history"
            ),
        }
    }
}

// --- Rotate NaN/infinity/zero-axis rejection tests (step-3) ---

#[test]
fn execute_rotate_nan_angle_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [0.0, 0.0, 1.0],
        angle_rad: f64::NAN,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN angle in rotate"),
    }
}

#[test]
fn execute_rotate_infinity_axis_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [f64::INFINITY, 0.0, 0.0],
        angle_rad: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity axis in rotate"),
    }
}

#[test]
fn execute_rotate_zero_axis_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [0.0, 0.0, 0.0],
        angle_rad: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for zero-length axis in rotate"),
    }
}

#[test]
fn execute_rotate_near_zero_axis_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // A near-zero axis (mag_sq = 1e-34, non-zero but physically meaningless)
    // should be rejected just like exact zero
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [1e-17, 0.0, 0.0],
        angle_rad: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for near-zero-length axis in rotate"),
    }
}

// --- Near-degenerate axis rejection tests (task-311 step-13) ---

#[test]
fn rotate_near_degenerate_axis_rejected() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // axis=[1e-10, 0, 0] has mag_sq=1e-20, physically meaningless (0.1 nanometer)
    // but above the current threshold of f64::EPSILON^2 ≈ 4.9e-32
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [1e-10, 0.0, 0.0],
        angle_rad: std::f64::consts::FRAC_PI_4,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("zero"),
                "error should mention 'zero', got: {msg}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for near-degenerate axis [1e-10, 0, 0] in rotate"),
    }
}

#[test]
fn rotate_around_near_degenerate_axis_rejected() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // axis=[0, 1e-8, 0] has mag_sq=1e-16, physically meaningless (10 nanometers)
    // but above the current threshold of f64::EPSILON^2 ≈ 4.9e-32
    let result = kernel.execute(&GeometryOp::RotateAround {
        target: box_h.id,
        point: [5.0, 0.0, 0.0],
        axis: [0.0, 1e-8, 0.0],
        angle_rad: std::f64::consts::FRAC_PI_4,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("zero"),
                "error should mention 'zero', got: {msg}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => {
            panic!("expected error for near-degenerate axis [0, 1e-8, 0] in rotate_around")
        }
    }
}

// --- Error message quality regression tests (step-7) ---

#[test]
fn translate_error_message_mentions_finite() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Translate {
        target: box_h.id,
        dx: f64::NAN,
        dy: 0.0,
        dz: 0.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.to_lowercase().contains("finite"),
                "translate error should mention 'finite', got: {msg}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn rotate_error_message_mentions_axis_or_finite() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Rotate {
        target: box_h.id,
        axis: [0.0, 0.0, 0.0],
        angle_rad: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("axis") || lower.contains("zero"),
                "rotate zero-axis error should mention 'axis' or 'zero', got: {msg}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error"),
    }
}

// --- Fillet too-large radius regression test (step-5) ---

#[test]
fn execute_fillet_radius_too_large_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Radius 100.0 is much larger than any edge of the 10x10x10 box
    let result = kernel.execute(&GeometryOp::Fillet {
        target: box_h.id,
        edges: vec![],
        radius: Value::Real(100.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for too-large fillet radius"),
    }
}

// --- Translate NaN/infinity rejection tests (step-1) ---

#[test]
fn execute_translate_nan_dx_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Translate {
        target: box_h.id,
        dx: f64::NAN,
        dy: 0.0,
        dz: 0.0,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for NaN dx in translate"),
    }
}

#[test]
fn execute_translate_infinity_dy_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Translate {
        target: box_h.id,
        dx: 0.0,
        dy: f64::INFINITY,
        dz: 0.0,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for infinity dy in translate"),
    }
}

#[test]
fn execute_translate_neg_infinity_dz_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Translate {
        target: box_h.id,
        dx: 0.0,
        dy: 0.0,
        dz: f64::NEG_INFINITY,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
        Ok(_) => panic!("expected error for neg-infinity dz in translate"),
    }
}
