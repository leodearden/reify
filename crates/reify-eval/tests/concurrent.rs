//! Tests for concurrent evaluation support in Engine.
//!
//! Verifies that Engine::prepare_concurrent_edit() correctly extracts state
//! for concurrent evaluation and Engine::apply_concurrent_edit() correctly
//! merges results back.

use reify_eval::cache::NodeId;
use reify_eval::Engine;
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ConstraintNodeId, Value, ValueCellId};

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
    let setup = engine.prepare_concurrent_edit(width_id.clone(), Value::length(0.1));

    // (1) eval_set should match sequential dirty∩demand set for width change
    // width change → dirty = {volume, C1}; all are demanded → eval_set = {volume, C1}
    assert_eq!(
        setup.eval_set.len(), 2,
        "eval_set should have 2 nodes (volume + C1), got: {:?}", setup.eval_set
    );
    assert!(
        setup.eval_set.contains(&NodeId::Value(ValueCellId::new(e, "volume"))),
        "eval_set should contain volume"
    );
    assert!(
        setup.eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "eval_set should contain C1"
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
        setup.graph.value_cells.len(), 6,
        "graph should have 6 value cells"
    );

    // (5) version should be bumped from initial (initial eval uses version 0)
    assert!(
        setup.version.0 > 0,
        "version should be bumped from initial, got: {:?}", setup.version
    );

    // Verify changed_cells contains the edited parameter
    assert!(
        setup.changed_cells.contains(&ValueCellId::new(e, "width")),
        "changed_cells should contain width"
    );
}
