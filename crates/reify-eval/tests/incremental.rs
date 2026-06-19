//! Integration tests for the incremental evaluator pipeline.
//!
//! These tests verify that Engine's incremental evaluation (edit_param)
//! produces correct results, proper provenance, partial re-evaluation,
//! early cutoff, and freshness transitions.

use std::collections::HashMap;

use reify_core::{
    ConstraintNodeId, ContentHash, ModulePath, SnapshotId, Type, ValueCellId, VersionId,
};
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, DeterminacyState, Freshness, SnapshotProvenance, Value,
};
use reify_test_support::bracket_compiled_module;
use reify_test_support::builders::{binop, gt, literal, value_ref, value_ref_typed};
use reify_test_support::mocks::{
    MockConstraintChecker, MockConstraintSolver, SequencedMockConstraintSolver,
};
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};

/// Canary backward-compatibility test: verifies that cold-start eval()
/// produces the correct values for the bracket fixture.
/// This test must pass BEFORE and AFTER the Engine refactoring.
#[test]
fn cold_start_eval_produces_correct_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let result = engine.eval(&module);

    let e = "Bracket";

    // 5 params
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.08)),
        "width should be 80mm = 0.08m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "height")),
        Some(&Value::length(0.10)),
        "height should be 100mm = 0.10m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "thickness")),
        Some(&Value::length(0.005)),
        "thickness should be 5mm = 0.005m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "fillet_radius")),
        Some(&Value::length(0.003)),
        "fillet_radius should be 3mm = 0.003m"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "hole_diameter")),
        Some(&Value::length(0.006)),
        "hole_diameter should be 6mm = 0.006m"
    );

    // 1 let binding: volume = width * height * thickness
    // = 0.08 * 0.10 * 0.005 = 0.00004 = 4e-5
    let volume = result.values.get(&ValueCellId::new(e, "volume"));
    assert!(volume.is_some(), "volume should exist");
    let vol_f64 = volume.unwrap().as_f64().expect("volume should be numeric");
    assert!(
        (vol_f64 - 4e-5).abs() < 1e-10,
        "volume should be ~4e-5 m³, got {}",
        vol_f64
    );

    // Total: 6 values
    assert_eq!(result.values.len(), 6, "should have exactly 6 values");
    assert!(result.diagnostics.is_empty(), "no diagnostics expected");
}

/// After cold-start eval, edit width from 80mm to 100mm.
/// Verify updated values: width=100mm, volume recomputed, others unchanged.
#[test]
fn edit_param_returns_updated_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    let initial = engine.eval(&module);
    let e = "Bracket";

    // Edit width from 80mm (0.08m) to 100mm (0.1m)
    let width_id = ValueCellId::new(e, "width");
    let result = engine.edit_param(width_id, Value::length(0.1)).unwrap();

    // Width should be updated
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.1)),
        "width should be 100mm = 0.1m after edit"
    );

    // Volume should be recomputed: 0.1 * 0.1 * 0.005 = 5e-5
    let volume = result.values.get(&ValueCellId::new(e, "volume"));
    assert!(volume.is_some(), "volume should exist");
    let vol_f64 = volume.unwrap().as_f64().expect("volume should be numeric");
    assert!(
        (vol_f64 - 5e-5).abs() < 1e-10,
        "volume should be ~5e-5 m³ after width edit, got {}",
        vol_f64
    );

    // Other params unchanged
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "height")),
        initial.values.get(&ValueCellId::new(e, "height")),
        "height should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "thickness")),
        initial.values.get(&ValueCellId::new(e, "thickness")),
        "thickness should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "fillet_radius")),
        initial.values.get(&ValueCellId::new(e, "fillet_radius")),
        "fillet_radius should be unchanged"
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "hole_diameter")),
        initial.values.get(&ValueCellId::new(e, "hole_diameter")),
        "hole_diameter should be unchanged"
    );
}

/// Verify snapshot provenance and IDs after eval() and edit_param().
#[test]
fn edit_param_snapshot_provenance() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // After eval(): provenance should be Initial, ID = 0
    engine.eval(&module);
    let snap = engine.snapshot().expect("snapshot should exist after eval");
    assert_eq!(snap.provenance, SnapshotProvenance::Initial);
    assert_eq!(snap.id, SnapshotId(0));

    // After edit_param(): provenance should be Edit, ID = 1
    let width_id = ValueCellId::new(e, "width");
    engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after edit_param");
    assert_eq!(snap.id, SnapshotId(1));

    let mut expected_changed = std::collections::HashSet::new();
    expected_changed.insert(width_id);
    assert_eq!(
        snap.provenance,
        SnapshotProvenance::Edit {
            changed: expected_changed,
            parent: SnapshotId(0),
        }
    );
}

/// Verify that edit_param() only re-evaluates the dirty∩demanded intersection.
/// When width changes with all constraints+values demanded:
/// - volume and C1 are in the eval set (they read width)
/// - fillet_radius, hole_diameter, C0, C2 are NOT in the eval set
#[test]
fn edit_param_partial_reeval_only_dirty_demanded() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // Cold-start eval (demands all value cells, constraints, realizations)
    engine.eval(&module);

    // Edit width from 80mm to 100mm
    let width_id = ValueCellId::new(e, "width");
    engine.edit_param(width_id, Value::length(0.1)).unwrap();

    let eval_set = engine.last_eval_set();

    // volume IS in eval set (reads width)
    let volume_id = ValueCellId::new(e, "volume");
    assert!(
        eval_set.contains(&NodeId::Value(volume_id)),
        "volume should be in eval set (reads width)"
    );

    // C1 IS in eval set (reads width and thickness)
    assert!(
        eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "C1 should be in eval set (reads width)"
    );

    // fillet_radius NOT in eval set (nothing reads fillet_radius, but also it doesn't read width)
    assert!(
        !eval_set.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))),
        "fillet_radius should NOT be in eval set"
    );

    // hole_diameter NOT in eval set (doesn't read width)
    assert!(
        !eval_set.contains(&NodeId::Value(ValueCellId::new(e, "hole_diameter"))),
        "hole_diameter should NOT be in eval set"
    );

    // C0 NOT in eval set (only reads thickness)
    assert!(
        !eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))),
        "C0 should NOT be in eval set (only reads thickness)"
    );

    // C2 NOT in eval set (reads hole_diameter and thickness, not width)
    assert!(
        !eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))),
        "C2 should NOT be in eval set (reads hole_diameter and thickness)"
    );
}

/// Verify content-hash early cutoff: when a re-evaluated node produces
/// the same value, its downstream dependents are removed from eval set.
///
/// Graph: param a (Real, default 5.0), let x = a - a (always 0.0), let y = x + 1.0
/// Edit a from 5.0 to 7.0:
/// - x is dirty (reads a), re-evaluated: still 0.0 → early cutoff
/// - y depends on x, but x didn't change → y NOT in eval set
/// - y's value should still be 1.0
#[test]
fn content_hash_early_cutoff_prevents_downstream_eval() {
    let e = "T";

    // let x = a - a (always 0.0 regardless of a)
    let x_expr = binop(
        BinOp::Sub,
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
    );
    // let y = x + 1.0
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "x", Type::dimensionless_scalar()),
        literal(Value::Real(1.0)),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(
                    e,
                    "a",
                    Type::dimensionless_scalar(),
                    Some(literal(Value::Real(5.0))),
                )
                .let_binding(e, "x", Type::dimensionless_scalar(), x_expr)
                .let_binding(e, "y", Type::dimensionless_scalar(), y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    let initial = engine.eval(&module);
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "x")),
        Some(&Value::Real(0.0)),
        "x = a - a should be 0.0"
    );
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(1.0)),
        "y = x + 1.0 should be 1.0"
    );

    // Edit a from 5.0 to 7.0
    let a_id = ValueCellId::new(e, "a");
    let result = engine.edit_param(a_id, Value::Real(7.0)).unwrap();

    let eval_set = engine.last_eval_set();

    // x IS in eval set (reads a, so x is dirty)
    assert!(
        eval_set.contains(&NodeId::Value(ValueCellId::new(e, "x"))),
        "x should be in eval set (reads a)"
    );

    // y should NOT be in eval set (x re-evaluated but same hash → early cutoff)
    assert!(
        !eval_set.contains(&NodeId::Value(ValueCellId::new(e, "y"))),
        "y should NOT be in eval set (early cutoff: x didn't change)"
    );

    // y's value should still be 1.0
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(1.0)),
        "y should still be 1.0 from cache"
    );
}

/// After cold-start eval(), all value cell nodes should have Freshness::Final in cache.
#[test]
fn freshness_final_after_cold_start() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let e = "Bracket";
    let cache = engine.cache_store();

    // All 6 value cells should have Final freshness
    for name in [
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
        "volume",
    ] {
        let node_id = NodeId::Value(ValueCellId::new(e, name));
        let entry = cache
            .get(&node_id)
            .unwrap_or_else(|| panic!("{} should be in cache", name));
        assert_eq!(
            entry.freshness,
            Freshness::Final,
            "{} should have Final freshness after cold start",
            name
        );
    }
}

