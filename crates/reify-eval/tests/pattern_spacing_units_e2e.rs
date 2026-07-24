//! End-to-end regression lock for task 5214 — the headline behaviour:
//! a BARE (dimensionless) length-semantic pattern argument must be REJECTED at
//! eval/build, producing a `Severity::Error` diagnostic and DROPPING the op,
//! rather than silently reading the bare number as SI **metres**.
//!
//! Before the fix, `linear_pattern_2d(..., spacing1: 20, ..., spacing2: 20)`
//! silently placed instances 20 SI **metres** apart (1000× a plausible 20 mm
//! pitch) — the root cause of the litter-tray "holes vanish" symptom, where a
//! bare-spacing grid scatters cutting tools hundreds of metres from the plate
//! so the difference-sieve removes ~1 hole per pattern. The eval-layer gate
//! (`eval_named_arg_length`) now fails closed.
//!
//! Error-diagnostic assertion modelled on `mirror_circular_value_forms_e2e.rs`;
//! emitted-op inspection (`operations_ref`) modelled on
//! `arbitrary_pattern_transform_e2e.rs`.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

/// BARE `20` spacings on `linear_pattern_2d` → the op is dropped: at least one
/// `Severity::Error` diagnostic is emitted and NO `LinearPattern2D` op reaches
/// the kernel (it is NOT silently built with 20 SI-metre spacing).
#[test]
fn linear_pattern_2d_bare_spacing_drops_op_with_error() {
    let source = r#"
        structure def BareSpacingGrid {
            let grid = linear_pattern_2d(
                box(10mm, 10mm, 10mm),
                1, 0, 0, 3, 20,
                0, 1, 0, 3, 20
            )
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
        "bare (dimensionless) linear_pattern_2d spacings must produce at least \
         one Error diagnostic; got diagnostics: {:?}",
        result.diagnostics
    );

    let ops = ops_ref.lock().unwrap();
    let pattern_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::LinearPattern2D { .. }))
        .collect();
    assert!(
        pattern_ops.is_empty(),
        "a bare-spacing linear_pattern_2d must be DROPPED, not silently built \
         with 20 SI-metre spacing; emitted LinearPattern2D ops: {:?}",
        pattern_ops.len()
    );
}

/// Build `source` against a mock kernel and return
/// `(error_diagnostic_count, matching_op_count)`, where an op matches when
/// `is_pattern` accepts it. Shared by the 1D `linear_pattern` pair below, whose
/// two cases differ only in the source and the expected counts.
fn build_and_count(source: &str, is_pattern: fn(&GeometryOp) -> bool) -> (usize, usize) {
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
    let op_count = ops.iter().filter(|r| is_pattern(&r.op)).count();
    (error_count, op_count)
}

/// The 1D `linear_pattern` needs its own e2e lock, not just the unit-level one:
/// its spacing is read at a SEPARATE call site from `linear_pattern_2d`'s
/// spacing1/spacing2, so a regression there would slip past the 2D e2e above.
///
/// BARE `20` spacing → at least one `Severity::Error` and NO `LinearPattern` op
/// reaches the kernel; DIMENSIONED `20mm` → zero Errors and exactly one op
/// (the positive control that keeps the rejection case from passing vacuously).
#[test]
fn linear_pattern_1d_bare_spacing_drops_op_dimensioned_builds() {
    let is_linear = |op: &GeometryOp| matches!(op, GeometryOp::LinearPattern { .. });

    let (bare_errors, bare_ops) = build_and_count(
        r#"
        structure def BareSpacingRow {
            let row = linear_pattern(box(10mm, 10mm, 10mm), 1, 0, 0, 3, 20)
        }
        "#,
        is_linear,
    );
    assert!(
        bare_errors > 0,
        "a bare (dimensionless) linear_pattern spacing must produce at least one \
         Error diagnostic; got {bare_errors}"
    );
    assert_eq!(
        bare_ops, 0,
        "a bare-spacing linear_pattern must be DROPPED, not silently built with \
         20 SI-metre spacing; emitted LinearPattern ops: {bare_ops}"
    );

    let (dim_errors, dim_ops) = build_and_count(
        r#"
        structure def DimSpacingRow {
            let row = linear_pattern(box(10mm, 10mm, 10mm), 1, 0, 0, 3, 20mm)
        }
        "#,
        is_linear,
    );
    assert_eq!(
        dim_errors, 0,
        "a dimensioned 20mm linear_pattern spacing must build with zero Error \
         diagnostics; got {dim_errors}"
    );
    assert_eq!(
        dim_ops, 1,
        "a dimensioned linear_pattern must emit exactly one LinearPattern op; \
         got {dim_ops}"
    );
}

/// Positive control / contrast: the SAME grid with DIMENSIONED `20mm` spacings
/// builds cleanly — zero Error diagnostics and exactly one `LinearPattern2D`
/// op reaches the kernel. This guards the rejection test above against a
/// vacuous pass (op absent for an unrelated reason).
#[test]
fn linear_pattern_2d_dimensioned_spacing_builds_op() {
    let source = r#"
        structure def DimSpacingGrid {
            let grid = linear_pattern_2d(
                box(10mm, 10mm, 10mm),
                1, 0, 0, 3, 20mm,
                0, 1, 0, 3, 20mm
            )
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
        "dimensioned 20mm spacings must build with zero Error diagnostics, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let pattern_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::LinearPattern2D { .. }))
        .collect();
    assert_eq!(
        pattern_ops.len(),
        1,
        "dimensioned linear_pattern_2d must emit exactly one LinearPattern2D op, \
         got: {}",
        pattern_ops.len()
    );
}
