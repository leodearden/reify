//! Two-way (compile ⟺ eval) boundary suite for the math-linalg **§1.2
//! trig/transcendental** family (task 4352 — the §1.2 acceptance gate).
//!
//! Every §1.2 transcendental builtin now carries its documented return type at
//! COMPILE time (the `math_fn_result_type` arms wired through
//! `expr.rs::resolve_function_overload`'s `is_math_typed_fn` arm). This file
//! pins, for one representative input per row, that the FORWARD compile-time
//! cell type and the BACKWARD eval-time `Value` AGREE on dimension and kind.
//!
//!   FORWARD  = `compile_with_stdlib_helper(src)` → `template.value_cells[..].cell_type`
//!   BACKWARD = full `reify_eval::Engine` (+ `MockConstraintChecker`) `.eval(&module)`
//!              → `result.values.get(&ValueCellId::new(T, cell))`
//!
//! # Input discipline & the RED signal
//!
//! sin/cos/tan rows feed an ANGLE-typed literal (`0.5rad`, `0.0rad`) — a bare
//! Real arg would mask the drift (the first-arg fallback returns the arg's type
//! == correct dimensionless for a Real input). The ANGLE input makes the
//! pre-fix RED genuine: WITHOUT the fix the cell types `Scalar<ANGLE>` (the
//! arg's type), so the explicit `assert_eq!(ty, Type::dimensionless_scalar())`
//! here FAILS (ANGLE ≠ DIMENSIONLESS), and the cascade row types
//! `Scalar<ANGLE·LENGTH>` instead of `Scalar<LENGTH>`.
//!
//! Note (post-4373 type-layer canonicalization): both `dimensionless_scalar()`
//! and `angle()` are now `Scalar`-kind (differing only in the dimension
//! vector), and `value_type_kind_matches` accepts `Value::Real` against ANY
//! `Scalar` type — so the pre-fix `Scalar<ANGLE>` cell does NOT trip a runtime
//! `TypeKindMismatch`. The RED signal is therefore the explicit DIMENSION
//! assertion in this suite (and the cascade dimension), which tests the
//! compile-time dimension contract directly.
//!
//! exp/log rows feed a dimensionless Real — eval Undefs on a dimensioned arg
//! (`unary_f64` rejects dimensions), so their rows are contract pins; their
//! behavioral RED is at the `is_math_typed_fn` recognition level.
//!
//! # Arg-independent arms
//!
//! The `math_fn_result_type` trig arms are arg-independent. For
//! sin/cos/tan/exp/log the arm returns `dimensionless_scalar()`, identical to
//! the `_ => dimensionless_scalar()` fallback — so they introduce no new
//! behavior (and no new hazard); they only pin the contract so a future change
//! to `_` cannot silently break the dimensionless-arg rows. The load-bearing
//! arm is asin/acos/atan/atan2 → `angle()`, which the fallback would otherwise
//! get wrong.
//!
//! Enforcement is PERMISSIVE (PRD §5): task 4352 adds NO new bespoke hard
//! error. The cell-typing fix is all compile-side — eval was already correct.

use crate::common::compile_with_stdlib_helper;
use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_eval::EvalResult;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

const EPS: f64 = 1e-9;

const STRUCT: &str = "TrigBoundary";

/// One row per §1.2 transcendental name on a DRIFT-OBSERVABLE input.
/// sin/cos/tan receive an ANGLE literal so the first-arg drift to
/// `Scalar<ANGLE>` is detectable in both directions. The cascade `xcas`
/// pins that `sin(<angle>) * 5mm` propagates `Scalar<LENGTH>`, not
/// `Scalar<ANGLE·LENGTH>` (the pre-fix mistype).
const BOUNDARY_SOURCE: &str = r#"
structure def TrigBoundary {
    let s    = sin(0.5rad)
    let c    = cos(0.0rad)
    let t    = tan(0.5rad)
    let as_  = asin(0.5)
    let ac   = acos(0.5)
    let at   = atan(1.0)
    let at2  = atan2(1.0, 1.0)
    let ex   = exp(2.0)
    let lg   = log(2.718281828459045)
    let xcas = sin(0.5rad) * 5.0mm
}
"#;

// ── Harness ───────────────────────────────────────────────────────────────────

