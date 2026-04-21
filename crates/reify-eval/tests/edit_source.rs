//! Integration tests for `Engine::edit_source` — incremental re-evaluation
//! across structural source edits.
//!
//! `edit_source` bridges the gap between `edit_param` (single-param incremental)
//! and `eval_cached` (same-content_hash fast path). It accepts a new
//! `CompiledModule` whose `content_hash` differs from the current one and
//! re-evaluates only the dependency cones touched by the structural diff.

use std::collections::HashSet;

use reify_constraints::SimpleConstraintChecker;
use reify_eval::cache::NodeId;
use reify_eval::{Engine, EngineError, EvalResult};
use reify_test_support::{bracket_compiled_module, parse_and_compile};

use reify_compiler::CompiledModule;
use reify_types::{SnapshotProvenance, Value, ValueCellId};

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

// ── Single-let structural edit ─────────────────────────────────────────────

/// Bracket source with a configurable `volume` let expression.  The params
/// are fixed at the canonical bracket defaults so that tests can mutate only
/// the let-binding to exercise the single-expression diff path.
fn bracket_with_volume_expr(volume_expr: &str) -> String {
    format!(
        r#"structure Bracket {{
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = {volume_expr}

    constraint thickness > 2mm
}}"#
    )
}

/// Pull the SI numeric value out of a `Value::Scalar`, panicking otherwise.
fn si(value: &Value, label: &str) -> f64 {
    match value {
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected Scalar for {label}, got {other:?}"),
    }
}

/// Changing only the `volume` let expression must (a) update `volume` to
/// the new expression's value, (b) leave the param values unchanged, and
/// (c) make `last_eval_set()` contain the dependent `volume` node while
/// excluding the unchanged param nodes. This is the canonical single-diff
/// case that drives step-6's graph-diff + dirty-cone logic.
#[test]
fn edit_source_modified_let_reevaluates_only_dependents() {
    let mut engine = fresh_engine();

    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    let result_a = engine.eval(&module_a);

    let e = "Bracket";
    let volume_id = ValueCellId::new(e, "volume");
    let width_id = ValueCellId::new(e, "width");
    let height_id = ValueCellId::new(e, "height");
    let thickness_id = ValueCellId::new(e, "thickness");

    let volume_a = si(
        result_a
            .values
            .get(&volume_id)
            .expect("volume must be computed by eval(module_a)"),
        "volume_a",
    );

    // Module B: volume = original * 2.0.  Params unchanged.
    let module_b =
        parse_and_compile(&bracket_with_volume_expr("width * height * thickness * 2.0"));
    let result_b = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let volume_b = si(
        result_b
            .values
            .get(&volume_id)
            .expect("volume must be present in edit_source result"),
        "volume_b",
    );
    assert!(
        (volume_b - 2.0 * volume_a).abs() < 1e-12,
        "volume_b should be 2 * volume_a; got volume_a={volume_a}, volume_b={volume_b}"
    );

    // Unchanged params retain their values.
    assert_eq!(
        result_b.values.get(&width_id),
        result_a.values.get(&width_id),
        "width must be preserved across a pure-let edit"
    );
    assert_eq!(
        result_b.values.get(&height_id),
        result_a.values.get(&height_id),
        "height must be preserved across a pure-let edit"
    );
    assert_eq!(
        result_b.values.get(&thickness_id),
        result_a.values.get(&thickness_id),
        "thickness must be preserved across a pure-let edit"
    );

    // The eval set contains Value(volume) but not the unchanged params —
    // the dirty cone excluded them via content_hash equality.
    let eval_set = engine.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Value(volume_id.clone())),
        "last_eval_set must contain volume, got: {:?}",
        eval_set
    );
    for param in [&width_id, &height_id, &thickness_id] {
        assert!(
            !eval_set.contains(&NodeId::Value(param.clone())),
            "last_eval_set must NOT contain unchanged param {param}, got: {:?}",
            eval_set
        );
    }
}

// ── Added / removed let bindings ───────────────────────────────────────────

