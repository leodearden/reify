//! Sweep-family parameter validation and volumes through `OcctKernel::execute`: extrude,
//! revolve, sweep, pipe and loft, on profiles from the `*_for_test` constructors.

#![cfg(has_occt)]

use crate::support::{assert_operation_fails_with, assert_volume_near, circle_torus_profile};
use reify_ir::{GeometryError, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// --- Extrude tests (task-308 step-7 + step-9) ---

#[test]
fn extrude_zero_distance_returns_error() {
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

// --- Loft tests ---

#[test]
fn loft_two_circles_creates_solid() {
    let mut kernel = OcctKernel::new();
    // Create two circle wire profiles at different heights using the fixture.
    // make_circle_wire creates a TopoDS_Wire circle profile.
    let id1 = kernel.store_circle_wire_for_test(10.0, 0.0);
    let id2 = kernel.store_circle_wire_for_test(5.0, 30.0);

    // Loft through both profiles
    let loft_h = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![id1, id2],
        })
        .unwrap();

    // Query volume - should be positive (a cone-like solid)
    let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                v > 100.0,
                "loft volume should be positive and significant, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn loft_one_profile_returns_error() {
    let mut kernel = OcctKernel::new();
    let id1 = kernel.store_circle_wire_for_test(10.0, 0.0);

    let result = kernel.execute(&GeometryOp::Loft {
        profiles: vec![id1],
    });
    assert!(result.is_err(), "loft with 1 profile should fail");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("at least 2"),
        "error should mention 'at least 2', got: {err_msg}"
    );
}

#[test]
fn loft_two_different_circles_cone_like() {
    let mut kernel = OcctKernel::new();
    let id1 = kernel.store_circle_wire_for_test(10.0, 0.0);
    let id2 = kernel.store_circle_wire_for_test(2.0, 20.0);

    let loft_h = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![id1, id2],
        })
        .unwrap();

    let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            // Should be between small cylinder (pi*4*20 ~= 251) and
            // large cylinder (pi*100*20 ~= 6283): a cone-like frustum
            assert!(
                v > 251.0 && v < 6283.0,
                "cone-like frustum volume should be between 251 and 6283, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn loft_four_circles_creates_solid() {
    let mut kernel = OcctKernel::new();
    // Create 4 circle wire profiles at different heights with decreasing radii
    let id1 = kernel.store_circle_wire_for_test(10.0, 0.0);
    let id2 = kernel.store_circle_wire_for_test(8.0, 10.0);
    let id3 = kernel.store_circle_wire_for_test(6.0, 20.0);
    let id4 = kernel.store_circle_wire_for_test(4.0, 30.0);

    // Loft through all 4 profiles
    let loft_h = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![id1, id2, id3, id4],
        })
        .unwrap();

    // Query volume — should fall between the smallest-enclosing cylinder
    // (π·r_min²·h_total = π·16·30 ≈ 1508) and the largest-enclosing
    // cylinder (π·r_max²·h_total = π·100·30 ≈ 9425). Expected OCCT result
    // for this four-circle frustum stack is ≈ 4900, comfortably inside.
    // Pattern mirrors loft_two_different_circles_cone_like.
    let vol = kernel.query(&GeometryQuery::Volume(loft_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                v > 1508.0 && v < 9425.0,
                "loft frustum volume should be between 1508 (smallest cylinder) \
                 and 9425 (largest cylinder), got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

// --- Integration tests ---

#[test]
fn new_ops_tessellate() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 15.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    let mirrored = kernel
        .execute(&GeometryOp::Mirror {
            target: translated.id,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1.0, 0.0, 0.0],
        })
        .unwrap();
    let mesh = kernel.tessellate(mirrored.id, 0.1).unwrap();
    assert!(
        !mesh.vertices.is_empty(),
        "mirrored tessellation should have vertices"
    );
    assert!(
        mesh.indices.len().is_multiple_of(3),
        "indices should be divisible by 3"
    );

    // Loft
    let id1 = kernel.store_circle_wire_for_test(10.0, 0.0);
    let id2 = kernel.store_circle_wire_for_test(5.0, 20.0);
    let loft = kernel
        .execute(&GeometryOp::Loft {
            profiles: vec![id1, id2],
        })
        .unwrap();
    let loft_mesh = kernel.tessellate(loft.id, 0.1).unwrap();
    assert!(
        !loft_mesh.vertices.is_empty(),
        "loft tessellation should have vertices"
    );
    assert!(
        loft_mesh.indices.len().is_multiple_of(3),
        "loft indices divisible by 3"
    );
}

