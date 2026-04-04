//! Tests for concurrent evaluation support in Engine.
//!
//! Verifies that Engine::prepare_concurrent_edit() correctly extracts state
//! for concurrent evaluation and Engine::apply_concurrent_edit() correctly
//! merges results back.

use std::collections::HashSet;

use reify_eval::cache::{EvalOutcome, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{ConcurrentEditResult, ConcurrentNodeResult, Engine};
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{
    ConstraintNodeId, DeterminacyState, Freshness, SnapshotProvenance, Value, ValueCellId,
};

/// Test that prepare_concurrent_edit returns ConcurrentEditSetup with correct state.
#[test]
fn prepare_concurrent_edit_returns_correct_setup() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");

    // Prepare concurrent edit: change width from 80mm to 100mm
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // (1) eval_set should match sequential dirty∩demand set for width change
    // width change → dirty = {volume, C1, R0}; all are demanded → eval_set = {volume, C1, R0}
    assert_eq!(
        setup.eval_set.len(),
        3,
        "eval_set should have 3 nodes (volume + C1 + R0), got: {:?}",
        setup.eval_set
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Value(ValueCellId::new(e, "volume"))),
        "eval_set should contain volume"
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "eval_set should contain C1"
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Realization(reify_types::RealizationNodeId::new(
                e, 0
            ))),
        "eval_set should contain R0"
    );

    // (2) previous_hashes should contain entries for nodes that had cache entries
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    assert!(
        setup.previous_hashes.contains_key(&volume_node),
        "previous_hashes should contain volume"
    );

    // (3) values map should have all current parameter values
    assert_eq!(
        setup.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.1)),
        "values should have updated width"
    );
    assert_eq!(
        setup.values.get(&ValueCellId::new(e, "height")),
        Some(&Value::length(0.10)),
        "values should have height"
    );

    // (4) graph should have correct number of value cells
    assert_eq!(
        setup.graph.value_cells.len(),
        6,
        "graph should have 6 value cells"
    );

    // (5) version should be bumped from initial (initial eval uses version 0)
    assert!(
        setup.version.0 > 0,
        "version should be bumped from initial, got: {:?}",
        setup.version
    );

    // Verify changed_cells contains the edited parameter
    assert!(
        setup.changed_cells.contains(&ValueCellId::new(e, "width")),
        "changed_cells should contain width"
    );
}

/// step-13: Engine::apply_concurrent_edit correctly updates Engine state.
#[test]
fn apply_concurrent_edit_updates_engine_state() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_id = ValueCellId::new(e, "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    // Prepare concurrent edit
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Simulate what ConcurrentEvalAdapter would produce:
    // Volume = width * height * thickness = 0.1 * 0.1 * 0.005 = 5e-5
    let new_volume = Value::Scalar {
        si_value: 5e-5,
        dimension: reify_types::dimension::DimensionVector::VOLUME,
    };

    let mut snapshot_values = setup.snapshot_values.clone();
    snapshot_values.insert(
        volume_id.clone(),
        (new_volume.clone(), DeterminacyState::Determined),
    );

    let node_results = vec![ConcurrentNodeResult {
        node: volume_node.clone(),
        value: new_volume.clone(),
        determinacy: DeterminacyState::Determined,
        trace: DependencyTrace {
            reads: vec![
                ValueCellId::new(e, "width"),
                ValueCellId::new(e, "height"),
                ValueCellId::new(e, "thickness"),
            ],
        },
        outcome: EvalOutcome::Changed,
    }];

    let mut values = setup.values.clone();
    values.insert(volume_id.clone(), new_volume.clone());

    // C1 is in eval_set but was not evaluated (constraint node)
    let c1_node = NodeId::Constraint(ConstraintNodeId::new(e, 1));
    let skipped: HashSet<NodeId> = [c1_node.clone()].into_iter().collect();

    let result = ConcurrentEditResult {
        values,
        snapshot_values,
        node_results,
        actual_eval_set: vec![volume_node.clone()],
        skipped: skipped.clone(),
        resolved_params: std::collections::HashMap::new(),
        diagnostics: Vec::new(),
    };

    // Apply the result
    let diags = engine.apply_concurrent_edit(&setup, result);
    assert!(diags.is_empty(), "expected no diagnostics from apply_concurrent_edit");

    // (1) Cache should have updated entry for volume with correct freshness
    let cache_entry = engine.cache_store().get(&volume_node);
    assert!(cache_entry.is_some(), "volume should be in cache");
    let entry = cache_entry.unwrap();
    assert_eq!(entry.freshness, Freshness::Final);
    assert_eq!(entry.basis_version, setup.version);

    // (2) Snapshot should be updated with Edit provenance
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.id, setup.snapshot_id);
    assert_eq!(snapshot.version, setup.version);
    match &snapshot.provenance {
        SnapshotProvenance::Edit { changed, parent } => {
            assert!(changed.contains(&width_id));
            assert_eq!(*parent, setup.parent_snapshot_id);
        }
        other => panic!("Expected Edit provenance, got: {:?}", other),
    }

    // (3) last_eval_set should match actual_eval_set
    assert!(
        engine.last_eval_set().contains(&volume_node),
        "last_eval_set should contain volume"
    );

    // (4) Journal should have Started+Completed event pairs for volume
    let volume_events = engine.journal().events_for_node(&volume_node);
    // After eval(), volume already has events. After apply, we add 2 more.
    let new_events: Vec<_> = volume_events
        .iter()
        .filter(|e| e.version == setup.version)
        .collect();
    assert_eq!(
        new_events.len(),
        2,
        "should have Started+Completed for volume"
    );
}

