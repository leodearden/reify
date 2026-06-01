//! Integration tests for `Engine::edit_source` — incremental re-evaluation
//! across structural source edits.
//!
//! `edit_source` bridges the gap between `edit_param` (single-param incremental)
//! and `eval_cached` (same-content_hash fast path). It accepts a new
//! `CompiledModule` whose `content_hash` differs from the current one and
//! re-evaluates only the dependency cones touched by the structural diff.

mod common;

use std::collections::HashSet;

use reify_constraints::SimpleConstraintChecker;
use reify_eval::cache::NodeId;
use reify_eval::{Engine, EngineError, EvalResult};
use reify_test_support::{
    MockConstraintChecker, bracket_compiled_module, parse_and_compile, wave2_flip_fixture,
};

use reify_compiler::CompiledModule;
use reify_core::{ConstraintNodeId, RealizationNodeId, ValueCellId};
use reify_ir::{Satisfaction, SnapshotProvenance, Value, ValueMap};

use common::ten_bool_guarded_groups;

/// Build a fresh Engine (no prior eval) backed by the real constraint checker.
fn fresh_engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None)
}

/// Cross-check two value maps entry-for-entry via key-union.
/// Panics on the first diverging key with a message identical to the
/// inline pattern previously copy-pasted across the Task 2087 tests.
fn assert_values_match(incr: &ValueMap, cold: &ValueMap) {
    let incr_keys: HashSet<&ValueCellId> = incr.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.get(key);
        let cold_val = cold.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }
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
    let b = engine
        .edit_source(module_b)
        .expect("edit_source should succeed");
    (a, b)
}

/// Run `eval(module_b)` on a fresh engine — used for the cold-eval
/// cross-check in the correctness test.
#[allow(dead_code)]
fn fresh_eval(module_b: &CompiledModule) -> EvalResult {
    let mut engine = fresh_engine();
    engine.eval(module_b)
}

/// Run a role-flip probe scenario end-to-end: parse both sources, run
/// `eval(A) + edit_source(B)` on one fresh engine, run cold `eval(B)` on
/// another fresh engine, cross-check the two value maps agree, and return
/// `(cold_result, probes)` for the caller's per-test positive-anchor and
/// perf-lock assertions.
#[allow(dead_code)]
fn run_probe_scenario(src_a: &str, src_b: &str) -> (EvalResult, usize) {
    let module_a = parse_and_compile(src_a);
    let module_b = parse_and_compile(src_b);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");
    let probes = incremental.last_role_flip_probes();

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    assert_values_match(&incr.values, &cold_result.values);

    (cold_result, probes)
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
    let module_b = parse_and_compile(&bracket_with_volume_expr(
        "width * height * thickness * 2.0",
    ));
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
    let module_b_src = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness
    let perimeter = 2.0 * (width + height)

    constraint thickness > 2mm
}"#;
    let module_b = parse_and_compile(module_b_src);
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

// ── Constraint diff (changed / added) ──────────────────────────────────────

/// Bracket source with a configurable constraint expression on `thickness`.
/// Params are fixed at canonical defaults; only the constraint text varies.
fn bracket_with_constraint(constraint_expr: &str) -> String {
    format!(
        r#"structure Bracket {{
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness

    constraint {constraint_expr}
}}"#
    )
}

/// Changing a constraint's expression in-place must (a) flip its satisfaction
/// under `check_snapshot` (which reads the engine's installed snapshot) and
/// (b) place the constraint's node in `last_eval_set()`. thickness=5mm
/// satisfies `> 2mm` but violates `> 10mm`, so the same params produce
/// different outcomes purely from the structural edit.
#[test]
fn edit_source_modified_constraint_invalidates_check_result() {
    let mut engine = fresh_engine();

    // Module A: thickness > 2mm — satisfied at thickness default = 5mm.
    let module_a = parse_and_compile(&bracket_with_constraint("thickness > 2mm"));
    let _ = engine.eval(&module_a);

    // Sanity check: pre-edit check_snapshot reports Satisfied.
    let pre = engine
        .check_snapshot(&module_a)
        .expect("check_snapshot must return after eval");
    assert!(
        pre.constraint_results
            .iter()
            .all(|e| e.satisfaction == Satisfaction::Satisfied),
        "pre-edit constraint should be Satisfied, got: {:?}",
        pre.constraint_results
    );

    // Module B: thickness > 10mm — violated at thickness default = 5mm.
    let module_b = parse_and_compile(&bracket_with_constraint("thickness > 10mm"));
    let _ = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // Post-edit: check_snapshot uses the installed snapshot (from edit_source)
    // and module_b's constraint expressions, so the result must be Violated.
    let post = engine
        .check_snapshot(&module_b)
        .expect("check_snapshot must return after edit_source");
    assert_eq!(
        post.constraint_results.len(),
        1,
        "expected exactly one constraint in module_b, got: {:?}",
        post.constraint_results
    );
    let entry = &post.constraint_results[0];
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Violated,
        "modified constraint must be Violated after edit_source; entry: {:?}",
        entry
    );

    // last_eval_set must include the changed constraint node so downstream
    // book-keeping (diagnostics, caller filters) can observe it.
    let eval_set = engine.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new("Bracket", 0))),
        "last_eval_set must contain the modified constraint, got: {:?}",
        eval_set
    );
}

/// Adding a brand-new constraint in module_B must (a) produce an additional
/// entry in `check_snapshot` keyed by the new constraint's `ConstraintNodeId`,
/// (b) evaluate with the current value map (thickness=5mm satisfies `< 20mm`),
/// and (c) include the added constraint node in `last_eval_set()`. This locks
/// the "added constraint" diff path — the new node has no prior cache entry
/// and must land in the dirty cone via the added-set.
#[test]
fn edit_source_added_constraint_is_demanded_and_checked() {
    let mut engine = fresh_engine();

    // Module A: single constraint thickness > 2mm.
    let module_a = parse_and_compile(&bracket_with_constraint("thickness > 2mm"));
    let _ = engine.eval(&module_a);

    // Module B: keeps the original constraint AND adds a second one.
    let module_b_src = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
    constraint thickness < 20mm
}"#;
    let module_b = parse_and_compile(module_b_src);
    let _ = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let post = engine
        .check_snapshot(&module_b)
        .expect("check_snapshot must return after edit_source");
    assert_eq!(
        post.constraint_results.len(),
        2,
        "expected 2 constraints (1 original + 1 added) in module_b, got: {:?}",
        post.constraint_results
    );
    for entry in &post.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "both constraints should be satisfied at thickness=5mm; entry: {:?}",
            entry
        );
    }

    // The added constraint lives at index 1 (the second declaration) —
    // it must appear in last_eval_set via the "added" diff path.
    let eval_set = engine.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new("Bracket", 1))),
        "last_eval_set must contain the added constraint (Bracket#1), got: {:?}",
        eval_set
    );
}

// ── Param-override interaction ─────────────────────────────────────────────

/// After establishing a param override via `set_param_and_invalidate` +
/// `edit_param`, a subsequent `edit_source` that only mutates a let binding
/// (leaving the param's source unchanged) must preserve the override rather
/// than reverting the param to its source-declared default.
///
/// This locks the precedence rule — "overrides win for unchanged Param cells
/// across a structural source edit" — symmetric with `eval_cached`. Volume
/// (the dependent let) must also re-evaluate against the overridden width,
/// not against module_B's source default.
#[test]
fn edit_source_preserves_param_overrides_for_unchanged_params() {
    let mut engine = fresh_engine();

    // Module A: canonical bracket with width=80mm default and a volume let.
    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    engine.eval(&module_a);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_id = ValueCellId::new(e, "volume");

    // Establish an override-driven baseline: width → 0.12 m.
    engine.set_param_and_invalidate(&width_id, Value::length(0.12));
    engine
        .edit_param(width_id.clone(), Value::length(0.12))
        .expect("edit_param(width, 0.12) must succeed after eval");

    // Sanity: the override is installed in the snapshot pre-edit_source.
    let snapshot_pre = engine.snapshot().expect("snapshot must exist");
    let width_pre = si(
        snapshot_pre
            .values
            .get(&width_id)
            .map(|(v, _)| v)
            .expect("snapshot must carry width after edit_param"),
        "width_pre",
    );
    assert!(
        (width_pre - 0.12).abs() < 1e-12,
        "pre-edit snapshot must reflect the override (0.12), got {width_pre}"
    );

    // Module B: keep the width param identical in source; mutate only the
    // volume let expression. Width's content_hash is therefore unchanged,
    // so the seeding path must preserve its prior value.
    let module_b = parse_and_compile(&bracket_with_volume_expr(
        "width * height * thickness * 2.0",
    ));
    let result_b = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // (a) width must retain the override value — not revert to module_B's
    //     source default of 80mm.
    let width_b = si(
        result_b
            .values
            .get(&width_id)
            .expect("width must be present in edit_source result"),
        "width_b",
    );
    assert!(
        (width_b - 0.12).abs() < 1e-12,
        "width must reflect the override (0.12) after edit_source, \
         not the source default; got {width_b}"
    );

    // (b) volume must be evaluated against the overridden width, not the
    //     module_B source default. Expected: 0.12 * 0.10 * 0.005 * 2.0.
    let volume_b = si(
        result_b
            .values
            .get(&volume_id)
            .expect("volume must be present in edit_source result"),
        "volume_b",
    );
    let expected = 0.12 * 0.10 * 0.005 * 2.0;
    assert!(
        (volume_b - expected).abs() < 1e-12,
        "volume must use the overridden width; got {volume_b}, expected {expected}"
    );
}

/// After `edit_source` populates `last_diff_value_cells`, a subsequent
/// `edit_param` must clear the snapshot so callers cannot observe a stale
/// diff from the prior `edit_source`.  The "most recent `edit_source`"
/// invariant is enforced by a cfg-gated reset at the top of `edit_param`
/// (task 2265).
#[test]
fn edit_param_clears_last_diff_value_cells_from_prior_edit_source() {
    let mut engine = fresh_engine();

    // 1. Populate last_diff_value_cells via eval + edit_source.
    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    engine.eval(&module_a);

    // Module B mutates the let binding — guarantees a non-empty `changed` set
    // so that last_diff_value_cells is populated (not `None`).
    let module_b = parse_and_compile(&bracket_with_volume_expr(
        "width * height * thickness * 2.0",
    ));
    engine
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // 2. Precondition: edit_source must have populated the field.
    assert!(
        engine.last_diff_value_cells().is_some(),
        "edit_source must populate last_diff_value_cells (precondition)"
    );

    // 3. Act: run edit_param; it must clear the snapshot.
    let width_id = ValueCellId::new("Bracket", "width");
    engine
        .edit_param(width_id, Value::length(0.12))
        .expect("edit_param must succeed");

    // 4. The prior edit_source's diff is no longer current.
    assert!(
        engine.last_diff_value_cells().is_none(),
        "edit_param must clear last_diff_value_cells — the prior edit_source's diff \
         is no longer current (if Some, the cfg-gated reset at the top of edit_param \
         was dropped)"
    );
}

/// When the new module REMOVES a param that carried an override, the removed
/// cell must be absent from the post-edit values map and from the installed
/// snapshot's graph. Any dormant override in `param_overrides` has no cell to
/// apply to, so it must not surface the removed param in the result.
#[test]
fn edit_source_discards_override_when_param_removed_in_source() {
    let mut engine = fresh_engine();

    // Module A: canonical bracket (has width, height, thickness, volume).
    let module_a = parse_and_compile(&bracket_with_volume_expr("width * height * thickness"));
    engine.eval(&module_a);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");

    // Stash an override on width before the removal edit.
    engine.set_param_and_invalidate(&width_id, Value::length(0.12));
    engine
        .edit_param(width_id.clone(), Value::length(0.12))
        .expect("edit_param(width, 0.12) must succeed after eval");

    // Module B removes width entirely and adjusts volume to not reference it.
    let module_b_src = r#"structure Bracket {
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = height * thickness

    constraint thickness > 2mm
}"#;
    let module_b = parse_and_compile(module_b_src);
    let result_b = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // (a) The removed param must not surface in the values map.
    assert!(
        result_b.values.get(&width_id).is_none(),
        "removed width must not appear in values map after edit_source; got: {:?}",
        result_b.values.get(&width_id)
    );

    // (b) And must be absent from the installed snapshot's graph.
    let snapshot = engine.snapshot().expect("snapshot must exist");
    assert!(
        !snapshot.graph.value_cells.contains_key(&width_id),
        "removed width must be absent from snapshot.graph.value_cells"
    );
}

// ── Cross-check correctness ────────────────────────────────────────────────

/// The primary correctness lock for `Engine::edit_source`.
///
/// Builds two engines for the same module_B: one reached via
/// `eval(A); edit_source(B)`, the other via a fresh `check(B)`. Their
/// `values` maps and their constraint satisfaction lists must agree
/// entry-for-entry. Module B is constructed to simultaneously exercise
/// several diff paths:
///
/// - param default change (`width` 80mm → 85mm)
/// - let expression change (`volume` gains a `* 2.0` factor)
/// - constraint expression change (`thickness > 2mm` → `thickness > 3mm`)
/// - added let (`perimeter = 2.0 * (width + height)`)
///
/// Any asymmetry between incremental and cold paths surfaces here.
#[test]
fn edit_source_matches_cold_eval_on_mixed_bracket_edit() {
    let module_a_src = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
}"#;
    let module_b_src = r#"structure Bracket {
    param width: Scalar = 85mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness * 2.0
    let perimeter = 2.0 * (width + height)

    constraint thickness > 3mm
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    // Incremental path: eval(A), then edit_source(B), then observe the
    // snapshot via check_snapshot(B) so constraint evaluation uses module B.
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr_edit = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");
    let incr_check = incremental
        .check_snapshot(&module_b)
        .expect("check_snapshot must succeed after edit_source");

    // Cold path: fresh engine, check(B) = eval(B) + constraint check.
    let mut cold = fresh_engine();
    let cold_check = cold.check(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr_edit.values, &cold_check.values);

    // Constraint check results must match entry-for-entry. Normalise by
    // `ConstraintNodeId` since ordering is implementation-defined. HashMap
    // rather than BTreeMap because ConstraintNodeId is not Ord.
    let incr_by_id: std::collections::HashMap<_, _> = incr_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction))
        .collect();
    let cold_by_id: std::collections::HashMap<_, _> = cold_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction))
        .collect();
    assert_eq!(
        incr_by_id, cold_by_id,
        "constraint satisfaction diverges between incremental and cold paths"
    );
}

/// When a guard expression's *text* changes in the new module (e.g.,
/// `where use_thick` → `where !use_thick`), the active branch must flip
/// so the values map matches a fresh cold eval of the new module. This
/// lock ensures the guard re-elaboration phase runs on edit_source —
/// the incremental path cannot rely on the old guard truth value.
#[test]
fn edit_source_guard_expr_change_flips_active_branch() {
    let module_a_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let effective = thickness * 2.0
    } else {
        let effective = thickness
    }
}"#;
    // Module B: same params, but the guard EXPRESSION is negated.
    // With use_thick=true (unchanged default), !use_thick = false, so the
    // else-branch activates and `effective = thickness = 5mm`.
    let module_b_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where !use_thick {
        let effective = thickness * 2.0
    } else {
        let effective = thickness
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let effective_id = ValueCellId::new("Bracket", "effective");

    // Incremental: eval(A) then edit_source(B).
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // Cold baseline: fresh eval(B).
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Either both maps contain `effective` with the same value, or both omit
    // it (implementation detail). The cross-check is the invariant.
    let incr_val = incr.values.get(&effective_id);
    let cold_val = cold_result.values.get(&effective_id);
    assert_eq!(
        incr_val, cold_val,
        "effective diverges after guard-expression flip: \
         incremental={:?}, cold={:?}",
        incr_val, cold_val
    );
}

