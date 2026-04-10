//! M9 pipeline integration tests.
//!
//! Exercises cross-feature composition combining all three M9 milestone features:
//! constraint def instantiation, trait conformance, and determinacy predicates.
//!
//! Cross-cutting scenarios tested:
//!   1. Constraint defs whose predicates use determinacy predicates internally
//!   2. Traits with determinacy constraints injected into implementing structures
//!   3. Recursive structures whose sub guards use determinacy predicates
//!   4. Multi-trait structures combining constraint defs, trait defaults, and determinacy
//!
//! Uses `examples/m9_integration.ri` as the capstone source file and inline source
//! strings for focused per-scenario assertions.

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the integration example file, resolved at compile time from crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_integration.ri"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module ready for eval or check.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

/// Parse, compile, eval with SimpleConstraintChecker, return EvalResult.
/// Use when asserting on values (SI scalars, strings, booleans).
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

/// Parse, compile, check with SimpleConstraintChecker, return CheckResult.
/// Use when asserting on constraint satisfaction, labels, and counts.
fn check_source(source: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile(source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.check(&compiled)
}
