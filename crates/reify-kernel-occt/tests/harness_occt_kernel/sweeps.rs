//! Sweep-family parameter validation and volumes through `OcctKernel::execute`: extrude,
//! revolve and pipe.

#![cfg(has_occt)]

use crate::support::assert_operation_fails_with;
use reify_ir::{GeometryError, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// --- Extrude tests (task-308 step-7 + step-9) ---

#[test]
fn extrude_zero_distance_returns_error() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    // Create a box as a stand-in profile (provides a valid handle)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    // Extrude with zero distance should fail
    let result = kernel.execute(&GeometryOp::Extrude {
        profile: box_h.id,
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
fn extrude_nan_distance_returns_error() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Extrude {
        profile: box_h.id,
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
fn extrude_inf_distance_returns_error() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Extrude {
        profile: box_h.id,
        distance: Value::Real(f64::INFINITY),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite"),
                "expected error message containing 'finite', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for Inf distance, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
fn revolve_zero_angle_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 0.0,
    });
    assert_operation_fails_with(result, "zero");
}

#[test]
fn revolve_nan_params_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: f64::NAN,
    });
    assert_operation_fails_with(result, "finite");
}

#[test]
fn revolve_zero_axis_dir_returns_error() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 0.0],
        angle_rad: std::f64::consts::TAU,
    });
    assert_operation_fails_with(result, "zero");
}

// --- Revolve error specificity tests (task-400) ---

#[test]
fn revolve_nan_axis_origin_error_mentions_origin() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [f64::NAN, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: std::f64::consts::TAU,
    });
    assert_operation_fails_with(result, "axis_origin");
}

#[test]
fn revolve_nan_axis_dir_error_mentions_dir() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, f64::NAN, 0.0],
        angle_rad: std::f64::consts::TAU,
    });
    assert_operation_fails_with(result, "axis_dir");
}

#[test]
fn revolve_inf_angle_error_mentions_angle() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: f64::INFINITY,
    });
    assert_operation_fails_with(result, "angle");
}

#[test]
fn revolve_zero_angle_error_includes_value() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 0.0,
    });
    // Error should include the actual value "0" for debuggability
    assert_operation_fails_with(result, "0");
}

#[test]
fn revolve_zero_axis_error_includes_values() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 0.0],
        angle_rad: std::f64::consts::TAU,
    });
    // Error should include the axis values
    assert_operation_fails_with(result, "0, 0, 0");
}

#[test]
fn revolve_near_zero_angle_rejected() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    // 1e-35 is below C++ threshold (1e-30) and below any reasonable physical angle.
    // Rust should catch this at its layer with a clear error.
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 1e-35,
    });
    assert_operation_fails_with(result, "zero");
}

#[test]
fn revolve_near_degenerate_axis_rejected() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(1.0),
        })
        .unwrap();
    // axis=[1e-8, 0, 0] has mag_sq=1e-16, physically meaningless
    // but above old EPSILON^2 ≈ 4.9e-32; threshold 1e-12 should catch it
    let result = kernel.execute(&GeometryOp::Revolve {
        profile: box_h.id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [1e-8, 0.0, 0.0],
        angle_rad: std::f64::consts::TAU,
    });
    assert_operation_fails_with(result, "zero");
}

// --- Pipe tests (task-324) ---

#[test]
fn kernel_pipe_straight_path_volume_matches_pi_r2_l() {
    // Path: straight line from (0,0,0) to (0,0,0.020), radius=0.002 →
    // Volume ≈ π*r²*L = π*(0.002)²*0.020 ≈ 2.513e-7 m³.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let wire_handle = kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: 0.020,
        })
        .expect("LineSegment execute should succeed");
    let pipe_handle = kernel
        .execute(&GeometryOp::Pipe {
            path: wire_handle.id,
            radius: Value::Real(0.002),
        })
        .expect("Pipe execute should succeed");

    let vol = kernel
        .query(&GeometryQuery::Volume(pipe_handle.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("Volume should be numeric");
    let expected = std::f64::consts::PI * 0.002_f64.powi(2) * 0.020;
    let rel_err = (v - expected).abs() / expected;
    // Direct BRep volume queries are analytic (not tessellation-based),
    // so a straight circular pipe should match the formula to within
    // floating-point noise. The tight tolerance protects against silent
    // unit-conversion regressions (a 1-3% error would previously pass a
    // lax 5% bound).
    assert!(
        rel_err < 1e-6,
        "pipe volume should be ≈ {expected:.3e} m³, got {v:.3e} (rel_err={rel_err:.4e})"
    );
}

#[test]
fn kernel_pipe_non_positive_radius_returns_error() {
    // radius=0 and radius=-1 must both be rejected with "pipe radius"
    // in the error message.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    for r in [0.0, -1.0] {
        let mut kernel = OcctKernel::new();
        let wire_handle = kernel
            .execute(&GeometryOp::LineSegment {
                x1: 0.0,
                y1: 0.0,
                z1: 0.0,
                x2: 0.0,
                y2: 0.0,
                z2: 0.020,
            })
            .expect("LineSegment execute should succeed");
        let result = kernel.execute(&GeometryOp::Pipe {
            path: wire_handle.id,
            radius: Value::Real(r),
        });
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains("pipe radius"),
                    "expected error mentioning 'pipe radius' for r={r}, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for radius={r}, got Ok"),
            Err(other) => panic!("expected OperationFailed for radius={r}, got {:?}", other),
        }
    }
}