/// After edit_param(), all re-evaluated nodes end up Final and all non-dirty
/// nodes remain Final. Verifies the freshness postcondition of edit_param().
#[test]
fn freshness_all_final_after_edit_param() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // Cold-start
    engine.eval(&module);

    // Edit width from 80mm to 100mm
    let width_id = ValueCellId::new(e, "width");
    engine.edit_param(width_id, Value::length(0.1)).unwrap();

    let cache = engine.cache_store();

    // volume was re-evaluated → should be Final
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    let volume_entry = cache.get(&volume_node).expect("volume should be in cache");
    assert_eq!(
        volume_entry.freshness,
        Freshness::Final,
        "volume should be Final after re-evaluation"
    );

    // fillet_radius was not in eval set → should still be Final
    let fillet_node = NodeId::Value(ValueCellId::new(e, "fillet_radius"));
    let fillet_entry = cache
        .get(&fillet_node)
        .expect("fillet_radius should be in cache");
    assert_eq!(
        fillet_entry.freshness,
        Freshness::Final,
        "fillet_radius should remain Final (not in eval set)"
    );

    // height was not in eval set → should still be Final
    let height_node = NodeId::Value(ValueCellId::new(e, "height"));
    let height_entry = cache.get(&height_node).expect("height should be in cache");
    assert_eq!(
        height_entry.freshness,
        Freshness::Final,
        "height should remain Final (not in eval set)"
    );
}

/// Verify that early-cutoff-skipped nodes have Freshness::Final after edit_param(),
/// NOT stuck in Pending. Uses the a-x-y fixture where x = a - a (always 0.0)
/// and y = x + 1.0. When a changes, x is dirty but produces the same hash →
/// early cutoff skips y. However, y was pre-marked Pending before the eval loop.
/// The postcondition requires y to be Final after edit_param returns.
///
/// Addresses invariant_violation: early-cutoff-skipped nodes stuck in Pending.
#[test]
fn early_cutoff_skipped_nodes_have_final_freshness() {
    let e = "T";

    // let x = a - a (always 0.0 regardless of a)
    let x_expr = binop(
        BinOp::Sub,
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
    );
    // let y = x + 1.0
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "x", Type::dimensionless_scalar()),
        literal(Value::Real(1.0)),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(
                    e,
                    "a",
                    Type::dimensionless_scalar(),
                    Some(literal(Value::Real(5.0))),
                )
                .let_binding(e, "x", Type::dimensionless_scalar(), x_expr)
                .let_binding(e, "y", Type::dimensionless_scalar(), y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    engine.eval(&module);

    // Edit a from 5.0 to 7.0: x re-evals to 0.0 (unchanged), early cutoff skips y
    let a_id = ValueCellId::new(e, "a");
    engine.edit_param(a_id, Value::Real(7.0)).unwrap();

    let cache = engine.cache_store();

    // x was re-evaluated → should be Final
    let x_node = NodeId::Value(ValueCellId::new(e, "x"));
    let x_entry = cache.get(&x_node).expect("x should be in cache");
    assert_eq!(
        x_entry.freshness,
        Freshness::Final,
        "x should be Final after re-evaluation"
    );

    // y was skipped by early cutoff → MUST be Final (not stuck in Pending)
    let y_node = NodeId::Value(ValueCellId::new(e, "y"));
    let y_entry = cache.get(&y_node).expect("y should be in cache");
    assert_eq!(
        y_entry.freshness,
        Freshness::Final,
        "y should be Final after edit_param (not stuck in Pending from pre-marking)"
    );
}

/// Verify that mark_pending() is actually called during edit_param() for
/// each node in the eval set. This proves the Pending intermediate state
/// exists during evaluation, even though it's not externally observable
/// (edit_param is synchronous). Without this test, removing mark_pending()
/// would not cause any test to fail.
///
/// Addresses test_does_not_test_what_it_claims: freshness_transitions_during_edit.
#[test]
fn mark_pending_is_called_for_eval_set_nodes() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // Cold-start eval
    engine.eval(&module);

    // Edit width from 80mm to 100mm
    let width_id = ValueCellId::new(e, "width");
    engine.edit_param(width_id, Value::length(0.1)).unwrap();

    // pending_transition_count should equal the number of cached nodes
    // in the eval set. Only Value nodes are cached during eval(); Constraint
    // and Realization nodes are tracked in the eval set but not cached,
    // so mark_pending() returns false for them.
    let eval_set = engine.last_eval_set();
    assert!(
        !eval_set.is_empty(),
        "eval set should be non-empty after editing width"
    );
    let cached_node_count = eval_set
        .iter()
        .filter(|n| matches!(n, NodeId::Value(_)))
        .count();
    assert!(
        cached_node_count > 0,
        "at least one Value node should be in eval set"
    );
    assert_eq!(
        engine.cache_store().pending_transition_count(),
        cached_node_count,
        "mark_pending should have been called once for each cached node in the eval set"
    );
}

/// Verify that consecutive edit_param() calls produce correct results and
/// that each call computes a fresh dirty cone from only its own changed set,
/// with no residual dirty state from prior edits.
///
/// Scenario:
/// 1. Cold-start eval of bracket module (volume = 0.08 * 0.10 * 0.005 = 4e-5)
/// 2. Edit width 0.08→0.1: dirty cone = {volume, C1}
/// 3. Edit height 0.10→0.12: dirty cone = {volume} only (NOT C1)
///
/// The key assertion is that the second edit's eval set contains ONLY volume
/// (not C1), proving that each edit_param computes its dirty cone independently.
#[test]
fn consecutive_edit_param_only_reevaluates_affected_nodes() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // ── (1) Cold-start eval ──────────────────────────────────────────
    let initial = engine.eval(&module);
    assert_eq!(initial.values.len(), 6, "bracket should have 6 values");

    let vol = initial
        .values
        .get(&ValueCellId::new(e, "volume"))
        .expect("volume should exist")
        .as_f64()
        .expect("volume should be numeric");
    assert!(
        (vol - 4e-5).abs() < 1e-10,
        "initial volume should be ~4e-5, got {}",
        vol
    );

    // ── (2) First edit: width 0.08 → 0.1 ────────────────────────────
    let width_id = ValueCellId::new(e, "width");
    let result1 = engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Volume recomputed: 0.1 * 0.10 * 0.005 = 5e-5
    let vol1 = result1
        .values
        .get(&ValueCellId::new(e, "volume"))
        .expect("volume should exist after first edit")
        .as_f64()
        .expect("volume should be numeric");
    assert!(
        (vol1 - 5e-5).abs() < 1e-10,
        "volume after width edit should be ~5e-5, got {}",
        vol1
    );

    // Eval set should contain volume and C1 (both read width)
    let eval_set1 = engine.last_eval_set();
    assert!(
        eval_set1.contains(&NodeId::Value(ValueCellId::new(e, "volume"))),
        "volume should be in first edit's eval set"
    );
    assert!(
        eval_set1.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "C1 should be in first edit's eval set (reads width)"
    );

    // Other params should NOT be in eval set
    assert!(
        !eval_set1.contains(&NodeId::Value(ValueCellId::new(e, "height"))),
        "height should NOT be in first edit's eval set"
    );
    assert!(
        !eval_set1.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))),
        "fillet_radius should NOT be in first edit's eval set"
    );
    assert!(
        !eval_set1.contains(&NodeId::Value(ValueCellId::new(e, "hole_diameter"))),
        "hole_diameter should NOT be in first edit's eval set"
    );
    assert!(
        !eval_set1.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))),
        "C0 should NOT be in first edit's eval set"
    );
    assert!(
        !eval_set1.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))),
        "C2 should NOT be in first edit's eval set"
    );

    // ── (3) Second edit: height 0.10 → 0.12 ─────────────────────────
    let height_id = ValueCellId::new(e, "height");
    let result2 = engine
        .edit_param(height_id.clone(), Value::length(0.12))
        .unwrap();

    // Volume recomputed: 0.1 * 0.12 * 0.005 = 6e-5
    let vol2 = result2
        .values
        .get(&ValueCellId::new(e, "volume"))
        .expect("volume should exist after second edit")
        .as_f64()
        .expect("volume should be numeric");
    assert!(
        (vol2 - 6e-5).abs() < 1e-10,
        "volume after height edit should be ~6e-5, got {}",
        vol2
    );

    // Width should be preserved from first edit (0.1, not original 0.08)
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.1)),
        "width should still be 0.1 (preserved from first edit)"
    );

    // Other params unchanged from initial
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "thickness")),
        initial.values.get(&ValueCellId::new(e, "thickness")),
        "thickness should be unchanged"
    );
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "fillet_radius")),
        initial.values.get(&ValueCellId::new(e, "fillet_radius")),
        "fillet_radius should be unchanged"
    );
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "hole_diameter")),
        initial.values.get(&ValueCellId::new(e, "hole_diameter")),
        "hole_diameter should be unchanged"
    );

    // KEY ASSERTION: eval set for second edit contains ONLY volume, NOT C1
    let eval_set2 = engine.last_eval_set();
    assert!(
        eval_set2.contains(&NodeId::Value(ValueCellId::new(e, "volume"))),
        "volume should be in second edit's eval set (reads height)"
    );
    assert!(
        !eval_set2.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "C1 should NOT be in second edit's eval set (does not read height) — \
         proves no residual dirty state from first edit"
    );

    // ── (4) Snapshot provenance chain ────────────────────────────────
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after second edit");
    assert_eq!(
        snap.id,
        SnapshotId(2),
        "second edit should produce snapshot ID 2"
    );

    let mut expected_changed = std::collections::HashSet::new();
    expected_changed.insert(height_id);
    assert_eq!(
        snap.provenance,
        SnapshotProvenance::Edit {
            changed: expected_changed,
            parent: SnapshotId(1),
        },
        "second edit's parent should be SnapshotId(1), not SnapshotId(0)"
    );

    // ── (5) All 6 value cells should have Freshness::Final ──────────
    let cache = engine.cache_store();
    for name in [
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
        "volume",
    ] {
        let node_id = NodeId::Value(ValueCellId::new(e, name));
        let entry = cache
            .get(&node_id)
            .unwrap_or_else(|| panic!("{} should be in cache", name));
        assert_eq!(
            entry.freshness,
            Freshness::Final,
            "{} should have Final freshness after both edits",
            name
        );
    }

    // ── (6) pending_transition_count ────────────────────────────────
    //
    // `pending_transition_count` counts `mark_pending()` calls over the
    // full pre-cutoff `eval_set`, NOT the post-cutoff `actual_eval_set`
    // returned by `last_eval_set()`. In the presence of early cutoff the
    // two sets diverge: nodes are marked pending (incrementing the
    // counter) then skipped and restored via `restore_final()`, which
    // does NOT decrement the counter.
    //
    // For this test's second edit (height → 0.12) there is no early
    // cutoff — the dirty cone intersected with demand yields exactly
    // {volume}, a terminal Value node with no downstream dependents in
    // the eval set. Therefore eval_set == actual_eval_set == {volume},
    // making the distinction moot here. We assert the known constant
    // directly rather than deriving it from `last_eval_set()` to avoid
    // creating a false equivalence that would silently break if the
    // fixture were extended with early-cutoff scenarios.
    assert_eq!(
        cache.pending_transition_count(),
        1,
        "second edit's eval_set has exactly one Value node (volume), no early cutoff"
    );
}

