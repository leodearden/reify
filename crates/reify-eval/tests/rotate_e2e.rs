//! End-to-end regression tests for rotate() and rotate_around() geometry operations.
//!
//! Verifies that the eval side correctly reads `ax/ay/az` arg names (matching
//! what the compiler emits in `geometry_transform.rs`) and that rotate/rotate_around
//! calls actually reach the geometry kernel with the correct axis and angle values.
//!
//! Before the fix, `compile_geometry_op` read `axis_x/axis_y/axis_z` while the
//! compiler emitted `ax/ay/az` — causing the Rotate/RotateAround realization to be
//! silently dropped (ops.len() == 1, only the Cylinder, not 2).

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

// ── step-1 (RED) ─────────────────────────────────────────────────────────────

/// `rotate()` with an explicit degree unit (`90deg`) should forward a `Rotate`
/// geometry op to the kernel with the correct axis and angle_rad ≈ π/2.
///
/// Before the fix this fails: `compile_geometry_op` looks up `axis_x` but the
/// compiler emits `ax`, so the Rotate arm returns Err and the realization is
/// silently dropped — ops.len() == 1 (Cylinder only), not 2.
#[test]
fn rotate_with_explicit_deg_unit_realization_lands_in_kernel() {
    let source = r#"
        structure def S {
            let r = rotate(cylinder(30mm, 800mm), 1, 0, 0, 90deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    // Guard: the build itself must not produce hard errors (which would make
    // the kernel-op assertions vacuously true or misleading).
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build produced unexpected errors: {:?}",
        errors
    );

    // Core assertion: both Cylinder and Rotate must reach the kernel.
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Cylinder + Rotate), got {} — \
         Rotate may have been silently dropped due to arg-name mismatch",
        ops.len()
    );

    // Op 0 must be the Cylinder; capture its handle for the target check below.
    assert!(
        matches!(ops[0].op, GeometryOp::Cylinder { .. }),
        "expected GeometryOp::Cylinder at ops[0], got {:?}",
        ops[0].op
    );
    let cylinder_handle = ops[0].result_handle;

    // Op 1 must be Rotate with the correct target, axis, and angle.
    match &ops[1].op {
        GeometryOp::Rotate {
            target,
            axis,
            angle_rad,
        } => {
            assert_eq!(
                *target, cylinder_handle,
                "Rotate target should be the cylinder handle"
            );
            assert!(
                (axis[0] - 1.0).abs() < 1e-12,
                "axis[0] should be 1.0, got {}",
                axis[0]
            );
            assert!(
                axis[1].abs() < 1e-12,
                "axis[1] should be 0.0, got {}",
                axis[1]
            );
            assert!(
                axis[2].abs() < 1e-12,
                "axis[2] should be 0.0, got {}",
                axis[2]
            );
            assert!(
                (angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
                "angle_rad should be π/2 ({:.6}), got {:.6}",
                std::f64::consts::FRAC_PI_2,
                angle_rad
            );
        }
        other => panic!("expected GeometryOp::Rotate at ops[1], got {:?}", other),
    }
}

/// `rotate_around()` with an explicit degree unit (`90deg`) should forward a
/// `RotateAround` geometry op to the kernel with the correct point, axis, and
/// angle_rad ≈ π/2.
///
/// Before the fix this fails for the same reason as `rotate`: the RotateAround
/// eval arm reads `axis_x/axis_y/axis_z` while the compiler emits `ax/ay/az`.
#[test]
fn rotate_around_with_explicit_deg_unit_realization_lands_in_kernel() {
    let source = r#"
        structure def S {
            let r = rotate_around(cylinder(30mm, 800mm), 0mm, 0mm, 0mm, 0, 0, 1, 90deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    // Guard: no hard errors.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build produced unexpected errors: {:?}",
        errors
    );

    // Core assertion: both Cylinder and RotateAround must reach the kernel.
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Cylinder + RotateAround), got {} — \
         RotateAround may have been silently dropped due to arg-name mismatch",
        ops.len()
    );

    // Op 0 must be the Cylinder; capture its handle.
    assert!(
        matches!(ops[0].op, GeometryOp::Cylinder { .. }),
        "expected GeometryOp::Cylinder at ops[0], got {:?}",
        ops[0].op
    );
    let cylinder_handle = ops[0].result_handle;

    // Op 1 must be RotateAround with the correct target, point, axis, and angle.
    match &ops[1].op {
        GeometryOp::RotateAround {
            target,
            point,
            axis,
            angle_rad,
        } => {
            assert_eq!(
                *target, cylinder_handle,
                "RotateAround target should be the cylinder handle"
            );
            assert!(
                point[0].abs() < 1e-12,
                "point[0] should be 0.0 m, got {}",
                point[0]
            );
            assert!(
                point[1].abs() < 1e-12,
                "point[1] should be 0.0 m, got {}",
                point[1]
            );
            assert!(
                point[2].abs() < 1e-12,
                "point[2] should be 0.0 m, got {}",
                point[2]
            );
            assert!(
                axis[0].abs() < 1e-12,
                "axis[0] should be 0.0, got {}",
                axis[0]
            );
            assert!(
                axis[1].abs() < 1e-12,
                "axis[1] should be 0.0, got {}",
                axis[1]
            );
            assert!(
                (axis[2] - 1.0).abs() < 1e-12,
                "axis[2] should be 1.0, got {}",
                axis[2]
            );
            assert!(
                (angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
                "angle_rad should be π/2 ({:.6}), got {:.6}",
                std::f64::consts::FRAC_PI_2,
                angle_rad
            );
        }
        other => panic!(
            "expected GeometryOp::RotateAround at ops[1], got {:?}",
            other
        ),
    }
}

/// `rotate()` with a bare numeric angle (no unit suffix) should pass the value
/// through unchanged as radians, **not** convert from degrees.
///
/// This locks in the current contract documented by the NOTE at
/// `geometry_ops.rs:437`: "bare numeric angle is passed through as-is (radians)."
/// `circular_pattern` treats bare numerics as degrees — this test guards against
/// accidentally applying the same degree-conversion to `Rotate` when that
/// follow-up alignment lands.
///
/// Input: `1.5707963267948966` (π/2 as a decimal literal).  Expected
/// `angle_rad ≈ π/2` (pass-through, not `1.5707963267948966 * π/180 ≈ 0.0274`).
#[test]
fn rotate_with_bare_radian_literal_lands_in_kernel() {
    // π/2 written as a bare decimal literal — no unit suffix.
    // The Rotate eval arm passes bare numerics through as-is (radians).
    let source = r#"
        structure def S {
            let r = rotate(cylinder(30mm, 800mm), 1, 0, 0, 1.5707963267948966)
        }
    "#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    // Guard: no hard errors.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build produced unexpected errors: {:?}",
        errors
    );

    // Both Cylinder and Rotate must reach the kernel.
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Cylinder + Rotate), got {}",
        ops.len()
    );

    // Op 1 must be Rotate with angle_rad ≈ π/2 — the bare literal is passed
    // through unchanged (not degree-converted).
    match &ops[1].op {
        GeometryOp::Rotate { angle_rad, .. } => {
            assert!(
                (angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
                "angle_rad should be π/2 ({:.9}) for bare radian input, got {:.9} \
                 (if this is ~0.0274 the eval accidentally converted as degrees)",
                std::f64::consts::FRAC_PI_2,
                angle_rad
            );
        }
        other => panic!("expected GeometryOp::Rotate at ops[1], got {:?}", other),
    }
}
