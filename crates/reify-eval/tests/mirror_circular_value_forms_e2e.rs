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
//!
//! # THREE layers now guard these two builtins (task 5662)
//!
//! Task 5662 gave the 7-arg `mirror` and 9-arg `circular_pattern` SCALAR origin
//! triples a COMPILE-layer LENGTH slot in
//! `crates/reify-compiler/src/builtin_signatures.rs`, so a bare scalar origin is
//! now caught three times over:
//!
//! 1. compile — `DiagnosticCode::ArgTypeMismatch`, before anything is built;
//! 2. eval — `DiagnosticCode::DimensionedArgRejected` (tasks 5214 / 5350 / 5745);
//! 3. the op is DROPPED, so nothing reaches the kernel.
//!
//! The two layers keep DISTINCT codes ON PURPOSE — PRD decision D2, two-layer
//! observability: a caller must be able to tell "the compiler rejected this
//! statically" from "the evaluator rejected it while building", because only the
//! second implies the design was actually evaluated. They share the C1 message
//! template and the `5mm` migration hint (D9), so the wording is the same even
//! though the code is not.
//!
//! The consequence for THIS file is mechanical: the two rows whose sources carry
//! a deliberately bare SCALAR origin can no longer use the strict
//! `parse_and_compile`, which hard-asserts zero Error diagnostics. They route
//! through `compile_bare_origin` instead — a TIGHTENING, not a loosening; see
//! that helper. Every dimensioned row and every VALUE-form row stays on the
//! strict helper, because the decoded-value route is structurally excluded from
//! the compile slot table and so still compiles clean.

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source, parse_and_compile,
};

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

/// (b) Back-compat: legacy 7-arg scalar form mirror(box, 0mm,0mm,0mm, 1,0,0) still builds
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
    build_circular_ops_compiled(parse_and_compile(source))
}

/// The BARE-source counterpart of [`build_circular_ops`] (task 5662).
///
/// Task 5662 gave the 9-arg scalar `circular_pattern` origin triple a
/// compile-layer LENGTH slot, so the bare source below no longer compiles clean
/// and the strict `parse_and_compile` — which hard-asserts zero Error
/// diagnostics — would panic before eval ever ran.
fn build_circular_ops_bare(source: &str) -> (usize, Vec<GeometryOp>) {
    build_circular_ops_compiled(compile_bare_origin(source))
}

/// Compile a source whose `mirror` / `circular_pattern` ORIGIN components are
/// deliberately BARE (task 5662).
///
/// Modelled on `compile_bare_spacing` in
/// `crates/reify-eval/tests/pattern_spacing_units_e2e.rs` (task 5652) and
/// `compile_bare_length` in
/// `crates/reify-eval/tests/harness_geometry/primitive_profile_length_units_e2e.rs`
/// (task 5750), which the two preceding leaves had to introduce for exactly this
/// reason.
///
/// Swapping the lenient `compile_source` in for the strict `parse_and_compile`
/// is a TIGHTENING, not a loosening, because this helper re-asserts BOTH halves
/// of what the strict one used to guarantee:
///
/// (i) the compile-layer `ArgTypeMismatch` really IS emitted, so this file
///     cannot silently stop noticing if task 5662's slots regress; and
/// (ii) it is the ONLY Error-severity compile diagnostic, so an unrelated
///     compile Error cannot make a caller's "no op reached the kernel"
///     assertion hold for the wrong reason.
///
/// The eval-layer assertions still run afterwards because
/// `check_builtin_arg_types` is anti-cascade: it touches only `diagnostics` and
/// never lowering, so the op is still emitted and must still be DROPPED at build
/// by the eval gate — which is the thing these rows actually test.
fn compile_bare_origin(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "a bare scalar mirror/circular_pattern origin must ALSO be rejected at \
         compile time (task 5662 ArgTypeMismatch), not only at eval; got no Error \
         diagnostics in: {:?}",
        compiled.diagnostics
    );
    assert!(
        errors
            .iter()
            .all(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)),
        "ArgTypeMismatch must be the ONLY compile Error in this fixture, else the \
         callers' \"no op reached the kernel\" assertions could pass because \
         compilation broke rather than because the eval gate dropped the op; \
         unexpected errors: {:?}",
        errors
            .iter()
            .filter(|d| d.code != Some(DiagnosticCode::ArgTypeMismatch))
            .collect::<Vec<_>>()
    );
    compiled
}

