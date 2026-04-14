//! M10 combined integration tests.
//!
//! Exercises all M10 milestone features in a single cohesive example:
//! geometric type params, Point/Vector arithmetic in lets, Frame/Transform
//! in port definitions, connect with connector type and port mapping, purpose
//! checking geometric determinacy, ad-hoc port selector, and where-block
//! reference safety. Uses examples/m10_combined.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_combined.ri"
);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read and return the contents of the m10_combined.ri example file.
/// The file is read only once per test process (cached in a `OnceLock`);
/// each caller receives an owned clone.
fn source() -> String {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/m10_combined.ri should exist")
    })
    .clone()
}

/// Parse, compile (with stdlib), eval with SimpleConstraintChecker, return EvalResult.
/// Use when asserting on values (geometric, scalars, etc.).
fn eval_source(src: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.eval(&compiled)
}

/// Parse, compile (with stdlib), check with SimpleConstraintChecker, return CheckResult.
/// Use when asserting on constraint satisfaction and counts.
fn check_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled)
}

/// Returns the constraint count from the current engine snapshot.
/// Mirrors the helper in purpose_activation.rs.
fn constraint_count(engine: &Engine) -> usize {
    engine
        .snapshot()
        .expect("snapshot should exist")
        .graph
        .constraints
        .len()
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: file produces constraint results and all are Satisfied.
#[test]
fn all_constraints_satisfied() {
    todo!("step-6 impl: assert all constraint results are Satisfied")
}

// ── Test 2: compiles with Assembly template ──────────────────────────────────

/// Compile m10_combined.ri (with stdlib) and verify the compiled module contains
/// an Assembly template (confirming compile-cleanliness and top-level structure name).
#[test]
fn m10_combined_compiles_with_assembly_template() {
    let compiled = parse_and_compile_with_stdlib(&source());
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("should have an Assembly template");
    assert!(
        !assembly.value_cells.is_empty(),
        "Assembly should have value cells (params and lets)"
    );
}

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m10_combined.ri and verify it parses without errors.
/// Note: the file may produce compiler warnings (e.g., orient_identity type inference)
/// but no error-severity diagnostics.
#[test]
fn m10_combined_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
