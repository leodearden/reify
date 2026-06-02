//! End-to-end eval gate for trait-static associated-fn dispatch (task η 3945).
//!
//! `examples/trait_assoc_fn_static.ri` declares a trait with a body-carrying
//! static fn (`make_default`, no `self`) and a second arg-taking variant
//! (`scaled(factor)`), then exercises them from a `structure def`.
//!
//! This test:
//! 1. Compiles and evals the example with the stdlib.
//! 2. Asserts no Error-severity diagnostics.
//! 3. Asserts the `Spacer.gap` cell is a `Value::Scalar` with `si_value` ≈ 0.01
//!    (10mm in SI metres — the `make_default()` return value).
//! 4. Asserts the `Spacer.wide` cell is a `Value::Scalar` with `si_value` ≈ 0.03
//!    (30mm in SI metres — `scaled(3)` → `10mm * 3`).
//!
//! RED until step-6 authors `examples/trait_assoc_fn_static.ri`.

#![allow(clippy::mutable_key_type)]

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

fn source() -> &'static str {
    include_str!("../../../examples/trait_assoc_fn_static.ri")
}

#[test]
fn trait_static_fn_dispatch_end_to_end() {
    let compiled = parse_and_compile_with_stdlib(source());
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);

    // The example must compile and eval with no Error diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // Spacer.gap = Defaultable::make_default() → 10mm = 0.01 m in SI.
    let gap_id = ValueCellId::new("Spacer", "gap");
    match eval_result.values.get(&gap_id) {
        Some(Value::Scalar { si_value, .. }) => {
            let expected = 0.01_f64; // 10mm
            let tol = 1e-9_f64;
            assert!(
                (si_value - expected).abs() < tol,
                "Spacer.gap: expected {expected} m (10mm), got {si_value} m"
            );
        }
        other => panic!("Spacer.gap: expected Value::Scalar (10mm), got {:?}", other),
    }

    // Spacer.wide = Defaultable::scaled(3) → 10mm * 3 = 30mm = 0.03 m in SI.
    let wide_id = ValueCellId::new("Spacer", "wide");
    match eval_result.values.get(&wide_id) {
        Some(Value::Scalar { si_value, .. }) => {
            let expected = 0.03_f64; // 30mm
            let tol = 1e-9_f64;
            assert!(
                (si_value - expected).abs() < tol,
                "Spacer.wide: expected {expected} m (30mm), got {si_value} m"
            );
        }
        other => panic!(
            "Spacer.wide: expected Value::Scalar (30mm), got {:?}",
            other
        ),
    }
}