/// Adding a `let` to the inactive branch of an existing `where ... else`
/// group must leave the added cell on the inactive branch Undef, matching
/// cold eval. Without a guard-re-elaboration trigger for added members, the
/// per-cell eval loop would write the default_expr's value (Determined),
/// diverging from the cold-eval contract that inactive-branch non-Auto
/// members are deactivated to `Undef`.
///
/// Reviewer comment #3 — correctness_edge_case.
#[test]
fn edit_source_added_else_branch_member_is_deactivated() {
    // Guard is true by default (`use_thick = true`) — the `members` branch
    // is active, and the `else_members` branch is inactive.
    let module_a_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_cell = thickness * 2.0
    } else {
        let inactive_cell = thickness
    }
}"#;
    // Module B: adds a NEW `let` to the inactive else-branch. Guard
    // expression and structure_controlling cells are unchanged.
    let module_b_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_cell = thickness * 2.0
    } else {
        let inactive_cell = thickness
        let inactive_added = thickness * 3.0
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let added_id = ValueCellId::new("Bracket", "inactive_added");

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check: whatever cold eval says about the added else-branch
    // cell, incremental edit_source must match.
    let incr_val = incr.values.get(&added_id);
    let cold_val = cold_result.values.get(&added_id);
    assert_eq!(
        incr_val, cold_val,
        "added else-branch member diverges from cold eval: \
         incremental={:?}, cold={:?}",
        incr_val, cold_val
    );
}

/// Adding a `let` to the ACTIVE branch of an existing `where ... else`
/// group must match cold eval — a symmetrically-added active-branch cell
/// should be Determined from its default_expr, unlike the inactive
/// counterpart. This pins the matching trigger for active-branch added
/// members.
///
/// Reviewer comment #3 — correctness_edge_case (symmetric variant).
#[test]
fn edit_source_added_active_branch_member_matches_cold_eval() {
    let module_a_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_cell = thickness * 2.0
    } else {
        let inactive_cell = thickness
    }
}"#;
    let module_b_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_cell = thickness * 2.0
        let active_added = thickness * 4.0
    } else {
        let inactive_cell = thickness
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let added_id = ValueCellId::new("Bracket", "active_added");

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    let incr_val = incr.values.get(&added_id);
    let cold_val = cold_result.values.get(&added_id);
    assert_eq!(
        incr_val, cold_val,
        "added active-branch member diverges from cold eval: \
         incremental={:?}, cold={:?}",
        incr_val, cold_val
    );
}

/// When an existing `let` **moves between branches** of a `where … else` group
/// without changing its id or expression text, `diff_value_cells` classifies
/// it as neither `changed` (same `content_hash`) nor `added` (still present).
/// The Phase 1 trigger therefore does not fire, leaving the old branch value
/// from before the edit intact — incremental diverges from cold eval.
///
/// Scenario: `use_thick = true` (members branch is active).
/// - Module A: `moving` is in the **active** members branch  → cold eval gives Determined (15mm).
/// - Module B: `moving` moves to the **inactive** else branch  → cold eval gives Undef.
///
/// Before the fix (task 2084): incremental keeps the old Determined value
/// (Phase 1 never fires). After the fix: `has_role_flipped_guard_member` fires
/// Phase 1, which calls `deactivate_if_not_auto` for the now-inactive `moving`,
/// matching cold eval.
///
/// Task 2084 — Phase 1 trigger gap: role-flipped guard members.
#[test]
fn edit_source_role_flipped_guard_member_matches_cold_eval() {
    // Module A: `moving` is on the active (where) branch.
    let module_a_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let moving = thickness * 3.0
        let active_only = thickness * 2.0
    } else {
        let inactive_only = thickness
    }
}"#;
    // Module B: `moving` (same id, same expr → same content_hash) relocates
    // to the inactive else-branch. `active_only` / `inactive_only` stay put.
    let module_b_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_only = thickness * 2.0
    } else {
        let moving = thickness * 3.0
        let inactive_only = thickness
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let moving_id = ValueCellId::new("Bracket", "moving");

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check: whatever cold eval says about the role-flipped cell,
    // incremental edit_source must match.
    let incr_val = incr.values.get(&moving_id);
    let cold_val = cold_result.values.get(&moving_id);
    assert_eq!(
        incr_val, cold_val,
        "role-flipped guard member diverges from cold eval: \
         incremental={:?}, cold={:?}",
        incr_val, cold_val
    );
    // Positive lock: with `use_thick = true`, `moving` moved from the active
    // branch to the inactive else branch, so cold eval must deactivate it to
    // Undef. If this assertion fails, cold eval itself regressed — both the
    // relative check above and this anchor are needed so a "both wrong"
    // regression does not slip through.
    assert!(
        matches!(cold_val, Some(Value::Undef)),
        "cold eval should deactivate inactive-branch member to Undef, got {:?}",
        cold_val
    );
}

/// When an existing `let` **moves between branches** of a `where … else` group
/// without changing its id or expression text, `diff_value_cells` classifies
/// it as neither `changed` (same `content_hash`) nor `added` (still present).
/// The Phase 1 trigger therefore does not fire, leaving the old branch value
/// from before the edit intact — incremental diverges from cold eval.
///
/// Symmetric counterpart to `edit_source_role_flipped_guard_member_matches_cold_eval`
/// (task 2084): exercises the **inactive → active** direction.
///
/// Scenario: `use_thick = true` (members branch is active).
/// - Module A: `moving` is in the **inactive** else branch  → cold eval gives Undef.
/// - Module B: `moving` moves to the **active** where branch  → cold eval gives Determined (15mm).
///
/// Before the fix (task 2084): incremental keeps the old Undef value
/// (Phase 1 never fires). After the fix: `has_role_flipped_guard_member` fires
/// Phase 1, which re-elaborates the now-active branch and writes a Determined
/// value for `moving`, matching cold eval.
///
/// Task 2091 — symmetric role-flip direction (inactive → active).
#[test]
fn edit_source_role_flipped_guard_member_inactive_to_active_matches_cold_eval() {
    // Module A: `moving` is on the inactive (else) branch.
    let module_a_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let active_only = thickness * 2.0
    } else {
        let moving = thickness * 3.0
        let inactive_only = thickness
    }
}"#;
    // Module B: `moving` (same id, same expr → same content_hash) relocates
    // to the active where-branch. `active_only` / `inactive_only` stay put.
    let module_b_src = r#"structure Bracket {
    param thickness: Scalar = 5mm
    param use_thick: Bool = true

    where use_thick {
        let moving = thickness * 3.0
        let active_only = thickness * 2.0
    } else {
        let inactive_only = thickness
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let moving_id = ValueCellId::new("Bracket", "moving");

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check: whatever cold eval says about the role-flipped cell,
    // incremental edit_source must match.
    let incr_val = incr.values.get(&moving_id);
    let cold_val = cold_result.values.get(&moving_id);
    assert_eq!(
        incr_val, cold_val,
        "role-flipped guard member (inactive→active) diverges from cold eval: \
         incremental={:?}, cold={:?}",
        incr_val, cold_val
    );
    // Positive lock: with `use_thick = true`, `moving` moved from the inactive
    // else branch to the active where branch, so cold eval must activate it to a
    // Determined value of 15mm (0.015 m = 5mm × 3.0). Uses epsilon comparison
    // because 0.005 * 3.0 is not bit-exactly 0.015 in IEEE 754. If this
    // assertion fails, cold eval itself regressed — both the relative check
    // above and this anchor are needed so a "both wrong" regression does not
    // slip through.
    assert!(
        matches!(cold_val, Some(Value::Scalar { si_value, .. }) if (*si_value - 0.015).abs() < 1e-10),
        "cold eval should activate where-branch member to Determined 15mm (0.015 m), got {:?}",
        cold_val
    );
}

// ── Coverage gap 1: Phase 4 collection count re-elaboration ──────────────────

/// Incrementally changing a collection-count param from 4 → 6 must re-elaborate
/// the collection, producing exactly 6 bolt-instance cells and a refreshed
/// synthetic `__list_bolts__diameter` list — matching a cold `eval(B)`.
///
/// Exercises `engine_edit.rs` Phase 4 (collection count re-elaboration).
/// Task 2087 — coverage gap 1.
#[test]
fn edit_source_collection_count_re_elaborates_against_cold_eval() {
    // Module A: S with n=4 bolts.
    let module_a_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;
    // Module B: same but n=6.  `edit_source` must re-elaborate the collection
    // so the incremental result matches a cold eval of module_b.
    let module_b_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 6
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // (b) __count_bolts == Int(6) on the incremental engine.
    let count_id = ValueCellId::new("S", "__count_bolts");
    assert_eq!(
        incr.values.get(&count_id),
        Some(&Value::Int(6)),
        "__count_bolts should be Int(6) after count increase to 6"
    );

    // (c) S.bolts[0..6].diameter == length(0.01) — all 6 instances present.
    for i in 0..6_usize {
        let bolt_id = ValueCellId::new(format!("S.bolts[{}]", i), "diameter");
        assert_eq!(
            incr.values.get(&bolt_id),
            Some(&Value::length(0.01)),
            "S.bolts[{}].diameter should be 10mm = 0.01 m after count re-elaboration to 6",
            i
        );
    }

    // (d) S.bolts[6].diameter is absent — no overrun past the new count.
    let overrun_id = ValueCellId::new("S.bolts[6]", "diameter");
    assert!(
        incr.values.get(&overrun_id).is_none(),
        "S.bolts[6].diameter must be absent: the new count is 6, so indices 0-5 are valid"
    );

    // (e) __list_bolts__diameter is a Value::List with 6 entries.
    let list_id = ValueCellId::new("S", "__list_bolts__diameter");
    match incr.values.get(&list_id) {
        Some(Value::List(items)) => assert_eq!(
            items.len(),
            6,
            "__list_bolts__diameter should have 6 entries after re-elaboration, got {}",
            items.len()
        ),
        other => panic!(
            "__list_bolts__diameter should be a Value::List with 6 entries, got {:?}",
            other
        ),
    }
}

// ── Task 2086: Phase 4 cache invalidation for shrunk+regrown collection ────────

/// Regression test: after `eval(n=4)` → `edit_source(n=2)` → `edit_source(n=4)`,
/// cache entries for `S.bolts[i].diameter` must be either absent (invalidated and
/// not repopulated by Phase 4's create loop) or fresh (basis_version ==
/// current snapshot version).  Without Fix 1, Phase 4's remove loop never calls
/// `self.cache.invalidate` for the scoped cells it evicts from the graph, so
/// `bolts[0].diameter` and `bolts[1].diameter` survive at the stale V_A version.
#[test]
fn edit_source_phase4_invalidates_cache_for_shrunk_and_regrown_collection_instance() {
    // Module A: n=4 (initial state — populates cache at V_A)
    let module_a_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;
    // Module B: n=2 (shrink — Phase 4 remove-loop evicts bolts[0..3], create-loop
    // re-inserts bolts[0..1]; without the fix, cache still holds stale V_A entries)
    let module_b_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 2
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;
    // Module C: n=4 again (re-grow — same pattern, bolts[2..3] come back as `added`
    // and are refreshed; bolts[0..1] are unchanged by source diff but re-removed
    // and re-created by Phase 4; without the fix their cache entries are V_A stale)
    let module_c_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;

    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);
    let module_c = parse_and_compile(module_c_src);

    let mut engine = fresh_engine();
    engine.eval(&module_a);

    // Confirm pre-edit state: all 4 bolt instances must already be in the cache
    // at V_A.  This proves the pre-edit state matters so that the post-edit
    // None-or-fresh assertion below is meaningful rather than trivially satisfied
    // by a cache that was never populated.
    let v_a = engine.snapshot().expect("snapshot after eval(A)").version;
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("S.bolts[{}]", i), "diameter"));
        let entry = engine
            .cache_store()
            .get(&bolt_node)
            .unwrap_or_else(|| panic!("S.bolts[{}].diameter must be in cache after eval(A)", i));
        assert_eq!(
            entry.basis_version, v_a,
            "S.bolts[{}].diameter must be at V_A before any edits",
            i
        );
    }

    engine
        .edit_source(&module_b)
        .expect("edit_source to B (shrink n=4→2) must succeed");
    engine
        .edit_source(&module_c)
        .expect("edit_source to C (re-grow n=2→4) must succeed");

    let current_version = engine
        .snapshot()
        .expect("snapshot must be present after two edit_source calls")
        .version;

    // Every cache entry for S.bolts[i].diameter must be either absent (properly
    // invalidated by Phase 4's remove loop and not re-populated by the create
    // loop) or fresh at the current version.  A Some(entry) with a prior version
    // is a stale cache artifact from V_A — the bug this test pins.
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("S.bolts[{}]", i), "diameter"));
        if let Some(entry) = engine.cache_store().get(&bolt_node) {
            assert_eq!(
                entry.basis_version, current_version,
                "S.bolts[{}].diameter cache entry must be fresh after grow→shrink→re-grow; \
                 got basis_version {:?}, expected {:?}",
                i, entry.basis_version, current_version
            );
        }
        // None is acceptable — Phase 4's create loop does not call
        // cache.record_evaluation, so invalidated entries remain absent.
    }
}

/// Regression test (Step 9 path): `edit_source(B)` where Module B *entirely removes*
/// the `sub bolts` declaration must not leave stale cache entries for
/// `S.bolts[i].diameter` established by `eval(A)`.
///
/// Mechanism: Step (9) of `edit_source` diffs `old_snapshot.graph.value_cells`
/// against `new_snapshot.graph.value_cells` by content_hash.  Scoped cells
/// present in the old graph but absent from the new graph (because the whole sub
/// was removed) appear in `diff_value_cells.removed`, and Step (9) calls
/// `self.cache.invalidate` for each.  Phase 4's main loop iterates only NEW
/// `collection_subs` and never visits removed subs, so Step (9) is the sole
/// responsible path here.
///
/// This test pins that behavior independent of any Fix-2 sweep so future changes
/// to Phase 4 cannot silently regress the entirely-removed-sub case.
#[test]
fn edit_source_step9_invalidates_cache_for_entirely_removed_collection_sub() {
    // Module A: S has a 4-instance bolt sub; eval populates cache at V_A.
    let module_a_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S {
    param n : Int = 4
    sub bolts : List<Bolt>
    constraint bolts.count == n
}
"#;
    // Module B: `sub bolts` is entirely removed; no scoped cells in new graph.
    let module_b_src = r#"
structure Bolt { param diameter : Scalar = 10mm }
structure S { param n : Int = 4 }
"#;

    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut engine = fresh_engine();
    engine.eval(&module_a);

    // Pre-edit: all 4 bolt cache entries must be present at V_A so the
    // post-edit assertion is meaningful (not trivially satisfied by an empty
    // cache).
    let v_a = engine.snapshot().expect("snapshot after eval(A)").version;
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("S.bolts[{}]", i), "diameter"));
        let entry = engine
            .cache_store()
            .get(&bolt_node)
            .unwrap_or_else(|| panic!("S.bolts[{}].diameter must be in cache after eval(A)", i));
        assert_eq!(
            entry.basis_version, v_a,
            "S.bolts[{}].diameter must be at V_A before the edit",
            i
        );
    }

    engine
        .edit_source(&module_b)
        .expect("edit_source to B (remove sub bolts entirely) must succeed");

    // Post-edit: Step (9) must have invalidated all old scoped-cell cache
    // entries via diff_value_cells.removed.  Each entry must be absent or
    // fresh at the new snapshot version — a Some(entry) at V_A is a stale
    // artifact that would cause correctness failures if the sub is re-added.
    let current_version = engine
        .snapshot()
        .expect("snapshot after edit_source")
        .version;
    for i in 0..4_usize {
        let bolt_node = NodeId::Value(ValueCellId::new(format!("S.bolts[{}]", i), "diameter"));
        if let Some(entry) = engine.cache_store().get(&bolt_node) {
            assert_eq!(
                entry.basis_version, current_version,
                "S.bolts[{}].diameter must be absent or fresh after sub removal via Step 9; \
                 got stale basis_version {:?}, expected {:?}",
                i, entry.basis_version, current_version
            );
        }
    }
}

