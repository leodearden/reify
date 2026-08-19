//! End-to-end leaf signal for task 5623 (PRD decision D8) — the R1 sweep's
//! headline behaviour, phrased against real `.ri` SOURCE TEXT rather than
//! hand-built `CompiledGeometryOp`s.
//!
//! A BARE (dimensionless) length-semantic argument to `translate`,
//! `rotate_around`, `revolve`, `line_segment`, `arc` or `helix` must be
//! REJECTED at build, dropping the op with a `Severity::Error` and offering the
//! `pass a dimensioned length such as \`5mm\`` migration hint — rather than
//! silently reading the bare number as SI **metres** (the `5` vs `5mm` = 1000×
//! hazard that task 5214 closed for pattern spacing and 5350 for the
//! circular-pattern axis origin).
//!
//! Two things here are structurally unreachable from the unit tests in
//! `geometry_ops/tests.rs`, which build their `CompiledGeometryOp`s by hand:
//!
//! 1. The COMPILER DESUGARING paths. `cylinder_centered`, `rounded_box`,
//!    `rounded_rect` and `revolve_full` synthesize arguments into slots this
//!    task has just gated. If any synthesized literal were a bare `Real`, every
//!    call of an otherwise-correct builtin would start failing — the single
//!    highest-risk unknown in this task. `desugared_builtins_build_clean` is
//!    the cross-check that dependency 5742's retyping actually covers our
//!    gates.
//! 2. Whether the whole parse → compile → build pipeline agrees. A gate that
//!    landed at the wrong layer would show up here and nowhere else.
//!
//! Modelled on `pattern_spacing_units_e2e.rs` (task 5214's own leaf signal):
//! error-diagnostic assertion plus `operations_ref` op inspection.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source, parse_and_compile,
};

/// The migration hint minted by `arg_acceptance::length_spec` — the single
/// piece of user-facing guidance this whole sweep exists to deliver. Asserted
/// literally so a reworded hint cannot silently stop reaching `.ri` authors.
const LENGTH_HINT: &str = "pass a dimensioned length such as `5mm`";

/// Build `compiled` against a mock kernel; return the diagnostics alongside the
/// count of ops matching `is_target`.
fn build_and_probe(
    compiled: &reify_compiler::CompiledModule,
    is_target: fn(&GeometryOp) -> bool,
) -> (Vec<reify_core::Diagnostic>, usize) {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(compiled, ExportFormat::Step);
    let op_count = ops_ref.lock().unwrap().iter().filter(|r| is_target(&r.op)).count();
    (result.diagnostics, op_count)
}

/// One gated builtin's leaf signal: the BARE form drops the op with an Error
/// naming the argument and `Length` plus the migration hint, and the
/// DIMENSIONED form builds cleanly and emits exactly one op.
///
/// The two halves are inseparable. Without the dimensioned control the
/// rejection assertion could pass vacuously (op absent because the builtin
/// stopped lowering); without the exact-wording assertion the rejection could
/// pass for an unrelated build failure. Only THIS gate mints the hint.
#[track_caller]
fn assert_length_gate(
    builtin: &str,
    arg: &str,
    bare_source: &str,
    dimensioned_source: &str,
    is_target: fn(&GeometryOp) -> bool,
) {
    // BARE — `compile_source` rather than `parse_and_compile`: whether a bare
    // arg is ALSO rejected at compile time is task 5662/eta's business, and
    // this file must keep testing the eval-layer gate either way. The wording
    // assertions below are what keep that non-vacuous.
    let (diags, op_count) = build_and_probe(&compile_source(bare_source), is_target);

    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains(arg) && d.message.contains("Length")),
        "a bare (dimensionless) {arg} for {builtin} must produce an Error-severity \
         diagnostic naming the argument and the Length requirement; got: {:?}",
        diags
    );
    assert!(
        diags.iter().any(|d| d.message.contains(LENGTH_HINT)),
        "the rejection for {builtin}'s bare {arg} must carry the migration hint \
         {LENGTH_HINT:?} so the author knows how to fix it; got: {:?}",
        diags
    );
    assert_eq!(
        op_count, 0,
        "a bare {arg} must DROP the {builtin} op, not silently build it with the \
         bare number read as SI metres; emitted ops: {op_count}"
    );

    // DIMENSIONED control — keeps the STRICT `parse_and_compile`, so this also
    // proves the gate does not fire on valid code.
    let (dim_diags, dim_ops) = build_and_probe(&parse_and_compile(dimensioned_source), is_target);
    let dim_errors: Vec<_> = dim_diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        dim_errors.is_empty(),
        "the dimensioned {builtin} form must build with zero Error diagnostics; \
         got: {:?}",
        dim_errors
    );
    assert!(
        !dim_diags.iter().any(|d| d.message.contains(LENGTH_HINT)),
        "the dimensioned {builtin} form must emit NO units diagnostic at all; \
         got: {:?}",
        dim_diags
    );
    assert_eq!(
        dim_ops, 1,
        "the dimensioned {builtin} form must emit exactly one op; got {dim_ops}"
    );
}