fn compile_and_eval() -> (CompiledModule, EvalResult) {
    let module = compile_with_stdlib_helper(BOUNDARY_SOURCE);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "boundary source must produce no Error-severity diagnostics; got: {errs:?}"
    );
    let mut engine = reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);
    let eval_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errs.is_empty(),
        "boundary eval must produce no Error-severity diagnostics; got: {eval_errs:?}"
    );
    (module, result)
}

/// The two-way `(forward cell type, backward eval value)` for member `cell`.
fn two_way(cell: &str) -> (Type, Value) {
    let (module, result) = compile_and_eval();
    let template = module
        .templates
        .iter()
        .find(|t| t.name == STRUCT)
        .unwrap_or_else(|| panic!("template `{STRUCT}` not found"));
    let cell_type = template
        .value_cells
        .iter()
        .find(|c| c.id.member == cell)
        .unwrap_or_else(|| panic!("cell `{cell}` not found on `{STRUCT}`"))
        .cell_type
        .clone();
    let value = result
        .values
        .get(&ValueCellId::new(STRUCT, cell))
        .unwrap_or_else(|| panic!("eval value for `{cell}` not found"))
        .clone();
    (cell_type, value)
}

// ── sin / cos / tan ───────────────────────────────────────────────────────────

/// `sin(0.5rad)` ⟺ dimensionless Scalar / eval `Value::Real(sin(0.5))`. ANGLE
/// input makes the pre-fix drift to `Scalar<ANGLE>` observable in both directions.
#[test]
fn sin_two_way_is_dimensionless() {
    let (ty, val) = two_way("s");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "sin(0.5rad) forward type must be a dimensionless Scalar"
    );
    match val {
        Value::Real(x) => assert!(
            (x - f64::sin(0.5)).abs() < EPS,
            "sin(0.5rad) eval value ≈ {}, got {x}",
            f64::sin(0.5)
        ),
        other => panic!("sin(0.5rad) eval should be Value::Real, got {other:?}"),
    }
}

/// `cos(0.0rad)` ⟺ dimensionless Scalar / eval `Value::Real(1.0)`.
#[test]
fn cos_two_way_is_dimensionless() {
    let (ty, val) = two_way("c");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "cos(0.0rad) forward type must be a dimensionless Scalar"
    );
    match val {
        Value::Real(x) => assert!((x - 1.0).abs() < EPS, "cos(0.0rad) eval value, got {x}"),
        other => panic!("cos(0.0rad) eval should be Value::Real, got {other:?}"),
    }
}

/// `tan(0.5rad)` ⟺ dimensionless Scalar / eval `Value::Real(tan(0.5))`.
#[test]
fn tan_two_way_is_dimensionless() {
    let (ty, val) = two_way("t");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "tan(0.5rad) forward type must be a dimensionless Scalar"
    );
    match val {
        Value::Real(x) => assert!(
            (x - f64::tan(0.5)).abs() < EPS,
            "tan(0.5rad) eval value ≈ {}, got {x}",
            f64::tan(0.5)
        ),
        other => panic!("tan(0.5rad) eval should be Value::Real, got {other:?}"),
    }
}

// ── asin / acos / atan / atan2 ────────────────────────────────────────────────

/// `asin(0.5)` ⟺ `Angle` (== `Scalar<ANGLE>`) / eval `Scalar { π/6, ANGLE }`.
#[test]
fn asin_two_way_is_angle() {
    let (ty, val) = two_way("as_");
    assert_eq!(ty, Type::angle(), "asin(0.5) forward type must be Angle");
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::ANGLE, "asin eval dimension");
            assert!(
                (si_value - f64::asin(0.5)).abs() < EPS,
                "asin(0.5) eval value ≈ π/6, got {si_value}"
            );
        }
        other => panic!("asin(0.5) eval should be Value::Scalar, got {other:?}"),
    }
}

/// `acos(0.5)` ⟺ `Angle` / eval `Scalar { π/3, ANGLE }`.
#[test]
fn acos_two_way_is_angle() {
    let (ty, val) = two_way("ac");
    assert_eq!(ty, Type::angle(), "acos(0.5) forward type must be Angle");
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::ANGLE, "acos eval dimension");
            assert!(
                (si_value - f64::acos(0.5)).abs() < EPS,
                "acos(0.5) eval value ≈ π/3, got {si_value}"
            );
        }
        other => panic!("acos(0.5) eval should be Value::Scalar, got {other:?}"),
    }
}