// ── Coverage gap 2: Step-11 functions table refresh ───────────────────────────

/// Changing a user-function body AND adding a call site that references the new
/// body must yield the updated result on the incremental path — matching a cold
/// `eval(B)`.  The added call-site cell is classified `added` (no prior cache
/// entry), so it must freshly evaluate against the refreshed `self.functions`
/// table installed by Step-11 of `edit_source`.
///
/// If Step-11's `self.functions = module.functions.clone()` refresh were skipped,
/// the incremental engine would compute `3.0 * 4.0 = 12.0`
/// (module_A's body) while cold eval would compute `3.0 + 4.0 = 7.0`.
///
/// Task 2087 — coverage gap 2.
#[test]
fn edit_source_refreshes_functions_table_against_cold_eval() {
    // Module A: fn foo multiplies; Panel has NO call site for foo.
    let module_a_src = r#"
fn foo(x: Real, y: Real) -> Real { x * y }
structure Panel {
    param width : Real = 3.0
    param height : Real = 4.0
}
"#;
    // Module B: fn foo now adds (body change); Panel has a NEW `let result`
    // call site classified `added` — forces a fresh eval against self.functions.
    let module_b_src = r#"
fn foo(x: Real, y: Real) -> Real { x + y }
structure Panel {
    param width : Real = 3.0
    param height : Real = 4.0
    let result = foo(width, height)
}
"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // Anchor: `result` must be Real(7.0) = 3.0 + 4.0 (refreshed sum body).
    // `3.0` and `4.0` are decimal-form literals → Value::Real after task 3184.
    // A missing functions-table refresh would produce Real(12.0) = 3.0 * 4.0 from
    // module_A's multiply body; seeing Real(7.0) on both paths confirms the '+' body is used.
    let result_id = ValueCellId::new("Panel", "result");
    assert_eq!(
        incr.values.get(&result_id),
        Some(&Value::Real(7.0)),
        "Panel.result should be Real(7.0) = foo(width, height) with the refreshed '+' body; \
         got {:?} — likely the functions table was not refreshed",
        incr.values.get(&result_id)
    );
    assert_eq!(
        cold_result.values.get(&result_id),
        Some(&Value::Real(7.0)),
        "cold Panel.result should also be Real(7.0)"
    );
}

// ── Coverage gap 3: Step-11 compiled_purposes table refresh ───────────────────

/// Changing a purpose body (adding a constraint) and then activating the purpose
/// on the incremental engine must inject module_B's constraints — matching the
/// cold path.  If Step-11's `self.compiled_purposes` refresh were skipped,
/// incremental would inject module_A's
/// 1-constraint purpose while cold injects module_B's 2-constraint purpose.
///
/// Task 2087 — coverage gap 3.
#[test]
fn edit_source_refreshes_compiled_purposes_against_cold_eval() {
    // Module A: Bracket (no structure-level constraints), purpose with 1 constraint.
    let module_a_src = r#"
structure Bracket {
    param width : Length = 80mm
}
purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;
    // Module B: same Bracket, purpose body changed to 2 constraints.
    let module_b_src = r#"
structure Bracket {
    param width : Length = 80mm
}
purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
    constraint 2 > 0
}
"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    // Incremental: eval(A), edit_source(B), activate_purpose.
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");
    incremental.activate_purpose("mfg_ready", "Bracket");

    // Cold: eval(B), activate_purpose.
    let mut cold = fresh_engine();
    cold.eval(&module_b);
    cold.activate_purpose("mfg_ready", "Bracket");

    // Compare graph constraint counts AND constraint identity after activation.
    // Purpose constraints are injected into the snapshot graph by activate_purpose;
    // the count and the exact ConstraintNodeId set reflect which module's purpose
    // body was used.
    let incr_snap = incremental
        .snapshot()
        .expect("incremental snapshot must exist after edit_source");
    let cold_snap = cold
        .snapshot()
        .expect("cold snapshot must exist after eval");

    let incr_count = incr_snap.graph.constraints.len();
    let cold_count = cold_snap.graph.constraints.len();

    assert_eq!(
        incr_count, cold_count,
        "after activate_purpose, incremental ({incr_count}) and cold ({cold_count}) \
         graph constraint counts must match — divergence means compiled_purposes \
         was not refreshed in edit_source"
    );
    // Anchor: module_B's purpose has 2 constraints, so both should have 2.
    assert_eq!(
        incr_count, 2,
        "both engines should have 2 graph constraints after activating module_B's \
         2-constraint purpose; incremental has {incr_count}"
    );

    // Pin constraint identity, not just arity: the sets of ConstraintNodeIds must
    // match exactly.  A regression where the stale purpose body (from module_A) is
    // used could produce 2 constraints via a different expansion path — the count
    // check would pass accidentally, but the id-set check would detect the mismatch.
    let incr_ids: HashSet<ConstraintNodeId> = incr_snap.graph.constraints.keys().cloned().collect();
    let cold_ids: HashSet<ConstraintNodeId> = cold_snap.graph.constraints.keys().cloned().collect();
    assert_eq!(
        incr_ids, cold_ids,
        "after activate_purpose, incremental and cold graph ConstraintNodeId sets \
         must be identical — divergence means the stale purpose body was used on \
         the incremental path"
    );
}

// ── Coverage gap 4: Step-11 meta_map refresh ──────────────────────────────────

/// Adding a new `let` cell that reads `meta.key` forces a fresh eval of that
/// added cell against the refreshed `meta_map` installed by Step-11.  If the
/// refresh is skipped, the incremental engine reads module_A's meta value
/// ("A widget") while cold eval reads module_B's ("A gadget"), and the
/// cross-check fails.
///
/// Note: `meta.key` access hashes entity+key only (not the meta value), so a
/// meta-dict-value change alone does not dirty an existing cell.  Pairing the
/// meta change with an ADDED reading cell guarantees that Step-11's
/// `self.meta_map` refresh is actually consulted.
///
/// Task 2087 — coverage gap 4.
#[test]
fn edit_source_refreshes_meta_map_against_cold_eval() {
    // Module A: Widget with meta only — no reading cells so there is no stable-hash
    // cell that would carry over a stale meta value.  (`meta.key` access sites hash
    // entity+key only, not the meta value, so changing the dict value alone does NOT
    // dirty an existing reading cell.)
    let module_a_src = r#"
structure def Widget {
    meta {
        description = "A widget"
    }
}
"#;
    // Module B: meta value changed to "A gadget" AND two NEW reading cells — `desc`
    // and `tag` are both ADDED (no prior cache entries) and must evaluate against the
    // refreshed `self.meta_map` installed by Step-11 of `edit_source`.
    let module_b_src = r#"
structure def Widget {
    meta {
        description = "A gadget"
    }
    let desc : String = meta.description
    let tag : String = meta.description
}
"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // Anchor: both added cells must read the refreshed meta_map on both paths.
    // If Step-11's `self.meta_map` refresh were skipped, `desc` and `tag` on the
    // incremental path would read module_A's meta ("A widget") instead of module_B's.
    let desc_id = ValueCellId::new("Widget", "desc");
    let tag_id = ValueCellId::new("Widget", "tag");
    assert_eq!(
        incr.values.get(&desc_id),
        Some(&Value::String("A gadget".to_string())),
        "Widget.desc (added cell) should read the refreshed meta_map ('A gadget'); \
         got {:?} — likely meta_map was not refreshed",
        incr.values.get(&desc_id)
    );
    assert_eq!(
        incr.values.get(&tag_id),
        Some(&Value::String("A gadget".to_string())),
        "Widget.tag (added cell) should read the refreshed meta_map ('A gadget'); \
         got {:?} — likely meta_map was not refreshed",
        incr.values.get(&tag_id)
    );
    assert_eq!(
        cold_result.values.get(&desc_id),
        Some(&Value::String("A gadget".to_string())),
        "cold Widget.desc should also be 'A gadget'"
    );
    assert_eq!(
        cold_result.values.get(&tag_id),
        Some(&Value::String("A gadget".to_string())),
        "cold Widget.tag should also be 'A gadget'"
    );
}

// ── Arc refactor regression: meta_map stability across edit_source + edit_param ──

/// Integration regression: `edit_source` followed by `edit_param` must both
/// read the *updated* meta map installed by the Arc refactor (task 397, step-2).
///
/// The Arc refactor changed `Engine.meta_map` from `HashMap<...>` to
/// `Arc<HashMap<...>>`, replacing it wholesale at the three write-sites
/// (`engine_eval.rs:203`, `:1065`, `engine_edit.rs:1443`).  The risk is that a
/// subtle bug could cause `edit_source`'s `self.meta_map = Arc::new(...)` to
/// NOT install a fresh Arc (e.g. by accidentally cloning the old Arc instead
/// of constructing a new one), so that `desc` / `tag` in module B would still
/// read module A's stale meta value.
///
/// Note: `meta.key` access expressions hash entity+key only (NOT the meta
/// value), so an existing reading cell's content_hash does not change when only
/// the meta value is updated.  Module A therefore has NO reading cells — only
/// a meta block and a param.  The reading cells (`desc`, `tag`) are ADDED in
/// module B, guaranteeing that they receive fresh evaluation against the new
/// meta_map Arc.  This matches the convention documented in
/// `edit_source_refreshes_meta_map_against_cold_eval`.
///
/// Additional coverage over the existing meta-refresh test: after `edit_source`
/// a subsequent `edit_param(width, Undef)` exercises the full incremental
/// pipeline against the newly-installed Arc.  `desc` and `tag` (which don't
/// depend on `width`) carry over their values from `edit_source` — they must
/// still reflect the B meta map ("A gadget"), not the A map ("A widget").
///
/// Also serves as a safety net for step-6 and step-7 (call-site conversions in
/// engine_eval.rs / engine_edit.rs): if any conversion accidentally drops the
/// `.with_meta(meta_map)` chain the `desc` / `tag` assertions catch it.
#[test]
fn edit_source_meta_map_arc_stability() {
    // Module A: Widget with meta description="A widget" and a Length param
    // `width`.  NO reading cells — per the established convention, adding
    // reading cells in module B forces their fresh evaluation against the
    // refreshed meta_map (meta-value changes do not dirty existing cells).
    let module_a_src = r#"
structure def Widget {
    meta {
        description = "A widget"
    }
    param width : Length = 10mm
}
"#;
    // Module B: meta value changed to "A gadget"; `width` param unchanged;
    // two reading cells `desc` and `tag` ADDED so they evaluate against the
    // freshly-installed meta_map Arc.
    let module_b_src = r#"
structure def Widget {
    meta {
        description = "A gadget"
    }
    param width : Length = 10mm
    let desc : String = meta.description
    let tag : String = meta.description
}
"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    // ── incremental path ──────────────────────────────────────────────────────
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // Subsequent edit_param on `width` (unchanged between A and B) exercises the
    // full incremental edit pipeline after edit_source has replaced self.meta_map.
    // desc and tag do not depend on width, so their values carry over from the
    // edit_source result — they must still reflect the B meta map ("A gadget"),
    // not the A meta map ("A widget") that was active before edit_source.
    let width_id = ValueCellId::new("Widget", "width");
    let after_edit_param = incremental
        .edit_param(width_id, Value::Undef)
        .expect("edit_param(width, Undef) must succeed after edit_source");

    // ── cold path ─────────────────────────────────────────────────────────────
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) edit_source result must match cold eval entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    let desc_id = ValueCellId::new("Widget", "desc");
    let tag_id = ValueCellId::new("Widget", "tag");

    // (b) Both desc and tag must resolve to the module_b meta value across
    // all phases.  If the Arc refactor's write-site at edit_source accidentally
    // retained the A snapshot, these would read "A widget" instead.
    // Note: the `edit_param(width, Undef)` result carries the added cells from
    // `edit_source` unchanged (desc/tag don't depend on width), so checking
    // them individually (rather than via assert_values_match against cold) is
    // correct — `width` itself is Undef in the override path but 10mm in cold.
    for (label, values) in [
        ("edit_source", &incr.values),
        ("edit_param", &after_edit_param.values),
        ("cold", &cold_result.values),
    ] {
        assert_eq!(
            values.get(&desc_id),
            Some(&Value::String("A gadget".to_string())),
            "Widget.desc must be 'A gadget' after {} (Arc refactor correctness); \
             got {:?} — likely edit_source did not install a fresh meta_map Arc",
            label,
            values.get(&desc_id)
        );
        assert_eq!(
            values.get(&tag_id),
            Some(&Value::String("A gadget".to_string())),
            "Widget.tag (added cell) must be 'A gadget' after {}; \
             got {:?} — added cells must evaluate against the refreshed meta_map",
            label,
            values.get(&tag_id)
        );
    }

    // (c) No diagnostics on the edit_param path (no error cascade from a stale
    // or dropped meta_map Arc).
    assert!(
        after_edit_param.diagnostics.is_empty(),
        "edit_param must not emit diagnostics; got {:?}",
        after_edit_param.diagnostics
    );
}

// ── Coverage gap 5: Step-11 objectives table refresh ─────────────────────────