#[test]
fn kernel_pipe_arbitrary_orientation_volume_matches_pi_r2_l() {
    // General path-orientation acceptance: a straight LineSegment in ANY
    // direction, piped with radius r, must yield a RIGHT circular cylinder
    // whose analytic BRep volume equals π·r²·L exactly (rel_err < 1e-6),
    // mirroring the +Z case kernel_pipe_straight_path_volume_matches_pi_r2_l.
    //
    // Covers +X, +Y, -Z, a (1,1,1) diagonal, and a NON-origin +X segment.
    // Before the oriented-profile rewire the profile is an XY circle at the
    // origin, so every in-plane (non-perpendicular) case sweeps a
    // degenerate/offset solid and fails this assertion (RED).
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let r = 0.002_f64;
    // (label, [x1, y1, z1, x2, y2, z2])
    let cases: &[(&str, [f64; 6])] = &[
        ("+X", [0.0, 0.0, 0.0, 0.020, 0.0, 0.0]),
        ("+Y", [0.0, 0.0, 0.0, 0.0, 0.020, 0.0]),
        ("-Z", [0.0, 0.0, 0.0, 0.0, 0.0, -0.020]),
        ("diagonal (1,1,1)", [0.0, 0.0, 0.0, 0.010, 0.010, 0.010]),
        ("non-origin +X", [0.005, 0.0, 0.0, 0.025, 0.0, 0.0]),
    ];
    for (label, c) in cases {
        let [x1, y1, z1, x2, y2, z2] = *c;
        let length = ((x2 - x1).powi(2) + (y2 - y1).powi(2) + (z2 - z1).powi(2)).sqrt();
        let mut kernel = OcctKernel::new();
        let wire_handle = kernel
            .execute(&GeometryOp::LineSegment {
                x1,
                y1,
                z1,
                x2,
                y2,
                z2,
            })
            .unwrap_or_else(|e| {
                panic!("{label}: LineSegment execute should succeed, got {e:?}")
            });
        let pipe_handle = kernel
            .execute(&GeometryOp::Pipe {
                path: wire_handle.id,
                radius: Value::Real(r),
            })
            .unwrap_or_else(|e| panic!("{label}: Pipe execute should succeed, got {e:?}"));
        let vol = kernel
            .query(&GeometryQuery::Volume(pipe_handle.id))
            .unwrap_or_else(|e| panic!("{label}: Volume query should succeed, got {e:?}"));
        let v = vol.as_f64().expect("Volume should be numeric");
        assert!(v > 0.0, "{label}: pipe volume must be positive, got {v}");
        let expected = std::f64::consts::PI * r.powi(2) * length;
        let rel_err = (v - expected).abs() / expected;
        assert!(
            rel_err < 1e-6,
            "{label}: pipe volume should be ≈ {expected:.3e} m³, got {v:.3e} (rel_err={rel_err:.4e})"
        );
    }
}

#[test]
fn kernel_pipe_xy_arc_quarter_torus_volume() {
    // A quarter-circle spine in the XY plane (bend radius R=0.010, start at
    // (0.010,0,0), start-tangent +Y) piped with tube radius r=0.002 sweeps
    // an exact torus segment: a planar circular spine has constant binormal
    // (no Frenet twist), so by Pappus's theorem V = π·r²·R·θ exactly
    // (R>r ⇒ no self-intersection) ≈ 1.9739e-7 m³.
    //
    // This case also guards the wire_start_point plumbing: the arc does NOT
    // start at the origin, so a start-point-ignoring profile would sweep
    // from the wrong location. RED until the oriented-profile rewire lands.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let r = 0.002_f64;
    let bend_r = 0.010_f64;
    let theta = std::f64::consts::FRAC_PI_2;
    let mut kernel = OcctKernel::new();
    let wire_handle = kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: bend_r,
            start_angle: 0.0,
            end_angle: theta,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc execute should succeed");
    let pipe_handle = kernel
        .execute(&GeometryOp::Pipe {
            path: wire_handle.id,
            radius: Value::Real(r),
        })
        .expect("Pipe execute should succeed for XY arc");
    let vol = kernel
        .query(&GeometryQuery::Volume(pipe_handle.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("Volume should be numeric");
    assert!(v > 0.0, "arc pipe volume must be positive, got {v}");
    let expected = std::f64::consts::PI * r.powi(2) * bend_r * theta;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "quarter-torus pipe volume should be ≈ {expected:.4e} m³ (π·r²·R·θ), got {v:.4e} (rel_err={rel_err:.4e})"
    );
}
