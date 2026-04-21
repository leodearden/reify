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
use reify_types::{ConstraintNodeId, Satisfaction, SnapshotProvenance, Value, ValueCellId};

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
    let module_b =
        parse_and_compile(&bracket_with_volume_expr("width * height * thickness * 2.0"));
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

    // The union of keys across both maps must agree.
    let incr_keys: HashSet<&ValueCellId> = incr_edit.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_check.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr_edit.values.get(*key);
        let cold_val = cold_check.values.get(*key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

    // Constraint check results must match entry-for-entry. Normalise by
    // `ConstraintNodeId` since ordering is implementation-defined. HashMap
    // rather than BTreeMap because ConstraintNodeId is not Ord.
    let incr_by_id: std::collections::HashMap<_, _> = incr_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction.clone()))
        .collect();
    let cold_by_id: std::collections::HashMap<_, _> = cold_check
        .constraint_results
        .iter()
        .map(|e| (e.id.clone(), e.satisfaction.clone()))
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