/// The kernel half of [`build_circular_ops`], shared with its bare counterpart.
fn build_circular_ops_compiled(
    compiled: reify_compiler::CompiledModule,
) -> (usize, Vec<GeometryOp>) {
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
///
/// Routed through [`build_circular_ops_bare`] since task 5662 gave this same
/// origin a COMPILE-layer slot: the strict `parse_and_compile` would now panic on
/// the ArgTypeMismatch before eval ran. The eval-layer assertions below are
/// unchanged and still run — see [`compile_bare_origin`] for why that is a
/// tightening.
#[test]
fn circular_pattern_scalar_bare_origin_drops_op_with_error() {
    let (error_count, circular_ops) = build_circular_ops_bare(
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

// ── units-length δ (task 5745): the decoded VALUE-FORM origin gate ────────────
//
// PRD §6 boundary rows 5 and 6, at the outermost seam. Until δ the value form
// BYPASSED the very gate its scalar sibling enforced: `mirror(b, plane_yz(10))`
// reached the kernel as a 10 SI-**metre** plane offset, exit 0, zero
// diagnostics, while `mirror(b, 10, 0, 0, 1, 0, 0)` was rejected at ox/oy/oz.
// That is the 1000× hazard this PRD exists for, arriving through the one route
// an author is most likely to take.
//
// These rows drive the real parser and unit system (not hand-built
// `CompiledExpr` fixtures, which the `geometry_ops/tests.rs` unit rows cover),
// so they would catch a regression introduced anywhere between the `.ri`
// surface and the kernel call.

/// Build `source` against a mock kernel and return `(diagnostics, mirror_ops)`.
///
/// Returns the diagnostics THEMSELVES rather than a count, unlike
/// `build_circular_ops` above: the δ rows below assert on the specific
/// `ArgRejection` diagnostic — its `DiagnosticCode` and its wording — not merely
/// that "some Error was emitted". That distinction matters, because the
/// op-compile Error which accompanies every rejection ALSO names the argument
/// and the word "Length", so a looser shape would pass without the gate ever
/// having fired.
fn build_value_form(
    source: &str,
    want: fn(&GeometryOp) -> bool,
) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form_compiled(parse_and_compile(source), want)
}

/// The BARE-source counterpart of [`build_value_form`], narrowed to `Mirror`
/// ops (task 5662) — see [`compile_bare_origin`] for why the strict helper can
/// no longer be used on a bare SCALAR origin.
fn build_mirror_bare(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form_compiled(compile_bare_origin(source), |op| {
        matches!(op, GeometryOp::Mirror { .. })
    })
}

/// The kernel half of [`build_value_form`], shared with its bare counterpart.
fn build_value_form_compiled(
    compiled: reify_compiler::CompiledModule,
    want: fn(&GeometryOp) -> bool,
) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);
    let ops = ops_ref.lock().unwrap();
    let kept: Vec<GeometryOp> = ops
        .iter()
        .filter(|r| want(&r.op))
        .map(|r| r.op.clone())
        .collect();
    (result.diagnostics.clone(), kept)
}

/// `build_value_form` narrowed to `Mirror` ops.
fn build_mirror(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form(source, |op| matches!(op, GeometryOp::Mirror { .. }))
}

/// `build_value_form` narrowed to `CircularPattern` ops.
fn build_circular(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form(source, |op| matches!(op, GeometryOp::CircularPattern { .. }))
}

