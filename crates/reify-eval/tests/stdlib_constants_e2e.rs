//! End-to-end evaluation tests for built-in mathematical constants (pi, tau).
//!
//! These tests exercise the full parse → compile → eval pipeline to confirm
//! that `pi` and `tau` evaluate to the expected Real values.

use reify_test_support::{
    assert_no_eval_errors, eval_source, make_engine, parse_and_compile_with_stdlib,
};
use reify_core::ValueCellId;
use reify_ir::Value;

// ─── step-9: pi and tau evaluate to correct Real values ─────────────────────

#[test]
fn pi_evaluates_to_real_pi() {
    let result = eval_source("structure S { let angle = pi }");
    let id = ValueCellId::new("S", "angle");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'angle' not found in eval result"));
    match val {
        Value::Real(v) => {
            assert!(
                (*v - std::f64::consts::PI).abs() < 1e-15,
                "expected PI ({:.16}), got {:.16}",
                std::f64::consts::PI,
                v
            );
        }
        other => panic!("expected Real(PI), got {:?}", other),
    }
}

#[test]
fn tau_evaluates_to_real_tau() {
    let result = eval_source("structure S { let angle = tau }");
    let id = ValueCellId::new("S", "angle");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'angle' not found in eval result"));
    match val {
        Value::Real(v) => {
            assert!(
                (*v - std::f64::consts::TAU).abs() < 1e-15,
                "expected TAU ({:.16}), got {:.16}",
                std::f64::consts::TAU,
                v
            );
        }
        other => panic!("expected Real(TAU), got {:?}", other),
    }
}

// ─── step-11: trig functions with pi and tau ────────────────────────────────

/// Helper: compile with stdlib, eval, assert no errors.
fn eval_with_stdlib(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    result
}

#[test]
fn sin_pi_is_approximately_zero() {
    let result =
        eval_with_stdlib("structure S {\n  let half_turn = pi\n  let x = sin(half_turn)\n}");
    let id = ValueCellId::new("S", "x");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'x' not found in eval result"));
    match val {
        Value::Real(v) => {
            assert!(v.abs() < 1e-10, "sin(pi) should be ~0, got {}", v);
        }
        other => panic!("expected Real(~0), got {:?}", other),
    }
}

#[test]
fn cos_tau_is_approximately_one() {
    let result =
        eval_with_stdlib("structure S {\n  let full_turn = tau\n  let y = cos(full_turn)\n}");
    let id = ValueCellId::new("S", "y");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'y' not found in eval result"));
    match val {
        Value::Real(v) => {
            assert!((*v - 1.0).abs() < 1e-10, "cos(tau) should be ~1, got {}", v);
        }
        other => panic!("expected Real(~1), got {:?}", other),
    }
}

// ─── step-13: arithmetic consistency between 2*pi and tau ───────────────────

#[test]
fn two_pi_equals_tau() {
    let result = eval_source("structure S {\n  let x = 2 * pi\n  let y = tau\n}");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    let x_val = result
        .values
        .get(&x_id)
        .unwrap_or_else(|| panic!("'x' not found in eval result"));
    let y_val = result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("'y' not found in eval result"));
    match (x_val, y_val) {
        (Value::Real(x), Value::Real(y)) => {
            assert!(
                (x - y).abs() < 1e-15,
                "2*pi ({:.16}) should equal tau ({:.16})",
                x,
                y
            );
        }
        other => panic!("expected (Real, Real), got {:?}", other),
    }
}