/// Flipping a template's objective from `Minimize` to `Maximize` (while also
/// adding a constraint so the dirty cone touches the auto-param group) must cause
/// `edit_source` to forward the *new* `Maximize` objective to the solver —
/// matching a cold `eval(B)`.
///
/// If Step-11's `self.objectives.clear(); self.objectives.insert(...)` refresh
/// were skipped, `self.objectives` would still carry
/// the stale `Minimize` objective from `eval(A)`, and the solver would receive
/// `Minimize` instead of `Maximize` during the `edit_source(B)` resolution phase.
/// The spy assertion on `captured_problems[1].objective` catches this divergence.
///
/// Design: the added constraint (`thickness > 3mm`) in module_b changes the
/// template's content_hash, puts the new `ConstraintNodeId` in `dirty_cone`,
/// and — because it references the auto-param — sets `constraints_dirty = true`,
/// triggering the solver phase in `edit_source`. The spy captures what objective
/// the engine forwarded to the solver on that call.
///
/// NOTE: `auto` params are not available in the reify source grammar (confirmed
/// 2026-04-22); this test uses `TopologyTemplateBuilder` + `MockConstraintChecker`
/// + `MultiCallSpyConstraintSolver` / `MockConstraintSolver` for its fixtures,
///   unlike the other 9 tests which use `parse_and_compile`.
///
/// Task 2087 — coverage gap 5.
#[test]
fn edit_source_refreshes_objectives_against_cold_eval() {
    use reify_test_support::{
        CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver,
        MultiCallSpyConstraintSolver, TopologyTemplateBuilder, gt, literal, lt, mm, value_ref,
    };
    use reify_core::{ModulePath, Type};
    use reify_ir::{ObjectiveSet, ObjectiveSense, SolveResult};
    use std::collections::HashMap;

    let thickness_id = ValueCellId::new("S", "thickness");

    // Pre-configure solver return values.
    // Call 1 (eval module_a, Minimize): returns thickness = 2mm (simulates near-lower-bound).
    let mut min_solved: HashMap<ValueCellId, Value> = HashMap::new();
    min_solved.insert(thickness_id.clone(), mm(2.0));
    // Call 2 (edit_source module_b, Maximize): returns thickness = 15mm (simulates near-upper-bound).
    let mut max_solved: HashMap<ValueCellId, Value> = HashMap::new();
    max_solved.insert(thickness_id.clone(), mm(15.0));

    // Module A: auto thickness, constraint thickness < 20mm, objective Minimize.
    let template_a = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            lt(value_ref("S", "thickness"), literal(mm(20.0))),
        )
        .objective(ObjectiveSet::single(ObjectiveSense::Minimize, value_ref("S", "thickness")))
        .build();

    let module_a = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .build();

    // Module B: same auto param + unchanged constraint 0, plus ADDED constraint 1
    // (thickness > 3mm) and objective changed to Maximize.
    //
    // The added constraint changes the template's content_hash so edit_source sees
    // a structural change. Constraint 1 is added to `added_constraints` →
    // `dirty_cone` → `constraints_dirty = true` → solver phase runs.
    //
    // Step-11 must refresh `self.objectives` so that the solver receives Maximize.
    // If the refresh is skipped, the spy captures Minimize on call 2.
    let template_b = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            lt(value_ref("S", "thickness"), literal(mm(20.0))),
        )
        .constraint(
            "S",
            1,
            None,
            gt(value_ref("S", "thickness"), literal(mm(3.0))),
        )
        .objective(ObjectiveSet::single(ObjectiveSense::Maximize, value_ref("S", "thickness")))
        .build();

    let module_b = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_b)
        .build();

    // Incremental engine: spy captures every ResolutionProblem in call order.
    // Call 0: eval(module_a) → receives Minimize objective → returns mm(2.0).
    // Call 1: edit_source(module_b) → should receive Maximize → returns mm(15.0).
    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved {
            values: min_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: max_solved.clone(),
            unique: true,
        },
    ]);
    let captured = spy.captured_problems();

    let mut incremental =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // Cold engine: single call (eval(module_b)) should receive Maximize and return mm(15.0).
    let cold_solver = MockConstraintSolver::new_solved(max_solved.clone());
    let mut cold = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(cold_solver));
    let cold_result = cold.eval(&module_b);

    // ── Objective spy assertion ───────────────────────────────────────────────
    // The spy must have recorded exactly 2 calls: eval(A) and edit_source(B).
    // If only 1 call was recorded, the solver was not re-invoked during edit_source
    // (constraints_dirty was false — the test setup is wrong).
    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        2,
        "expected 2 solver calls (eval(A) + edit_source(B)), got {}; \
         if 1, edit_source did not trigger the solver (constraints not dirty)",
        problems.len()
    );

    // Call 0 (eval(A)): objective must be Minimize.
    assert!(
        problems[0].objective.as_ref().and_then(|o| o.terms.first()).map(|t| t.sense) == Some(ObjectiveSense::Minimize),
        "eval(A) should forward Minimize objective, got: {:?}",
        problems[0].objective
    );

    // Call 1 (edit_source(B)): objective must be Maximize — proving Step-11 refreshed
    // self.objectives before the solver phase ran.
    assert!(
        problems[1].objective.as_ref().and_then(|o| o.terms.first()).map(|t| t.sense) == Some(ObjectiveSense::Maximize),
        "edit_source(B) should forward the refreshed Maximize objective; \
         got {:?} — likely self.objectives was not refreshed in edit_source \
         (stale Minimize carried from eval(A))",
        problems[1].objective
    );
    drop(problems); // release lock before cross-check

    // ── Cross-check: resolved_params must agree ───────────────────────────────
    // Both incremental and cold paths resolve thickness = mm(15.0).
    // If objectives were not refreshed, the incremental solver would still
    // get Minimize but return mm(15.0) from the pre-configured sequence —
    // so the cross-check alone is not sufficient to detect the bug; the spy
    // assertion above is the primary detector.
    assert_eq!(
        incr.resolved_params.get(&thickness_id),
        cold_result.resolved_params.get(&thickness_id),
        "resolved_params[thickness] diverges between incremental and cold; \
         incremental={:?}, cold={:?}",
        incr.resolved_params.get(&thickness_id),
        cold_result.resolved_params.get(&thickness_id),
    );

    // Anchor: both paths must resolve thickness = mm(15.0).
    assert_eq!(
        incr.resolved_params.get(&thickness_id),
        Some(&mm(15.0)),
        "incremental resolved thickness should be mm(15.0) from the sequenced solver"
    );
}

// ── Coverage gap 6: removed cell whose dependents remain ─────────────────────

/// Removing an intermediate `let` binding (`fudge`) while keeping the dependent
/// cell (`volume`, now rewritten to inline the expression) must produce the
/// correct re-evaluated `volume` — matching a cold `eval(B)`.
///
/// Scenario: Module A has `fudge = thickness * 2.0`, `volume = width * height * fudge`.
/// Module B removes `fudge` and rewrites `volume = width * height * thickness * 2.0`.
/// The cross-check forces the "removed-cell dependents" invariant: whether the
/// dirty-set inclusion or the `old_reverse_index` defensive arm caught `volume`,
/// the final state must be correct.
///
/// Task 2087 — coverage gap 6.
#[test]
fn edit_source_removed_cell_with_dependent_matches_cold_eval() {
    // Module A: two-step chain thickness → fudge → volume.
    let module_a_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm
    let fudge = thickness * 2.0
    let volume = width * height * fudge
}"#;
    // Module B: `fudge` removed; `volume` now inlines the expression.
    // The removed-cell defensive arm (Step 6 — `old_reverse_index` dependents
    // traversal) ensures `volume` is re-evaluated even though its new expression
    // no longer references `fudge`.
    let module_b_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm
    let volume = width * height * thickness * 2.0
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // (a) volume must match cold (dependent recomputed correctly).
    let volume_id = ValueCellId::new("Bracket", "volume");
    assert_eq!(
        incr.values.get(&volume_id),
        cold_result.values.get(&volume_id),
        "Bracket.volume must match cold eval after removing the fudge intermediate cell"
    );

    // (b) fudge is absent from incremental's values map.
    let fudge_id = ValueCellId::new("Bracket", "fudge");
    assert!(
        incr.values.get(&fudge_id).is_none(),
        "Bracket.fudge should be absent from incremental values after its removal"
    );
}

// ── Coverage gap 7: removed constraint ────────────────────────────────────────

/// Removing a constraint must (a) shrink `constraint_results` by one,
/// (b) invalidate cached constraint entries, and (c) produce results
/// matching cold eval — symmetric with the existing added/modified tests.
///
/// Module A has two constraints (thickness > 2mm AND thickness < 20mm).
/// Module B keeps only the first.  The cross-check pins the removed-constraint
/// diff path in `engine_edit.rs`.
///
/// Task 2087 — coverage gap 7.
#[test]
fn edit_source_removed_constraint_invalidates_and_matches_cold_eval() {
    // Module A: two constraints.
    let module_a_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
    constraint thickness < 20mm
}"#;
    // Module B: second constraint REMOVED.
    let module_b_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    cold.eval(&module_b);

    // Collect constraint results from both paths via check_snapshot.
    let incr_check = incremental
        .check_snapshot(&module_b)
        .expect("check_snapshot must succeed after edit_source");
    let cold_check = cold
        .check_snapshot(&module_b)
        .expect("check_snapshot must succeed after cold eval");

    // (a) Cross-check values entry-for-entry.
    assert_values_match(&incr_check.values, &cold_check.values);

    // (b) constraint_results.len() == 1 on both (removed constraint is gone).
    assert_eq!(
        incr_check.constraint_results.len(),
        1,
        "incremental should have 1 constraint result after removal, got: {:?}",
        incr_check.constraint_results
    );
    assert_eq!(
        cold_check.constraint_results.len(),
        1,
        "cold should have 1 constraint result, got: {:?}",
        cold_check.constraint_results
    );

    // (c) Constraint satisfaction normalised by ConstraintNodeId must match.
    let incr_by_id: std::collections::HashMap<_, _> = incr_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction))
        .collect();
    let cold_by_id: std::collections::HashMap<_, _> = cold_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction))
        .collect();
    assert_eq!(
        incr_by_id, cold_by_id,
        "constraint satisfaction diverges between incremental and cold after constraint removal"
    );

    // (d) last_eval_set must NOT contain the removed constraint (Bracket#1).
    let eval_set = incremental.last_eval_set();
    assert!(
        !eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new("Bracket", 1))),
        "last_eval_set must NOT contain the removed constraint Bracket#1; \
         removed nodes should only be invalidated from cache, not re-demanded. \
         Got: {:?}",
        eval_set
    );
}

// ── Coverage gap 8: added realization ─────────────────────────────────────────

/// Adding a second geometry `let` must add a new `RealizationNodeId` to the
/// snapshot graph and place it in `last_eval_set()` — matching cold eval.
///
/// Exercises the `for rid in &added_realizations` splice into `dirty_cone` (Step 6b).
/// Task 2087 — coverage gap 8.
#[test]
fn edit_source_added_realization_is_tracked_and_matches_cold_eval() {
    // Module A: one realization `let body = box(...)`.
    let module_a_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(width, height, thickness)
}"#;
    // Module B: ADDS a second geometry let (two realizations).
    let module_b_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(width, height, thickness)
    let body2 = box(width, height, thickness)
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // (a) Both engines should have 2 realizations in the graph.
    let incr_snap = incremental
        .snapshot()
        .expect("incremental snapshot must exist");
    let cold_snap = cold.snapshot().expect("cold snapshot must exist");
    assert_eq!(
        incr_snap.graph.realizations.len(),
        2,
        "incremental: expected 2 realizations after adding body2, got {}",
        incr_snap.graph.realizations.len()
    );
    assert_eq!(
        cold_snap.graph.realizations.len(),
        2,
        "cold: expected 2 realizations, got {}",
        cold_snap.graph.realizations.len()
    );

    // (b) Incremental snapshot contains the added realization Bracket#1.
    assert!(
        incr_snap
            .graph
            .realizations
            .contains_key(&RealizationNodeId::new("Bracket", 1)),
        "incremental snapshot must contain Bracket#realization[1] after adding body2"
    );

    // (c) The added realization must appear in last_eval_set (added path).
    let eval_set = incremental.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Realization(RealizationNodeId::new("Bracket", 1))),
        "last_eval_set must contain the added Bracket#realization[1]; \
         got: {:?}",
        eval_set
    );
}

// ── Coverage gap 9: removed realization ───────────────────────────────────────

/// Dropping a geometry `let` must remove its `RealizationNodeId` from the
/// snapshot graph — matching cold eval.
///
/// Exercises the `for rid in &removed_realizations` cache invalidation (Step 9).
/// Task 2087 — coverage gap 9.
#[test]
fn edit_source_removed_realization_is_dropped_and_matches_cold_eval() {
    // Module A: TWO geometry lets (two realizations).
    let module_a_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(width, height, thickness)
    let body2 = box(width, height, thickness)
}"#;
    // Module B: `body2` REMOVED (one realization).
    let module_b_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(width, height, thickness)
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // (a) Both engines should have exactly 1 realization.
    let incr_snap = incremental
        .snapshot()
        .expect("incremental snapshot must exist");
    let cold_snap = cold.snapshot().expect("cold snapshot must exist");
    assert_eq!(
        incr_snap.graph.realizations.len(),
        1,
        "incremental: expected 1 realization after dropping body2, got {}",
        incr_snap.graph.realizations.len()
    );
    assert_eq!(
        cold_snap.graph.realizations.len(),
        1,
        "cold: expected 1 realization, got {}",
        cold_snap.graph.realizations.len()
    );

    // (b) The removed realization must no longer be in the incremental snapshot.
    assert!(
        !incr_snap
            .graph
            .realizations
            .contains_key(&RealizationNodeId::new("Bracket", 1)),
        "incremental snapshot must NOT contain Bracket#realization[1] after removing body2"
    );
}

// ── Coverage gap 10: modified realization ─────────────────────────────────────

/// Swapping argument order in a geometry `let` produces a different
/// `RealizationNodeData::content_hash` — the modified realization must appear
/// in `last_eval_set()` and its content_hash must match cold eval.
///
/// Exercises the `for rid in &changed_realizations` splice into `dirty_cone` (Step 6b).
/// Task 2087 — coverage gap 10.
#[test]
fn edit_source_modified_realization_content_hash_change_matches_cold_eval() {
    // Module A: `let body = box(width, height, thickness)`.
    let module_a_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(width, height, thickness)
}"#;
    // Module B: argument order swapped — same cell name, different content_hash
    // (classified `changed`, not added/removed).
    let module_b_src = r#"structure Bracket {
    param width : Scalar = 80mm
    param height : Scalar = 100mm
    param thickness : Scalar = 5mm

    let body = box(height, width, thickness)
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // Cross-check values entry-for-entry.
    assert_values_match(&incr.values, &cold_result.values);

    // (a) Both engines should have exactly 1 realization.
    let incr_snap = incremental
        .snapshot()
        .expect("incremental snapshot must exist");
    let cold_snap = cold.snapshot().expect("cold snapshot must exist");
    assert_eq!(
        incr_snap.graph.realizations.len(),
        1,
        "incremental: expected 1 realization after modifying body, got {}",
        incr_snap.graph.realizations.len()
    );
    assert_eq!(
        cold_snap.graph.realizations.len(),
        1,
        "cold: expected 1 realization, got {}",
        cold_snap.graph.realizations.len()
    );

    // (b) The realization's content_hash in incremental must match cold (not stale).
    let rid = RealizationNodeId::new("Bracket", 0);
    let incr_rnode = incr_snap
        .graph
        .realizations
        .get(&rid)
        .expect("Bracket#realization[0] must exist in incremental snapshot");
    let cold_rnode = cold_snap
        .graph
        .realizations
        .get(&rid)
        .expect("Bracket#realization[0] must exist in cold snapshot");
    assert_eq!(
        incr_rnode.content_hash, cold_rnode.content_hash,
        "Bracket#realization[0] content_hash must match between incremental and cold \
         after modifying the geometry arguments"
    );

    // (c) The modified realization must be in last_eval_set (changed path).
    let eval_set = incremental.last_eval_set();
    assert!(
        eval_set.contains(&NodeId::Realization(RealizationNodeId::new("Bracket", 0))),
        "last_eval_set must contain the modified Bracket#realization[0]; \
         got: {:?}",
        eval_set
    );
}