/// PRD §6 row 5 — the BYPASS, closed: a bare `plane_yz(10)` origin must be
/// REJECTED and the op DROPPED, never silently built with a 10 SI-metre plane
/// offset.
///
/// The assertion is deliberately specific: the diagnostic must be the shared
/// `ArgRejection` one (`DiagnosticCode::DimensionedArgRejected`, worded
/// "argument expects Length") and must name the ORIGIN component `ox` — the same
/// name the scalar form has used since task 5214, so the two forms of the same
/// author mistake now read identically.
#[test]
fn mirror_value_form_bare_plane_origin_drops_op_with_error() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def BarePlaneMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_yz(10))
        }
        "#,
    );

    let rejection = diagnostics.iter().find(|d| {
        d.severity == reify_core::Severity::Error
            && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)
    });
    let rejection = rejection.unwrap_or_else(|| {
        panic!(
            "a bare `plane_yz(10)` origin must produce the shared \
             DimensionedArgRejected Error; got: {:?}",
            diagnostics
        )
    });
    assert!(
        rejection.message.contains("argument expects Length"),
        "the wording must come from `ArgRejection::message`, shared with the \
         named-arg and variadic routes; got: {}",
        rejection.message
    );
    assert!(
        rejection.message.contains("ox"),
        "the rejection must name the ORIGIN component `ox` — the same name the \
         scalar form uses; got: {}",
        rejection.message
    );
    assert!(
        mirror_ops.is_empty(),
        "a bare-origin value-form mirror must be DROPPED, not silently built \
         with a 10 SI-metre plane origin; emitted Mirror ops: {:?}",
        mirror_ops
    );
}

/// PRD §6 row 6 — the positive control that keeps row 5 from passing vacuously,
/// and the compatibility promise: the DIMENSIONED value form still builds, and
/// is geometrically identical to what `plane_yz(0.01)` produced before δ.
///
/// `1e-12` rather than `==` because the parser computes `10 * 1e-3`; the f64
/// error is at most ~1 ulp (order 1e-18 at this magnitude), so this clears it by
/// six orders of magnitude while staying far tighter than the 1000× defect being
/// guarded against. It is the same tolerance the sibling
/// `decode_plane_producer_round_trip_*` unit tests use for this same quantity.
#[test]
fn mirror_value_form_dimensioned_plane_origin_builds_unchanged() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def MmPlaneMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_yz(10mm))
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the dimensioned value form must build with ZERO Error diagnostics; \
         got: {:?}",
        errors
    );
    assert_eq!(
        mirror_ops.len(),
        1,
        "expected exactly one Mirror op to reach the kernel, got {:?}",
        mirror_ops
    );
    match &mirror_ops[0] {
        GeometryOp::Mirror {
            plane_origin,
            plane_normal,
            ..
        } => {
            for (i, expected) in [0.01_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (plane_origin[i] - expected).abs() < 1e-12,
                    "plane_yz(10mm) must reach the kernel with plane_origin[{i}] \
                     = {expected} SI metres (NOT {} m); got {:?}",
                    expected * 1000.0,
                    plane_origin
                );
            }
            for (i, expected) in [1.0_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (plane_normal[i] - expected).abs() < 1e-12,
                    "plane_yz's normal is the dimensionless unit vector [1,0,0] \
                     and is NOT gated; got {:?}",
                    plane_normal
                );
            }
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// NO-REGRESSION lock on the SCALAR form, which δ must leave completely alone.
/// Its three `ox`/`oy`/`oz` rejections keep their exact pre-δ wording — this is
/// what proves the decoded-value route joined the shared chokepoint rather than
/// forking a second copy of the text.
///
/// Routed through [`build_mirror_bare`] since task 5662 gave this same origin a
/// COMPILE-layer slot. The EVAL-layer assertions below are deliberately
/// untouched, and they are what makes the two-layer split observable: the
/// compile diagnostic carries `ArgTypeMismatch` while these carry
/// `DimensionedArgRejected`, with byte-identical wording (PRD D2 + D9).
#[test]
fn mirror_scalar_bare_origin_rejections_are_unchanged_by_delta() {
    let (diagnostics, mirror_ops) = build_mirror_bare(
        r#"
        structure def BareScalarMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, 10, 0, 0, 1, 0, 0)
        }
        "#,
    );

    for name in ["ox", "oy", "oz"] {
        let expected = format!(
            "mirror: {name} argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`"
        );
        assert!(
            diagnostics.iter().any(|d| d.message == expected
                && d.severity == reify_core::Severity::Error
                && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)),
            "the scalar form's `{name}` rejection must be unchanged by δ; \
             expected {expected:?}, got: {:?}",
            diagnostics
        );
    }
    assert!(
        mirror_ops.is_empty(),
        "the scalar bare-origin form must still be DROPPED; got {:?}",
        mirror_ops
    );
}

