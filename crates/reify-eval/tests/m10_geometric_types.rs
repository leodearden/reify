//! M10 geometric types integration tests.
//!
//! Exercises geometric type features through the full pipeline:
//! parse → compile → eval/check → verify.
//! Uses examples/m10_geometric_types.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_core::{ModulePath, Severity};
use reify_ir::Satisfaction;

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_geometric_types.ri"
);

// ── Step 1: parse succeeds ──────────────────────────────────────────

/// Read m10_geometric_types.ri and verify it parses without errors.
#[test]
fn geometric_types_ri_parses() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_geometric_types.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

// ── Step 2: compile succeeds ────────────────────────────────────────

/// Compile the .ri file and verify no compile errors.
#[test]
fn geometric_types_compiles() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_geometric_types.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    // Should have at least 1 template (GeometricDemo)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

    let demo = compiled
        .templates
        .iter()
        .find(|t| t.name == "GeometricDemo")
        .expect("should have a GeometricDemo template");

    // Should have at least 12 constraints
    assert!(
        demo.constraints.len() >= 12,
        "expected >=12 constraints, got {}",
        demo.constraints.len()
    );
}

// ── Step 3: eval + check all constraints satisfied ──────────────────

/// Compile, eval, and verify all constraints are Satisfied with SimpleConstraintChecker.
#[test]
fn geometric_types_all_constraints_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_geometric_types.ri should exist");

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
        check.constraint_results.len() >= 12,
        "expected >=12 constraint results, got {}",
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
