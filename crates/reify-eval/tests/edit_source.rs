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
use reify_types::{
    ConstraintNodeId, RealizationNodeId, Satisfaction, SnapshotProvenance, Value, ValueCellId,
    ValueMap,
};

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

/// Self-test for the `assert_values_match` helper: verifies both the pass path
/// (identical maps do not panic) and the fail path (diverging maps panic with a
/// message containing the literal "diverges").
#[test]
fn assert_values_match_panics_on_divergence() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    // Pass path: two maps with identical entries must not panic.
    let mut a = ValueMap::new();
    let mut b = ValueMap::new();
    let k1 = ValueCellId::new("S", "x");
    let k2 = ValueCellId::new("S", "y");
    a.insert(k1.clone(), Value::Int(1));
    a.insert(k2.clone(), Value::length(0.01));
    b.insert(k1.clone(), Value::Int(1));
    b.insert(k2.clone(), Value::length(0.01));
    assert_values_match(&a, &b); // must not panic

    // Fail path: maps that disagree on one key must panic; message must contain "diverges".
    let mut c = ValueMap::new();
    let mut d = ValueMap::new();
    let k3 = ValueCellId::new("S", "z");
    c.insert(k3.clone(), Value::Int(42));
    d.insert(k3.clone(), Value::Int(99));
    let payload = catch_unwind(AssertUnwindSafe(|| {
        assert_values_match(&c, &d);
    }))
    .expect_err("assert_values_match must panic on diverging values");
    let msg = payload
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("panic payload must be a string");
    assert!(
        msg.contains("diverges"),
        "panic message must contain \"diverges\", got: {msg}"
    );
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
        let incr_val = incr_edit.values.get(key);
        let cold_val = cold_check.values.get(key);
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

    // (a) Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

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

// ── Coverage gap 2: Step-11 functions table refresh ───────────────────────────

/// Changing a user-function body AND adding a call site that references the new
/// body must yield the updated result on the incremental path — matching a cold
/// `eval(B)`.  The added call-site cell is classified `added` (no prior cache
/// entry), so it must freshly evaluate against the refreshed `self.functions`
/// table installed by Step-11 of `edit_source`.
///
/// If Step-11's `self.functions = ...` refresh at `engine_edit.rs:1169-1172`
/// were skipped, the incremental engine would compute `3.0 * 4.0 = 12.0`
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

    // Anchor: `result` must be Int(7) = 3 + 4 (refreshed sum body).
    // A missing functions-table refresh would produce Int(12) = 3 * 4 from module_A's
    // multiply body; seeing Int(7) on both paths confirms the '+' body is used.
    let result_id = ValueCellId::new("Panel", "result");
    assert_eq!(
        incr.values.get(&result_id),
        Some(&Value::Int(7)),
        "Panel.result should be Int(7) = foo(width, height) with the refreshed '+' body; \
         got {:?} — likely the functions table was not refreshed",
        incr.values.get(&result_id)
    );
    assert_eq!(
        cold_result.values.get(&result_id),
        Some(&Value::Int(7)),
        "cold Panel.result should also be Int(7)"
    );
}

// ── Coverage gap 3: Step-11 compiled_purposes table refresh ───────────────────

/// Changing a purpose body (adding a constraint) and then activating the purpose
/// on the incremental engine must inject module_B's constraints — matching the
/// cold path.  If Step-11's `self.compiled_purposes = ...` refresh at
/// `engine_edit.rs:1172` were skipped, incremental would inject module_A's
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
    let incr_ids: HashSet<ConstraintNodeId> =
        incr_snap.graph.constraints.keys().cloned().collect();
    let cold_ids: HashSet<ConstraintNodeId> =
        cold_snap.graph.constraints.keys().cloned().collect();
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
/// meta change with an ADDED reading cell guarantees that the refreshed
/// `self.meta_map` at `engine_edit.rs:1173-1178` is actually consulted.
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

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

// ── Coverage gap 5: Step-11 objectives table refresh ─────────────────────────