// --- Extrude kernel-path volume tests ---
// These exercise the full OcctKernel::execute() + query() path on circle
// faces stored as kernel handles by the `store_circle_face_for_test` fixture.

#[test]
fn extrude_circle_face_through_kernel() {
    // Store a circle face (radius=5) via the fixture, then extrude via execute().
    // Expected volume: π * r² * h = π * 25 * 10 = 785.4
    let mut kernel = OcctKernel::new();
    let face_id = kernel.store_circle_face_for_test(5.0, 0.0);

    let result = kernel
        .execute(&GeometryOp::Extrude {
            profile: face_id,
            distance: Value::Real(10.0),
        })
        .expect("Extrude through kernel should succeed");

    let vol = kernel
        .query(&GeometryQuery::Volume(result.id))
        .expect("Volume query should succeed");
    let vol_f64 = vol.as_f64().expect("Volume should be numeric");
    let expected = std::f64::consts::PI * 25.0 * 10.0;
    let rel_err = (vol_f64 - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "expected extrude circle volume ≈ {:.2}, got {:.2} (rel_err={:.4})",
        expected,
        vol_f64,
        rel_err
    );
}

#[test]
fn extrude_negative_distance_through_kernel() {
    // Negative distance should produce same volume as positive.
    let mut kernel = OcctKernel::new();

    let face_pos_id = kernel.store_circle_face_for_test(5.0, 0.0);
    let result_pos = kernel
        .execute(&GeometryOp::Extrude {
            profile: face_pos_id,
            distance: Value::Real(10.0),
        })
        .expect("Extrude +Z through kernel should succeed");
    let vol_pos = kernel
        .query(&GeometryQuery::Volume(result_pos.id))
        .expect("Volume query +Z should succeed")
        .as_f64()
        .expect("Volume should be numeric");

    let face_neg_id = kernel.store_circle_face_for_test(5.0, 0.0);
    let result_neg = kernel
        .execute(&GeometryOp::Extrude {
            profile: face_neg_id,
            distance: Value::Real(-10.0),
        })
        .expect("Extrude -Z through kernel should succeed");
    let vol_neg = kernel
        .query(&GeometryQuery::Volume(result_neg.id))
        .expect("Volume query -Z should succeed")
        .as_f64()
        .expect("Volume should be numeric");

    let rel_diff = (vol_pos - vol_neg).abs() / vol_pos;
    assert!(
        rel_diff < 0.01,
        "positive and negative extrude should have same volume, got pos={:.2} neg={:.2}",
        vol_pos,
        vol_neg
    );
}

// --- Revolve kernel volume tests ---

#[test]
fn revolve_circle_face_full_volume() {
    // Pappus' theorem: V = 2πR × A where R = centroid-to-axis distance, A = profile area.
    // Circle face r=5 at offset R=20, revolve around Z axis by 2π → torus volume = 2π²Rr²
    let mut kernel = OcctKernel::new();
    let face_id = circle_torus_profile(&mut kernel, 5.0, 20.0);

    let result = kernel
        .execute(&GeometryOp::Revolve {
            profile: face_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::TAU,
        })
        .expect("Revolve full should succeed");
    let expected = 2.0 * std::f64::consts::PI.powi(2) * 20.0 * 25.0; // 2π²Rr²
    assert_volume_near(&mut kernel, result.id, expected, 0.02, "circle torus full");
}

