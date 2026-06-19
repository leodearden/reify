//! End-to-end tests for mirror and circular_pattern with value-form args (Plane/Axis).
//!
//! Step-5 (RED → GREEN after step-6): mirror value form, back-compat, wrong-variant rejection.
//! Step-7 (RED → GREEN after step-8): circular_pattern value form, back-compat, wrong-variant.
//!
//! RED state for mirror tests (step-5): mirror(box, plane_xy(0mm)) fails compile with
//! "expects 7 arguments" — parse_and_compile panics, making those tests RED.
//! GREEN after step-6: compiler accepts 2-arg form; eval decodes the Plane value.
//!
//! RED state for circular_pattern tests (step-7): circular_pattern(box, axis_z(...), 4, 60deg)
//! fails compile with "expects 9 arguments" — parse_and_compile panics.
//! GREEN after step-8: compiler accepts 4-arg form; eval decodes the Axis value.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

// ── step-5: mirror consumer tests ─────────────────────────────────────────────

/// (a) Value form: mirror(box, plane_xy(0mm)) builds with zero Error diagnostics
/// and emits exactly one Mirror op with plane_origin ≈ [0,0,0] and plane_normal ≈ [0,0,1].
///
/// RED today: parse_and_compile panics — 2-arg mirror fails compile ("expects 7 arguments").
/// GREEN after step-6.
#[test]
fn mirror_value_form_plane_xy_builds_and_emits_correct_mirror_op() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_xy(0mm))
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for mirror value form, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::Mirror { .. }))
        .collect();
    assert_eq!(
        mirror_ops.len(),
        1,
        "expected exactly one Mirror op, got {}",
        mirror_ops.len()
    );

    match &mirror_ops[0].op {
        GeometryOp::Mirror {
            plane_origin,
            plane_normal,
            ..
        } => {
            assert!(
                plane_origin[0].abs() < 1e-9,
                "plane_origin[0] should be 0, got {}",
                plane_origin[0]
            );
            assert!(
                plane_origin[1].abs() < 1e-9,
                "plane_origin[1] should be 0, got {}",
                plane_origin[1]
            );
            assert!(
                plane_origin[2].abs() < 1e-9,
                "plane_origin[2] should be 0 (plane_xy at z=0mm), got {}",
                plane_origin[2]
            );
            assert!(
                plane_normal[0].abs() < 1e-9,
                "plane_normal[0] should be 0, got {}",
                plane_normal[0]
            );
            assert!(
                plane_normal[1].abs() < 1e-9,
                "plane_normal[1] should be 0, got {}",
                plane_normal[1]
            );
            assert!(
                (plane_normal[2] - 1.0).abs() < 1e-9,
                "plane_normal[2] should be 1.0 (Z-axis for plane_xy), got {}",
                plane_normal[2]
            );
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// (b) Back-compat: legacy 7-arg scalar form mirror(box, 0,0,0, 1,0,0) still builds
/// without errors and emits Mirror with plane_normal ≈ [1,0,0].
///
/// GREEN before and after step-6 (back-compat must hold).
#[test]
fn mirror_scalar_back_compat_emits_correct_plane() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, 0mm, 0mm, 0mm, 1, 0, 0)
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for back-compat scalar mirror, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::Mirror { .. }))
        .collect();
    assert_eq!(mirror_ops.len(), 1, "expected exactly one Mirror op");

    match &mirror_ops[0].op {
        GeometryOp::Mirror { plane_normal, .. } => {
            assert!(
                (plane_normal[0] - 1.0).abs() < 1e-9,
                "plane_normal[0] should be 1.0, got {}",
                plane_normal[0]
            );
            assert!(
                plane_normal[1].abs() < 1e-9,
                "plane_normal[1] should be 0, got {}",
                plane_normal[1]
            );
            assert!(
                plane_normal[2].abs() < 1e-9,
                "plane_normal[2] should be 0, got {}",
                plane_normal[2]
            );
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// (c) Wrong-variant rejection (H signal): mirror(box, axis_z(...)) must produce an
/// Error diagnostic because axis_z yields Value::Axis not Value::Plane.  No Mirror op.
///
/// RED today: parse_and_compile panics — 2-arg mirror fails compile ("expects 7 arguments").
/// GREEN after step-6: 2-arg compiles (value form); eval rejects Axis → Error diagnostic.
#[test]
fn mirror_wrong_variant_axis_rejected_with_error_diagnostic() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, axis_z(point3(0mm, 0mm, 0mm)))
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic for wrong-variant axis→mirror, got: {:?}",
        result.diagnostics
    );

    let ops = ops_ref.lock().unwrap();
    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::Mirror { .. }))
        .collect();
    assert!(
        mirror_ops.is_empty(),
        "expected NO Mirror op when Axis is passed where Plane is required, got {} Mirror op(s)",
        mirror_ops.len()
    );
}