/// Mixed fan-in: when an unchanged intermediary's dependents ALSO read the
/// changed param directly, early cutoff must NOT skip them.
///
/// Graph:
///   param a (Int, default 5)
///   let x = if a > 0 then 1 else 1   (reads a, always produces 1 → Unchanged)
///   let y = a + x                      (reads BOTH a and x → mixed fan-in)
///
/// Cold start: a=5, x=1, y=5+1=6
/// Edit a → 10: x re-evals to 1 (Unchanged), but y MUST re-eval to 10+1=11.
/// Bug: y was being added to `skipped` because x is Unchanged, even though
/// y also reads a directly and a changed.
#[test]
fn mixed_fan_in_edit_param_unchanged_upstream_does_not_skip_shared_downstream() {
    let e = "T";

    // Build conditional: if a > 0 then 1 else 1 (always 1, reads a)
    let condition = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
    let then_branch = literal(Value::Int(1));
    let else_branch = literal(Value::Int(1));
    let conditional = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    // let y = a + x (reads both a and x — diamond/mixed fan-in)
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "a", Type::Int),
        value_ref_typed(e, "x", Type::Int),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                .let_binding(e, "x", Type::Int, conditional)
                .let_binding(e, "y", Type::Int, y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start: a=5, x=1, y=6
    let initial = engine.eval(&module);
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "x")),
        Some(&Value::Int(1)),
        "x should be 1"
    );
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(6)),
        "y should be 5 + 1 = 6"
    );

    // Edit a from 5 to 10
    let a_id = ValueCellId::new(e, "a");
    let result = engine.edit_param(a_id, Value::Int(10)).unwrap();

    let eval_set = engine.last_eval_set();

    // x IS in eval set (reads a)
    assert!(
        eval_set.contains(&NodeId::Value(ValueCellId::new(e, "x"))),
        "x should be in eval set (reads a)"
    );

    // y MUST be in eval set — it reads a directly, and a changed.
    // The bug was that y was incorrectly added to `skipped` because
    // x (its other parent) was Unchanged.
    assert!(
        eval_set.contains(&NodeId::Value(ValueCellId::new(e, "y"))),
        "y should be in eval set (reads changed param a directly, \
         even though x is Unchanged)"
    );

    // y must have the correct re-evaluated value: 10 + 1 = 11
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(11)),
        "y should be 10 + 1 = 11, NOT stale 6"
    );
}

/// Triple fan-in: two unchanged intermediaries plus a changed param all
/// feed into the same downstream node. Early cutoff must not skip it.
///
/// Graph:
///   param a (Int, default 5)
///   let x = if a > 0 then 1 else 1   (reads a, always 1 → Unchanged)
///   let z = if a > 0 then 2 else 2   (reads a, always 2 → Unchanged)
///   let y = a + x + z                 (reads a, x, AND z → triple fan-in)
///
/// Cold start: a=5, x=1, z=2, y=5+1+2=8
/// Edit a → 10: x=1 (Unchanged), z=2 (Unchanged), y MUST re-eval to 10+1+2=13.
#[test]
fn triple_fan_in_mixed_changed_unchanged_edit_param() {
    let e = "T";

    // Build conditional: if a > 0 then 1 else 1 (always 1)
    let x_cond = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
    let x_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(x_cond),
            then_branch: Box::new(literal(Value::Int(1))),
            else_branch: Box::new(literal(Value::Int(1))),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    // Build conditional: if a > 0 then 2 else 2 (always 2)
    let z_cond = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
    let z_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(z_cond),
            then_branch: Box::new(literal(Value::Int(2))),
            else_branch: Box::new(literal(Value::Int(2))),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_2_else_2"),
    };

    // let y = a + x + z  →  (a + x) + z
    let a_plus_x = binop(
        BinOp::Add,
        value_ref_typed(e, "a", Type::Int),
        value_ref_typed(e, "x", Type::Int),
    );
    let y_expr = binop(BinOp::Add, a_plus_x, value_ref_typed(e, "z", Type::Int));

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                .let_binding(e, "x", Type::Int, x_expr)
                .let_binding(e, "z", Type::Int, z_expr)
                .let_binding(e, "y", Type::Int, y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start: a=5, x=1, z=2, y=8
    let initial = engine.eval(&module);
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(8)),
        "y should be 5 + 1 + 2 = 8"
    );

    // Edit a from 5 to 10
    let a_id = ValueCellId::new(e, "a");
    let result = engine.edit_param(a_id, Value::Int(10)).unwrap();

    let eval_set = engine.last_eval_set();

    // y MUST be in eval set despite both x and z being Unchanged
    assert!(
        eval_set.contains(&NodeId::Value(ValueCellId::new(e, "y"))),
        "y should be in eval set (reads changed param a directly)"
    );

    // y must have the correct value: 10 + 1 + 2 = 13
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(13)),
        "y should be 10 + 1 + 2 = 13, NOT stale 8"
    );
}

/// After mixed fan-in edit_param, ALL nodes must have Freshness::Final.
/// No node should be left in Pending state.
///
/// Same diamond graph as mixed_fan_in_edit_param test:
///   param a (Int, 5), let x = if a>0 then 1 else 1, let y = a + x
/// Edit a → 10: x Unchanged, y re-evaluated.
#[test]
fn freshness_all_final_after_mixed_fan_in_edit_param() {
    let e = "T";

    let condition = gt(value_ref_typed(e, "a", Type::Int), literal(Value::Int(0)));
    let conditional = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(literal(Value::Int(1))),
            else_branch: Box::new(literal(Value::Int(1))),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "a", Type::Int),
        value_ref_typed(e, "x", Type::Int),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Int, Some(literal(Value::Int(5))))
                .let_binding(e, "x", Type::Int, conditional)
                .let_binding(e, "y", Type::Int, y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start
    engine.eval(&module);

    // Edit a from 5 to 10
    let a_id = ValueCellId::new(e, "a");
    engine.edit_param(a_id, Value::Int(10)).unwrap();

    let cache = engine.cache_store();

    // ALL nodes must have Final freshness — none stuck in Pending
    for name in ["a", "x", "y"] {
        let node_id = NodeId::Value(ValueCellId::new(e, name));
        let entry = cache
            .get(&node_id)
            .unwrap_or_else(|| panic!("{} should be in cache", name));
        assert_eq!(
            entry.freshness,
            Freshness::Final,
            "{} should have Final freshness after mixed fan-in edit_param",
            name
        );
    }
}

