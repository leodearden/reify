//! Integration tests for `Engine::edit_source` — incremental re-evaluation
//! across structural source edits.
//!
//! `edit_source` bridges the gap between `edit_param` (single-param incremental)
//! and `eval_cached` (same-content_hash fast path). It accepts a new
//! `CompiledModule` whose `content_hash` differs from the current one and
//! re-evaluates only the dependency cones touched by the structural diff.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{Engine, EngineError, EvalResult};
use reify_test_support::bracket_compiled_module;

use reify_compiler::CompiledModule;

/// Build a fresh Engine (no prior eval) backed by the real constraint checker.
fn fresh_engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None)
}

/// Run `eval(module_a)` then `edit_source(module_b)` on a single engine,
/// returning both results. Used by tests that compare the incremental edit
/// against the baseline eval.
#[allow(dead_code)]
fn run_eval_then_edit_source(
    module_a: &CompiledModule,
    module_b: &CompiledModule,
) -> (EvalResult, EvalResult) {
    let mut engine = fresh_engine();
    let a = engine.eval(module_a);
    let b = engine.edit_source(module_b).expect("edit_source should succeed");
    (a, b)
}

/// Run `eval(module_b)` on a fresh engine — used for the cold-eval
/// cross-check in the correctness test.
#[allow(dead_code)]
fn fresh_eval(module_b: &CompiledModule) -> EvalResult {
    let mut engine = fresh_engine();
    engine.eval(module_b)
}

// ── Precondition tests ─────────────────────────────────────────────────────

/// `edit_source` requires a prior `eval()` to establish the baseline snapshot,
/// mirroring the `edit_param` precondition. Calling `edit_source` on a freshly
/// constructed engine must return `Err(EngineError::NotInitialized)`.
#[test]
fn edit_source_returns_err_not_initialized_before_eval() {
    let mut engine = fresh_engine();
    let module = bracket_compiled_module();
    let result = engine.edit_source(&module);
    match result {
        Err(EngineError::NotInitialized) => {}
        other => panic!(
            "expected Err(EngineError::NotInitialized) before eval, got: {:?}",
            other.map(|r| r.values.len())
        ),
    }
}