// ── Phase 1 & 3 performance: skip unchanged guarded groups ────────────────────

/// Performance lock: When a guard expression changes for exactly ONE of ten
/// independent `where` groups, `edit_source` must skip the remaining nine
/// groups in both Phase 1 and Phase 3, performing at most 2 non-skipped
/// group iterations total (one per phase for the affected group).
///
/// Test design:
/// - module_a: structure S with 10 independent `where uN { let xN = 1mm }` blocks,
///   all guarded by `uN: Bool = true`. eval(module_a) → all x0..x9 = 1mm.
/// - module_b: identical except group 3 changes guard from `where u3` to `where !u3`.
///   With `u3 = true`, `!u3` evaluates to `false`, so x3 deactivates to Undef.
///   Both Phase 1 (guard cell content_hash changed → dirty cone trigger) AND
///   Phase 3 (guard value changed: true → false → `guard_changed` outer gate) fire.
///   However, only group 3 actually changed its guard value — groups 0,1,2,4..9
///   all have `uN = true` unchanged — so only group 3 needs re-elaboration.
///
/// Without per-group skip (pre-task-2088):          counter = 10 (Phase 1) + 10 (Phase 3) = 20.
/// With per-group skip, no cross-phase dedup:       counter =  1 (Phase 1) +  1 (Phase 3) =  2.
/// With cross-phase dedup via phase1_reelaborated:  counter =  1 (Phase 1) +  0 (Phase 3) =  1.
///
/// Task 2088 — edit_source Phase 1 & 3 per-group skip.
/// Task 2142 — cross-phase dedup via `phase1_reelaborated` set.
#[test]
fn edit_source_phase1_and_3_skip_unchanged_guarded_groups() {
    let module_a_src = ten_bool_guarded_groups("u3");
    // Module B: identical except group 3's guard expression is negated.
    // With u3=true, `!u3` evaluates to false → x3 deactivates to Undef.
    // The guard cell for group 3 has a different expression text (content_hash
    // changes), so `has_dirty_guards` fires Phase 1; the guard value also
    // changes (true → false), so `guard_changed` fires Phase 3.
    let module_b_src = ten_bool_guarded_groups("!u3");
    let module_a = parse_and_compile(&module_a_src);
    let module_b = parse_and_compile(&module_b_src);

    // Incremental: eval(A) then edit_source(B).
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // Cold baseline: fresh eval(B).
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) Cross-check: incremental and cold must agree on all cells.
    assert_values_match(&incr.values, &cold_result.values);

    // (b) Positive anchor: x3 must be Undef in cold eval since guard (!u3) is
    // false when u3=true. Ensures the cross-check above can't silently pass
    // if both paths are wrong.
    let x3_id = ValueCellId::new("S", "x3");
    assert!(
        matches!(cold_result.values.get(&x3_id), Some(Value::Undef)),
        "cold eval must deactivate x3 to Undef when guard (!u3) is false with u3=true; \
         got {:?}",
        cold_result.values.get(&x3_id)
    );

    // (c) Performance lock: only group 3 must be re-elaborated — in Phase 1
    // (guard expression content_hash changed) but NOT in Phase 3 (cross-phase
    // dedup skips it via phase1_reelaborated). The other 9 groups have
    // unchanged guard values (uN=true in both modules) and must be skipped by
    // the per-group skip. Expected total: 1 (Phase 1) + 0 (Phase 3) = exactly 1.
    // Without the cross-phase dedup, this counter would be 1 + 1 = 2.
    // Without the per-group skip, this counter would be 10 + 10 = 20.
    let counter = incremental.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected exactly 1 non-skipped guard-phase group iteration \
         (Phase 1 processes group 3; Phase 3 skips it via phase1_reelaborated); \
         got {} — if 0, the counter increment is missing from edit_source \
         (instrumentation dropped); if 2, the cross-phase dedup is broken \
         (phase1_reelaborated set not populated or not consulted in Phase 3); \
         if > 2, the per-group skip is broken",
        counter
    );
}

// ── Phase 1-only perf locks: guard body fires, per-group skip suppresses all ───

/// Phase 1 fires (guard cell content_hash changed) but every per-group skip
/// applies (guard VALUE unchanged for all groups), so no group is
/// re-elaborated. Phase 3 never iterates (no guard value changed). Overall
/// `last_guard_phase_group_evals()` == 0.
///
/// Scenario: 10 groups `where uN { let xN = 1mm }` with all `uN: Bool = true`.
/// Module B rewrites group 3's condition from `where u3` to `where u3 && true`.
/// The guard cell `__guard_3`'s content_hash changes (expression text differs)
/// → `has_dirty_guards` fires Phase 1 (structure_controlling contains __guard_3
/// because guard cells are unconditionally added to structure_controlling at
/// compile time). Guard VALUE stays `true` for every
/// group (`u3 && true` == `u3` == true when u3=true). Phase 1's per-group skip
/// fires for all 10 groups (old_val == new_val == true, no added, no role-flip).
/// Counter == 0. Phase 3's `guard_changed` outer gate is false (no group's
/// value changed) → Phase 3 never iterates.
///
/// Regression classes caught:
/// (i)  Dropping `old_guard_val == Some(&guard_val)` from the Phase 1 per-group
///      skip: counter rises to 1 (group 3 re-processed despite unchanged value;
///      expression-text-only edits over-fire Phase 1).
/// (ii) Dropping the `guard_changed` outer gate from Phase 3: counter rises to
///      10 (all groups re-processed by Phase 3 on every guard-cell edit even
///      with no value change).
///
/// Task 2138 — Phase-1-only perf lock (T1).
#[test]
fn edit_source_phase1_fires_but_skips_when_guard_expr_text_changes_value_unchanged() {
    // Module A: 10 groups, each with a trivial `uN` guard expression.
    let module_a_src = ten_bool_guarded_groups("u3");
    // Module B: identical except group 3's guard is `u3 && true` instead of
    // `u3`. Expression text differs → content_hash of __guard_3 changes →
    // has_dirty_guards fires Phase 1. But `u3 && true` evaluates to the same
    // Bool(true) as plain `u3` when u3=true → per-group skip applies for all
    // 10 groups → no group is re-elaborated.
    let module_b_src = ten_bool_guarded_groups("u3 && true");
    let module_a = parse_and_compile(&module_a_src);
    let module_b = parse_and_compile(&module_b_src);

    // Incremental: eval(A) then edit_source(B).
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // Cold baseline: fresh eval(B).
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) Cross-check: incremental and cold must agree on all cells.
    assert_values_match(&incr.values, &cold_result.values);

    // (b) Positive anchor: x3 must stay ≈1mm since the guard `u3 && true`
    // evaluates to true when u3=true. Prevents a "both wrong" false pass on
    // the counter assertion below.
    let x3_id = ValueCellId::new("S", "x3");
    assert!(
        matches!(
            incr.values.get(&x3_id),
            Some(Value::Scalar { si_value, .. }) if (*si_value - 0.001).abs() < 1e-12
        ),
        "x3 must stay active (≈1mm) when guard `u3 && true` is true with u3=true; \
         got {:?}",
        incr.values.get(&x3_id)
    );

    // (c) Performance lock: counter == 0.
    // has_dirty_guards fires Phase 1 (__guard_3's content_hash changed). But
    // all 10 groups' guard VALUES are unchanged (true → true), so the
    // per-group skip (`old_guard_val == Some(&guard_val)`) suppresses all 10.
    // Phase 3's outer `guard_changed` gate is false → Phase 3 never iterates.
    // Expected: 0 group iterations in total.
    //
    // Regression catches:
    // == 1  → per-group skip regressed for expression-text-only changes
    //          (old_guard_val == Some(&guard_val) arm dropped; group 3
    //          re-processed despite unchanged value).
    // == 10 → per-group skip completely absent (all groups re-processed).
    // > 0 via Phase 3 → guard_changed outer gate regressed.
    let counter = incremental.last_guard_phase_group_evals();
    assert_eq!(
        counter, 0,
        "expected 0 non-skipped guard-phase group iterations \
         (Phase 1 enters body via has_dirty_guards but per-group skip suppresses \
          all 10 groups since guard values are unchanged; \
          Phase 3 outer gate never fires since no guard value changed); \
         got {} — \
         if 1, per-group skip regressed for expression-text-only changes \
           (old_guard_val == Some(&guard_val) arm of the Phase 1 per-group skip dropped); \
         if 10, per-group skip completely missing; \
         if > 0 from Phase 3, guard_changed outer gate regressed",
        counter
    );
}

// ── has_added_in_group forces non-skip: counter == 1 ────────────────────────

/// `has_added_in_group` forces Phase 1 re-elaboration for the affected group
/// even when the guard VALUE is unchanged. Phase 3 does not fire (no guard
/// value change). Overall `last_guard_phase_group_evals()` == 1.
///
/// Scenario: Module A has a single group `where u { let a = 1mm }` with
/// `u: Bool = true`. Module B adds a second member `let b = 2mm`. Cell `b` is
/// in `added`. `has_added_guard_member` is true → `has_dirty_guards` fires.
/// Guard VALUE unchanged (still true). For group 0: old_val == new_val == true,
/// BUT `has_added_in_group` is true (b ∈ group.members ∩ added) → the
/// has_added_in_group arm of the Phase 1 per-group skip fails → group 0 is re-elaborated → counter
/// increments to 1. Phase 3: `guard_changed` false (value unchanged) → Phase 3
/// never iterates.
///
/// Regression catch: dropping `has_added_in_group` from the Phase 1 per-group
/// skip condition would reduce counter to 0, meaning an added member on
/// the active branch would skip re-elaboration — the added member's
/// `default_expr` (here `2mm`) would not be evaluated and `b` would remain
/// Undef instead of being activated to its declared value. This perf lock
/// pins the clause that triggers re-elaboration.
///
/// Locks: the `has_added_in_group` detection logic and the
/// has_added_in_group arm of the Phase 1 per-group skip condition.
///
/// Task 2138 — has_added_in_group forces non-skip perf lock (T2).
#[test]
fn edit_source_added_member_in_unchanged_guard_group_forces_non_skip() {
    // Module A: single guarded group with one member.
    let module_a_src = r#"structure S {
    param u: Bool = true
    where u {
        let a = 1mm
    }
}"#;
    // Module B: adds a second member `b` to the same group.
    // Cell `b` is in `added`; `has_added_in_group` becomes true for group 0.
    // Guard value (u=true) is unchanged.
    let module_b_src = r#"structure S {
    param u: Bool = true
    where u {
        let a = 1mm
        let b = 2mm
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    // Incremental: eval(A) then edit_source(B).
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // Cold baseline: fresh eval(B).
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) Cross-check: incremental and cold must agree on all cells.
    assert_values_match(&incr.values, &cold_result.values);

    // (b) Positive anchor: `b` must evaluate to ≈2mm on the active branch
    // (u=true → members branch active; b's default_expr = 2mm is evaluated).
    // Prevents a "both wrong" false pass on the counter assertion.
    let b_id = ValueCellId::new("S", "b");
    assert!(
        matches!(
            incr.values.get(&b_id),
            Some(Value::Scalar { si_value, .. }) if (*si_value - 0.002).abs() < 1e-12
        ),
        "added member `b` on the active branch must evaluate to ≈2mm; \
         got {:?}",
        incr.values.get(&b_id)
    );

    // (c) Performance lock: counter == 1.
    // `has_added_in_group` is true (b ∈ group.members ∩ added) → the
    // has_added_in_group arm of the Phase 1 per-group skip is suppressed even though the
    // guard value is unchanged → group 0 is re-elaborated in Phase 1 →
    // counter increments to 1. Phase 3's outer `guard_changed` gate is false
    // (guard value did not change) → Phase 3 never iterates.
    //
    // Regression catches:
    // == 0 → `has_added_in_group` dropped from the Phase 1 per-group skip
    //         condition; added members on the active branch would silently
    //         retain Undef instead of being activated to their default value.
    // > 1  → Phase 3 guard_changed gate fired spuriously despite unchanged value.
    let counter = incremental.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected exactly 1 non-skipped guard-phase group iteration \
         (Phase 1 re-elaborates group 0 because has_added_in_group=true, \
          despite guard value being unchanged; Phase 3 does not fire); \
         got {} — \
         if 0, has_added_in_group arm of the Phase 1 per-group skip condition was \
           dropped (added member on the active branch would \
           silently retain Undef instead of being activated to its declared default value); \
         if > 1, Phase 3 guard_changed gate regressed",
        counter
    );
}

// ── has_role_flipped_guard_member forces non-skip: counter == 1 ──────────────