/// Regression guard: linear chain early cutoff must still work after the
/// mixed fan-in fix. When there is NO mixed fan-in (no node reads the
/// changed param directly), Unchanged intermediaries should still skip
/// their downstream dependents.
///
/// Graph: param a (Real, 5.0), let x = a - a (always 0.0), let y = x + 1.0
/// Edit a → 7.0: x dirty, re-evals to 0.0 (Unchanged) → y correctly skipped.
/// y does NOT read a directly, only x, so no mixed fan-in.
#[test]
fn linear_chain_early_cutoff_still_skips_after_fix() {
    let e = "T";

    // let x = a - a (always 0.0)
    let x_expr = binop(
        BinOp::Sub,
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
        value_ref_typed(e, "a", Type::dimensionless_scalar()),
    );
    // let y = x + 1.0
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "x", Type::dimensionless_scalar()),
        literal(Value::Real(1.0)),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(
                    e,
                    "a",
                    Type::dimensionless_scalar(),
                    Some(literal(Value::Real(5.0))),
                )
                .let_binding(e, "x", Type::dimensionless_scalar(), x_expr)
                .let_binding(e, "y", Type::dimensionless_scalar(), y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start
    let initial = engine.eval(&module);
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "x")),
        Some(&Value::Real(0.0)),
    );
    assert_eq!(
        initial.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(1.0)),
    );

    // Edit a from 5.0 to 7.0
    let a_id = ValueCellId::new(e, "a");
    let result = engine.edit_param(a_id, Value::Real(7.0)).unwrap();

    let eval_set = engine.last_eval_set();

    // x IS in eval set (reads a)
    assert!(
        eval_set.contains(&NodeId::Value(ValueCellId::new(e, "x"))),
        "x should be in eval set"
    );

    // y should NOT be in eval set — x Unchanged and y does NOT read a directly.
    // This confirms the fix didn't break valid early cutoff.
    assert!(
        !eval_set.contains(&NodeId::Value(ValueCellId::new(e, "y"))),
        "y should NOT be in eval set (x unchanged, y doesn't read a directly)"
    );

    // y's value should still be 1.0
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(1.0)),
        "y should retain cached value 1.0"
    );
}

/// After edit_param changes a param that affects constraints governing auto
/// params, the solver should re-run and resolved_params should be non-empty.
///
/// Module: param `a` (default mm(3.0)), auto `x`, constraint `x > a`.
/// MockConstraintSolver returns x=mm(5.0).
/// Cold eval → solver resolves x to mm(5.0), resolved_params contains x.
/// Edit `a` to mm(8.0) → constraint `x > a` is in dirty cone (depends on `a`).
/// Assert:
///   (1) result.resolved_params is non-empty
///   (2) result.resolved_params contains x with mm(5.0)
///   (3) values[x] == mm(5.0)
///
/// Currently FAILS because edit_param returns resolved_params: HashMap::new().
#[test]
fn edit_param_re_resolves_auto_params_when_constraints_dirty() {
    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        // constraint: x > a  (references both x and a)
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver resolves x to mm(5.0)
    let result = engine.eval(&module);
    assert!(
        !result.resolved_params.is_empty(),
        "cold eval should resolve auto params"
    );
    assert!(result.resolved_params.contains_key(&x_id));

    // Edit a from mm(3.0) to mm(8.0)
    // Constraint `x > a` depends on `a`, so it's in the dirty cone.
    // Solver should re-run.
    let result2 = engine.edit_param(a_id.clone(), mm(8.0)).unwrap();

    // (1) resolved_params is non-empty (solver was re-run)
    assert!(
        !result2.resolved_params.is_empty(),
        "edit_param should re-resolve auto params when constraints are dirty, \
         got empty resolved_params"
    );

    // (2) resolved_params contains x
    assert!(
        result2.resolved_params.contains_key(&x_id),
        "resolved_params should contain x, got {:?}",
        result2.resolved_params
    );

    // (3) values[x] == mm(5.0) = 0.005 SI
    let x_val = result2.values.get(&x_id).expect("x should be in values");
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected x = mm(5.0) = 0.005 SI, got {:?}",
        x_val
    );
}

/// After edit_param triggers re-resolution, let bindings that depend on auto
/// params must also be re-evaluated with the updated resolved values.
///
/// Module: param `a` (default mm(3.0)), auto `x`, let `y = x * 2.0`,
/// constraint `x > a`. Sequenced solver: 1st call returns x=mm(5.0),
/// 2nd call returns x=mm(20.0).
///
/// Cold eval → x=mm(5.0)=0.005 SI, y = 0.005*2 = 0.01 SI.
/// Edit `a` to mm(8.0) → solver re-resolves x to mm(20.0)=0.02 SI.
/// y depends on x and must be re-evaluated: y = 0.02*2 = 0.04 SI.
///
/// Currently FAILS because edit_param's resolution phase does not re-evaluate
/// let bindings that depend on re-resolved auto params (no second propagation wave).
#[test]
fn edit_param_let_binding_re_evaluates_after_re_resolution() {
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // First call: x = mm(5.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    // Second call: x = mm(20.0) (different value!)
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

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

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        // y = x * 2.0
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        // constraint: x > a
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver returns x=mm(5.0), y = 0.005 * 2 = 0.01
    let result = engine.eval(&module);
    let y_val = result
        .values
        .get(&y_id)
        .expect("y should be in values after cold eval");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected y ≈ 0.01 after cold eval (x=mm(5.0)*2), got {:?}",
        y_val
    );

    // Edit a from mm(3.0) to mm(8.0) — constraint `x > a` in dirty cone.
    // Solver re-resolves x to mm(20.0) (different value!).
    // Let binding y depends on x and MUST be re-evaluated: y = 0.02*2 = 0.04.
    let result2 = engine.edit_param(a_id.clone(), mm(8.0)).unwrap();

    // x must have the new resolved value
    let x_val2 = result2
        .values
        .get(&x_id)
        .expect("x should be in values after edit");
    assert!(
        matches!(x_val2, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected x = mm(20.0) = 0.02 SI, got {:?}",
        x_val2
    );

    // y must be re-evaluated with new x: y = 0.02 * 2 = 0.04
    let y_val2 = result2
        .values
        .get(&y_id)
        .expect("y should be in values after edit");
    assert!(
        matches!(y_val2, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected y ≈ 0.04 after re-resolution (x=mm(20.0)*2), got {:?} (stale!)",
        y_val2
    );
}

/// After edit_check with a value that violates a constraint, the CheckResult
/// should report the constraint as Violated.
///
/// Uses SimpleConstraintChecker (real checker). Module: param `width` (default
/// mm(10.0)), constraint `width > mm(5.0)`. Cold check → Satisfied.
/// edit_check(width, mm(2.0)) → constraint should be Violated because 2 < 5.
///
/// Currently FAILS because edit_check() doesn't exist.
#[test]
fn edit_check_returns_incremental_constraint_satisfaction() {
    use reify_ir::Satisfaction;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        // constraint: width > mm(5.0)
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(5.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: width=mm(10.0) > mm(5.0) → Satisfied
    let result = engine.check(&module);
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied
    );

    // edit_check: width=mm(2.0) < mm(5.0) → Violated
    let result2 = engine.edit_check(width_id.clone(), mm(2.0)).unwrap();
    assert_eq!(result2.constraint_results.len(), 1);
    assert_eq!(
        result2.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "constraint should be Violated when width=mm(2.0) < mm(5.0)"
    );

    // Values should be updated
    let width_val = result2
        .values
        .get(&width_id)
        .expect("width should be in values");
    assert!(
        matches!(width_val, Value::Scalar { si_value, .. } if (si_value - 0.002).abs() < 1e-10),
        "expected width = mm(2.0) = 0.002 SI, got {:?}",
        width_val
    );
}

/// Verify constraint satisfaction transitions correctly across multiple
/// edit_check calls: Satisfied → Violated → Satisfied.
///
/// Uses SimpleConstraintChecker. Module: param `width` (default mm(10.0)),
/// constraint `width > mm(5.0)`.
/// Cold check → Satisfied. edit_check(width, mm(2.0)) → Violated.
/// edit_check(width, mm(8.0)) → Satisfied again.
#[test]
fn edit_check_constraint_transitions_satisfied_to_violated_and_back() {
    use reify_ir::Satisfaction;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(5.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: width=mm(10.0) > mm(5.0) → Satisfied
    let result = engine.check(&module);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied
    );

    // edit_check: width=mm(2.0) < mm(5.0) → Violated
    let result2 = engine.edit_check(width_id.clone(), mm(2.0)).unwrap();
    assert_eq!(
        result2.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "constraint should be Violated when width=mm(2.0) < mm(5.0)"
    );

    // edit_check: width=mm(8.0) > mm(5.0) → Satisfied again
    let result3 = engine.edit_check(width_id.clone(), mm(8.0)).unwrap();
    assert_eq!(
        result3.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "constraint should be Satisfied when width=mm(8.0) > mm(5.0)"
    );
}

/// When the solver returns Infeasible during re-resolution in edit_param,
/// the diagnostics must be propagated in the EvalResult.
///
/// Module: param `a` (default mm(1.0)), auto `x`, constraint `x > a`.
/// Sequenced solver: 1st call returns Solved (cold eval works), 2nd call
/// returns Infeasible with diagnostic 'constraints are infeasible'.
/// Cold eval → solver resolves x. Edit `a` to mm(5.0) → constraint in dirty
/// cone → solver re-runs → Infeasible → diagnostics in result.
#[test]
fn edit_param_solver_diagnostics_propagated() {
    use reify_core::Diagnostic;
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(5.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved_values,
            unique: true,
        },
        SolveResult::Infeasible {
            diagnostics: vec![Diagnostic::error("constraints are infeasible")],
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(1.0))))
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver returns Solved
    let result = engine.eval(&module);
    assert!(
        result.diagnostics.is_empty(),
        "cold eval should have no diagnostics"
    );

    // Edit a → constraint in dirty cone → solver returns Infeasible
    let result2 = engine.edit_param(a_id.clone(), mm(5.0)).unwrap();

    // Diagnostics should be propagated
    assert!(
        !result2.diagnostics.is_empty(),
        "edit_param should propagate solver diagnostics when Infeasible"
    );
    assert!(
        result2
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible")),
        "expected 'infeasible' in diagnostics, got: {:?}",
        result2.diagnostics
    );
}