// ── step-7: circular_pattern consumer tests ───────────────────────────────────

/// (a) Value form: circular_pattern(box, axis_z(point3(0,0,0)), 6, 60deg) builds
/// with zero Error diagnostics and emits exactly one CircularPattern with
/// axis_dir ≈ [0,0,1] (within 1e-9) and count == 6.
///
/// RED today: parse_and_compile panics — 4-arg circular_pattern fails compile
/// ("expects 9 arguments"). GREEN after step-8.
#[test]
fn circular_pattern_value_form_axis_z_emits_correct_op() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(0mm, 0mm, 0mm)), 6, 60deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for circular_pattern value form, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let cp_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::CircularPattern { .. }))
        .collect();
    assert_eq!(
        cp_ops.len(),
        1,
        "expected exactly one CircularPattern op, got {}",
        cp_ops.len()
    );

    match &cp_ops[0].op {
        GeometryOp::CircularPattern {
            axis_origin,
            axis_dir,
            count,
            ..
        } => {
            assert!(
                axis_origin[0].abs() < 1e-9,
                "axis_origin[0] should be 0, got {}",
                axis_origin[0]
            );
            assert!(
                axis_origin[1].abs() < 1e-9,
                "axis_origin[1] should be 0, got {}",
                axis_origin[1]
            );
            assert!(
                axis_origin[2].abs() < 1e-9,
                "axis_origin[2] should be 0, got {}",
                axis_origin[2]
            );
            assert!(
                axis_dir[0].abs() < 1e-9,
                "axis_dir[0] should be 0, got {}",
                axis_dir[0]
            );
            assert!(
                axis_dir[1].abs() < 1e-9,
                "axis_dir[1] should be 0, got {}",
                axis_dir[1]
            );
            assert!(
                (axis_dir[2] - 1.0).abs() < 1e-9,
                "axis_dir[2] should be 1.0 (Z-axis), got {}",
                axis_dir[2]
            );
            assert_eq!(*count, 6, "count should be 6, got {}", count);
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}

/// (b) Back-compat: legacy 9-arg scalar form circular_pattern(box, 0,0,0, 0,0,1, 6, 60deg)
/// still builds without errors and emits CircularPattern with count==6.
///
/// GREEN before and after step-8 (back-compat must hold).
#[test]
fn circular_pattern_scalar_back_compat_emits_correct_op() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, 0, 0, 0, 0, 0, 1, 6, 60deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for back-compat scalar circular_pattern, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let cp_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::CircularPattern { .. }))
        .collect();
    assert_eq!(cp_ops.len(), 1, "expected exactly one CircularPattern op");

    match &cp_ops[0].op {
        GeometryOp::CircularPattern { count, .. } => {
            assert_eq!(*count, 6, "count should be 6, got {}", count);
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}

/// (c) Wrong-variant rejection: circular_pattern(box, plane_xy(0mm), 6, 60deg) must
/// produce an Error diagnostic because plane_xy yields Value::Plane, not Value::Axis.
/// No CircularPattern op should be emitted.
///
/// RED today: parse_and_compile panics — 4-arg circular_pattern fails compile.
/// GREEN after step-8: 4-arg compiles; eval rejects Plane → Error diagnostic.
#[test]
fn circular_pattern_wrong_variant_plane_rejected_with_error_diagnostic() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, plane_xy(0mm), 6, 60deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic for wrong-variant plane→circular_pattern, got: {:?}",
        result.diagnostics
    );

    let ops = ops_ref.lock().unwrap();
    let cp_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::CircularPattern { .. }))
        .collect();
    assert!(
        cp_ops.is_empty(),
        "expected NO CircularPattern op when Plane is passed where Axis is required, got {} op(s)",
        cp_ops.len()
    );
}
