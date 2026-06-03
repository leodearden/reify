//! End-to-end regression test for dimensionless Scalar +/- Real/Int arithmetic
//! (task 4319 — dogfooding repro from dev_capstan).
//!
//! Root cause: eval_add and eval_sub had no (Scalar, Real) / (Real, Scalar) /
//! (Scalar, Int) / (Int, Scalar) arms, so a dimensionless Scalar (e.g. the
//! result of 100mm / 4mm) added to a Real silently produced Value::Undef.
//! eval_mul and eval_div already handled these arms; this suite is the
//! integration gate that confirms the fix composes through the real compiler
//! and eval pipeline.
//!
//! Each binding uses `100mm / 4mm` (= 25.0, exactly representable) to produce
//! the dimensionless Scalar without relying on `pi` or other irrational
//! constants.

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;
use reify_test_support::eval_source;

const EPSILON: f64 = 1e-9;

fn get_f64(source: &str, binding: &str) -> f64 {
    let result = eval_source(source);
    let id = ValueCellId::new("T", binding);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("binding '{}' not found in eval result", binding));
    match val {
        Value::Real(v) => *v,
        other => panic!("expected Value::Real for '{}', got {:?}", binding, other),
    }
}

// ── Addition: dimensionless Scalar + Real ────────────────────────────────────

#[test]
fn dscalar_plus_real_evaluates_to_real() {
    let source = "structure T { let d = 100mm / 4mm   let r = d + 4.0 }";
    let v = get_f64(source, "r");
    assert!(
        (v - 29.0).abs() < EPSILON,
        "expected 29.0 (25.0 + 4.0), got {v}"
    );
}

#[test]
fn real_plus_dscalar_evaluates_to_real() {
    let source = "structure T { let d = 100mm / 4mm   let r = 4.0 + d }";
    let v = get_f64(source, "r");
    assert!(
        (v - 29.0).abs() < EPSILON,
        "expected 29.0 (4.0 + 25.0), got {v}"
    );
}

// ── Subtraction: dimensionless Scalar - Real ─────────────────────────────────

#[test]
fn dscalar_minus_real_evaluates_to_real() {
    let source = "structure T { let d = 100mm / 4mm   let r = d - 4.0 }";
    let v = get_f64(source, "r");
    assert!(
        (v - 21.0).abs() < EPSILON,
        "expected 21.0 (25.0 - 4.0), got {v}"
    );
}

#[test]
fn real_minus_dscalar_evaluates_to_real() {
    let source = "structure T { let d = 100mm / 4mm   let r = 4.0 - d }";
    let v = get_f64(source, "r");
    assert!(
        (v - (-21.0)).abs() < EPSILON,
        "expected -21.0 (4.0 - 25.0), got {v}"
    );
}

// ── Downstream composition: 7mm * (dscalar + 4.0) → Scalar{LENGTH} ──────────

#[test]
fn dscalar_plus_real_then_times_length_yields_scalar_length() {
    // (100mm / 4mm) = 25.0 (dimensionless); 25.0 + 4.0 = 29.0 (Real);
    // 7mm * 29.0 = 203mm = 0.203 m (SI), dimension LENGTH.
    let source = "structure T { let d = 100mm / 4mm   let r = 7mm * (d + 4.0) }";
    let result = eval_source(source);
    let id = ValueCellId::new("T", "r");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("binding 'r' not found in eval result"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(*dimension, DimensionVector::LENGTH, "expected LENGTH dim");
            assert!(
                (si_value - 0.203).abs() < EPSILON,
                "expected 0.203 m (203mm), got {si_value}"
            );
        }
        other => panic!("expected Value::Scalar{{LENGTH}} for 'r', got {:?}", other),
    }
}

// ── Comparison regression lock: dimensionless Scalar > Real already worked ───

#[test]
fn dscalar_gt_real_is_true() {
    // Locks the already-correct comparison path so it cannot regress.
    let source = "structure T { let d = 100mm / 4mm   let r = d > 4.0 }";
    let result = eval_source(source);
    let id = ValueCellId::new("T", "r");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("binding 'r' not found in eval result"));
    match val {
        Value::Bool(true) => {}
        other => panic!("expected Bool(true) for 'd > 4.0', got {:?}", other),
    }
}