/// After edit_param triggers re-resolution, verify the snapshot is correctly
/// updated with resolved auto param values and re-evaluated let binding values.
///
/// Module: param `a`, auto `x`, let `y = x * 2.0`, constraint `x > a`.
/// Sequenced solver: 1st call returns x=mm(5.0), 2nd call returns x=mm(20.0).
/// Cold eval. Edit `a`. Assert:
///   (1) snapshot exists
///   (2) snapshot.values contains x with resolved value (0.02 SI, not Undef)
///   (3) snapshot.values contains y with re-evaluated value (0.04 SI)
///   (4) snapshot provenance is Edit (not Initial)
#[test]
fn edit_param_snapshot_updated_after_re_resolution() {
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

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

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval
    engine.eval(&module);

    // Edit a → re-resolution with x=mm(20.0), y=0.04
    engine.edit_param(a_id.clone(), mm(8.0)).unwrap();

    // (1) snapshot exists
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after edit_param");

    // (4) provenance is Edit
    assert!(
        matches!(snap.provenance, SnapshotProvenance::Edit { .. }),
        "snapshot provenance should be Edit, got {:?}",
        snap.provenance
    );

    // (2) snapshot.values contains x with resolved value (not Undef)
    let (x_val, x_det) = snap
        .values
        .get(&x_id)
        .expect("x should be in snapshot values");
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected x = mm(20.0) = 0.02 SI in snapshot, got {:?}",
        x_val
    );
    assert_eq!(*x_det, reify_ir::DeterminacyState::Determined);

    // (3) snapshot.values contains y with re-evaluated value
    let (y_val, y_det) = snap
        .values
        .get(&y_id)
        .expect("y should be in snapshot values");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected y = mm(20.0)*2 = 0.04 SI in snapshot, got {:?}",
        y_val
    );
    assert_eq!(*y_det, reify_ir::DeterminacyState::Determined);
}

/// Regression guard: when editing a param that does NOT affect auto param
/// constraints, the solver should NOT be re-run and resolved_params should
/// be empty.
///
/// Module: param `a` (default mm(1.0)), param `b` (default mm(2.0)),
/// auto `x`, let `y = b * 2.0`, constraint `x > a`.
/// Solver returns x=mm(5.0).
/// Cold eval → x resolved. Edit `b` to mm(3.0) — constraint `x > a` is NOT
/// in dirty cone (doesn't depend on `b`). Assert:
///   (1) result.resolved_params is empty (solver not re-run)
///   (2) y = mm(3.0) * 2.0 = 0.006 SI (re-evaluated)
///   (3) x remains mm(5.0) in values
#[test]
fn edit_param_no_re_resolution_when_auto_constraints_not_dirty() {
    let _a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(5.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(1.0))))
        .param("S", "b", Type::length(), Some(literal(mm(2.0))))
        .auto_param("S", "x", Type::length())
        // y = b * 2.0
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "b"), literal(Value::Real(2.0))),
        )
        // constraint: x > a  (does NOT reference b)
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0), y = mm(2.0) * 2 = 0.004
    let result = engine.eval(&module);
    assert!(
        !result.resolved_params.is_empty(),
        "cold eval should resolve x"
    );

    // Edit b from mm(2.0) to mm(3.0) — constraint `x > a` NOT in dirty cone
    let result2 = engine.edit_param(b_id.clone(), mm(3.0)).unwrap();

    // (1) resolved_params should be empty (solver NOT re-run)
    assert!(
        result2.resolved_params.is_empty(),
        "edit_param should NOT re-resolve when auto constraints are not dirty, \
         got resolved_params: {:?}",
        result2.resolved_params
    );

    // (2) y should be re-evaluated: mm(3.0) * 2.0 = 0.006 SI
    let y_val = result2.values.get(&y_id).expect("y should be in values");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.006).abs() < 1e-10),
        "expected y ≈ 0.006 (mm(3.0)*2), got {:?}",
        y_val
    );

    // (3) x should remain mm(5.0) = 0.005 SI
    let x_val = result2.values.get(&x_id).expect("x should be in values");
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected x = mm(5.0) = 0.005 SI (unchanged), got {:?}",
        x_val
    );
}

/// Verify that constraint labels survive through the edit_check path.
///
/// Module: param `width` (default mm(10.0)), two constraints:
///   C0: label="min_width",  width > mm(5.0)
///   C1: label=None,         width < mm(100.0)
///
/// Cold check() → labels come from CompiledConstraint, so both are correct.
/// edit_check(width, mm(2.0)) → constraint checking routes through
/// check_constraints_with_values, which currently always sets label: None.
///
/// This test WILL FAIL because check_constraints_with_values always sets
/// `label: None`, so the labeled constraint loses its label.
#[test]
fn edit_check_preserves_constraint_labels() {
    use reify_ir::Satisfaction;
    use reify_test_support::builders::lt;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        // C0: labeled "min_width", width > mm(5.0)
        .constraint(
            "S",
            0,
            Some("min_width"),
            gt(value_ref("S", "width"), literal(mm(5.0))),
        )
        // C1: no label, width < mm(100.0)
        .constraint(
            "S",
            1,
            None,
            lt(value_ref("S", "width"), literal(mm(100.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: both constraints satisfied, labels correct
    let result = engine.check(&module);
    assert_eq!(result.constraint_results.len(), 2);

    // Find the constraint entries by ID (iteration order may vary)
    let c0 = result
        .constraint_results
        .iter()
        .find(|c| c.id == ConstraintNodeId::new("S", 0))
        .expect("C0 should be in results");
    let c1 = result
        .constraint_results
        .iter()
        .find(|c| c.id == ConstraintNodeId::new("S", 1))
        .expect("C1 should be in results");

    assert_eq!(
        c0.label,
        Some("min_width".to_string()),
        "cold check: C0 label"
    );
    assert_eq!(c1.label, None, "cold check: C1 label");
    assert_eq!(c0.satisfaction, Satisfaction::Satisfied);
    assert_eq!(c1.satisfaction, Satisfaction::Satisfied);

    // edit_check: width=mm(2.0) — C0 Violated, C1 Satisfied
    let result2 = engine.edit_check(width_id.clone(), mm(2.0)).unwrap();
    assert_eq!(result2.constraint_results.len(), 2);

    let c0_edit = result2
        .constraint_results
        .iter()
        .find(|c| c.id == ConstraintNodeId::new("S", 0))
        .expect("C0 should be in edit_check results");
    let c1_edit = result2
        .constraint_results
        .iter()
        .find(|c| c.id == ConstraintNodeId::new("S", 1))
        .expect("C1 should be in edit_check results");

    // Labels must be preserved through edit_check path
    assert_eq!(
        c0_edit.label,
        Some("min_width".to_string()),
        "edit_check: C0 label should be preserved as 'min_width'"
    );
    assert_eq!(
        c1_edit.label, None,
        "edit_check: C1 label should remain None"
    );

    // Satisfaction assertions
    assert_eq!(
        c0_edit.satisfaction,
        Satisfaction::Violated,
        "C0 should be Violated when width=mm(2.0) < mm(5.0)"
    );
    assert_eq!(
        c1_edit.satisfaction,
        Satisfaction::Satisfied,
        "C1 should be Satisfied when width=mm(2.0) < mm(100.0)"
    );

    // Task 848.1: diagnostics for the labeled constraint must use the friendly
    // label ("min_width") — not the raw ConstraintNodeId ("S#constraint[0]").
    // Only C0 is violated, so exactly its label should appear in the post-edit
    // diagnostics; C1 is satisfied and contributes nothing.
    use reify_core::Severity;
    let error_msgs: Vec<&str> = result2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        !error_msgs.is_empty(),
        "edit_check: expected at least one Error diagnostic for violated C0, got: {:?}",
        result2.diagnostics
    );
    let has_friendly = error_msgs.iter().any(|m| m.contains("min_width"));
    assert!(
        has_friendly,
        "edit_check: expected an error diagnostic containing 'min_width' (the C0 label), got: {:?}",
        error_msgs
    );
    let leaks_raw_id = error_msgs.iter().any(|m| m.contains("S#constraint[0]"));
    assert!(
        !leaks_raw_id,
        "edit_check: labeled diagnostics must not leak the raw ConstraintNodeId \
         'S#constraint[0]' for a constraint that carries a label, got: {:?}",
        error_msgs
    );
}

// ────────────────────────────────────────────────────────────────────
//  Forward let-binding reference tests
// ────────────────────────────────────────────────────────────────────

/// Cold-start eval must handle forward let-binding references correctly.
///
/// Template: param a (default 5, Int)
///   let y = x + 1   (forward ref to x — declared *before* x)
///   let x = a + 10  (declared *after* y)
///
/// Expected: x = 15, y = 16.
/// Without topological sorting, y evaluates before x and gets Undef.
#[test]
fn forward_let_ref_cold_start_simple() {
    let _a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // y = x + 1  (forward reference to x)
    let y_expr = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    // x = a + 10
    let x_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(10)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(5))))
        .let_binding("S", "y", Type::Int, y_expr) // y declared first (forward ref to x)
        .let_binding("S", "x", Type::Int, x_expr) // x declared second
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    let x_val = result.values.get(&x_id).expect("x should be in values");
    let y_val = result.values.get(&y_id).expect("y should be in values");

    assert_eq!(*x_val, Value::Int(15), "x = a + 10 = 5 + 10 = 15");
    assert_eq!(*y_val, Value::Int(16), "y = x + 1 = 15 + 1 = 16");
}

