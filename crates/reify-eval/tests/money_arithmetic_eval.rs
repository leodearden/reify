//! Runtime evaluation tests for Money-dimension currency-mass arithmetic
//! (task 2379).
//!
//! Compile-side structural assertions live in
//! `crates/reify-compiler/tests/money_arithmetic_tests.rs`; this binary locks
//! the runtime end-to-end behaviour: cancellation arithmetic that returns a
//! bare-MONEY scalar, and cross-currency addition that converts each operand
//! to SI via its registry factor before summing.
//!
//! Each source begins with `pub unit USD : Money` for hermeticity — the test
//! does not depend on the stdlib USD entry's merge order.  Underlying impl
//! wired by deps 57 (Money slot 9), 208 (unit registry), 209 (user-defined
//! units), and 2378 (`unit USD : Money` instances).

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;
use reify_test_support::eval_source;

// ─── test 1: runtime `(25USD/1kg) * 2kg → 50.0 USD` ──────────────────────────

/// At runtime, `(25USD/1kg) * 2kg` should evaluate to
/// `Value::Scalar { si_value: 50.0, dimension: MONEY }`, confirming that the
/// runtime `eval_mul`/`eval_div` path correctly cancels the MASS dimension and
/// propagates the SI product.
#[test]
fn money_per_mass_times_mass_evaluates_to_50_usd() {
    let source = "pub unit USD : Money\n\
                  structure S { param p : Money = (25USD/1kg) * 2kg }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - 50.0).abs() < 1e-9,
                "expected si_value 50.0, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MONEY,
                "expected MONEY dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── test 2: runtime `5GBP + 5USD → 11.25 USD` ───────────────────────────────

/// At runtime, `5GBP + 5USD` should evaluate to
/// `Value::Scalar { si_value: 11.25, dimension: MONEY }` — locking the
/// user-factor → SI-conversion → addition pipeline (5 * 1.25 + 5 * 1.0 = 11.25).
#[test]
fn cross_currency_addition_evaluates_to_1125_usd() {
    let source = "pub unit USD : Money\n\
                  unit GBP : Money = 1.25USD\n\
                  structure S { param p : Money = 5GBP + 5USD }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (*si_value - 11.25).abs() < 1e-9,
                "expected si_value 11.25 (5*1.25 + 5*1.0), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MONEY,
                "expected MONEY dimension, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}