/// step-19: Engine::rollback_concurrent_edit() restores all eval_set nodes
/// from Pending back to Final and rolls back version/snapshot IDs.
#[test]
fn rollback_concurrent_edit_restores_pending_to_final() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    let c1_node = NodeId::Constraint(ConstraintNodeId::new(e, 1));

    // Record pre-prepare state for verification after rollback
    let _pre_snapshot_id = engine.snapshot().unwrap().id;
    let _pre_version = engine.snapshot().unwrap().version;
    let pre_volume_hash = engine.cache_store().get(&volume_node).unwrap().result_hash;

    // Prepare concurrent edit — marks eval_set nodes as Pending
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Verify nodes are in Pending state after prepare
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert!(
        matches!(volume_entry.freshness, Freshness::Pending { .. }),
        "volume should be Pending after prepare, got: {:?}",
        volume_entry.freshness
    );

    // Rollback the concurrent edit
    engine.rollback_concurrent_edit(&setup);

    // (1) All nodes in eval_set should have freshness=Final (not Pending)
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert_eq!(
        volume_entry.freshness,
        Freshness::Final,
        "volume should be Final after rollback"
    );
    // C1 may not have a cache entry (constraint nodes might not be cached),
    // but if it does, it should be Final
    if let Some(c1_entry) = engine.cache_store().get(&c1_node) {
        assert_eq!(
            c1_entry.freshness,
            Freshness::Final,
            "C1 should be Final after rollback"
        );
    }

    // (2) Cache entries should still contain original result_hash values
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert_eq!(
        volume_entry.result_hash, pre_volume_hash,
        "volume result_hash should be preserved after rollback"
    );

    // (3) Version and snapshot IDs should be rolled back to pre-prepare values
    // The next_snapshot_id and next_version_id should be decremented so next
    // prepare/edit uses the same IDs (no gaps).
    // We can verify this indirectly: calling edit_param should produce the
    // same version/snapshot IDs that the failed prepare would have used.
    let seq_result = engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // The snapshot after edit_param should have the same IDs as the setup had
    let post_snapshot = engine.snapshot().unwrap();
    assert_eq!(
        post_snapshot.id, setup.snapshot_id,
        "snapshot ID should be reused after rollback"
    );
    assert_eq!(
        post_snapshot.version, setup.version,
        "version ID should be reused after rollback"
    );

    // (4) Subsequent edit_param produces correct values (engine not corrupted)
    let volume_val = seq_result.values.get(&ValueCellId::new(e, "volume"));
    assert!(
        volume_val.is_some(),
        "volume should have a value after sequential edit"
    );
}