/// `has_role_flipped_guard_member` forces Phase 1 re-elaboration for the
/// affected group even when the guard VALUE is unchanged. Phase 3 does not
/// fire (no guard value change). Overall `last_guard_phase_group_evals()` == 1.
///
/// Scenario: Module A: `where u { let x = 1mm } else { let y = 2mm }` with
/// `u: Bool = true`. Module B swaps branches: `where u { let y = 2mm } else
/// { let x = 1mm }`. `u` unchanged (true); `x` retains its ValueCellId (S.x)
/// with unchanged default_expr → not `changed` (the change classifier operates
/// per-cell-id; x's expression text did not change). `y` likewise retains its
/// id (S.y) with unchanged default_expr → not `changed`. Neither is in `added`.
/// The `build_old_role_map` helper and the has_role_flipped_guard_member probe
/// are therefore the ONLY signal driving `has_dirty_guards`:
/// `has_role_flipped_guard_member` true → `has_dirty_guards` fires. Guard VALUE
/// unchanged (still true). For group 0: old == new == true,
/// `has_added_in_group` false, BUT `has_role_flipped_guard_member` true → the
/// has_role_flipped_guard_member arm of the Phase 1 per-group skip fails →
/// process → counter == 1. Phase 3: `guard_changed` false → never iterates.
///
/// Note: if the change classifier is ever updated to classify a cell's move
/// between `members` and `else_members` as `changed`, the role-flip probe
/// would no longer be the sole signal driving `has_dirty_guards`. Adjust this
/// test (update the rationale and, if needed, the expected counter) rather
/// than silently accepting a passing test whose stated reasons are wrong.
///
/// Assert values match cold eval: x=Undef, y=≈2mm (guard=true; x now on else
/// branch, y now on active branch).
///
/// Regression catch: dropping `has_role_flipped_guard_member` from the Phase 1
/// skip guard (has_role_flipped_guard_member arm of the Phase 1 per-group skip)
/// reduces counter to 0 — the guard loop skips
/// the group entirely and role-flipped members retain their old-branch values
/// (e.g. x remains Determined=1mm instead of deactivating to Undef).
///
/// Task 2138 — has_role_flipped_guard_member forces non-skip perf lock (T3).
#[test]
fn edit_source_role_flipped_member_in_unchanged_guard_group_forces_non_skip() {
    // Module A: x on active (where) branch, y on else branch.
    // With u=true: x=1mm (active), y=Undef (inactive).
    let module_a_src = r#"structure S {
    param u: Bool = true
    where u {
        let x = 1mm
    } else {
        let y = 2mm
    }
}"#;
    // Module B: branches swapped. x retains its id (S.x) with unchanged
    // default_expr → not `changed`. y retains its id (S.y) with unchanged
    // default_expr → not `changed`. Neither is `added`. The role-flip probe
    // (`build_old_role_map` + `has_role_flipped_guard_member`) is therefore
    // the only signal driving has_dirty_guards. Role-flip detected:
    // x moved members→else, y moved else→members.
    // With u=true: y now active (2mm), x now inactive (Undef).
    let module_b_src = r#"structure S {
    param u: Bool = true
    where u {
        let y = 2mm
    } else {
        let x = 1mm
    }
}"#;
    let module_a = parse_and_compile(module_a_src);
    let module_b = parse_and_compile(module_b_src);

    // Incremental: eval(A) then edit_source(B).
    let mut incremental = fresh_engine();
    incremental.eval(&module_a);
    let incr = incremental
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // Cold baseline: fresh eval(B).
    let mut cold = fresh_engine();
    let cold_result = cold.eval(&module_b);

    // (a) Cross-check: incremental and cold must agree on all cells.
    assert_values_match(&incr.values, &cold_result.values);

    // (b) Positive anchors (both needed to prevent "both wrong" false pass):
    //   - x is now on the else (inactive) branch → must be Undef.
    //   - y is now on the active branch → must be ≈2mm.
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    assert!(
        matches!(incr.values.get(&x_id), Some(Value::Undef)),
        "x must deactivate to Undef after role-flip (x moved to inactive else \
         branch with u=true); got {:?}",
        incr.values.get(&x_id)
    );
    assert!(
        matches!(
            incr.values.get(&y_id),
            Some(Value::Scalar { si_value, .. }) if (*si_value - 0.002).abs() < 1e-12
        ),
        "y must activate to ≈2mm after role-flip (y moved to active members \
         branch with u=true); got {:?}",
        incr.values.get(&y_id)
    );

    // (c) Performance lock: counter == 1.
    // `has_role_flipped_guard_member` is true → the has_role_flipped_guard_member
    // arm of the Phase 1 per-group skip is suppressed even though the guard
    // value is unchanged → group 0 is re-elaborated in Phase 1 → counter == 1.
    // Phase 3's outer `guard_changed` gate is false (guard value did not
    // change) → Phase 3 never iterates.
    //
    // Locks: `build_old_role_map` detection helper and the
    // has_role_flipped_guard_member probe; the has_role_flipped_guard_member
    // arm of the Phase 1 per-group skip condition.
    //
    // Regression catches:
    // == 0 → `has_role_flipped_guard_member` dropped from skip clause
    //         (role-flipped members retain old-branch values; e.g. x remains
    //         Determined=1mm instead of deactivating to Undef).
    // > 1  → Phase 3 guard_changed gate fired spuriously despite unchanged value.
    let counter = incremental.last_guard_phase_group_evals();
    assert_eq!(
        counter, 1,
        "expected exactly 1 non-skipped guard-phase group iteration \
         (Phase 1 re-elaborates group 0 because has_role_flipped_guard_member=true, \
          despite guard value being unchanged; Phase 3 does not fire); \
         got {} — \
         if 0, has_role_flipped_guard_member arm of the Phase 1 per-group skip \
           was dropped (role-flipped members would retain old-branch values); \
         if > 1, Phase 3 guard_changed gate regressed",
        counter
    );

    // (d) Premise lock: ValueCellNode::content_hash must NOT incorporate the
    // member/else_member role. If content_hash is ever extended to hash the
    // role, x and y would appear in `changed` on their own — has_dirty_guards
    // would fire via the changed-cell path even if the role-flip probe were
    // dropped, and T3's counter assertion above would keep passing for the
    // wrong reason (silent test-drift).
    //
    // We capture the exact diff tuple that edit_source consumed (not a
    // recomputed one) to rule out input-drift.
    let (changed, added, removed) = incremental
        .last_diff_value_cells()
        .expect("edit_source must populate last_diff_value_cells");
    assert!(
        !changed.contains(&x_id) && !added.contains(&x_id) && !removed.contains(&x_id),
        "Premise violated: S.x appeared in diff_value_cells (changed={}, added={}, \
         removed={}). ValueCellNode::content_hash must not incorporate the \
         member/else_member role — if it does, T3's perf-lock passes for the \
         wrong reason (role-flip probe is no longer the sole signal).",
        changed.contains(&x_id),
        added.contains(&x_id),
        removed.contains(&x_id),
    );
    assert!(
        !changed.contains(&y_id) && !added.contains(&y_id) && !removed.contains(&y_id),
        "Premise violated: S.y appeared in diff_value_cells (changed={}, added={}, \
         removed={}). ValueCellNode::content_hash must not incorporate the \
         member/else_member role — if it does, T3's perf-lock passes for the \
         wrong reason (role-flip probe is no longer the sole signal).",
        changed.contains(&y_id),
        added.contains(&y_id),
        removed.contains(&y_id),
    );
}

// ── Wave2 interaction: inactive members must stay Undef after cleanup ─────────

/// Regression guard for the post-wave2 cleanup (task 2142):
/// when Phase 1 deactivates a member and wave2 subsequently rewrites it
/// (because the member's default_expr reads a resolved auto param), the
/// post-wave2 cleanup step must re-deactivate it so Phase 3's
/// `phase1_reelaborated` skip leaves the engine in a correct state.
///
/// `auto` params are not expressible in reify source grammar (confirmed
/// 2026-04-22), so this test uses `TopologyTemplateBuilder` +
/// `CompiledModuleBuilder` + `SequencedMockConstraintSolver`.
///
/// Test design:
/// - Structure `S`: `param x: Length = 10mm`, `auto depth: Length`,
///   constraint `depth >= x` (dirty when x changes, so solver re-runs),
///   guarded group `where x > 5mm { let m = depth }` (m reads auto param).
/// - Module A: x=10mm → guard=true; solver→depth=10mm; m=10mm.
/// - Module B: x=3mm → guard=false; solver→depth=3mm.
/// - edit_source(B) sequence:
///   (1) Phase 1 fires (guard cell reads x; x in dirty cone); guard goes
///       false → m deactivated to Undef; phase1_reelaborated = {guard_cell}.
///   (2) Solver re-runs (constraint depth >= x reads x, so constraint is
///       dirty); solver returns depth = 3mm.
///   (3) Wave2 re-evaluates m (m reads depth) → writes m = 3mm,
///       overwriting Undef ← bug trigger.
///   (4) Post-wave2 cleanup (fix): inactive branch (members when guard=false)
///       re-deactivated → m = Undef again.
///   (5) Phase 3 skips the group via `phase1_reelaborated` (correct: cleanup
///       already restored the deactivated state).
/// - Assert: m = Undef after the edit.
///
/// Without the post-wave2 cleanup, m would be 3mm (wave2 value) because
/// Phase 3's dedup would skip the group and never re-deactivate m.
///
/// Task 2142 — post-wave2 cleanup for edit_source cross-phase dedup.
#[test]
#[allow(clippy::doc_overindented_list_items)]
fn edit_source_wave2_does_not_corrupt_inactive_members() {
    let fixture = wave2_flip_fixture();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(fixture.solver);

    // Initial eval: x=10mm, guard=true, solver→depth=10mm, m=depth=10mm.
    let initial = engine.eval(&fixture.module_initial);
    assert!(
        matches!(initial.values.get(&fixture.m_id), Some(Value::Scalar { si_value, .. }) if (*si_value - 0.010).abs() < 1e-10),
        "initial eval: m should be 10mm (= depth) when guard is true, got {:?}",
        initial.values.get(&fixture.m_id),
    );

    // edit_source(B): x changes 10mm → 3mm.
    //   Phase 1: guard cell (__guard_0, reads x) is in dirty_cone; x>5mm with
    //   x=3mm → false → m deactivated to Undef; phase1_reelaborated = {guard_cell}.
    //   Solver: constraint depth >= x reads x → dirty → solver returns depth=3mm.
    //   Wave2: m re-evaluated (reads depth=3mm) → m=3mm (overwrites Undef).
    //   Post-wave2 cleanup (fix): m re-deactivated to Undef (inactive branch).
    //   Phase 3: skips group via phase1_reelaborated (cleanup already correct).
    let edited = engine
        .edit_source(&fixture.module_edited)
        .expect("edit_source must succeed");

    // m must be Undef: guard is false (x=3mm ≤ 5mm) → members branch inactive.
    // Without the post-wave2 cleanup, m would be 3mm (wave2 re-evaluation result)
    // because Phase 3 is skipped by the dedup and never re-deactivates m.
    assert!(
        matches!(edited.values.get(&fixture.m_id), Some(Value::Undef)),
        "m must be Undef after guard flips false; \
         if m is a concrete value (e.g. 3mm), wave2 corrupted the inactive member \
         and the post-wave2 cleanup is missing or broken. Got {:?}",
        edited.values.get(&fixture.m_id),
    );
}

// ── Wave2 guard flip: else_members must be activated when guard flips post-wave2 ──

