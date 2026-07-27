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

// ── Orientation overload e2e tests (task γ, #4166) ───────────────────────────

/// G2 signal: rotate(box, orient_axis_angle(z, 90deg)) emits a Rotate op with
/// axis ≈ [0,0,1] and angle_rad ≈ π/2 — identical to rotate(box, 0,0,1, 90deg).
///
/// Both forms must produce Rotate ops with axis/angle equality within tolerance,
/// realizing the "bbox equals the axis+angle form" signal at op level.
#[test]
fn rotate_orientation_overload_equals_axis_angle_form() {
    let orientation_src = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let r = rotate(b, orient_axis_angle(vec3(0.0, 0.0, 1.0), 90deg))
        }
    "#;
    let axis_angle_src = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let r = rotate(b, 0, 0, 1, 90deg)
        }
    "#;

    // Build orientation form
    let compiled_orient = parse_and_compile(orientation_src);
    let checker_o = MockConstraintChecker::new();
    let kernel_o = MockGeometryKernel::new();
    let ops_ref_o = kernel_o.operations_ref();
    let mut engine_o = Engine::new(Box::new(checker_o), Some(Box::new(kernel_o)));
    let result_o: BuildResult = engine_o.build(&compiled_orient, ExportFormat::Step);

    let errors_o: Vec<_> = result_o
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_o.is_empty(),
        "orientation form: unexpected errors: {:?}",
        errors_o
    );

    let ops_o = ops_ref_o.lock().unwrap();
    assert_eq!(
        ops_o.len(),
        2,
        "orientation form: expected 2 ops (Box + Rotate), got {}",
        ops_o.len()
    );

    // Build axis+angle form
    let compiled_aa = parse_and_compile(axis_angle_src);
    let checker_a = MockConstraintChecker::new();
    let kernel_a = MockGeometryKernel::new();
    let ops_ref_a = kernel_a.operations_ref();
    let mut engine_a = Engine::new(Box::new(checker_a), Some(Box::new(kernel_a)));
    let _result_a: BuildResult = engine_a.build(&compiled_aa, ExportFormat::Step);

    let ops_a = ops_ref_a.lock().unwrap();
    assert_eq!(ops_a.len(), 2, "axis+angle form: expected 2 ops, got {}", ops_a.len());

    // Extract both Rotate ops and compare
    let (orient_axis, orient_angle) = match &ops_o[1].op {
        GeometryOp::Rotate { axis, angle_rad, .. } => (*axis, *angle_rad),
        other => panic!("orientation form: expected Rotate at ops[1], got {:?}", other),
    };
    let (aa_axis, aa_angle) = match &ops_a[1].op {
        GeometryOp::Rotate { axis, angle_rad, .. } => (*axis, *angle_rad),
        other => panic!("axis+angle form: expected Rotate at ops[1], got {:?}", other),
    };

    // Orientation form: axis ≈ [0,0,1], angle ≈ π/2
    assert!(orient_axis[0].abs() < 1e-12, "orient axis[0] should be 0, got {}", orient_axis[0]);
    assert!(orient_axis[1].abs() < 1e-12, "orient axis[1] should be 0, got {}", orient_axis[1]);
    assert!(
        (orient_axis[2] - 1.0).abs() < 1e-12,
        "orient axis[2] should be 1, got {}",
        orient_axis[2]
    );
    assert!(
        (orient_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "orient angle should be π/2, got {}",
        orient_angle
    );

    // Both forms agree
    assert!(
        (orient_axis[0] - aa_axis[0]).abs() < 1e-12,
        "axis[0] mismatch: orient={} aa={}",
        orient_axis[0],
        aa_axis[0]
    );
    assert!(
        (orient_axis[1] - aa_axis[1]).abs() < 1e-12,
        "axis[1] mismatch: orient={} aa={}",
        orient_axis[1],
        aa_axis[1]
    );
    assert!(
        (orient_axis[2] - aa_axis[2]).abs() < 1e-12,
        "axis[2] mismatch: orient={} aa={}",
        orient_axis[2],
        aa_axis[2]
    );
    assert!(
        (orient_angle - aa_angle).abs() < 1e-9,
        "angle mismatch: orient={} aa={}",
        orient_angle,
        aa_angle
    );
}

/// Malformed orientation (zero-length axis → Undef after orient_axis_angle) →
/// no GeometryOp::Rotate in the recorded ops (op dropped) and non-empty diagnostics.
#[test]
fn rotate_orientation_malformed_drops_op() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let r = rotate(b, orient_axis_angle(vec3(0.0, 0.0, 0.0), 90deg))
        }
    "#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    let has_rotate = ops.iter().any(|r| matches!(r.op, GeometryOp::Rotate { .. }));
    assert!(!has_rotate, "malformed orientation: Rotate op must be dropped");
    assert!(
        !result.diagnostics.is_empty(),
        "malformed orientation: expected at least one diagnostic"
    );
}
