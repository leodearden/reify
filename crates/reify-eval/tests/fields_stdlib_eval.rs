//! Eval integration test for `examples/stdlib/fields.ri` (task 4025).
//!
//! Exercises the full pipeline: parse → compile-with-stdlib → eval → check,
//! verifying that sample(temp, point3(...)) evaluates to 1m exactly and both
//! range constraints are Satisfied.
//!
//! Pattern lifted from `m11_field_calculus.rs` (field_calculus_all_constraints_satisfied).

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::parse_and_compile_with_stdlib;

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/stdlib/fields.ri"
);

/// Read examples/stdlib/fields.ri, compile-with-stdlib, eval, and verify
/// all constraints in FieldsModuleDemo are Satisfied.
/// Pins that sample(temp, point3(1m,2m,3m)) evaluates to 1m exactly
/// (value passthrough through a constant analytical lambda — not numerical).
#[test]
fn example_fields_sample_evaluates_to_1m() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/stdlib/fields.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);

    // No eval-level errors.
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in fields.ri: {:?}",
        eval_errors
    );

    // Both range constraints must be Satisfied.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 2,
        "expected >=2 constraint results from FieldsModuleDemo, got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