/// `atan(1.0)` ⟺ `Angle` / eval `Scalar { π/4, ANGLE }`.
#[test]
fn atan_two_way_is_angle() {
    let (ty, val) = two_way("at");
    assert_eq!(ty, Type::angle(), "atan(1.0) forward type must be Angle");
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::ANGLE, "atan eval dimension");
            assert!(
                (si_value - f64::atan(1.0)).abs() < EPS,
                "atan(1.0) eval value ≈ π/4, got {si_value}"
            );
        }
        other => panic!("atan(1.0) eval should be Value::Scalar, got {other:?}"),
    }
}

/// `atan2(1.0, 1.0)` ⟺ `Angle` / eval `Scalar { π/4, ANGLE }`.
#[test]
fn atan2_two_way_is_angle() {
    let (ty, val) = two_way("at2");
    assert_eq!(
        ty,
        Type::angle(),
        "atan2(1.0,1.0) forward type must be Angle"
    );
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::ANGLE, "atan2 eval dimension");
            assert!(
                (si_value - f64::atan2(1.0, 1.0)).abs() < EPS,
                "atan2(1.0,1.0) eval value ≈ π/4, got {si_value}"
            );
        }
        other => panic!("atan2(1.0,1.0) eval should be Value::Scalar, got {other:?}"),
    }
}

// ── exp / log ─────────────────────────────────────────────────────────────────

/// `exp(2.0)` ⟺ dimensionless Scalar / eval `Value::Real(e²)`. Contract pin:
/// the explicit arm prevents the `_` fallback silently masking a future change.
#[test]
fn exp_two_way_is_dimensionless() {
    let (ty, val) = two_way("ex");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "exp(2.0) forward type must be a dimensionless Scalar"
    );
    match val {
        Value::Real(x) => assert!(
            (x - f64::exp(2.0)).abs() < EPS,
            "exp(2.0) eval value ≈ e², got {x}"
        ),
        other => panic!("exp(2.0) eval should be Value::Real, got {other:?}"),
    }
}

/// `log(e)` ⟺ dimensionless Scalar / eval `Value::Real(1.0)`. Contract pin.
#[test]
fn log_two_way_is_dimensionless() {
    let (ty, val) = two_way("lg");
    assert_eq!(
        ty,
        Type::dimensionless_scalar(),
        "log(e) forward type must be a dimensionless Scalar"
    );
    match val {
        Value::Real(x) => {
            assert!(
                (x - 1.0).abs() < EPS,
                "log(e) eval value should be 1.0, got {x}"
            )
        }
        other => panic!("log(e) eval should be Value::Real, got {other:?}"),
    }
}

// ── Consumer cascade (G1) ─────────────────────────────────────────────────────

/// `sin(0.5rad) * 5.0mm` ⟺ `Scalar<LENGTH>` / eval `Scalar { sin(0.5)*0.005, LENGTH }`.
///
/// Pins the G1 consumer cascade: a dimensionless result from `sin` multiplied by
/// a `Scalar<LENGTH>` propagates to `Scalar<LENGTH>`, not `Scalar<ANGLE·LENGTH>`
/// (the pre-fix mistype that arose when `sin` returned `Scalar<ANGLE>`).
#[test]
fn sin_times_mm_cascade_is_length() {
    let (ty, val) = two_way("xcas");
    assert_eq!(
        ty,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "sin(0.5rad) * 5mm forward type must be Scalar<LENGTH>"
    );
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "cascade eval dimension");
            let expected = f64::sin(0.5) * 0.005;
            assert!(
                (si_value - expected).abs() < EPS,
                "cascade eval value ≈ {expected}, got {si_value}"
            );
        }
        other => panic!("sin*mm cascade eval should be Value::Scalar, got {other:?}"),
    }
}

// ── Forward-trig argument-dimension rejection (task 6249, step-3 RED) ─────────
//
// `sin`/`cos`/`tan` reject a dimensioned, non-ANGLE argument at COMPILE time
// with a coded `ArgTypeMismatch` Error. RED before step-4 wires
// `math_signatures::check_transcendental_arg_dimensions` into
// `expr.rs::resolve_function_overload` — verified first-hand on the base
// commit, where `reify check` on `sin(5.0mm)` exits 0 with
// "All constraints satisfied" and zero diagnostics.