#[test]
fn revolve_half_angle_half_volume() {
    // Same setup as full volume but angle=π → half torus should be ~50% of full.
    let mut kernel = OcctKernel::new();
    let face_id = circle_torus_profile(&mut kernel, 5.0, 20.0);

    let full = kernel
        .execute(&GeometryOp::Revolve {
            profile: face_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::TAU,
        })
        .expect("Revolve full should succeed");
    let vol_full = kernel
        .query(&GeometryQuery::Volume(full.id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric");

    // Create another face for half revolution
    let face2_id = circle_torus_profile(&mut kernel, 5.0, 20.0);

    let half = kernel
        .execute(&GeometryOp::Revolve {
            profile: face2_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::PI,
        })
        .expect("Revolve half should succeed");
    let vol_half = kernel
        .query(&GeometryQuery::Volume(half.id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric");

    let ratio = vol_half / vol_full;
    assert!(
        (ratio - 0.5).abs() < 0.02,
        "half-angle volume should be ~50% of full, got ratio {:.4} (full={:.2}, half={:.2})",
        ratio,
        vol_full,
        vol_half
    );
}

// --- Sweep tests ---

#[test]
fn sweep_solid_path_returns_error() {
    let mut kernel = OcctKernel::new();
    // Create a box solid as (invalid) path
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Create a circle face as profile
    let profile_id = kernel.store_circle_face_for_test(2.0, 0.0);

    let result = kernel.execute(&GeometryOp::Sweep {
        profile: profile_id,
        path: box_h.id,
    });
    // `make_pipe` downcasts only the path to a wire, so this names the path.
    assert_operation_fails_with(result, "TopoDS::Wire");
}

#[test]
fn revolve_rect_face_torus_volume() {
    // Rect face w=4, h=2, centered at (10, 0, 0) in XZ plane.
    // Pappus: V = 2π × R × A = 2π × 10 × (4×2) = 160π ≈ 502.65
    let mut kernel = OcctKernel::new();
    let face_id = kernel.store_rect_face_at_for_test(4.0, 2.0, 10.0, 0.0, 0.0);

    let result = kernel
        .execute(&GeometryOp::Revolve {
            profile: face_id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::TAU,
        })
        .expect("Revolve rect should succeed");
    let expected = 2.0 * std::f64::consts::PI * 10.0 * (4.0 * 2.0); // 2πR×A = 160π
    assert_volume_near(&mut kernel, result.id, expected, 0.02, "rect torus full");
}

// --- Revolve positive boundary tests ---

#[test]
fn revolve_just_above_axis_threshold_accepted() {
    // axis_dir=[0, 0, 1e-3] has mag_sq=1e-6, well above AXIS_MAG_SQ_MIN=1e-12.
    // Uses Z-direction so the torus profile (offset 20 along X) doesn't
    // intersect the revolve axis.
    let mut kernel = OcctKernel::new();
    let face_id = circle_torus_profile(&mut kernel, 5.0, 20.0);

    let result = kernel.execute(&GeometryOp::Revolve {
        profile: face_id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1e-3],
        angle_rad: std::f64::consts::TAU,
    });
    assert!(
        result.is_ok(),
        "axis mag_sq=1e-6 (above AXIS_MAG_SQ_MIN=1e-12) should be accepted, got {:?}",
        result.err()
    );
}

#[test]
fn revolve_just_above_angle_threshold_accepted() {
    // angle_rad=1e-6 is far above ANGLE_ABS_MIN=1e-30 and also above
    // OCCT's internal Precision::Angular()≈1e-12, so both the Rust
    // validation and the OCCT operation succeed.
    let mut kernel = OcctKernel::new();
    let face_id = circle_torus_profile(&mut kernel, 5.0, 20.0);

    let result = kernel.execute(&GeometryOp::Revolve {
        profile: face_id,
        axis_origin: [0.0, 0.0, 0.0],
        axis_dir: [0.0, 0.0, 1.0],
        angle_rad: 1e-6,
    });
    assert!(
        result.is_ok(),
        "angle 1e-6 (above ANGLE_ABS_MIN=1e-30) should be accepted, got {:?}",
        result.err()
    );
}

#[test]
fn sweep_circle_along_line_creates_pipe() {
    let mut kernel = OcctKernel::new();
    // Circle face profile: r=2.0 at z=0
    let profile_id = kernel.store_circle_face_for_test(2.0, 0.0);
    // Line wire path: (0,0,0) to (0,0,10)
    let path_id = kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: 10.0,
        })
        .expect("line wire")
        .id;

    let pipe_h = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_id,
            path: path_id,
        })
        .unwrap();

    // Volume should be approximately pi*r^2*h = pi*4*10 ≈ 125.66
    let vol = kernel.query(&GeometryQuery::Volume(pipe_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            let expected = std::f64::consts::PI * 4.0 * 10.0;
            let rel_err = (v - expected).abs() / expected;
            assert!(
                rel_err < 0.05,
                "pipe volume should be ≈ {expected:.2}, got {v:.2} (rel_err={rel_err:.4})"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}