/// Flipping a template's objective from `Minimize` to `Maximize` (while also
/// adding a constraint so the dirty cone touches the auto-param group) must cause
/// `edit_source` to forward the *new* `Maximize` objective to the solver —
/// matching a cold `eval(B)`.
///
/// If Step-11's `self.objectives.clear(); self.objectives.insert(...)` refresh at
/// `engine_edit.rs:1221-1225` were skipped, `self.objectives` would still carry
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
/// unlike the other 9 tests which use `parse_and_compile`.
///
/// Task 2087 — coverage gap 5.
#[test]
fn edit_source_refreshes_objectives_against_cold_eval() {
    use std::collections::HashMap;
    use reify_test_support::{
        CompiledModuleBuilder, MockConstraintChecker, MockConstraintSolver,
        MultiCallSpyConstraintSolver, TopologyTemplateBuilder, gt, lt, literal, mm, value_ref,
    };
    use reify_types::{ModulePath, OptimizationObjective, SolveResult, Type};

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
        .objective(OptimizationObjective::Minimize(value_ref(
            "S",
            "thickness",
        )))
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
        .objective(OptimizationObjective::Maximize(value_ref(
            "S",
            "thickness",
        )))
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

    let mut incremental = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(spy));
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
        matches!(
            &problems[0].objective,
            Some(OptimizationObjective::Minimize(_))
        ),
        "eval(A) should forward Minimize objective, got: {:?}",
        problems[0].objective
    );

    // Call 1 (edit_source(B)): objective must be Maximize — proving Step-11 refreshed
    // self.objectives before the solver phase ran.
    assert!(
        matches!(
            &problems[1].objective,
            Some(OptimizationObjective::Maximize(_))
        ),
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

    // Final call-count guard: the spy's SequencedMockConstraintSolver repeats the
    // last result on exhaustion rather than panicking, so a silent third solver call
    // during edit_source would return mm(15.0) again and all preceding assertions
    // would still pass.  Re-asserting the total call count here catches that case.
    // The spy is pre-configured with exactly 2 results (vec! above), so any extra
    // invocation is an unintended re-solve.
    {
        let problems = captured.lock().unwrap();
        assert_eq!(
            problems.len(),
            2,
            "expected exactly 2 solver calls total (eval(A) + edit_source(B)); \
             got {} — extra calls indicate an unintended re-solve; if intentional, \
             extend the spy's result sequence accordingly",
            problems.len()
        );
    }
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
    // The removed-cell defensive arm at engine_edit.rs:1002-1017 ensures
    // `volume` is re-evaluated even though its new expression no longer
    // references `fudge`.
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

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

    // (a) Cross-check values via key-union.
    let incr_keys: HashSet<&ValueCellId> =
        incr_check.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> =
        cold_check.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr_check.values.get(key);
        let cold_val = cold_check.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

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
/// Exercises the `for rid in &added_realizations` splice at
/// `engine_edit.rs:1040-1042`.  Task 2087 — coverage gap 8.
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

    // (a) Both engines should have 2 realizations in the graph.
    let incr_snap = incremental.snapshot().expect("incremental snapshot must exist");
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
/// Exercises cache invalidation at `engine_edit.rs:1152-1154`.
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

    // (a) Both engines should have exactly 1 realization.
    let incr_snap = incremental.snapshot().expect("incremental snapshot must exist");
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
/// Exercises the `for rid in &changed_realizations` splice at
/// `engine_edit.rs:1037-1039`.  Task 2087 — coverage gap 10.
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

    // Cross-check values entry-for-entry via key-union.
    let incr_keys: HashSet<&ValueCellId> = incr.values.iter().map(|(k, _)| k).collect();
    let cold_keys: HashSet<&ValueCellId> = cold_result.values.iter().map(|(k, _)| k).collect();
    let all_keys: HashSet<&ValueCellId> = incr_keys.union(&cold_keys).copied().collect();
    for key in &all_keys {
        let incr_val = incr.values.get(key);
        let cold_val = cold_result.values.get(key);
        assert_eq!(
            incr_val, cold_val,
            "value for {key} diverges: incremental={:?}, cold={:?}",
            incr_val, cold_val
        );
    }

    // (a) Both engines should have exactly 1 realization.
    let incr_snap = incremental.snapshot().expect("incremental snapshot must exist");
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