/// Every `ArgTypeMismatch` Error in `compiled` — mirrors the helper of the same
/// name in `tests/builtin_arg_signature_tests.rs:214` so assertions here read
/// the same way as the established sibling suite.
fn arg_type_mismatch_errors(compiled: &CompiledModule) -> Vec<&reify_core::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(reify_core::DiagnosticCode::ArgTypeMismatch)
                && d.severity == Severity::Error
        })
        .collect()
}

/// Compile a one-`let` structure body and return its `ArgTypeMismatch` Errors'
/// messages. `expr` is spliced verbatim into `let b = <expr>`.
fn trig_arg_mismatch_messages(expr: &str) -> Vec<String> {
    let src = format!("structure def TrigArgCheck {{\n    let b = {expr}\n}}\n");
    let module = compile_with_stdlib_helper(&src);
    arg_type_mismatch_errors(&module)
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Assert `expr` raises EXACTLY one `ArgTypeMismatch` Error naming `name`.
///
/// Exactly 1, not "at least 1": a duplicate would mean the diagnostic is being
/// emitted from two wiring sites.
fn assert_one_trig_arg_mismatch(expr: &str, name: &str) {
    let msgs = trig_arg_mismatch_messages(expr);
    assert_eq!(
        msgs.len(),
        1,
        "`{expr}` must raise exactly one ArgTypeMismatch Error, got: {msgs:#?}"
    );
    assert!(
        msgs[0].contains(name),
        "`{expr}` diagnostic must name `{name}`, got: {:?}",
        msgs[0]
    );
}

/// `sin(5.0mm)` — a LENGTH argument. Eval silently yields `Value::Undef`
/// (`trig_input` returns `None` for a non-ANGLE dimension); this pins the
/// compile-time rejection that surfaces it instead.
#[test]
fn sin_of_length_raises_arg_type_mismatch() {
    assert_one_trig_arg_mismatch("sin(5.0mm)", "sin");
}

/// `cos(5.0mm)` — sibling of the `sin` case.
#[test]
fn cos_of_length_raises_arg_type_mismatch() {
    assert_one_trig_arg_mismatch("cos(5.0mm)", "cos");
}

/// `tan(2.0kg)` — a MASS argument, pinning that the rule is "any dimensioned
/// non-ANGLE scalar", not a LENGTH special case.
#[test]
fn tan_of_mass_raises_arg_type_mismatch() {
    assert_one_trig_arg_mismatch("tan(2.0kg)", "tan");
}

// ── Acceptance boundary + scope fence (task 6249, step-5) ─────────────────────
//
// These pin the two ways step-4's wiring could be WRONG in the OVER-BROAD
// direction. Each case is RED against a plausible mis-implementation.

/// Assert `expr` raises ZERO `ArgTypeMismatch` Errors.
fn assert_no_trig_arg_mismatch(expr: &str, why: &str) {
    let msgs = trig_arg_mismatch_messages(expr);
    assert!(
        msgs.is_empty(),
        "`{expr}` must raise no ArgTypeMismatch ({why}), got: {msgs:#?}"
    );
}

// (a) ACCEPTANCE ─────────────────────────────────────────────────────────────

/// `sin(0.5rad)` / `cos(0.0rad)` / `tan(0.5rad)` — a true `Angle` argument is
/// accepted (`trig_input`'s `Value::Scalar { dimension: ANGLE }` arm).
///
/// RED against an implementation that accepts only dimensionless. This is the
/// load-bearing acceptance case: this file's own `BOUNDARY_SOURCE` already
/// depends on it, so an ANGLE-rejecting checker would red the whole suite.
#[test]
fn forward_trig_accepts_angle_arguments() {
    assert_no_trig_arg_mismatch("sin(0.5rad)", "ANGLE is the canonical trig input");
    assert_no_trig_arg_mismatch("cos(0.0rad)", "ANGLE is the canonical trig input");
    assert_no_trig_arg_mismatch("tan(0.5rad)", "ANGLE is the canonical trig input");
}

/// `sin(2.5)` — a bare dimensionless literal is accepted as radians
/// (`trig_input`'s `Value::Real` arm).
///
/// RED against an ANGLE-only implementation.
#[test]
fn forward_trig_accepts_dimensionless_arguments() {
    assert_no_trig_arg_mismatch("sin(2.5)", "bare Real is treated as radians at eval");
}

/// `cos(3 * 2)` — an `Int`-typed argument is accepted (`trig_input`'s
/// `Value::Int` arm).
///
/// RED against a `Type::Scalar`-only implementation. Overload resolution
/// carries no Int→Real widening, so such a rule would statically reject what
/// eval happily evaluates.
#[test]
fn forward_trig_accepts_int_arguments() {
    assert_no_trig_arg_mismatch("cos(3 * 2)", "trig_input accepts Value::Int");
}

// (b) SCOPE FENCE ────────────────────────────────────────────────────────────
//
// The six non-forward-trig transcendental names must ALSO stay unchecked here.
// RED against an implementation that gates on `MATH_TRANSCENDENTAL_NAMES`
// wholesale instead of the three forward-trig names — the most likely
// over-broad mistake, and the one that would break
// `tests/prd-gate/fixtures/dcr_langsurface_crossdim_silent.ri` (registered in
// `scripts/verify.sh`'s `_GUI_COUPLED_RI_FIXTURES`, which must keep exiting 0
// until #5809's leaf κ lands).

/// SCOPE FENCE — `exp`/`log` argument dimensions are OWNED BY TASK #6943
/// (`docs/prds/v0_6/builtin-signature-registry.md` §3 decision 6(b),
/// Leo-ratified 2026-08-07), which delivers them as registry rows with a
/// chartered `E_ArgDimension` code family and dimension-aware fixit hints.
///
/// Their failure mode is also DIFFERENT from forward trig's: `unary_f64`
/// extracts via `Value::as_f64`, whose `Value::Scalar` arm accepts any
/// dimension, so `log(2mm)` silently evaluates `ln(0.002)` rather than Undefing.
///
/// **Implementer of #6943: FLIP this assertion, do not delete it.**
#[test]
fn scope_fence_exp_and_log_stay_unchecked_pending_6943() {
    assert_no_trig_arg_mismatch("exp(5.0mm)", "exp arg dimension is owned by #6943");
    assert_no_trig_arg_mismatch("log(5.0mm)", "log arg dimension is owned by #6943");
}

/// SCOPE FENCE — inverse-trig argument dimensions are OWNED BY TASK #5809
/// (`docs/prds/v0_6/dimension-checked-readers.md` leaf κ), which already has a
/// committed before-image fixture.
///
/// `atan2` is a further trap: its correct rule is same-dimension-PAIR, not
/// dimensionless-only (`atan2(dy, dx)` over two LENGTHs is legitimate — the
/// ratio is dimensionless), which is why it is not expressible with this
/// leaf's mechanism at all.
///
/// **Implementer of #5809: FLIP this assertion, do not delete it.**
#[test]
fn scope_fence_inverse_trig_stays_unchecked_pending_5809() {
    assert_no_trig_arg_mismatch("asin(3.0mm)", "asin arg dimension is owned by #5809");
    assert_no_trig_arg_mismatch("acos(3.0mm)", "acos arg dimension is owned by #5809");
    assert_no_trig_arg_mismatch("atan(3.0mm)", "atan arg dimension is owned by #5809");
    assert_no_trig_arg_mismatch(
        "atan2(1.0mm, 1.0kg)",
        "atan2 needs a same-dimension-PAIR rule, owned by #5809",
    );
}

// (c) CORPUS GUARD ───────────────────────────────────────────────────────────

/// The only live forward-trig call sites in the whole tracked `.ri` corpus are
/// `examples/generate_bolt_circle.ri:35-36`. Compile both shapes here so the
/// corpus is covered by a fast unit-level test rather than only by the slower
/// `examples_smoke` sweep.
///
/// `i * 2 * pi / bolt_count` is dimensionless throughout (`pi` is a
/// dimensionless Real; `i` and `bolt_count` are Ints), so both must stay clean.
#[test]
fn corpus_guard_bolt_circle_forward_trig_sites_stay_clean() {
    let src = r#"
structure def BoltCircleTrig {
    let bolt_count = 4
    let radius = 50mm
    let i = 1
    let x = radius * cos(i * 2 * pi / bolt_count)
    let y = radius * sin(i * 2 * pi / bolt_count)
}
"#;
    let module = compile_with_stdlib_helper(src);
    let mismatches: Vec<_> = arg_type_mismatch_errors(&module)
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        mismatches.is_empty(),
        "the live examples/generate_bolt_circle.ri trig call shapes must stay clean, got: \
         {mismatches:#?}"
    );
}
