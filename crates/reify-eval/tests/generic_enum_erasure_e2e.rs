//! Generic-enum end-to-end integration gate (task ε #4033, step-1).
//!
//! Confirms the full generic data-carrying-enum eval pipeline — construction
//! type-arg inference (γ #4031), type-preserving pattern binders (δ #4032),
//! and DCE payload-binding eval (ζ #3946) — is wired end-to-end, and that eval
//! is UNCHANGED under F-Mono type erasure (D1/INV-2, PRD
//! docs/prds/v0_6/generic-data-carrying-enums.md §8).
//!
//! Mirrors crates/reify-eval/tests/m6_data_carrying_enum.rs: parse → compile →
//! eval; extract Demo.bore / Demo.r.
//!
//! Tests:
//!   1. bore_ok_default_is_5mm (PRIMARY §1 signal) — `Ok { value: 5mm }` default
//!      → Demo.bore = 0.005 m; Demo.r = Result::Ok.
//!   (tree_sum_total_is_3mm, bore_err_switch_is_6mm, and
//!   recursive_tree_decl_emits_no_error_diagnostics are added in step-3.)

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

// ── helper ───────────────────────────────────────────────────────────────────

fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ── test 1: PRIMARY §1 signal ─────────────────────────────────────────────────

/// `reify eval examples/m6_generic_enum.ri` → Demo.bore = 0.005 m
/// (`Ok { value: 5mm }` default, extracted through the Ok arm's `v` binder).
///
/// The PRD §1/§8 user-observable signal. RED until step-2 authors
/// examples/m6_generic_enum.ri: `read_to_string(...)` panics.
#[test]
fn bore_ok_default_is_5mm() {
    let source = std::fs::read_to_string("../../examples/m6_generic_enum.ri")
        .expect("examples/m6_generic_enum.ri should exist");

    let result = eval_source(&source);

    let bore_id = ValueCellId::new("Demo", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Demo.bore not found in eval result"));

    match bore_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected Demo.bore ≈ 0.005 m (5mm), got {si_value} m"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Demo", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Demo.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(variant, "Ok", "Demo.r should be Result::Ok (default variant)");
        }
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
}