/// Cold-start eval handles a fully reversed 3-deep dependency chain.
///
/// Template: param a (default 0, Int)
///   let z = y + 1  (declared 1st — depends on y)
///   let y = x + 1  (declared 2nd — depends on x)
///   let x = a + 1  (declared 3rd — depends on a)
///
/// Expected: x = 1, y = 2, z = 3.
#[test]
fn forward_let_ref_cold_start_deep_reverse_chain() {
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    let z_id = ValueCellId::new("S", "z");

    // z = y + 1
    let z_expr = binop(BinOp::Add, value_ref("S", "y"), literal(Value::Int(1)));
    // y = x + 1
    let y_expr = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    // x = a + 1
    let x_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(1)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(0))))
        .let_binding("S", "z", Type::Int, z_expr) // z declared first
        .let_binding("S", "y", Type::Int, y_expr) // y declared second
        .let_binding("S", "x", Type::Int, x_expr) // x declared third
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    let x_val = result.values.get(&x_id).expect("x should be in values");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    let z_val = result.values.get(&z_id).expect("z should be in values");

    assert_eq!(*x_val, Value::Int(1), "x = a + 1 = 0 + 1 = 1");
    assert_eq!(*y_val, Value::Int(2), "y = x + 1 = 1 + 1 = 2");
    assert_eq!(*z_val, Value::Int(3), "z = y + 1 = 2 + 1 = 3");
}

/// Cold-start eval handles diamond-shaped forward references.
///
/// Template: param a (default 10, Int)
///   let d = b + c  (declared 1st — forward refs to both b and c)
///   let b = a + 1  (declared 2nd)
///   let c = a + 2  (declared 3rd)
///
/// Expected: b = 11, c = 12, d = 23.
#[test]
fn forward_let_ref_cold_start_diamond() {
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");
    let d_id = ValueCellId::new("S", "d");

    // d = b + c (forward refs to both b and c)
    let d_expr = binop(BinOp::Add, value_ref("S", "b"), value_ref("S", "c"));
    // b = a + 1
    let b_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(1)));
    // c = a + 2
    let c_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(2)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(10))))
        .let_binding("S", "d", Type::Int, d_expr) // d declared first
        .let_binding("S", "b", Type::Int, b_expr) // b declared second
        .let_binding("S", "c", Type::Int, c_expr) // c declared third
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    let b_val = result.values.get(&b_id).expect("b should be in values");
    let c_val = result.values.get(&c_id).expect("c should be in values");
    let d_val = result.values.get(&d_id).expect("d should be in values");

    assert_eq!(*b_val, Value::Int(11), "b = a + 1 = 10 + 1 = 11");
    assert_eq!(*c_val, Value::Int(12), "c = a + 2 = 10 + 2 = 12");
    assert_eq!(*d_val, Value::Int(23), "d = b + c = 11 + 12 = 23");
}

/// Post-resolution re-evaluation must handle forward let-binding references.
///
/// Template: param p (default mm(1.0), length), auto x (length)
///   let c = b + x  (declared 1st — forward ref to b)
///   let b = x + p  (declared 2nd)
///   constraint: x > p
///
/// Solver resolves x = mm(10.0) = 0.01 SI.
/// After resolution: b = x + p = 0.01 + 0.001 = 0.011 SI
///                   c = b + x = 0.011 + 0.01 = 0.021 SI
///
/// Without topological sorting in the post-resolution re-eval, c evaluates
/// before b is re-evaluated with the resolved x, producing a stale result.
#[test]
fn forward_let_ref_post_resolution() {
    let _p_id = ValueCellId::new("S", "p");
    let x_id = ValueCellId::new("S", "x");
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");

    // c = b + x (forward ref to b)
    let c_expr = binop(BinOp::Add, value_ref("S", "b"), value_ref("S", "x"));
    // b = x + p
    let b_expr = binop(BinOp::Add, value_ref("S", "x"), value_ref("S", "p"));
    // constraint: x > p
    let constraint_expr = gt(value_ref("S", "x"), value_ref("S", "p"));

    let mut solved_values = HashMap::new();
    solved_values.insert(x_id.clone(), mm(10.0));

    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "p", Type::length(), Some(literal(mm(1.0))))
        .auto_param("S", "x", Type::length())
        .let_binding("S", "c", Type::length(), c_expr) // c declared first (forward ref to b)
        .let_binding("S", "b", Type::length(), b_expr) // b declared second
        .constraint("S", 0, None, constraint_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    let result = engine.eval(&module);

    // x resolved to mm(10.0) = 0.01 SI
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-10),
        "expected x = mm(10.0) = 0.01 SI, got {:?}",
        x_val
    );

    // b = x + p = 0.01 + 0.001 = 0.011 SI
    let b_val = result.values.get(&b_id).expect("b should be in values");
    assert!(
        matches!(b_val, Value::Scalar { si_value, .. } if (*si_value - 0.011).abs() < 1e-10),
        "expected b = x + p = 0.011 SI, got {:?}",
        b_val
    );

    // c = b + x = 0.011 + 0.01 = 0.021 SI
    let c_val = result.values.get(&c_id).expect("c should be in values");
    assert!(
        matches!(c_val, Value::Scalar { si_value, .. } if (*si_value - 0.021).abs() < 1e-10),
        "expected c = b + x = 0.021 SI, got {:?}",
        c_val
    );
}

/// Incremental edit_param handles forward let-binding references correctly.
///
/// The incremental path (compute_eval_set → topological_sort) already sorts
/// by dependency, so this test confirms that forward-declared lets work
/// correctly both in cold-start and after an incremental parameter edit.
///
/// Template: param a (default 5, Int)
///   let y = x + 1  (forward ref to x)
///   let x = a + 10
///
/// Cold-start: x = 15, y = 16.
/// Edit a = 20: x = 30, y = 31.
#[test]
fn forward_let_ref_incremental_edit_param() {
    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // y = x + 1 (forward reference to x)
    let y_expr = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    // x = a + 10
    let x_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(10)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(5))))
        .let_binding("S", "y", Type::Int, y_expr) // y declared first (forward ref)
        .let_binding("S", "x", Type::Int, x_expr) // x declared second
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    // Cold-start: x = 15, y = 16
    assert_eq!(*result.values.get(&x_id).unwrap(), Value::Int(15));
    assert_eq!(*result.values.get(&y_id).unwrap(), Value::Int(16));

    // Incremental edit: a = 20 → x = 30, y = 31
    let edit_result = engine.edit_param(a_id.clone(), Value::Int(20)).unwrap();
    assert_eq!(
        *edit_result.values.get(&x_id).unwrap(),
        Value::Int(30),
        "x = a + 10 = 20 + 10 = 30"
    );
    assert_eq!(
        *edit_result.values.get(&y_id).unwrap(),
        Value::Int(31),
        "y = x + 1 = 30 + 1 = 31"
    );
}

