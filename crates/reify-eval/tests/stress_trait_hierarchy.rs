//! Stress tests for trait hierarchy via trait_hierarchy.ri fixture.
//!
//! Covers:
//!   - smoke test: fixture parses, compiles, evaluates without errors
//!   - 3-deep chain value assertions: x (Root), y (Middle), computed let, z (Leaf)
//!   - 3-deep chain constraint assertions: all levels enforced
//!   - diamond inheritance: single 'x' member, all constraints enforced
//!   - multi-trait implementation: 3+ independent traits, all params/constraints
//!   - constrained diamond: conjunction of constraints from all levels

use std::fs;

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, ValueCellId};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Load a .ri file, parse, compile (asserting no errors), and evaluate.
/// Returns the full EvalResult for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        errors
    );
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );
    result
}

/// Load and compile a .ri file, returning both compiled module and eval result.
fn compile_and_eval_ri(path: &str, module_name: &str) -> (reify_compiler::CompiledModule, reify_eval::EvalResult) {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        errors
    );
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );
    (compiled, result)
}

// ── step-1: smoke test ────────────────────────────────────────────────────────

/// Load trait_hierarchy.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn trait_hierarchy_parses_and_compiles() {
    let result = eval_ri_file(
        "../../examples/trait_hierarchy.ri",
        "trait_hierarchy",
    );
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for trait_hierarchy.ri"
    );
}