// ---------------------------------------------------------------------------
// The six gated builtins, one leaf signal each
// ---------------------------------------------------------------------------

#[test]
fn translate_bare_component_drops_op_dimensioned_builds() {
    assert_length_gate(
        "translate",
        "dx",
        r#"structure def BareTranslate {
            let s = translate(box(10mm, 10mm, 10mm), 5, 0, 0)
        }"#,
        r#"structure def DimTranslate {
            let s = translate(box(10mm, 10mm, 10mm), 5mm, 0mm, 0mm)
        }"#,
        |op| matches!(op, GeometryOp::Translate { .. }),
    );
}

#[test]
fn rotate_around_bare_pivot_drops_op_dimensioned_builds() {
    // The axis `0, 0, 1` and the angle stay BARE in BOTH forms — they are a
    // dimensionless unit vector and PRD 3's angle respectively, and the
    // dimensioned control's zero-diagnostic assertion is what pins that.
    assert_length_gate(
        "rotate_around",
        "px",
        r#"structure def BareRotateAround {
            let s = rotate_around(box(10mm, 10mm, 10mm), 5, 0, 0, 0, 0, 1, 1.5707963267948966)
        }"#,
        r#"structure def DimRotateAround {
            let s = rotate_around(box(10mm, 10mm, 10mm), 5mm, 0mm, 0mm, 0, 0, 1, 1.5707963267948966)
        }"#,
        |op| matches!(op, GeometryOp::RotateAround { .. }),
    );
}

#[test]
fn revolve_bare_axis_origin_drops_op_dimensioned_builds() {
    assert_length_gate(
        "revolve",
        "ox",
        r#"structure def BareRevolve {
            let s = revolve(rectangle(20mm, 10mm), -10, 0, 0, 0.0, 1.0, 0.0, 6.283185307179586)
        }"#,
        r#"structure def DimRevolve {
            let s = revolve(rectangle(20mm, 10mm), -10mm, 0mm, 0mm, 0.0, 1.0, 0.0, 6.283185307179586)
        }"#,
        |op| matches!(op, GeometryOp::Revolve { .. }),
    );
}

#[test]
fn line_segment_bare_endpoint_drops_op_dimensioned_builds() {
    assert_length_gate(
        "line_segment",
        "x1",
        r#"structure def BareLineSegment {
            let w = line_segment(0, 0, 0, 10, 0, 0)
        }"#,
        r#"structure def DimLineSegment {
            let w = line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm)
        }"#,
        |op| matches!(op, GeometryOp::LineSegment { .. }),
    );
}

#[test]
fn arc_bare_center_drops_op_dimensioned_builds() {
    assert_length_gate(
        "arc",
        "cx",
        r#"structure def BareArc {
            let w = arc(0, 0, 0, 10, 0.0, 1.5707963267948966, 0, 0, 1)
        }"#,
        r#"structure def DimArc {
            let w = arc(0mm, 0mm, 0mm, 10mm, 0.0, 1.5707963267948966, 0, 0, 1)
        }"#,
        |op| matches!(op, GeometryOp::Arc { .. }),
    );
}

#[test]
fn helix_bare_radius_drops_op_dimensioned_builds() {
    assert_length_gate(
        "helix",
        "radius",
        r#"structure def BareHelix {
            let w = helix(5, 2, 20)
        }"#,
        r#"structure def DimHelix {
            let w = helix(5mm, 2mm, 20mm)
        }"#,
        |op| matches!(op, GeometryOp::Helix { .. }),
    );
}

// ---------------------------------------------------------------------------
// The VARIADIC curve constructors (task 5658, R2) — same leaf signal, one
// layer further out
//
// `interp` / `bezier` / `nurbs` take an arity-open flat coordinate stream that
// the compiler names positionally (`c0`…`cN`). Only this file can show that the
// DISPLAY names the gate mints (`x1`, `y1`, …) are what a `.ri` author actually
// sees, because the unit tests hand-build the `CompiledGeometryOp` and so never
// exercise parse → compile → build agreement on an arity-open signature.
// ---------------------------------------------------------------------------

#[test]
fn interp_bare_coordinate_drops_op_dimensioned_builds() {
    assert_length_gate(
        "interp",
        "x1",
        r#"structure def BareInterp {
            let w = interp(0, 0, 0, 10, 0, 0, 20, 0, 0)
        }"#,
        r#"structure def DimInterp {
            let w = interp(0mm, 0mm, 0mm, 10mm, 0mm, 0mm, 20mm, 0mm, 0mm)
        }"#,
        |op| matches!(op, GeometryOp::InterpCurve { .. }),
    );
}

