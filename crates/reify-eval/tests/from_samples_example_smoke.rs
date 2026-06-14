//! End-to-end smoke test for `from_samples` via the worked example fixture.
//!
//! Exercises `from_samples` as a user-facing built-in that constructs a
//! gridded (Regular1D) SampledField from explicit sample points
//! (task 4221 γ, PRD docs/prds/v0_6/std-fields-api.md §D3/D5).
//!
//! Boundary tests covered:
//!   B2 — `sample(from_samples([0,1,2],[0,10,20], InterpolationMethod.Linear), 0.5) == 5.0`
//!        via reify check: both range constraints `v > 4.999` and `v < 5.001` are Satisfied.
//!   B3 — non-uniform spacing → `DiagnosticCode::FieldSamplesNotGrid` Error in result.diagnostics
//!        (added in step-5 after the diagnostic variant lands in step-6)
//!   B4 — unsupported method (RBF) → `DiagnosticCode::InterpMethodUnsupported` Error
//!        (added in step-7 after the diagnostic variant lands in step-8)
//!
//! Model: `fn_field_example_smoke.rs` — same
//! `parse_and_compile_with_stdlib` → `Engine::eval` + `Engine::check` →
//! `Satisfaction::Satisfied` pattern.
//!
//! The B2 test is RED before step-4's eval_from_samples arm lands: from_samples
//! falls through to `reify_stdlib::eval_builtin` (no binding) → `Value::Undef`
//! → `sample(Undef, ..)` → Undef → constraints yield `Indeterminate`.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/from_samples.ri"
);

/// Read the fixture and compile it with the stdlib, asserting no error
/// diagnostics.  Returns the compiled program for further use.
fn compile_from_samples_fixture() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/from_samples.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in from_samples.ri, got: {:?}",
        errors_only(&compiled)
    );

    compiled
}

/// Compile `examples/fields/from_samples.ri` and verify it has no error-severity
/// diagnostics (compile-clean signal).
#[test]
fn from_samples_ri_compiles_with_stdlib() {
    compile_from_samples_fixture();
}

/// B2 via reify check: eval and check `examples/fields/from_samples.ri` and
/// verify all structure constraints are `Satisfaction::Satisfied`.
///
/// The fixture declares **2** range constraints in `FromSamplesDemo`:
///   - `v > 4.999` and `v < 5.001`
///
/// **RED before step-4**: constraints are `Indeterminate` (from_samples → Undef).
/// **GREEN after step-4**: all constraints are `Satisfied`.
#[test]
fn from_samples_constraints_all_satisfied_b2() {
    let compiled = compile_from_samples_fixture();

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

    // At least the 2 constraints from from_samples.ri must be present and
    // all Satisfied.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 2,
        "expected at least 2 constraint results, got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied (B2), got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
