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

mod common;

use common::compile_with_stdlib_helper;
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
    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
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
        Value::Scalar { si_value, dimension } => {
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
        Value::Scalar { si_value, dimension } => {
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
        Value::Scalar { si_value, dimension } => {
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
    assert_eq!(ty, Type::angle(), "atan2(1.0,1.0) forward type must be Angle");
    match val {
        Value::Scalar { si_value, dimension } => {
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
            assert!((x - 1.0).abs() < EPS, "log(e) eval value should be 1.0, got {x}")
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
        Type::Scalar { dimension: DimensionVector::LENGTH },
        "sin(0.5rad) * 5mm forward type must be Scalar<LENGTH>"
    );
    match val {
        Value::Scalar { si_value, dimension } => {
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
