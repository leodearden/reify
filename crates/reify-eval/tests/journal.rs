//! Integration tests for the EventJournal instrumentation in Engine.

use reify_eval::cache::NodeId;
use reify_eval::cache::EvalOutcome;
use reify_eval::journal::{EventJournal, EventKind};
use reify_eval::Engine;
use reify_test_support::bracket_compiled_module;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ValueCellId, VersionId};

/// After eval(), the journal should contain events for all evaluated nodes.
#[test]
fn eval_populates_journal() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let journal = engine.journal();
    assert!(
        journal.len() > 0,
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
    for param in &["width", "height", "thickness", "fillet_radius", "hole_diameter", "volume"] {
        let node = NodeId::Value(ValueCellId::new(e, *param));
        let events = journal.events_for_node(&node);
        if events.len() >= 2 {
            let first_started = events.iter().position(|e| matches!(e.kind, EventKind::Started));
            let first_completed = events.iter().position(|e| matches!(e.kind, EventKind::Completed { .. }));
            if let (Some(s), Some(c)) = (first_started, first_completed) {
                assert!(
                    s < c,
                    "Started should precede Completed for {}",
                    param
                );
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
    engine.edit_param(width_id.clone(), reify_types::Value::length(0.1));

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

    // Verify Started/Completed pairs for re-evaluated Value nodes
    let width_node = NodeId::Value(width_id);
    let width_events: Vec<_> = journal
        .events_for_node(&width_node)
        .into_iter()
        .filter(|e| e.version >= VersionId(1))
        .collect();
    assert!(
        !width_events.is_empty(),
        "should have edit_param events for width"
    );

    // Verify Started precedes Completed in edit_param events for width
    let started_pos = width_events.iter().position(|e| matches!(e.kind, EventKind::Started));
    let completed_pos = width_events.iter().position(|e| matches!(e.kind, EventKind::Completed { .. }));
    if let (Some(s), Some(c)) = (started_pos, completed_pos) {
        assert!(s < c, "Started should precede Completed for width in edit_param");
    }

    // The volume node (let binding depending on width) should also have events
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

    // Nodes that weren't in the dirty cone (e.g., height, thickness, fillet_radius, hole_diameter)
    // should NOT have Started events in this version
    for unchanged_param in &["height", "thickness", "fillet_radius", "hole_diameter"] {
        let node = NodeId::Value(ValueCellId::new(e, *unchanged_param));
        let param_edit_events: Vec<_> = journal
            .events_for_node(&node)
            .into_iter()
            .filter(|e| e.version >= VersionId(1))
            .collect();
        let has_started = param_edit_events.iter().any(|e| matches!(e.kind, EventKind::Started));
        assert!(
            !has_started,
            "{} should NOT have Started events in edit_param version",
            unchanged_param
        );
    }
}