// ---------------------------------------------------------------------------
// The desugaring cross-check — the highest-risk unknown in task 5623
// ---------------------------------------------------------------------------

/// Four builtins SYNTHESIZE arguments straight into slots this task just gated,
/// and a hand-built `CompiledGeometryOp` can never reach them:
///
/// | builtin             | synthesized into              | must be            |
/// |---------------------|-------------------------------|--------------------|
/// | `cylinder_centered` | Translate dx/dy (literals)    | LENGTH `Scalar`    |
/// |                     | Translate dz (`height × -0.5`)| LENGTH-preserving  |
/// | `rounded_box`       | Translate dx/dy/dz (binops)   | LENGTH-preserving  |
/// | `rounded_rect`      | Translate dz                  | LENGTH `Scalar`    |
/// | `revolve_full`      | Revolve angle (2π)            | ANGLE, ungated     |
///
/// Task 5742 retyped the literal ones ahead of this gate; the binop ones rely
/// on `Scalar{LENGTH} × Real` preserving LENGTH at eval (the documented
/// INVARIANT at `reify-compiler/src/geometry.rs`, which is why the `-0.5`
/// multiplier must stay a BARE dimensionless `Real` — retyping it would yield
/// `Scalar{AREA}` and our gate would reject it).
///
/// If this test fails, the residual is COMPILER-side and belongs to task 5742,
/// not here.
#[test]
fn desugared_builtins_build_clean() {
    for (builtin, source) in [
        (
            "cylinder_centered",
            r#"structure def CylCentered {
                let s = cylinder_centered(5mm, 20mm)
            }"#,
        ),
        (
            "rounded_box",
            r#"structure def RBox {
                let s = rounded_box(50mm, 30mm, 10mm, 5mm)
            }"#,
        ),
        (
            "rounded_rect",
            r#"structure def RRect {
                let s = rounded_rect(50mm, 30mm, 5mm)
            }"#,
        ),
        (
            "revolve_full",
            r#"structure def RevFull {
                let s = revolve_full(rectangle(20mm, 10mm), -10mm, 0mm, 0mm, 0.0, 1.0, 0.0)
            }"#,
        ),
    ] {
        let (diags, _) = build_and_probe(&parse_and_compile(source), |_| false);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{builtin} desugars into slots gated by task 5623 and must still build \
             with zero Error diagnostics — a failure here means a COMPILER-side \
             synthesized literal reaches a gated slot as a bare Real, which is \
             task 5742's territory, not this task's; got: {:?}",
            errors
        );
        assert!(
            !diags.iter().any(|d| d.message.contains(LENGTH_HINT)),
            "{builtin} must emit NO units diagnostic — its synthesized args are \
             already LENGTH-dimensioned; got: {:?}",
            diags
        );
    }
}

/// The MIXED-DIMENSION arithmetic path, and the one place task 5623 changes
/// user-visible WORDING rather than user-visible acceptance.
///
/// `rounded_box(50mm, 30mm, 10mm, 5)` mixes a LENGTH with a bare `Real` inside
/// the desugared corner arithmetic. `reify-expr`'s `eval_sub` has no
/// `Scalar{LENGTH} - Real` arm, so the expression evaluates to `Value::Undef`.
/// Our gate routes `Undef` to the DISTINCT "unresolved (Undef)" message
/// (`required_length_arg`, PRD decision D10) where the old bare-accepting path
/// reported "non-finite".
///
/// That is a deliberate improvement, not a regression: asserting "missing or
/// non-finite" for a cell that is merely not-yet-resolved is actively
/// misleading. Pinned here because it is the only wording change a `.ri` author
/// can observe without writing a bare literal into a gated slot directly.
#[test]
fn rounded_box_bare_corner_radius_reports_unresolved_not_non_finite() {
    let source = r#"structure def MixedCorner {
        let s = rounded_box(50mm, 30mm, 10mm, 5)
    }"#;

    let (diags, op_count) = build_and_probe(&compile_source(source), |op| {
        matches!(op, GeometryOp::Translate { .. })
    });

    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unresolved") || d.message.contains("Undef")),
        "a bare corner radius makes the desugared corner arithmetic evaluate to \
         Undef, which must be reported as UNRESOLVED rather than as a missing or \
         non-finite argument (PRD decision D10); got: {:?}",
        diags
    );
    assert_eq!(
        op_count, 0,
        "the mixed-dimension rounded_box must be dropped, not built; emitted \
         Translate ops: {op_count}"
    );
}
