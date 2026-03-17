//! Integration tests for the incremental evaluator pipeline.
//!
//! These tests verify that Engine's incremental evaluation (edit_param)
//! produces correct results, proper provenance, partial re-evaluation,
//! early cutoff, and freshness transitions.

use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::builders::{literal, value_ref_typed, binop};
use reify_types::{
    BinOp, ConstraintNodeId, Freshness, ModulePath, SnapshotId, SnapshotProvenance, Type,
    Value, ValueCellId,
};

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
    let result = engine.edit_param(width_id, Value::length(0.1));

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
    engine.edit_param(width_id.clone(), Value::length(0.1));
    let snap = engine.snapshot().expect("snapshot should exist after edit_param");
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
    engine.edit_param(width_id, Value::length(0.1));

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
        value_ref_typed(e, "a", Type::Real),
        value_ref_typed(e, "a", Type::Real),
    );
    // let y = x + 1.0
    let y_expr = binop(
        BinOp::Add,
        value_ref_typed(e, "x", Type::Real),
        literal(Value::Real(1.0)),
    );

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Real, Some(literal(Value::Real(5.0))))
                .let_binding(e, "x", Type::Real, x_expr)
                .let_binding(e, "y", Type::Real, y_expr)
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
    let result = engine.edit_param(a_id, Value::Real(7.0));

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
    for name in ["width", "height", "thickness", "fillet_radius", "hole_diameter", "volume"] {
        let node_id = NodeId::Value(ValueCellId::new(e, name));
        let entry = cache.get(&node_id)
            .unwrap_or_else(|| panic!("{} should be in cache", name));
        assert_eq!(
            entry.freshness,
            Freshness::Final,
            "{} should have Final freshness after cold start",
            name
        );
    }
}

/// After edit_param(), re-evaluated nodes should be back to Freshness::Final.
/// Nodes not in eval set should remain Freshness::Final throughout.
#[test]
fn freshness_transitions_during_edit() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "Bracket";

    // Cold-start
    engine.eval(&module);

    // Edit width from 80mm to 100mm
    let width_id = ValueCellId::new(e, "width");
    engine.edit_param(width_id, Value::length(0.1));

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
    let fillet_entry = cache.get(&fillet_node).expect("fillet_radius should be in cache");
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
