//! End-to-end smoke test for `pointwise_max(f, g)` and `pointwise_min(f, g)`
//! from std.fields (task #4629 W4).
//!
//! Boundary tests covered:
//!   W4 — `sample(pointwise_max(f, g), p)` == `max(sample(f, p), sample(g, p))`
//!   W4 — `sample(pointwise_min(f, g), p)` == `min(sample(f, p), sample(g, p))`
//!
//! These are COMBINE-form combinators (field × field → field), distinct from
//! the REDUCE form `max(field)` → scalar (W1).  Each result is a Field<D, Scalar<Q>>
//! whose per-point value is the pointwise max/min of the two input fields.
//!
//! Model: `compose_example_smoke.rs` — same
//! `parse_and_compile_with_stdlib` → `Engine::eval` + `Engine::check` →
//! `Satisfaction::Satisfied` pattern.
//!
//! The test is RED before step-10's `pointwise_max`/`pointwise_min` `.ri` fns land:
//! the identifiers are unresolved → compile Error → constraints yield Indeterminate,
//! not Satisfied.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/pointwise_field_combinators.ri"
);

/// Read the fixture and compile it with the stdlib, asserting no error
/// diagnostics.  Returns the compiled program for further use.
fn compile_pointwise_fixture() -> CompiledModule {
    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/fields/pointwise_field_combinators.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in pointwise_field_combinators.ri, got: {:?}",
        errors_only(&compiled)
    );

    compiled
}

/// Compile the fixture and verify it has no error-severity diagnostics
/// (compile-clean signal; faster subset of the full e2e test).
///
/// RED before step-10: `pointwise_max`/`pointwise_min` are undeclared identifiers →
/// compile Error.
#[test]
fn pointwise_field_combinators_compile_with_stdlib() {
    compile_pointwise_fixture();
}

/// Eval and check the fixture and verify all structure constraints are
/// `Satisfaction::Satisfied`.
///
/// The fixture declares **8** range constraints in `PointwiseDemo`:
///   - `pmax_at_3 > 5.999` and `< 6.001` (pointwise_max sample at 3.0)
///   - `pmin_at_3 > 3.999` and `< 4.001` (pointwise_min sample at 3.0)
///   - `pmax_at_half > 1.499` and `< 1.501` (pointwise_max sample at 0.5)
///   - `pmin_at_half > 0.999` and `< 1.001` (pointwise_min sample at 0.5)
///
/// The exact count is asserted as `>= 8` so that adding constraints to the
/// fixture doesn't break this test — the per-entry Satisfied loop provides the
/// real behavioural signal (W4).
///
/// RED before step-10: constraints are Indeterminate (unresolved identifiers).
/// GREEN after step-10: all constraints are Satisfied.
#[test]
fn pointwise_field_combinators_all_constraints_satisfied() {
    let compiled = compile_pointwise_fixture();

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

    // At least the 8 constraints from the fixture must be present and all Satisfied.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 8,
        "expected at least 8 constraint results, got {} — W4 pointwise combinator signal",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?} — W4 pointwise combinator signal",
            entry.id,
            entry.satisfaction
        );
    }
}