/// Regression guard for the cross-phase dedup guard-flip bug (task 2146).
///
/// When Phase 1 re-elaborates a guarded group (guard fires because guard_cell
/// reads an edited input `x`), it records a guard value V_phase1.  Then wave2
/// re-evaluates the guard cell (because the guard also reads a resolved auto
/// param `depth`) to a DIFFERENT value V_wave2.  Phase 3's dedup must NOT skip
/// this group — its old work (based on V_phase1) is stale, so newly-active
/// else_members must be evaluated.
///
/// Without the fix, Phase 3 sees `phase1_reelaborated.contains(guard_cell)` →
/// true → skips the group → newly-active else_members whose literal
/// default_exprs weren't touched by wave2 remain Undef.
///
/// Test design (the minimal three-state guard sequence):
/// - Struct S: `param x: Length`, `auto depth: Length`,
///   constraint `depth >= x` (dirty when x changes → solver re-runs),
///   composite guard `(x > 0mm) && (depth > 5mm)`,
///   member `let m = literal(mm(99))` (active when guard=true),
///   else_member `let n = literal(mm(42))` (active when guard=false).
///   Both default_exprs are literals that do NOT read `depth`, so wave2 will
///   not re-evaluate m or n directly — only the guard cell.
///
/// - Module A: x=-1mm.
///   Solver first call → depth=8mm.
///   After eval(A): guard = (-1>0)&&(8>5) = false (x≤0 makes guard false).
///   m=Undef, n=42mm.
///
/// - Module B: x=1mm.
/// - edit_source(B) Phase1→Wave2→Phase3 sequence:
///   (1) Phase 1 fires (guard_cell reads x; x changed in module_b → guard_cell
///   in dirty_cone).  Evaluates guard with x=1mm, depth=8mm (stale) →
///   (1>0)&&(8>5) = true.  old_guard_val=Bool(false) ≠ Bool(true) → Phase 1
///   re-elaborates.  phase1_reelaborated = {guard_cell: Bool(true)}.
///   m=99mm (members active), n=Undef (else_members deactivated).
///   (2) Solver: constraint `depth >= x` reads x (dirty) → solver runs → depth=3mm.
///   (3) Wave2: guard_cell reads depth (in all_resolved_ids) → re-evaluates
///   guard: (1>0)&&(3>5) = false.  Guard flips Bool(true) → Bool(false).
///   (4) reapply_phase1_deactivations: guard_val=Bool(false) → members (m) are
///   inactive → m re-deactivated to Undef.  else_members (n) are active →
///   skipped (n stays Undef from Phase 1's deactivation).
///   (5) Phase 3 (OLD, buggy): phase1_reelaborated.contains(guard_cell) → true
///   → continue → n stays Undef.  BUG: guard flipped after Phase 1.
///   (5) Phase 3 (FIXED): phase1_reelaborated.get(guard_cell) = Some(&Bool(true))
///   ≠ current Bool(false) → falls through to full re-elaboration → n=42mm.
///
/// Expected: m=Undef, n=42mm (Determined), matches cold eval of module_b.
///
/// Task 2146 — cross-phase dedup guard-flip fix.
#[test]
fn edit_source_wave2_guard_flip_activates_else_members() {
    use reify_compiler::{ValueCellDecl, ValueCellKind, Visibility};
    use reify_test_support::{
        CompiledModuleBuilder, MockConstraintChecker, SequencedMockConstraintSolver,
        TopologyTemplateBuilder, and, ge, gt, literal, mm, value_ref,
    };
    use reify_core::{ModulePath, SourceSpan, Type};
    use reify_ir::SolveResult;
    use std::collections::HashMap;

    let depth_id = ValueCellId::new("S", "depth");
    let guard_id = ValueCellId::new("S", "__guard_0");
    let m_id = ValueCellId::new("S", "m");
    let n_id = ValueCellId::new("S", "n");

    // Composite guard: (x > 0mm) && (depth > 5mm).
    // Reads x (edited → Phase 1 fires) AND depth (resolved → wave2 flips guard).
    let guard_expr = and(
        gt(value_ref("S", "x"), literal(mm(0.0))),
        gt(value_ref("S", "depth"), literal(mm(5.0))),
    );

    // Member m: literal 99mm. Does NOT read depth — wave2 won't overwrite it.
    // Active when guard=true.
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(99.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Else_member n: literal 42mm. Does NOT read depth — wave2 won't overwrite it.
    // Active when guard=false.
    let n_decl = ValueCellDecl {
        id: n_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(42.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Module A: x=-1mm → guard = (-1>0)&&(depth>5) = false regardless of depth.
    // n=42mm (else_members active), m=Undef (members inactive).
    let template_a = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(literal(mm(-1.0))))
        .auto_param("S", "depth", Type::length())
        // constraint reads both depth and x → dirty when x changes → solver re-runs
        .constraint(
            "S",
            0,
            Some("depth_ge_x"),
            ge(value_ref("S", "depth"), value_ref("S", "x")),
        )
        // guarded group: guard reads x (Phase 1) and depth (wave2 flip)
        .guarded_group(
            guard_expr.clone(),
            guard_id.clone(),
            vec![m_decl.clone()], // members: active when guard=true
            vec![],               // constraints
            vec![n_decl.clone()], // else_members: active when guard=false
            vec![],               // else_constraints
        )
        .build();
    let module_a = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .build();

    // Module B: x=1mm. Solver will return depth=3mm for the edit_source call.
    // After fix: guard = (1>0)&&(3>5) = false → m=Undef, n=42mm.
    let template_b = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(literal(mm(1.0))))
        .auto_param("S", "depth", Type::length())
        .constraint(
            "S",
            0,
            Some("depth_ge_x"),
            ge(value_ref("S", "depth"), value_ref("S", "x")),
        )
        .guarded_group(
            guard_expr.clone(),
            guard_id.clone(),
            vec![m_decl.clone()],
            vec![],
            vec![n_decl.clone()],
            vec![],
        )
        .build();
    let module_b = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_b)
        .build();

    // Sequenced solver:
    //   first call (eval A)       → depth=8mm (stale in Phase 1: 8>5=true → guard flips)
    //   second call (edit_source) → depth=3mm (wave2 re-eval: 3>5=false → guard flips back)
    let mut solved1 = HashMap::new();
    solved1.insert(depth_id.clone(), mm(8.0));
    let mut solved2 = HashMap::new();
    solved2.insert(depth_id.clone(), mm(3.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval(A): x=-1mm, guard=false, solver→depth=8mm.
    // m=Undef (members inactive), n=42mm (else_members active).
    let initial = engine.eval(&module_a);
    assert!(
        matches!(initial.values.get(&m_id), Some(Value::Undef)),
        "initial eval: m should be Undef (guard=false; x=-1mm), got {:?}",
        initial.values.get(&m_id),
    );
    assert!(
        matches!(initial.values.get(&n_id), Some(Value::Scalar { si_value, .. })
            if (*si_value - 0.042).abs() < 1e-10),
        "initial eval: n should be 42mm (else_members active; guard=false), got {:?}",
        initial.values.get(&n_id),
    );

    // edit_source(B): x changes -1mm → 1mm.
    //   Phase 1: guard_cell in dirty_cone(x); eval with x=1mm, depth=8mm(stale)
    //     → (1>0)&&(8>5)=true. old=false≠new=true → Phase 1 fires.
    //     phase1_reelaborated = {guard_cell: Bool(true)}.
    //     m=99mm, n=Undef.
    //   Solver: constraint depth>=x reads x (dirty) → depth=3mm.
    //   Wave2: guard_cell reads depth → re-eval: (1>0)&&(3>5)=false. Guard flips!
    //   reapply: m deactivated (Undef). n skipped (else_members; active branch).
    //   Phase 3 (OLD): guard_cell in phase1_reelaborated → skip → n stays Undef.  BUG.
    //   Phase 3 (FIXED): recorded Bool(true) ≠ current Bool(false) → n=42mm.  FIX.
    let edited = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // (a) m must be Undef: guard is false → members branch inactive.
    assert!(
        matches!(edited.values.get(&m_id), Some(Value::Undef)),
        "m must be Undef after guard ends up false (x=1mm; guard=(1>0)&&(3>5)=false). \
         Got {:?}",
        edited.values.get(&m_id),
    );

    // (b) n must be 42mm (Determined): guard is false → else_members branch active.
    // BUG (pre-fix): n remains Undef because Phase 3 skips the group via the stale
    // phase1_reelaborated entry (recorded Bool(true)) without detecting the wave2
    // guard flip to Bool(false). n's literal default_expr is never evaluated.
    assert!(
        matches!(edited.values.get(&n_id), Some(Value::Scalar { si_value, .. })
            if (*si_value - 0.042).abs() < 1e-10),
        "n must be 42mm (else_members active; guard ended up false after wave2 flip); \
         if n is Undef, Phase 3 incorrectly skipped the group via stale \
         phase1_reelaborated (task 2146 bug). Got {:?}",
        edited.values.get(&n_id),
    );

    // (c) Cross-check: incremental result must match cold eval of module_b.
    let mut cold_solved = HashMap::new();
    cold_solved.insert(depth_id.clone(), mm(3.0));
    let cold_solver = SequencedMockConstraintSolver::new(vec![SolveResult::Solved {
        values: cold_solved,
        unique: true,
    }]);
    let cold_checker = MockConstraintChecker::new();
    let mut cold_engine =
        Engine::new(Box::new(cold_checker), None).with_solver(Box::new(cold_solver));
    let cold = cold_engine.eval(&module_b);

    assert_eq!(
        edited.values.get(&m_id),
        cold.values.get(&m_id),
        "m: incremental edit_source result must match cold eval of module_b",
    );
    assert_eq!(
        edited.values.get(&n_id),
        cold.values.get(&n_id),
        "n: incremental edit_source result must match cold eval of module_b",
    );
}

// ── Role-flip + wave2 + Phase 3 dedup interaction ────────────────────────────

/// Regression guard for the interaction between the role-flip trigger, the
/// post-wave2 cleanup, and the Phase 3 cross-phase dedup (task 2147).
///
/// Coverage gap closed: existing tests cover only one trigger at a time.
/// `edit_source_wave2_does_not_corrupt_inactive_members` (task 2142) exercises
/// guard-value-change + wave2 + cleanup, but NOT the role-flip trigger.
/// `edit_source_role_flipped_guard_member_matches_cold_eval` (task 2084)
/// exercises role-flip, but predates the dedup and has no auto param → no wave2.
/// This test exercises role-flip + wave2 + Phase 3 dedup in one scenario,
/// per the esc-2142-102 reviewer's suggestion.
///
/// Test design — two-group structure:
///   Group 1 exercises the role-flip trigger (guard value unchanged, m moved).
///   Group 2 exercises a guard-value flip (true → false), which is needed to
///   open Phase 3's outer `guard_changed` gate, putting the dedup lookup on the
///   hot path.  Without group 2, Phase 3 would never iterate at all, so the
///   dedup skip (`if phase1_reelaborated.contains(&group.guard_cell)`) could
///   silently regress without the counter catching it.
///
/// Module A:
///   - param x = 10mm, param u = true
///   - auto depth; constraint depth == 10mm (solver placeholder)
///   - Group 1: guard (x > 5mm), members = [m = depth], else = []
///   - Group 2: guard u,          members = [n = 1mm],  else = []
///
/// Module B:
///   - param x = 10mm (UNCHANGED — group 1 guard value stays true)
///   - param u = false (CHANGED — group 2 guard value flips true → false)
///   - auto depth; constraint depth == 20mm (literal differs → content_hash
///     differs → constraint in changed_constraints → spliced into dirty_cone →
///     solver runs → depth = 20mm → wave2 re-evaluates m)
///   - Group 1: guard (x > 5mm), members = [],   else = [m = depth]
///     (m ROLE-FLIPPED; same id + same default_expr → same content_hash →
///      diff_value_cells does NOT classify m as changed or added)
///   - Group 2: guard u, members = [n = 1mm], else = []
///     (structure unchanged; only the input value of guard u flips)
///
/// Expected execution trace under current code:
///   1. eval(A):   depth=10mm, m=10mm (active), n=1mm (active).
///   2. edit_source(B):
///      Phase 1: has_role_flipped_guard_member=true, has_dirty_guards=true.
///        Group 1: old_guard=true, new_guard=true; no added-in-group; BUT
///                 global has_role_flipped_guard_member=true → skip fails →
///                 process. Counter=1. phase1_reelaborated += guard_1.
///                 Else_members=[m] deactivated (guard=true → else inactive).
///                 m = Undef.
///        Group 2: old_guard=true, new_guard=false → skip fails → process.
///                 Counter=2. phase1_reelaborated += guard_2.
///                 Members=[n] deactivated (guard=false). n = Undef.
///      Phase 2 (solver): constraint dirty → solver returns depth=20mm.
///      Wave2: m reads depth=20mm → m=20mm (OVERWRITES Undef). ← bug trigger.
///      reapply_phase1_deactivations (fix):
///        Group 1: guard=true, else_members=[m] inactive → m = Undef (restored).
///        Group 2: guard=false, members=[n] inactive → n=Undef (no-op).
///      Phase 3: guard_changed fires (group 2 guard changed). Iterate all groups:
///        Group 1: phase1_reelaborated contains guard_1 → DEDUP SKIP.
///        Group 2: phase1_reelaborated contains guard_2 → DEDUP SKIP.
///      Final: m=Undef, n=Undef, counter=2.
///
/// Assertions:
///   (a) m = Undef — pins post-wave2 cleanup running for the role-flipped
///       group. If m is a concrete value (e.g. 20mm), wave2 corrupted the
///       inactive member AND either: phase1_reelaborated.insert at line 1590
///       was moved before the Phase 1 skip (so role-flip groups are no longer
///       tracked), OR reapply_phase1_deactivations was removed from the
///       edit_source wave2 tail.
///   (b) n = Undef — sanity lock on group 2's guard flip. Guards against a
///       "both incremental and cold are wrong" false pass on assertion (a).
///   (c) last_guard_phase_group_evals() == 2 — Phase 1 processes both groups
///       (counter=2); Phase 3 dedup skips both (no Phase 3 increment). If the
///       counter is 3, the dedup consultation (phase1_reelaborated.contains in
///       Phase 3) regressed for role-flipped groups. If ≥4, the per-group skip
///       logic regressed. If <2, the Phase 1 counter increment was dropped.
///
/// `auto` params are not expressible in reify source grammar (confirmed
/// 2026-04-22), so this test uses `TopologyTemplateBuilder` +
/// `CompiledModuleBuilder` + `SequencedMockConstraintSolver`.
///
/// Task 2147 — role-flip/added-member interaction with edit_source cross-phase dedup.
/// Responds to esc-2142-102 (reviewer_comprehensive) post-merge coverage gap.
#[test]
#[allow(clippy::doc_overindented_list_items)]
fn edit_source_role_flip_wave2_and_phase3_dedup() {
    use reify_compiler::{ValueCellDecl, ValueCellKind, Visibility};
    use reify_test_support::{
        CompiledModuleBuilder, MockConstraintChecker, SequencedMockConstraintSolver,
        TopologyTemplateBuilder, eq, gt, literal, mm, value_ref, value_ref_typed,
    };
    use reify_core::{ModulePath, SourceSpan, Type};
    use reify_ir::SolveResult;
    use std::collections::HashMap;

    let depth_id = ValueCellId::new("S", "depth");
    let guard_1_id = ValueCellId::new("S", "__guard_0");
    let guard_2_id = ValueCellId::new("S", "__guard_1");
    let m_id = ValueCellId::new("S", "m");
    let n_id = ValueCellId::new("S", "n");

    // Guard 1 expression: x > 5mm (Length comparison → Bool).
    // References x (param), so guard_1 is in dirty_cone(x).
    let guard_1_expr = gt(value_ref("S", "x"), literal(mm(5.0)));
    // Guard 2 expression: u (Bool param reference).
    // When u changes default_expr, u is in dirty_cone, pulling guard_2 in too.
    let guard_2_expr = value_ref_typed("S", "u", Type::Bool);

    // Member m: let m = depth (reads auto param; role-flipped between A and B).
    // Same id + same default_expr in both modules → same content_hash →
    // diff_value_cells treats m as neither changed nor added.
    let m_decl = ValueCellDecl {
        id: m_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: Type::length(),
        default_expr: Some(value_ref("S", "depth")),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Member n: let n = 1mm (literal; does NOT read depth → wave2 won't touch it).
    let n_decl = ValueCellDecl {
        id: n_id.clone(),
        kind: ValueCellKind::Let,
        visibility: Visibility::Private,
        is_aux: false,
        cell_type: Type::length(),
        default_expr: Some(literal(mm(1.0))),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    };

    // Module A:
    //   x=10mm (guard_1: x>5mm = true → members=[m] active),
    //   u=true  (guard_2: u      = true → members=[n] active),
    //   constraint depth==10mm (solver placeholder; literal will change in B).
    let template_a = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(literal(mm(10.0))))
        .param("S", "u", Type::Bool, Some(literal(Value::Bool(true))))
        .auto_param("S", "depth", Type::length())
        .constraint(
            "S",
            0,
            Some("depth_eq"),
            eq(value_ref("S", "depth"), literal(mm(10.0))),
        )
        // Group 1: guard (x>5mm), m on members branch.
        .guarded_group(
            guard_1_expr.clone(),
            guard_1_id.clone(),
            vec![m_decl.clone()], // members (active when guard=true)
            vec![],               // constraints
            vec![],               // else_members
            vec![],               // else_constraints
        )
        // Group 2: guard u, n on members branch.
        .guarded_group(
            guard_2_expr.clone(),
            guard_2_id.clone(),
            vec![n_decl.clone()], // members (active when guard=true)
            vec![],
            vec![],
            vec![],
        )
        .build();
    let module_a = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_a)
        .build();

    // Module B:
    //   x=10mm UNCHANGED  (guard_1: x>5mm = true, value unchanged → role-flip only)
    //   u=false CHANGED   (guard_2: u     = false → n deactivates)
    //   constraint depth==20mm (literal changed → content_hash differs →
    //     constraint in changed_constraints → dirty_cone → solver runs → wave2)
    //   Group 1: m ROLE-FLIPPED to else_members (same id + default_expr → same content_hash).
    //   Group 2: structure unchanged; only u's default_expr changed.
    let template_b = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(literal(mm(10.0)))) // unchanged
        .param(
            "S",
            "u",
            Type::Bool,
            Some(literal(Value::Bool(false))), // flipped
        )
        .auto_param("S", "depth", Type::length())
        .constraint(
            "S",
            0,
            Some("depth_eq"),
            eq(value_ref("S", "depth"), literal(mm(20.0))), // expr text changed
        )
        // Group 1: guard unchanged; m moved to else_members (role-flip).
        .guarded_group(
            guard_1_expr,
            guard_1_id.clone(),
            vec![], // members empty in B
            vec![],
            vec![m_decl], // m ROLE-FLIPPED to else (inactive when guard=true)
            vec![],
        )
        // Group 2: structure unchanged; guard value will flip due to u=false.
        .guarded_group(
            guard_2_expr,
            guard_2_id.clone(),
            vec![n_decl],
            vec![],
            vec![],
            vec![],
        )
        .build();
    let module_b = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template_b)
        .build();

    // Sequenced solver:
    //   Call 1 (eval A):        depth = 10mm
    //   Call 2 (edit_source B): depth = 20mm  (drives wave2 overwrite of m)
    let mut solved_a = HashMap::new();
    solved_a.insert(depth_id.clone(), mm(10.0));
    let mut solved_b = HashMap::new();
    solved_b.insert(depth_id.clone(), mm(20.0));
    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved_a,
            unique: true,
        },
        SolveResult::Solved {
            values: solved_b,
            unique: true,
        },
    ]);

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Initial eval(A): x=10mm, guard_1=true, solver→depth=10mm, m=10mm, n=1mm.
    let initial = engine.eval(&module_a);

    // (d) Sanity anchor: m must be a concrete 10mm value in eval(A).
    // If this fails, the Undef observed after edit_source(B) could be from an
    // incorrect initial state rather than the incremental edit.
    assert!(
        matches!(initial.values.get(&m_id), Some(Value::Scalar { si_value, .. }) if (*si_value - 0.010).abs() < 1e-10),
        "initial eval: m should be 10mm (= depth) when guard_1 is true; \
         if m is Undef or wrong, the eval(A) setup is incorrect and the \
         post-edit Undef check would be a false pass. Got {:?}",
        initial.values.get(&m_id),
    );

    // edit_source(B):
    //   Phase 1 triggers: has_role_flipped_guard_member=true (m moved A→else);
    //                     has_dirty_guards=true (u changed → guard_2 in dirty_cone).
    //   Group 1 processed (role-flip defeats per-group skip). m deactivated → Undef.
    //   Group 2 processed (guard value true→false). n deactivated → Undef.
    //   Solver: constraint depth==20mm differs → runs → depth=20mm.
    //   Wave2: m reads depth → m=20mm (overwrites Undef). ← bug trigger.
    //   reapply_phase1_deactivations: group 1 (guard=true, else=[m]) → m=Undef.
    //                                  group 2 (guard=false, members=[n]) → n=Undef.
    //   Phase 3: guard_changed fires (group 2). Both groups dedup-skipped.
    let edited = engine
        .edit_source(&module_b)
        .expect("edit_source must succeed");

    // (a) m must be Undef: guard_1=true but m is on the now-inactive else branch.
    // If m is a concrete value (e.g. 20mm), wave2 corrupted the inactive member
    // AND the post-wave2 cleanup failed for the role-flipped group — either
    // phase1_reelaborated.insert (engine_edit.rs:1590) was moved before the Phase 1
    // skip (role-flip groups no longer tracked), or reapply_phase1_deactivations
    // was removed from the edit_source wave2 tail.
    assert!(
        matches!(edited.values.get(&m_id), Some(Value::Undef)),
        "m must be Undef after edit_source(B): guard_1=true so else_members=[m] \
         is inactive. If m is a concrete value (e.g. 20mm), wave2 corrupted the \
         inactive member and the post-wave2 cleanup is missing or broken for \
         role-flipped groups. Got {:?}",
        edited.values.get(&m_id),
    );

    // (b) n must be Undef: guard_2 flipped true→false so members=[n] is inactive.
    // Sanity lock — rules out a "both incremental and cold are wrong" false pass
    // on assertion (a). If this fails independently, group 2's guard flip did
    // not deactivate n, indicating a regression in Phase 1's basic deactivation
    // path (not specific to role-flip or wave2).
    assert!(
        matches!(edited.values.get(&n_id), Some(Value::Undef)),
        "n must be Undef after edit_source(B): guard_2 flipped false so \
         members=[n] is inactive. Got {:?}",
        edited.values.get(&n_id),
    );

    // (c) Performance / dedup lock: counter must be exactly 2.
    // Phase 1 processes both groups (counter=2: group 1 via role-flip, group 2
    // via guard-value change). Phase 3's outer guard_changed gate fires (group 2
    // guard changed), but the dedup check (phase1_reelaborated.contains) skips
    // both groups → no Phase 3 increment.
    // If counter==3: Phase 3 dedup is broken for role-flipped groups (group 2
    //   would be re-processed by Phase 3's per-group path since old≠new guard).
    // If counter>=4: per-group skip logic in Phase 3 regressed.
    // If counter<2: Phase 1 counter increment missing for role-flip or guard-flip path.
    let counter = engine.last_guard_phase_group_evals();
    assert_eq!(
        counter, 2,
        "expected exactly 2 non-skipped guard-phase group iterations \
         (Phase 1: group 1 via role-flip, group 2 via guard-value change; \
          Phase 3: both groups dedup-skipped via phase1_reelaborated); \
         got {} — if 3, Phase 3 dedup consultation regressed for the \
         role-flipped group (phase1_reelaborated not consulted or not populated \
         for role-flip triggers); if >=4, per-group skip in Phase 3 regressed; \
         if <2, Phase 1 counter increment dropped for one of the trigger paths",
        counter,
    );
}

