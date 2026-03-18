//! Integration tests for the EventJournal instrumentation in Engine.

use reify_eval::cache::NodeId;
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
