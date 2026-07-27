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

/// (b) Back-compat: legacy 9-arg scalar form
/// circular_pattern(box, 0mm,0mm,0mm, 0,0,1, 6, 60deg) still builds without
/// errors and emits CircularPattern with count==6.
///
/// GREEN before and after step-8 (back-compat must hold). The axis ORIGIN is
/// dimensioned since task 5350 gated it as a Length; the axis DIRECTION stays
/// a bare dimensionless unit vector.
#[test]
fn circular_pattern_scalar_back_compat_emits_correct_op() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, 0mm, 0mm, 0mm, 0, 0, 1, 6, 60deg)
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

// ── task 5350: scalar-form axis-origin units lock ─────────────────────────────

/// Build `source` against a mock kernel and return
/// `(error_diagnostic_count, circular_pattern_ops)`. Shared by the bare-origin /
/// mm-origin pair below, which differ only in the source and expected counts.
/// Modelled on `pattern_spacing_units_e2e.rs`'s `build_and_count`, but returns
/// the ops themselves so the positive control can inspect `axis_origin`.
fn build_circular_ops(source: &str) -> (usize, Vec<GeometryOp>) {
    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let ops = ops_ref.lock().unwrap();
    let circular_ops: Vec<GeometryOp> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::CircularPattern { .. }))
        .map(|r| r.op.clone())
        .collect();
    (error_count, circular_ops)
}

/// The headline behaviour of task 5350, locked at the outermost seam: a BARE
/// (dimensionless) axis origin in the 9-arg scalar form must be REJECTED, so the
/// op is DROPPED rather than silently placing the rotation axis 12 SI **metres**
/// out (1000× a plausible 12 mm offset).
///
/// The unit tests in `geometry_ops/tests.rs` hand-build `CompiledExpr` fixtures;
/// this drives the real parser and unit system, so it would catch a regression
/// introduced anywhere between the `.ri` surface and the kernel call.
#[test]
fn circular_pattern_scalar_bare_origin_drops_op_with_error() {
    let (error_count, circular_ops) = build_circular_ops(
        r#"
        structure def BareOriginRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, 12, 0, 0, 0, 0, 1, 6, 60deg)
        }
        "#,
    );

    assert!(
        error_count > 0,
        "a bare (dimensionless) circular_pattern axis origin must produce at \
         least one Error diagnostic; got {error_count}"
    );
    assert!(
        circular_ops.is_empty(),
        "a bare-origin circular_pattern must be DROPPED, not silently built with \
         a 12 SI-metre axis origin; emitted CircularPattern ops: {:?}",
        circular_ops
    );
}

/// The positive control that keeps the rejection case above from passing
/// vacuously: a DIMENSIONED `12mm, 34mm, 56mm` origin builds with zero Errors
/// and reaches the kernel as `[0.012, 0.034, 0.056]`, not `[12, 34, 56]`.
///
/// The three components are DISTINCT so this also pins the ox/oy/oz →
/// `axis_origin` COMPONENT ORDERING end to end: the eval layer reads the triple
/// outside its `f64_arg` closure and assembles the array separately, so a
/// transposition to `[ox, oz, oy]` is a live regression that an all-zero (or
/// single-non-zero) origin could not detect.
///
/// A tolerance rather than `==` because the parser computes `12 * 1e-3`; the f64
/// error is at most ~1 ulp (order 1e-18 absolute at this magnitude), so `1e-12`
/// clears it by six orders of magnitude while staying far tighter than the 1000×
/// defect being guarded against.
#[test]
fn circular_pattern_scalar_mm_origin_builds_op() {
    let (error_count, circular_ops) = build_circular_ops(
        r#"
        structure def MmOriginRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, 12mm, 34mm, 56mm, 0, 0, 1, 6, 60deg)
        }
        "#,
    );

    assert_eq!(
        error_count, 0,
        "a dimensioned circular_pattern axis origin must build with zero Error \
         diagnostics; got {error_count}"
    );
    assert_eq!(
        circular_ops.len(),
        1,
        "expected exactly one CircularPattern op to reach the kernel, got {:?}",
        circular_ops
    );
    match &circular_ops[0] {
        GeometryOp::CircularPattern { axis_origin, .. } => {
            // Component-wise AND in order: ox→[0], oy→[1], oz→[2].
            for (i, (label, expected)) in
                [("12mm", 0.012), ("34mm", 0.034), ("56mm", 0.056)]
                    .into_iter()
                    .enumerate()
            {
                assert!(
                    (axis_origin[i] - expected).abs() < 1e-12,
                    "{label} must reach the kernel as {expected} SI metres at \
                     axis_origin[{i}] (NOT {} m, and NOT permuted into another \
                     component); got axis_origin = {:?}",
                    expected * 1000.0,
                    axis_origin
                );
            }
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}
