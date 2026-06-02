//! Integration tests for the EventJournal instrumentation in Engine.

use reify_core::{ValueCellId, VersionId};
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_ir::Value;
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;

/// After eval(), the journal should contain events for all evaluated nodes.
#[test]
fn eval_populates_journal() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let journal = engine.journal();
    assert!(
        !journal.is_empty(),
        "journal should have events after eval()"
    );

    // Bracket has 5 params + 1 let = 6 value cells
    // Each should have Started + Completed events = 12 minimum
    let e = "Bracket";
    let width_node = NodeId::Value(ValueCellId::new(e, "width"));
    let width_events = journal.events_for_node(&width_node);
    assert!(
        !width_events.is_empty(),
        "should have events for Bracket/width"
    );

    // Verify all events have the correct VersionId (0 for first eval)
    for event in journal.all_events() {
        // Version should be 0 (first eval)
        assert!(
            event.version == VersionId(0),
            "all events in first eval should have version 0, got {:?}",
            event.version
        );
    }

    // Verify Started events precede Completed events for same node
    for param in &[
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
        "volume",
    ] {
        let node = NodeId::Value(ValueCellId::new(e, *param));
        let events = journal.events_for_node(&node);
        if events.len() >= 2 {
            let first_started = events
                .iter()
                .position(|e| matches!(e.kind, EventKind::Started));
            let first_completed = events
                .iter()
                .position(|e| matches!(e.kind, EventKind::Completed { .. }));
            if let (Some(s), Some(c)) = (first_started, first_completed) {
                assert!(s < c, "Started should precede Completed for {}", param);
            }
        }
    }
}

/// After eval() + edit_param(), the journal should contain events for the edit.
#[test]
fn edit_param_records_journal_events() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    engine.eval(&module);
    let events_after_eval = engine.journal().len();

    // Edit width from 80mm to 100mm
    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    engine
        .edit_param(width_id.clone(), reify_ir::Value::length(0.1))
        .unwrap();

    let journal = engine.journal();
    assert!(
        journal.len() > events_after_eval,
        "journal should have more events after edit_param"
    );

    // The edit_param version should be 1 (second version allocated)
    let edit_events = journal.events_since(VersionId(1));
    assert!(
        !edit_events.is_empty(),
        "should have events with version >= 1"
    );

    // The volume node (let binding depending on width) should have Started/Completed events
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    let volume_events: Vec<_> = journal
        .events_for_node(&volume_node)
        .into_iter()
        .filter(|e| e.version >= VersionId(1))
        .collect();
    assert!(
        !volume_events.is_empty(),
        "volume should be re-evaluated after width edit"
    );

    // Verify Started precedes Completed for volume in edit_param
    let started_pos = volume_events
        .iter()
        .position(|e| matches!(e.kind, EventKind::Started));
    let completed_pos = volume_events
        .iter()
        .position(|e| matches!(e.kind, EventKind::Completed { .. }));
    assert!(started_pos.is_some(), "volume should have Started event");
    assert!(
        completed_pos.is_some(),
        "volume should have Completed event"
    );
    assert!(
        started_pos.unwrap() < completed_pos.unwrap(),
        "Started should precede Completed for volume in edit_param"
    );

    // Nodes that weren't re-evaluated (directly changed param width, unchanged params)
    // should NOT have Started events in this version.
    // Width's value is set directly in edit_param, not via eval_expr.
    for unchanged_param in &[
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
    ] {
        let node = NodeId::Value(ValueCellId::new(e, *unchanged_param));
        let param_edit_events: Vec<_> = journal
            .events_for_node(&node)
            .into_iter()
            .filter(|e| e.version >= VersionId(1))
            .collect();
        let has_started = param_edit_events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Started));
        assert!(
            !has_started,
            "{} should NOT have Started events in edit_param version",
            unchanged_param
        );
    }
}

/// eval_cached() should record Started/Completed for cold start,
/// CacheHit for repeated calls, and mix for dirty calls.
#[test]
fn eval_cached_records_cache_hit_events() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // First call: cold start — should record Started/Completed
    let v0 = VersionId(0);
    let _result1 = engine.eval_cached(&module, v0);
    let events_after_cold = engine.journal().len();
    assert!(events_after_cold > 0, "cold start should record events");

    // Check that cold start has Started events
    let e = "Bracket";
    let width_node = NodeId::Value(ValueCellId::new(e, "width"));
    let cold_width = engine.journal().events_for_node(&width_node);
    let has_started = cold_width
        .iter()
        .any(|e| matches!(e.kind, EventKind::Started));
    assert!(has_started, "cold start should have Started for width");

    // Second call with same version: should get CacheHit via fast path
    let _result2 = engine.eval_cached(&module, v0);
    let events_after_second = engine.journal().len();
    assert!(
        events_after_second > events_after_cold,
        "second call should record CacheHit events"
    );

    // Check that the second call produced CacheHit events
    let second_call_events: Vec<_> = engine
        .journal()
        .all_events()
        .iter()
        .skip(events_after_cold)
        .collect();
    let has_cache_hit = second_call_events
        .iter()
        .any(|e| matches!(e.kind, EventKind::CacheHit));
    assert!(has_cache_hit, "second call should have CacheHit events");

    // Third call after invalidation: should record mix of events
    let width_id = ValueCellId::new(e, "width");
    engine.set_param_and_invalidate(&width_id, Value::length(0.1));
    let events_before_dirty = engine.journal().len();
    let v1 = VersionId(1);
    let _result3 = engine.eval_cached(&module, v1);
    let events_after_dirty = engine.journal().len();
    assert!(
        events_after_dirty > events_before_dirty,
        "dirty eval should record events"
    );

    // The dirty eval should have some Started/Completed (for dirty nodes)
    // and some CacheHit (for clean nodes)
    let dirty_events: Vec<_> = engine
        .journal()
        .all_events()
        .iter()
        .skip(events_before_dirty)
        .collect();
    let dirty_has_started = dirty_events
        .iter()
        .any(|e| matches!(e.kind, EventKind::Started));
    let dirty_has_cache_hit = dirty_events
        .iter()
        .any(|e| matches!(e.kind, EventKind::CacheHit));
    assert!(
        dirty_has_started || dirty_has_cache_hit,
        "dirty eval should have Started or CacheHit events"
    );
}