/// Cold-start and incremental evaluation produce identical results
/// for templates with forward let-binding references.
///
/// Template: param a (default 3, Int)
///   let c = b * 2  (forward ref to b)
///   let b = a + 7
///
/// Cold-start with a=3: b=10, c=20.
/// Edit a→13: b=20, c=40.
/// Fresh cold-start with a=13: b=20, c=40. Must match incremental result.
#[test]
fn forward_let_ref_cold_start_matches_incremental() {
    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");

    // c = b * 2 (forward reference to b)
    let c_expr = binop(BinOp::Mul, value_ref("S", "b"), literal(Value::Int(2)));
    // b = a + 7
    let b_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(7)));

    // ── Incremental path ──
    let template1 = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(3))))
        .let_binding("S", "c", Type::Int, c_expr.clone())
        .let_binding("S", "b", Type::Int, b_expr.clone())
        .build();

    let module1 = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template1)
        .build();

    let mut engine1 = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result1 = engine1.eval(&module1);

    // Cold-start: b=10, c=20
    assert_eq!(*result1.values.get(&b_id).unwrap(), Value::Int(10));
    assert_eq!(*result1.values.get(&c_id).unwrap(), Value::Int(20));

    // Edit a → 13: b=20, c=40
    let edit_result = engine1.edit_param(a_id.clone(), Value::Int(13)).unwrap();
    let incr_b = edit_result.values.get(&b_id).unwrap().clone();
    let incr_c = edit_result.values.get(&c_id).unwrap().clone();

    // ── Fresh cold-start with a=13 ──
    let c_expr2 = binop(BinOp::Mul, value_ref("S", "b"), literal(Value::Int(2)));
    let b_expr2 = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(7)));

    let template2 = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(13))))
        .let_binding("S", "c", Type::Int, c_expr2)
        .let_binding("S", "b", Type::Int, b_expr2)
        .build();

    let module2 = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template2)
        .build();

    let mut engine2 = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result2 = engine2.eval(&module2);

    let fresh_b = result2.values.get(&b_id).unwrap().clone();
    let fresh_c = result2.values.get(&c_id).unwrap().clone();

    assert_eq!(
        incr_b, fresh_b,
        "incremental b should match fresh cold-start b"
    );
    assert_eq!(
        incr_c, fresh_c,
        "incremental c should match fresh cold-start c"
    );
    assert_eq!(fresh_b, Value::Int(20), "b = a + 7 = 13 + 7 = 20");
    assert_eq!(fresh_c, Value::Int(40), "c = b * 2 = 20 * 2 = 40");
}

/// Declaration order must be irrelevant for forward let-binding references.
///
/// Two modules with identical DAGs but different declaration orders must
/// produce identical results in both cold-start and incremental evaluation.
///
/// Module A: param a (default 5, Int)
///   let y = x + 1   (y declared first — forward ref to x)
///   let x = a + 10  (x declared second)
///
/// Module B: param a (default 5, Int)
///   let x = a + 10  (x declared first — no forward ref)
///   let y = x + 1   (y declared second — backward ref to x)
///
/// Both must produce x=15, y=16 on cold-start, and x=30, y=31 after edit a→20.
#[test]
fn forward_let_ref_declaration_order_irrelevant() {
    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // -- Module A: y declared before x (forward ref) --
    let y_expr_a = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    let x_expr_a = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(10)));

    let template_a = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(5))))
        .let_binding("S", "y", Type::Int, y_expr_a) // y first (forward ref to x)
        .let_binding("S", "x", Type::Int, x_expr_a) // x second
        .build();

    let module_a = CompiledModuleBuilder::new(ModulePath::single("test_a"))
        .template(template_a)
        .build();

    // -- Module B: x declared before y (natural order, no forward ref) --
    let y_expr_b = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    let x_expr_b = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(10)));

    let template_b = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(5))))
        .let_binding("S", "x", Type::Int, x_expr_b) // x first (no forward ref)
        .let_binding("S", "y", Type::Int, y_expr_b) // y second
        .build();

    let module_b = CompiledModuleBuilder::new(ModulePath::single("test_b"))
        .template(template_b)
        .build();

    // -- Cold-start both --
    let mut engine_a = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result_a = engine_a.eval(&module_a);

    let mut engine_b = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result_b = engine_b.eval(&module_b);

    // Both must produce identical values
    assert_eq!(
        *result_a.values.get(&x_id).unwrap(),
        *result_b.values.get(&x_id).unwrap(),
        "cold-start x must be identical regardless of declaration order"
    );
    assert_eq!(
        *result_a.values.get(&y_id).unwrap(),
        *result_b.values.get(&y_id).unwrap(),
        "cold-start y must be identical regardless of declaration order"
    );
    assert_eq!(*result_a.values.get(&x_id).unwrap(), Value::Int(15));
    assert_eq!(*result_a.values.get(&y_id).unwrap(), Value::Int(16));

    // -- Incremental edit: a → 20, both modules --
    let edit_a = engine_a.edit_param(a_id.clone(), Value::Int(20)).unwrap();
    let edit_b = engine_b.edit_param(a_id.clone(), Value::Int(20)).unwrap();

    assert_eq!(
        *edit_a.values.get(&x_id).unwrap(),
        *edit_b.values.get(&x_id).unwrap(),
        "incremental x must be identical regardless of declaration order"
    );
    assert_eq!(
        *edit_a.values.get(&y_id).unwrap(),
        *edit_b.values.get(&y_id).unwrap(),
        "incremental y must be identical regardless of declaration order"
    );
    assert_eq!(*edit_a.values.get(&x_id).unwrap(), Value::Int(30));
    assert_eq!(*edit_a.values.get(&y_id).unwrap(), Value::Int(31));
}

/// Diamond-shaped forward references update correctly through incremental edit.
///
/// Template: param a (default 10, Int)
///   let d = b + c  (declared 1st — forward refs to both b and c)
///   let b = a + 1  (declared 2nd)
///   let c = a + 2  (declared 3rd)
///
/// Cold-start: b=11, c=12, d=23.
/// Edit a→20: b=21, c=22, d=43.
#[test]
fn forward_let_ref_diamond_incremental_edit() {
    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");
    let d_id = ValueCellId::new("S", "d");

    // d = b + c (forward refs to both b and c)
    let d_expr = binop(BinOp::Add, value_ref("S", "b"), value_ref("S", "c"));
    // b = a + 1
    let b_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(1)));
    // c = a + 2
    let c_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(2)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(10))))
        .let_binding("S", "d", Type::Int, d_expr) // d declared first
        .let_binding("S", "b", Type::Int, b_expr) // b declared second
        .let_binding("S", "c", Type::Int, c_expr) // c declared third
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    // Cold-start: b=11, c=12, d=23
    assert_eq!(*result.values.get(&b_id).unwrap(), Value::Int(11));
    assert_eq!(*result.values.get(&c_id).unwrap(), Value::Int(12));
    assert_eq!(*result.values.get(&d_id).unwrap(), Value::Int(23));

    // Incremental edit: a → 20
    let edit_result = engine.edit_param(a_id.clone(), Value::Int(20)).unwrap();

    assert_eq!(
        *edit_result.values.get(&b_id).unwrap(),
        Value::Int(21),
        "b = a + 1 = 20 + 1 = 21"
    );
    assert_eq!(
        *edit_result.values.get(&c_id).unwrap(),
        Value::Int(22),
        "c = a + 2 = 20 + 2 = 22"
    );
    assert_eq!(
        *edit_result.values.get(&d_id).unwrap(),
        Value::Int(43),
        "d = b + c = 21 + 22 = 43"
    );
}

/// Deep reversed chain updates correctly through incremental edit.
///
/// Template: param a (default 0, Int)
///   let z = y + 1  (declared 1st — depends on y)
///   let y = x + 1  (declared 2nd — depends on x)
///   let x = a + 1  (declared 3rd — depends on a)
///
/// Cold-start: x=1, y=2, z=3.
/// Edit a→10: x=11, y=12, z=13.
#[test]
fn forward_let_ref_deep_chain_incremental_edit() {
    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    let z_id = ValueCellId::new("S", "z");

    // z = y + 1
    let z_expr = binop(BinOp::Add, value_ref("S", "y"), literal(Value::Int(1)));
    // y = x + 1
    let y_expr = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Int(1)));
    // x = a + 1
    let x_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(1)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(0))))
        .let_binding("S", "z", Type::Int, z_expr) // z declared first
        .let_binding("S", "y", Type::Int, y_expr) // y declared second
        .let_binding("S", "x", Type::Int, x_expr) // x declared third
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    // Cold-start: x=1, y=2, z=3
    assert_eq!(*result.values.get(&x_id).unwrap(), Value::Int(1));
    assert_eq!(*result.values.get(&y_id).unwrap(), Value::Int(2));
    assert_eq!(*result.values.get(&z_id).unwrap(), Value::Int(3));

    // Incremental edit: a → 10
    let edit_result = engine.edit_param(a_id.clone(), Value::Int(10)).unwrap();

    assert_eq!(
        *edit_result.values.get(&x_id).unwrap(),
        Value::Int(11),
        "x = a + 1 = 10 + 1 = 11"
    );
    assert_eq!(
        *edit_result.values.get(&y_id).unwrap(),
        Value::Int(12),
        "y = x + 1 = 11 + 1 = 12"
    );
    assert_eq!(
        *edit_result.values.get(&z_id).unwrap(),
        Value::Int(13),
        "z = y + 1 = 12 + 1 = 13"
    );
}

