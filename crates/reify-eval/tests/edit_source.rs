//! Integration tests for `Engine::edit_source` — incremental re-evaluation
//! across structural source edits.
//!
//! `edit_source` bridges the gap between `edit_param` (single-param incremental)
//! and `eval_cached` (same-content_hash fast path). It accepts a new
//! `CompiledModule` whose `content_hash` differs from the current one and
//! re-evaluates only the dependency cones touched by the structural diff.

use std::collections::HashSet;

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{Engine, EngineError, EvalResult};
use reify_test_support::bracket_compiled_module;

use reify_compiler::CompiledModule;
use reify_types::SnapshotProvenance;

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

// ── Identity / no-change path ──────────────────────────────────────────────

/// When `edit_source` is handed an identical module (no structural edit),
/// every cached value must be preserved and no re-evaluation should occur.
/// Provenance must be `Edit { changed: ∅, parent: <prior snapshot id> }`.
#[test]
fn edit_source_with_identical_module_preserves_all_values() {
    let mut engine = fresh_engine();
    let module = bracket_compiled_module();

    let eval_result = engine.eval(&module);
    let parent_id = engine
        .snapshot()
        .expect("eval() must populate a snapshot")
        .id;

    // Clone-equivalent: a second fixture build yields the same graph/content.
    let module_clone = bracket_compiled_module();
    let edit_result = engine
        .edit_source(&module_clone)
        .expect("edit_source must succeed after eval");

    // Values map equals the pre-edit baseline, entry-for-entry.
    for (id, val) in eval_result.values.iter() {
        assert_eq!(
            edit_result.values.get(id),
            Some(val),
            "value for {id} diverged from eval baseline after no-op edit_source"
        );
    }
    assert_eq!(
        edit_result.values.len(),
        eval_result.values.len(),
        "edit_source values map must have the same size as eval baseline"
    );

    // Provenance: Edit with an empty changed set and the pre-edit parent id.
    match &engine
        .snapshot()
        .expect("edit_source must install a snapshot")
        .provenance
    {
        SnapshotProvenance::Edit { changed, parent } => {
            assert_eq!(
                changed,
                &HashSet::new(),
                "no-op edit_source must leave changed set empty, got: {:?}",
                changed
            );
            assert_eq!(
                *parent, parent_id,
                "no-op edit_source parent must equal the pre-edit snapshot id"
            );
        }
        other => panic!(
            "expected SnapshotProvenance::Edit after edit_source, got: {:?}",
            other
        ),
    }
}
