//! End-to-end evaluation tests for built-in mathematical constants (pi, tau).
//!
//! These tests exercise the full parse → compile → eval pipeline to confirm
//! that `pi` and `tau` evaluate to the expected Real values.

use reify_test_support::eval_source;
use reify_types::{Value, ValueCellId};

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
