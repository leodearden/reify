//! End-to-end smoke test for `fn_field` via the worked example fixture.
//!
//! Exercises `fn_field` as a user-facing built-in that wraps a lambda as an
//! Analytical Field (task 4220 β, PRD docs/prds/v0_6/std-fields-api.md §5.2).
//!
//! Boundary tests covered:
//!   B1  — `sample(fn_field(|p| 2.0 * p), 3.0)` evaluates to 6.0 via reify eval
//!   B10 — the sampled values type as Real (codomain), enabling numeric range
//!          constraints (proven implicitly: the constraints compare Real scalars)
//!
//! Model: `field_source_kinds_smoke.rs` — same
//! `parse_and_compile_with_stdlib` → `Engine::eval` + `Engine::check` →
//! `Satisfaction::Satisfied` pattern.
//!
//! The test is RED before step-2's fn_field eval arm lands: `fn_field` falls
//! through to `reify_stdlib::eval_builtin` (no binding) → `Value::Undef` →
//! `sample(Undef, ..)` → `Undef` → constraints yield `Indeterminate`, not
//! `Satisfied`.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/fn_field.ri"
);

/// Compile `examples/fields/fn_field.ri` and verify it has no error-severity
/// diagnostics (compile-clean signal; faster subset of the full e2e test).
#[test]
fn fn_field_ri_compiles_with_stdlib() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/fn_field.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in fn_field.ri, got: {:?}",
        errors_only(&compiled)
    );
}

/// Eval and check `examples/fields/fn_field.ri` and verify all structure
/// constraints are `Satisfaction::Satisfied`.
///
/// The fixture declares exactly **4** range constraints in `FnFieldDemo`:
///   - `doubled_at_3 > 5.999` and `doubled_at_3 < 6.001` (fn_field sample at 3.0)
///   - `plus1_at_4 > 4.999` and `plus1_at_4 < 5.001` (fn_field sample at 4.0)
///
/// If you add constraints to `fn_field.ri`, update the exact count below.
///
/// **RED before step-2**: constraints are `Indeterminate` (fn_field → Undef).
/// **GREEN after step-2**: all 4 constraints are `Satisfied`.
#[test]
fn fn_field_constraints_all_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/fn_field.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    // Compile must be clean.
    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors, got: {:?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Check constraints — exactly 4, one per range bound in FnFieldDemo.
    let check = engine.check(&compiled);
    assert_eq!(
        check.constraint_results.len(),
        4,
        "expected exactly 4 constraint results, got {}",
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