// ── Role-flip probe deferred behind short-circuit (task 2094) ───────────────

/// `detect_role_flip` builds two O(N) `HashMap`s over `guarded_groups` on
/// every call. On hot edits where a cheaper trigger (`sc_dirty` or
/// `has_added_guard_member`) already fires `has_dirty_guards`, paying that
/// cost is wasted work.
///
/// This test targets the scenario where:
/// - `sc_dirty = true` (every guard param `uN` changed → every `__guard_N` in
///   dirty_cone via `structure_controlling`).
/// - For every group, `guard_value_unchanged = false` (Bool(true) → Bool(false))
///   → the per-group skip check short-circuits before reaching the role-flip
///   branch.
///
/// Expected after the deferred-probe refactor (task 2094 step-2):
/// `last_role_flip_probes() == 0` — `sc_dirty` fires the outer trigger, and
/// the per-group skip never needs to consult the role-flip result, so
/// `detect_role_flip` is never called.
///
/// Correctness is pinned by task-2084's lock
/// `edit_source_role_flipped_guard_member_matches_cold_eval` (:945) and its
/// symmetric counterpart `_inactive_to_active_matches_cold_eval` (:1027),
/// which remain unaffected by the timing change.
///
/// Task 2094 — deferred role-flip probe perf lock.
#[test]
fn edit_source_role_flip_probe_deferred_when_every_guard_value_changes() {
    // Module A: 2 guarded groups, each guard param defaulting to `true`.
    let module_a_src = r#"structure S {
    param u0: Bool = true
    param u1: Bool = true
    where u0 {
        let x0 = 1mm
    }
    where u1 {
        let x1 = 1mm
    }
}"#;
    // Module B: identical structure, but guard params default to `false`.
    // Every `uN` cell is in `changed_set` (default_expr differs) →
    // every `__guard_N` is in dirty_cone (structure_controlling) →
    // `sc_dirty = true`. Guard VALUES flip from Bool(true) to Bool(false),
    // so `guard_value_unchanged = false` for every group → per-group skip
    // short-circuits before the role-flip branch is consulted.
    let module_b_src = r#"structure S {
    param u0: Bool = false
    param u1: Bool = false
    where u0 {
        let x0 = 1mm
    }
    where u1 {
        let x1 = 1mm
    }
}"#;
    let (cold_result, probes) = run_probe_scenario(module_a_src, module_b_src);

    // (b) Positive anchor: x0 and x1 must be Undef in cold (guards now false).
    // Prevents a "both wrong" false-pass on the counter assertion below.
    let x0_id = ValueCellId::new("S", "x0");
    let x1_id = ValueCellId::new("S", "x1");
    assert!(
        matches!(cold_result.values.get(&x0_id), Some(Value::Undef)),
        "x0 must be Undef after guard u0 flips to false (inactive branch); \
         got {:?}",
        cold_result.values.get(&x0_id)
    );
    assert!(
        matches!(cold_result.values.get(&x1_id), Some(Value::Undef)),
        "x1 must be Undef after guard u1 flips to false (inactive branch); \
         got {:?}",
        cold_result.values.get(&x1_id)
    );

    // (c) Performance lock: detect_role_flip must NOT be called when sc_dirty
    // fires the outer trigger and every group's guard VALUE changed.
    //
    // Regression catalogue:
    // == 1 → eager call regressed: detect_role_flip invoked unconditionally
    //         on every edit_source (the pre-refactor behaviour that task 2094
    //         eliminates; probe is back behind sc_dirty short-circuit failure).
    // >  1 → memoization broke: detect_role_flip called more than once per edit.
    assert_eq!(
        probes, 0,
        "expected 0 detect_role_flip probes (sc_dirty fires the outer trigger; \
         all groups' guard VALUES changed so per-group skip short-circuits before \
         the role-flip branch); \
         got {} — \
         if 1, eager call regressed (detect_role_flip invoked unconditionally); \
         if >1, memoization broke",
        probes
    );
}

/// Tests the `has_added_guard_member` short-circuit path of the deferred
/// role-flip probe (task 2094 amendment).
///
/// When a new `let` is inserted into an existing `where` group without
/// touching the guard param (`sc_dirty = false`, `has_added_guard_member =
/// true`), the outer `||` chain fires at the `has_added_guard_member` arm —
/// the role-flip deferred block is never entered and `last_role_flip_probes`
/// stays at 0.
///
/// This exercises the second "0 probes" short-circuit path promised by task
/// 2094. A regression that mistakenly placed `has_added_guard_member` inside
/// the deferred-probe block (instead of before it) would produce probes == 1.
///
/// Task 2094 — has_added_guard_member short-circuit perf lock.
#[test]
fn edit_source_role_flip_probe_skipped_when_guard_member_added() {
    // Module A: one guarded group, single member x0.
    let module_a_src = r#"structure S {
    param u0: Bool = true
    where u0 {
        let x0 = 1mm
    }
}"#;
    // Module B: same guard param (u0 default unchanged → u0 not in changed_set
    // → __guard_0 not in dirty_cone → sc_dirty = false), new member x1 added to
    // the group (x1 in added ∩ group.members → has_added_guard_member = true).
    let module_b_src = r#"structure S {
    param u0: Bool = true
    where u0 {
        let x0 = 1mm
        let x1 = 2mm
    }
}"#;
    let (cold_result, probes) = run_probe_scenario(module_a_src, module_b_src);

    // (b) Positive anchor: x1 must be a determined Scalar in cold (u0=true
    // activates the where-branch). Prevents a "both wrong" false-pass.
    let x1_id = ValueCellId::new("S", "x1");
    assert!(
        matches!(cold_result.values.get(&x1_id), Some(Value::Scalar { .. })),
        "x1 must be a determined Scalar when guard u0=true activates it; \
         got {:?}",
        cold_result.values.get(&x1_id)
    );

    // (c) Performance lock: detect_role_flip must NOT be called when
    // has_added_guard_member fires the outer trigger (sc_dirty = false,
    // has_added_guard_member = true → short-circuits before role-flip block).
    //
    // Regression catalogue:
    // == 1 → has_added_guard_member was placed inside the deferred-probe block
    //         rather than before it; detect_role_flip invoked when only the
    //         added-member trigger fires.
    // >  1 → memoization broke.
    assert_eq!(
        probes, 0,
        "expected 0 detect_role_flip probes (has_added_guard_member fires the \
         outer trigger without entering the role-flip block); \
         got {} — \
         if 1, has_added_guard_member short-circuit regressed; \
         if >1, memoization broke",
        probes
    );
}

/// Tests that role-flip memoization limits `detect_role_flip` to exactly one
/// call even when multiple guarded groups reach the per-group skip check
/// (task 2094 amendment).
///
/// Scenario: two guarded groups, both with unchanged guard VALUES (`sc_dirty =
/// false`, `has_added_guard_member = false`). A role-flip in group 1 causes
/// the outer deferred-probe block to fire: `detect_role_flip` returns true,
/// probe count = 1, `role_flip_memo = Some(true)`. Both groups then reach the
/// per-group skip check (`guard_value_unchanged = true`, no added members) and
/// read from the memoised result — no additional `detect_role_flip` calls.
///
/// Expected: `last_role_flip_probes() == 1`.
///
/// Correctness remains pinned by the task-2084 lock
/// `edit_source_role_flipped_guard_member_matches_cold_eval` (:945) and its
/// symmetric counterpart `_inactive_to_active_matches_cold_eval` (:1027).
///
/// Task 2094 — memo reuse across multiple groups perf lock.
#[test]
fn edit_source_role_flip_probe_memoised_across_multiple_groups() {
    // Module A: two guarded groups; `moving` is on the active where-branch of
    // group 1 (guarded by u0). Group 2 (guarded by u1) is stable.
    let module_a_src = r#"structure S {
    param u0: Bool = true
    param u1: Bool = true
    where u0 {
        let x0 = 1mm
        let moving = 2mm
    }
    where u1 {
        let y0 = 3mm
    }
}"#;
    // Module B: `moving` (same id, same expr → same content_hash → not in
    // `added`) relocates to the inactive else-branch of group 1. Group 2 is
    // unchanged. Both u0 and u1 default to true in A and B:
    //   → neither u0 nor u1 is in changed_set
    //   → __guard_0 and __guard_1 are not in dirty_cone → sc_dirty = false.
    //   → `moving` has the same content_hash → has_added_guard_member = false.
    // The outer deferred-probe block evaluates, finds role_flip = true (probe
    // #1, memo = Some(true)). Both groups reach the per-group skip check with
    // guard_value_unchanged = true and no added members; both read the memo
    // without a second probe.
    let module_b_src = r#"structure S {
    param u0: Bool = true
    param u1: Bool = true
    where u0 {
        let x0 = 1mm
    } else {
        let moving = 2mm
    }
    where u1 {
        let y0 = 3mm
    }
}"#;
    let (cold_result, probes) = run_probe_scenario(module_a_src, module_b_src);

    // (b) Positive anchor: `moving` must be Undef in cold (relocated to
    // else-branch, which is inactive when u0=true). Prevents a "both wrong"
    // false-pass on the counter assertion.
    let moving_id = ValueCellId::new("S", "moving");
    assert!(
        matches!(cold_result.values.get(&moving_id), Some(Value::Undef)),
        "moving must be Undef after role-flip to else-branch (u0=true, \
         else-branch inactive); got {:?}",
        cold_result.values.get(&moving_id)
    );

    // (c) Performance lock: exactly one detect_role_flip call (from the outer
    // deferred-probe block); both per-group iterations read the memoised
    // Some(true) without triggering a second probe.
    //
    // Regression catalogue:
    // == 0 → outer probe incorrectly skipped (sc_dirty or has_added_guard_member
    //         short-circuited when they should not have).
    // >= 2 → memoization broke: the per-group None arm called detect_role_flip
    //         again for a group that should have read role_flip_memo.
    assert_eq!(
        probes, 1,
        "expected exactly 1 detect_role_flip probe (outer deferred-probe block \
         fires once; per-group loop reads memo for both groups); \
         got {} — \
         if 0, outer probe was incorrectly skipped; \
         if >=2, memoization broke (per-group arm probed again)",
        probes
    );
}

// ── Invariant-check tests ──────────────────────────────────────────────────

/// `edit_source` must panic (in debug builds) on a malformed module whose
/// `ValueCellDecl.cell_type` is an unrepresentable variant — `Type::TypeParam`
/// — that has no `Value` counterpart.
///
/// `Type::Geometry` was removed from this loop in task 3604 / GHR-β because
/// `Value::GeometryHandle` now makes Geometry a representable cell type; see
/// `is_representable_cell_type_admits_geometry` in invariant_tests (engine_eval.rs).
///
/// Mirrors the in-crate unit test
/// `invariant_tests::panics_on_unrepresentable_cell_types` (engine_eval.rs).
/// The source of truth for which variants are unrepresentable is the runtime
/// guard `assert_value_cell_types_representable` at engine_eval.rs:60.
///
/// This mirrors the defensive invariant already present on the `Engine::eval`
/// cold-start path.  Without the corresponding fix in engine_edit.rs the call
/// either returns normally or panics later with an unrelated message — the
/// assertion must fire immediately after `Snapshot::from_compiled_module`.
#[test]
#[cfg(debug_assertions)]
#[allow(clippy::single_element_loop)]
fn edit_source_panics_on_unrepresentable_cell_type() {
    use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
    use reify_core::{ModulePath, Type};
    use std::panic;

    for ty in [Type::TypeParam("T".into())] {
        // edit_source requires an Initialized engine — seed one first with a valid eval.
        // The engine must be re-created per iteration: edit_source mutates state before
        // the assertion fires, leaving the engine potentially poisoned after a panic.
        let mut engine = fresh_engine();
        let good = bracket_compiled_module();
        engine.eval(&good);

        // Build a malformed module that bypasses the compiler: a single ValueCellDecl
        // whose cell_type is the bound `ty` from the loop, an unrepresentable variant
        // (either Type::TypeParam or Type::Geometry) that triggers the invariant assertion.
        let bad_module = CompiledModuleBuilder::new(ModulePath::single("bad"))
            .template(
                TopologyTemplateBuilder::new("Bad")
                    .param("Bad", "x", ty, None)
                    .build(),
            )
            .build();

        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = engine.edit_source(&bad_module);
        }));
        assert!(
            result.is_err(),
            "expected edit_source to panic on unrepresentable cell_type but it returned normally",
        );
        let err = result.unwrap_err();
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic>");
        assert!(
            msg.contains(reify_eval::ASSERT_MSG_PREFIX),
            "panic message did not contain expected substring {:?}: {msg}",
            reify_eval::ASSERT_MSG_PREFIX,
        );
    }
}
