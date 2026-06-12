//! End-to-end smoke test for `compose(f, g)` via the worked example fixture.
//!
//! Exercises `compose` as a callable generic `.ri` stdlib fn from std.fields
//! (task 4224 ζ, PRD docs/prds/v0_6/std-fields-api.md §1 + §9 ζ).
//!
//! Boundary tests covered:
//!   B9 — `sample(compose(f, g), p)` evaluates to `sample(f, sample(g, p))`
//!          (compose(f_double, g_plus1)(3.0) = 2*(3+1) = 8.0)
//!
//! Model: `fn_field_example_smoke.rs` — same
//! `parse_and_compile_with_stdlib` → `Engine::eval` + `Engine::check` →
//! `Satisfaction::Satisfied` pattern.
//!
//! The test is RED before step-2's compose `.ri` fn lands: `compose(f, g)` has
//! no native eval dispatch (not yet a user fn), so it falls through to
//! `reify_stdlib::eval_builtin` → `Value::Undef` → `sample(Undef, ..)` →
//! `Undef` → constraints yield `Indeterminate`, not `Satisfied`.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::Satisfaction;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/compose.ri"
);

/// Read the fixture and compile it with the stdlib, asserting no error
/// diagnostics.  Returns the compiled program for further use.
///
/// Factored out to avoid duplicating the read + error-check boilerplate across
/// the two tests below.
fn compile_compose_fixture() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/compose.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in compose.ri, got: {:?}",
        errors_only(&compiled)
    );

    compiled
}

/// Compile `examples/fields/compose.ri` and verify it has no error-severity
/// diagnostics (compile-clean signal; faster subset of the full e2e test).
#[test]
fn compose_ri_compiles_with_stdlib() {
    compile_compose_fixture();
}

/// Eval and check `examples/fields/compose.ri` and verify all structure
/// constraints are `Satisfaction::Satisfied`.
///
/// The fixture declares **4** range constraints in `ComposeDemo`:
///   - `via_compose > 7.999` and `via_compose < 8.001` (compose sample at 3.0)
///   - `via_manual > 7.999` and `via_manual < 8.001` (manual nesting at 3.0)
///
/// The exact count is asserted as `>= 4` so that adding illustrative constraints
/// to `compose.ri` doesn't break this test — the per-entry Satisfied loop
/// provides the real behavioural signal (B9).
///
/// **RED before step-2**: constraints are `Indeterminate` (compose → Undef).
/// **GREEN after step-2**: all constraints are `Satisfied`.
#[test]
fn compose_constraints_all_satisfied() {
    let compiled = compile_compose_fixture();

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

    // At least the 4 constraints from compose.ri must be present and all
    // Satisfied.  Using >= rather than == so that adding illustrative
    // constraints to the fixture doesn't break this unrelated assertion.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 4,
        "expected at least 4 constraint results, got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?} — compose eval B9 signal",
            entry.id,
            entry.satisfaction
        );
    }
}