/// Adding a brand-new let binding (`perimeter`) to module_B must (a) evaluate
/// it against the current param values, (b) preserve the unchanged `volume`
/// value, (c) include the added cell in `last_eval_set()`, and (d) leave all
/// params untouched. This locks the "added cell" diff path: the cell is in
/// neither the old snapshot nor the old cache, so the eval loop must fill it
/// in from scratch without disturbing any upstream cached state.
#[test]
fn edit_source_added_cell_is_evaluated_and_unchanged_cells_preserved() {
    let mut engine = fresh_engine();

    // Module A: canonical bracket with volume let. No perimeter.
    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    let result_a = engine.eval(&module_a);

    let e = "Bracket";
    let volume_id = ValueCellId::new(e, "volume");
    let perimeter_id = ValueCellId::new(e, "perimeter");
    let width_id = ValueCellId::new(e, "width");
    let height_id = ValueCellId::new(e, "height");
    let thickness_id = ValueCellId::new(e, "thickness");

    let volume_a = result_a
        .values
        .get(&volume_id)
        .expect("volume must be computed by eval(module_a)")
        .clone();
    let width_a = result_a.values.get(&width_id).cloned();
    let height_a = result_a.values.get(&height_id).cloned();
    let thickness_a = result_a.values.get(&thickness_id).cloned();

    // Module B: identical to A except a new `perimeter = 2 * (width + height)`
    // let binding is inserted after `volume`. No other semantic changes.
    let module_b_src = format!(
        r#"structure Bracket {{
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness
    let perimeter = 2.0 * (width + height)

    constraint thickness > 2mm
}}"#
    );
    let module_b = parse_and_compile(&module_b_src);
    let result_b = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // (a) perimeter evaluated against the canonical param defaults
    //     2 * (80mm + 100mm) = 360mm = 0.36 m.
    let perimeter_b = si(
        result_b
            .values
            .get(&perimeter_id)
            .expect("perimeter must be present in edit_source result"),
        "perimeter_b",
    );
    assert!(
        (perimeter_b - 0.36).abs() < 1e-12,
        "perimeter_b should be 0.36 m (2 * (80mm + 100mm)); got {perimeter_b}"
    );

    // (b) volume preserved — its content_hash is unchanged across the edit.
    assert_eq!(
        result_b.values.get(&volume_id),
        Some(&volume_a),
        "volume must be preserved when only a new let is added"
    );

    // (c) last_eval_set contains the added cell but not the unchanged params.
    let eval_set = engine.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Value(perimeter_id.clone())),
        "last_eval_set must contain the added perimeter, got: {:?}",
        eval_set
    );
    for param in [&width_id, &height_id, &thickness_id] {
        assert!(
            !eval_set.contains(&NodeId::Value(param.clone())),
            "last_eval_set must NOT contain unchanged param {param}, got: {:?}",
            eval_set
        );
    }

    // (d) params retained verbatim.
    assert_eq!(result_b.values.get(&width_id), width_a.as_ref());
    assert_eq!(result_b.values.get(&height_id), height_a.as_ref());
    assert_eq!(result_b.values.get(&thickness_id), thickness_a.as_ref());
}

/// Removing a let binding (`volume`) from module_B must (a) drop that cell's
/// entry from the returned `values` map, (b) drop it from
/// `snapshot.graph.value_cells`, and (c) leave the retained params untouched.
/// This locks the "removed cell" diff path: the cell was evaluated by
/// module_A but is absent from module_B, so seeding + eval must skip it
/// and cache invalidation must not surface it downstream.
#[test]
fn edit_source_removed_cell_drops_value_from_map() {
    let mut engine = fresh_engine();

    // Module A: canonical bracket with volume let.
    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    let _result_a = engine.eval(&module_a);

    let e = "Bracket";
    let volume_id = ValueCellId::new(e, "volume");
    let width_id = ValueCellId::new(e, "width");
    let height_id = ValueCellId::new(e, "height");
    let thickness_id = ValueCellId::new(e, "thickness");

    // Module B: drop the `volume` let entirely. Params and constraint stay.
    let module_b_src = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    constraint thickness > 2mm
}"#;
    let module_b = parse_and_compile(module_b_src);
    let result_b = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // (a) values map contains no entry for the removed volume cell.
    assert!(
        result_b.values.get(&volume_id).is_none(),
        "removed volume must not appear in the values map; got: {:?}",
        result_b.values.get(&volume_id)
    );

    // (b) snapshot's graph no longer carries the removed cell.
    let snapshot = engine.snapshot().expect("snapshot must be installed");
    assert!(
        !snapshot.graph.value_cells.contains_key(&volume_id),
        "removed volume must be absent from snapshot.graph.value_cells"
    );

    // (c) retained params are still present.
    for param in [&width_id, &height_id, &thickness_id] {
        assert!(
            result_b.values.get(param).is_some(),
            "retained param {param} must still have a value after removal edit"
        );
        assert!(
            snapshot.graph.value_cells.contains_key(param),
            "retained param {param} must still be present in the graph"
        );
    }
}
