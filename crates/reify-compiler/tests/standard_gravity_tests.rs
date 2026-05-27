//! Tests for the `STANDARD_GRAVITY` constant in `std/units` (task 3647).
//!
//! Pins two leaf signals:
//!
//!   1. The function `STANDARD_GRAVITY` is present in `std/units`, is `pub`,
//!      takes no parameters, and has return type `Scalar<ACCELERATION>`.
//!   2. Evaluating `STANDARD_GRAVITY()` via `eval_expr` yields
//!      `Value::Scalar { si_value ≈ 9.80665, dimension: ACCELERATION }`.
//!
//! Pattern lifted from `standard_stock_tests.rs` (zero-arg `pub fn` returning
//! a dimensioned scalar).

mod common;

use reify_core::{DimensionVector, Type};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ─── Test 1: STANDARD_GRAVITY present and has correct signature ───────────────

/// `STANDARD_GRAVITY` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<ACCELERATION>`.
#[test]
fn standard_gravity_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "STANDARD_GRAVITY")
        .unwrap_or_else(|| {
            panic!(
                "STANDARD_GRAVITY not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "STANDARD_GRAVITY should be pub");
    assert!(
        func.params.is_empty(),
        "STANDARD_GRAVITY should take no params, got: {:?}",
        func.params
    );
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: DimensionVector::ACCELERATION
        },
        "STANDARD_GRAVITY return type should be Scalar<ACCELERATION>, got {:?}",
        func.return_type
    );
}

// ─── Test 2: STANDARD_GRAVITY evaluates to 9.80665 m/s² ─────────────────────

/// Evaluating `STANDARD_GRAVITY()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 9.80665` and `dimension = ACCELERATION`.
///
/// Routing through `eval_expr` (rather than evaluating `func.body.result_expr`
/// directly) is intentional — it is robust against future refactors that
/// introduce `let` bindings inside the function body.
#[test]
fn standard_gravity_evaluates_to_9p80665_si_with_acceleration_dimension() {
    let module = common::units_module();

    let call_expr = CompiledExpr::user_function_call(
        "STANDARD_GRAVITY".to_string(),
        vec![],
        Type::Scalar {
            dimension: DimensionVector::ACCELERATION,
        },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::ACCELERATION,
                "STANDARD_GRAVITY() should have ACCELERATION dimension, got {:?}",
                dimension
            );
            assert!(
                (si_value - 9.80665).abs() < 1e-12,
                "STANDARD_GRAVITY() si_value: expected 9.80665, got {}",
                si_value
            );
        }
        other => panic!(
            "STANDARD_GRAVITY() should return Value::Scalar, got {:?}",
            other
        ),
    }
}