/// Mixed forward and backward references work correctly.
///
/// Template: param a (default 1, Int)
///   let b = a + 1   (declared 1st — backward ref to a only)
///   let d = c + b   (declared 2nd — forward ref to c, backward ref to b)
///   let c = a + 2   (declared 3rd — backward ref to a only)
///
/// Cold-start: b=2, c=3, d=5.
/// Edit a→10: b=11, c=12, d=23.
#[test]
fn forward_let_ref_mixed_forward_backward() {
    let a_id = ValueCellId::new("S", "a");
    let b_id = ValueCellId::new("S", "b");
    let c_id = ValueCellId::new("S", "c");
    let d_id = ValueCellId::new("S", "d");

    // b = a + 1 (backward ref to a)
    let b_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(1)));
    // d = c + b (forward ref to c, backward ref to b)
    let d_expr = binop(BinOp::Add, value_ref("S", "c"), value_ref("S", "b"));
    // c = a + 2 (backward ref to a)
    let c_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(2)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(1))))
        .let_binding("S", "b", Type::Int, b_expr) // b first (backward ref only)
        .let_binding("S", "d", Type::Int, d_expr) // d second (forward ref to c)
        .let_binding("S", "c", Type::Int, c_expr) // c third
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    // Cold-start: b=2, c=3, d=5
    assert_eq!(
        *result.values.get(&b_id).unwrap(),
        Value::Int(2),
        "b = a + 1 = 1 + 1 = 2"
    );
    assert_eq!(
        *result.values.get(&c_id).unwrap(),
        Value::Int(3),
        "c = a + 2 = 1 + 2 = 3"
    );
    assert_eq!(
        *result.values.get(&d_id).unwrap(),
        Value::Int(5),
        "d = c + b = 3 + 2 = 5"
    );

    // Incremental edit: a → 10
    let edit_result = engine.edit_param(a_id.clone(), Value::Int(10)).unwrap();

    assert_eq!(
        *edit_result.values.get(&b_id).unwrap(),
        Value::Int(11),
        "b = a + 1 = 10 + 1 = 11"
    );
    assert_eq!(
        *edit_result.values.get(&c_id).unwrap(),
        Value::Int(12),
        "c = a + 2 = 10 + 2 = 12"
    );
    assert_eq!(
        *edit_result.values.get(&d_id).unwrap(),
        Value::Int(23),
        "d = c + b = 12 + 11 = 23"
    );
}

/// Early cutoff works correctly with forward-declared bindings.
///
/// Template: param a (Real, default 5.0)
///   let y = x + 1.0  (declared 1st — forward ref to x)
///   let x = a - a    (declared 2nd — always 0.0 regardless of a)
///
/// Cold-start: x=0.0, y=1.0.
/// Edit a→7.0: x re-evals to 0.0 (unchanged → early cutoff).
///   y should NOT be re-evaluated because x didn't change.
///   y must still be 1.0.
#[test]
fn forward_let_ref_early_cutoff_with_forward_decl() {
    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // y = x + 1.0 (forward ref to x)
    let y_expr = binop(BinOp::Add, value_ref("S", "x"), literal(Value::Real(1.0)));
    // x = a - a (always 0.0)
    let x_expr = binop(BinOp::Sub, value_ref("S", "a"), value_ref("S", "a"));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "a",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(5.0))),
        )
        .let_binding("S", "y", Type::dimensionless_scalar(), y_expr) // y first (forward ref to x)
        .let_binding("S", "x", Type::dimensionless_scalar(), x_expr) // x second (always 0.0)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);

    // Cold-start: x=0.0, y=1.0
    assert_eq!(
        *result.values.get(&x_id).unwrap(),
        Value::Real(0.0),
        "x = a - a = 0.0"
    );
    assert_eq!(
        *result.values.get(&y_id).unwrap(),
        Value::Real(1.0),
        "y = x + 1.0 = 1.0"
    );

    // Incremental edit: a → 7.0
    let edit_result = engine.edit_param(a_id.clone(), Value::Real(7.0)).unwrap();

    // x still 0.0 (a-a = 0.0 regardless of a)
    assert_eq!(
        *edit_result.values.get(&x_id).unwrap(),
        Value::Real(0.0),
        "x = a - a = 0.0 (unchanged)"
    );
    // y still 1.0 (should not have been re-evaluated due to early cutoff on x)
    assert_eq!(
        *edit_result.values.get(&y_id).unwrap(),
        Value::Real(1.0),
        "y = x + 1.0 = 1.0 (unchanged, early cutoff on x)"
    );

    // Verify early cutoff: y should NOT be in the actual eval set
    let eval_set = engine.last_eval_set();
    assert!(
        !eval_set.contains(&NodeId::Value(y_id.clone())),
        "y should NOT be in eval set — early cutoff on x means y is not re-evaluated"
    );
    // x SHOULD be in the eval set (it was re-evaluated, found unchanged)
    assert!(
        eval_set.contains(&NodeId::Value(x_id.clone())),
        "x should be in eval set — it was re-evaluated (though value unchanged)"
    );
}

/// After edit_param, the param's own cache entry must hold the new value/determinacy,
/// not the old default that was stored during initial eval().
///
/// Bug: edit_param updates the snapshot but never calls record_evaluation for the
/// param itself, so the cache retains CachedResult::Value(old_default, Determined).
#[test]
fn edit_param_updates_param_cache_entry() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";
    engine.eval(&module);

    // Edit width from 80mm (0.08m) to 100mm (0.1m)
    let width_id = ValueCellId::new(e, "width");
    engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // The cache entry for the param itself must reflect the edited value
    let cache = engine.cache_store();
    let width_node = NodeId::Value(width_id);
    let entry = cache
        .get(&width_node)
        .expect("width should be in cache after edit_param");

    assert!(
        matches!(
            &entry.result,
            CachedResult::Value(v, DeterminacyState::Determined)
            if (v.as_f64().unwrap() - 0.1).abs() < 1e-12
        ),
        "cache entry for edited param should hold new value 0.1m, got {:?}",
        entry.result
    );
}

/// Consecutive edit_param calls on the SAME param must each update the cache.
/// This proves no stale carryover between consecutive edits.
#[test]
fn double_edit_param_updates_param_cache_both_times() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";
    engine.eval(&module);

    let width_id = ValueCellId::new(e, "width");

    // First edit: width → 100mm (0.1m)
    let result1 = engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();
    // volume = 0.1 * 0.1 * 0.005 = 5e-5
    let vol1 = result1
        .values
        .get(&ValueCellId::new(e, "volume"))
        .unwrap()
        .as_f64()
        .unwrap();
    assert!(
        (vol1 - 5e-5).abs() < 1e-10,
        "volume after first edit should be ~5e-5, got {}",
        vol1
    );

    // Second edit: width → 120mm (0.12m)
    let result2 = engine
        .edit_param(width_id.clone(), Value::length(0.12))
        .unwrap();
    // volume = 0.12 * 0.1 * 0.005 = 6e-5
    let vol2 = result2
        .values
        .get(&ValueCellId::new(e, "volume"))
        .unwrap()
        .as_f64()
        .unwrap();
    assert!(
        (vol2 - 6e-5).abs() < 1e-10,
        "volume after second edit should be ~6e-5, got {}",
        vol2
    );

    // The cache entry for the param must reflect the SECOND edit's value (0.12)
    let cache = engine.cache_store();
    let width_node = NodeId::Value(width_id);
    let entry = cache
        .get(&width_node)
        .expect("width should be in cache after double edit");

    assert!(
        matches!(
            &entry.result,
            CachedResult::Value(v, DeterminacyState::Determined)
            if (v.as_f64().unwrap() - 0.12).abs() < 1e-12
        ),
        "cache entry for param should hold second edit value 0.12m, got {:?}",
        entry.result
    );
}

/// A param with no default_expr starts as Undef (not cached during initial eval).
/// After edit_param sets a concrete value, the cache must contain the new value.
#[test]
fn edit_param_on_undef_param_updates_cache() {
    let e = "T";

    // Build a module with a single param that has no default_expr (None)
    // and a let binding that reads it: let y = a + 1
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "a", Type::Int),
        literal(Value::Int(1)),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Int, None)
                .let_binding(e, "y", Type::Int, y_expr)
                .build(),
        )
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start: a has no default, so a=Undef (not cached for params without default_expr)
    engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");

    // Edit a → 42
    engine.edit_param(a_id.clone(), Value::Int(42)).unwrap();

    // The cache entry for a must now hold Value::Int(42)
    let cache = engine.cache_store();
    let a_node = NodeId::Value(a_id);
    let entry = cache
        .get(&a_node)
        .expect("param a should be in cache after edit_param (even if it had no default)");

    assert!(
        matches!(
            &entry.result,
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined)
        ),
        "cache entry for undef param should hold edited value Int(42), got {:?}",
        entry.result
    );
}

/// After edit_param, the param's cache entry must have a basis_version matching
/// the edit's version, not the initial eval's version. This proves version
/// metadata is also fresh.
#[test]
fn edit_param_cache_basis_version_updated() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";
    engine.eval(&module);

    // After initial eval, width's cache should have basis_version = VersionId(0)
    let width_id = ValueCellId::new(e, "width");
    let width_node = NodeId::Value(width_id.clone());
    let initial_entry = engine
        .cache_store()
        .get(&width_node)
        .expect("width should be in cache after initial eval");
    assert_eq!(
        initial_entry.basis_version,
        VersionId(0),
        "initial eval should produce basis_version 0"
    );

    // Edit width → 100mm
    engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // After edit, the param's basis_version must be VersionId(1), not VersionId(0)
    let edited_entry = engine
        .cache_store()
        .get(&width_node)
        .expect("width should be in cache after edit_param");
    assert_eq!(
        edited_entry.basis_version,
        VersionId(1),
        "edit_param should update param's basis_version to the edit's version (1), not retain initial (0)"
    );
}
