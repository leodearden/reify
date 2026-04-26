//! End-to-end Kleene 3-valued logic integration test.
//!
//! Exercises the full `compile_with_stdlib` → eval pipeline with a fixture
//! (`examples/kleene_e2e.ri`) that mixes `&&`, `||`, `!`-rewritten implication,
//! and `forall` over a list containing an undef element.
//!
//! # Spec coverage
//! - §9.2.3 – boolean operators under 3-valued (Kleene) semantics:
//!   AND absorption (`false && undef = false`),
//!   OR absorption (`undef || true = true`)
//! - §9.2.6 – quantifiers: `forall` with a mixed-Bool list propagates `Undef`
//!   when no element is `false` but at least one is `undef`.
//!
//! # Integration vector
//! Using `compile_with_stdlib` (via `parse_and_compile_with_stdlib`) catches
//! regressions where the Kleene operators work in isolation but break under
//! the real stdlib type-inference registry — e.g., `Bool` being widened to a
//! type that the Kleene evaluator doesn't recognise.

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Severity, Value, ValueCellId};

// ── Path constant ─────────────────────────────────────────────────────────────

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kleene_e2e.ri"
);

// ── Cached helpers ────────────────────────────────────────────────────────────

/// Read `examples/kleene_e2e.ri`, caching the result. Returns `&'static str`.
fn source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .unwrap_or_else(|e| panic!("{EXAMPLE_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile `examples/kleene_e2e.ri` with stdlib, caching the result.
/// Panics on any parse or compile error.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source()))
}

/// Evaluate `examples/kleene_e2e.ri` with a fresh `SimpleConstraintChecker` engine.
fn eval_kleene() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    let result = engine.eval(compiled());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "eval-phase errors: {errors:?}");
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Kleene AND absorption: `undef && false = false`.
///
/// The `a` param has no default → `Value::Undef`. `b = false`.
/// `p1 = a && b` should short-circuit on the right operand and return `Bool(false)`.
#[test]
fn kleene_e2e_and_absorption() {
    let result = eval_kleene();
    let id = ValueCellId::new("Foo", "p1");
    assert_eq!(
        result.values.get(&id).expect("Foo.p1 not found"),
        &Value::Bool(false),
        "undef && false should be Bool(false) (Kleene AND absorption)"
    );
}