/// The scalar-form positive control, unchanged by δ: a dimensioned scalar origin
/// still builds clean. Paired with the row above so neither can pass vacuously.
#[test]
fn mirror_scalar_dimensioned_origin_still_builds_clean() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def MmScalarMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, 0mm, 0mm, 0mm, 1, 0, 0)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected zero Errors, got: {:?}", errors);
    assert_eq!(mirror_ops.len(), 1, "got {:?}", mirror_ops);
}

/// PRD §6 row 5, axis half — the counterpart of
/// `circular_pattern_scalar_bare_origin_drops_op_with_error` directly above,
/// for the route that BYPASSED the gate that test locks.
///
/// A bare `axis_z(point3(12, 0, 0))` origin must be rejected and the op DROPPED,
/// never silently built with a 12 SI-**metre** axis origin — 1000× a plausible
/// 12 mm offset. Placing the rotation axis 12 metres out silently reproduces the
/// scalar form's headline defect through a different door, which is exactly what
/// made the two forms' disagreement worth closing.
#[test]
fn circular_pattern_value_form_bare_axis_origin_drops_op_with_error() {
    let (diagnostics, circular_ops) = build_circular(
        r#"
        structure def BareAxisRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(12, 0, 0)), 6, 60deg)
        }
        "#,
    );

    let rejection = diagnostics
        .iter()
        .find(|d| {
            d.severity == reify_core::Severity::Error
                && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)
        })
        .unwrap_or_else(|| {
            panic!(
                "a bare `axis_z(point3(12, 0, 0))` origin must produce the shared \
                 DimensionedArgRejected Error; got: {:?}",
                diagnostics
            )
        });
    assert!(
        rejection.message.contains("argument expects Length"),
        "wording must come from `ArgRejection::message`; got: {}",
        rejection.message
    );
    assert!(
        rejection.message.contains("ox"),
        "the rejection must name the ORIGIN component `ox`, matching the scalar \
         form; got: {}",
        rejection.message
    );
    assert!(
        circular_ops.is_empty(),
        "a bare-origin value-form circular_pattern must be DROPPED, not silently \
         built with a 12 SI-metre axis origin; emitted ops: {:?}",
        circular_ops
    );
}

/// PRD §6 row 6, axis half — the positive control that keeps the row above from
/// passing vacuously, and the compatibility promise: the DIMENSIONED value form
/// still builds and still reaches the kernel at `[0.012, 0, 0]`, not `[12, 0, 0]`.
///
/// `1e-12` for `circular_pattern_scalar_mm_origin_builds_op`'s reason: the parser
/// computes `12 * 1e-3`, so the f64 error is at most ~1 ulp (order 1e-18 here),
/// six orders below this bound and far tighter than the 1000× defect guarded
/// against.
#[test]
fn circular_pattern_value_form_dimensioned_axis_origin_builds_unchanged() {
    let (diagnostics, circular_ops) = build_circular(
        r#"
        structure def MmAxisRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(12mm, 0mm, 0mm)), 6, 60deg)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the dimensioned value form must build with ZERO Error diagnostics; \
         got: {:?}",
        errors
    );
    assert_eq!(
        circular_ops.len(),
        1,
        "expected exactly one CircularPattern op, got {:?}",
        circular_ops
    );
    match &circular_ops[0] {
        GeometryOp::CircularPattern {
            axis_origin,
            axis_dir,
            ..
        } => {
            for (i, expected) in [0.012_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (axis_origin[i] - expected).abs() < 1e-12,
                    "axis_origin[{i}] must be {expected} SI metres (NOT {} m); \
                     got {:?}",
                    expected * 1000.0,
                    axis_origin
                );
            }
            for (i, expected) in [0.0_f64, 0.0, 1.0].into_iter().enumerate() {
                assert!(
                    (axis_dir[i] - expected).abs() < 1e-12,
                    "axis_z's direction is the dimensionless unit vector [0,0,1] \
                     and is NOT gated; got {:?}",
                    axis_dir
                );
            }
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}
