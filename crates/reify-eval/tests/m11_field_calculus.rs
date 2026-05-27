//! M11 field calculus integration tests.
//!
//! Exercises field differential operators (gradient, sample) through the full
//! pipeline: parse → compile → eval/check → verify.
//! Uses examples/m11_field_calculus.ri as the source file.
//!
//! Follows the m10_geometric_types.rs pattern exactly.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_core::{ModulePath, Severity};
use reify_ir::Satisfaction;

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m11_field_calculus.ri"
);

/// Read m11_field_calculus.ri and verify it parses without errors.
#[test]
fn field_calculus_ri_parses() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m11_field_calculus.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

/// Compile the .ri file and verify no compile errors.
#[test]
fn field_calculus_compiles() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m11_field_calculus.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    // Should have at least 1 template (FieldCalculusDemo)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

    let demo = compiled
        .templates
        .iter()
        .find(|t| t.name == "FieldCalculusDemo")
        .expect("should have a FieldCalculusDemo template");

    // Should have at least 6 constraints
    assert!(
        demo.constraints.len() >= 6,
        "expected >=6 constraints, got {}",
        demo.constraints.len()
    );
}

/// Compile, eval, and verify all constraints are Satisfied with SimpleConstraintChecker.
#[test]
fn field_calculus_all_constraints_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m11_field_calculus.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Check constraints
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 6,
        "expected >=6 constraint results, got {}",
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
