use reify_types::DiagnosticInfo;

use crate::diff::{StateDelta, delta_to_events, diff_gui_state, push_serialized_event};
use crate::types::*;

fn sample_diagnostic(severity: &str, message: &str) -> DiagnosticInfo {
    DiagnosticInfo {
        file_path: "test.ri".to_string(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
        severity: severity.to_string(),
        message: message.to_string(),
        code: None,
    }
}

fn sample_value(cell_id: &str, value: &str) -> ValueData {
    ValueData {
        cell_id: cell_id.to_string(),
        name: cell_id
            .split('.')
            .next_back()
            .unwrap_or(cell_id)
            .to_string(),
        value: value.to_string(),
        unit: "mm".to_string(),
        determinacy: "determined".to_string(),
        entity_path: cell_id.split('.').next().unwrap_or("").to_string(),
        kind: "Param".to_string(),
        freshness: "final".to_string(),
    }
}

fn sample_constraint(node_id: &str, status: &str) -> ConstraintData {
    ConstraintData {
        node_id: node_id.to_string(),
        expression: "x > 0".to_string(),
        status: status.to_string(),
        label: None,
        parameter_ids: vec![],
    }
}

fn sample_mesh(entity_path: &str, vertices: Vec<f32>) -> MeshData {
    MeshData {
        entity_path: entity_path.to_string(),
        vertices,
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    }
}

#[test]
fn diff_identical_states_returns_empty_delta() {
    let state = GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0])],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![sample_constraint("Bracket.0", "Satisfied")],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&state, &state);

    assert!(delta.changed_meshes.is_empty(), "no meshes changed");
    assert!(delta.changed_values.is_empty(), "no values changed");
    assert!(
        delta.changed_constraints.is_empty(),
        "no constraints changed"
    );
    assert!(delta.removed_mesh_paths.is_empty(), "no meshes removed");
    assert!(delta.removed_value_ids.is_empty(), "no values removed");
    assert!(
        delta.removed_constraint_ids.is_empty(),
        "no constraints removed"
    );
}

#[test]
fn diff_detects_changed_value() {
    let old = GuiState {
        meshes: vec![],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![sample_value("Bracket.width", "120")],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);

    assert_eq!(delta.changed_values.len(), 1, "one value changed");
    assert_eq!(delta.changed_values[0].cell_id, "Bracket.width");
    assert_eq!(delta.changed_values[0].value, "120");
    assert!(delta.removed_value_ids.is_empty());
}

#[test]
fn diff_detects_changed_constraint() {
    let old = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![sample_constraint("Bracket.0", "Satisfied")],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![sample_constraint("Bracket.0", "Violated")],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);

    assert_eq!(delta.changed_constraints.len(), 1, "one constraint changed");
    assert_eq!(delta.changed_constraints[0].node_id, "Bracket.0");
    assert_eq!(delta.changed_constraints[0].status, "Violated");
    assert!(delta.removed_constraint_ids.is_empty());
}

#[test]
fn diff_detects_changed_mesh_ignores_unchanged() {
    let old = GuiState {
        meshes: vec![
            sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0]),
            sample_mesh("Bracket.hole", vec![1.0, 1.0, 1.0]),
        ],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("Bracket.body", vec![2.0, 2.0, 2.0]), // changed
            sample_mesh("Bracket.hole", vec![1.0, 1.0, 1.0]), // unchanged
        ],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);

    assert_eq!(delta.changed_meshes.len(), 1, "only changed mesh in delta");
    assert_eq!(delta.changed_meshes[0].entity_path, "Bracket.body");
    assert_eq!(delta.changed_meshes[0].vertices, vec![2.0, 2.0, 2.0]);
    assert!(delta.removed_mesh_paths.is_empty());
}

#[test]
fn diff_handles_added_and_removed_entities() {
    let old = GuiState {
        meshes: vec![],
        values: vec![
            sample_value("Bracket.width", "80"),
            sample_value("Bracket.old_param", "10"),
        ],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![
            sample_value("Bracket.width", "80"),
            sample_value("Bracket.new_param", "20"), // added
        ],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);

    // new_param is added (appears in changed_values)
    assert_eq!(delta.changed_values.len(), 1);
    assert_eq!(delta.changed_values[0].cell_id, "Bracket.new_param");

    // old_param is removed
    assert_eq!(delta.removed_value_ids.len(), 1);
    assert_eq!(delta.removed_value_ids[0], "Bracket.old_param");
}

#[test]
fn full_delta_contains_all_items_from_state() {
    let state = GuiState {
        meshes: vec![
            sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0]),
            sample_mesh("Bracket.hole", vec![1.0, 1.0, 1.0]),
        ],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![sample_constraint("Bracket.0", "Satisfied")],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = StateDelta::full(&state);

    assert_eq!(delta.changed_meshes.len(), 2);
    assert_eq!(delta.changed_values.len(), 1);
    assert_eq!(delta.changed_constraints.len(), 1);
    assert!(delta.removed_mesh_paths.is_empty());
    assert!(delta.removed_value_ids.is_empty());
    assert!(delta.removed_constraint_ids.is_empty());
}

#[test]
fn compute_delta_none_last_state_returns_full_then_diff() {
    use crate::diff::compute_delta;
    use std::sync::Mutex;

    let last_state: Mutex<Option<GuiState>> = Mutex::new(None);

    let state1 = GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0])],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    // First call with None last_state → full delta
    let delta = compute_delta(&last_state, &state1);
    assert_eq!(delta.changed_meshes.len(), 1, "full: all meshes");
    assert_eq!(delta.changed_values.len(), 1, "full: all values");

    // Second call with changed state → minimal diff
    let state2 = GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0])], // unchanged
        values: vec![sample_value("Bracket.width", "120")],             // changed
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let delta = compute_delta(&last_state, &state2);
    assert!(delta.changed_meshes.is_empty(), "diff: mesh unchanged");
    assert_eq!(delta.changed_values.len(), 1, "diff: value changed");
    assert_eq!(delta.changed_values[0].value, "120");
}

#[test]
fn delta_to_events_returns_correct_tuples_for_changes_and_removals() {
    use crate::diff::delta_to_events;

    let delta = StateDelta {
        changed_meshes: vec![sample_mesh("Bracket.body", vec![1.0, 2.0, 3.0])],
        changed_values: vec![sample_value("Bracket.width", "120")],
        changed_constraints: vec![sample_constraint("Bracket.0", "Violated")],
        removed_mesh_paths: vec!["Bracket.old_body".to_string()],
        removed_value_ids: vec!["Bracket.old_param".to_string()],
        removed_constraint_ids: vec!["Bracket.old_constraint".to_string()],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);

    // Should have 6 events: 3 changes + 3 removals
    assert_eq!(events.len(), 6, "expected 6 events, got {}", events.len());

    // Check mesh-update event
    let mesh_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "mesh-update")
        .collect();
    assert_eq!(mesh_events.len(), 1);
    assert_eq!(mesh_events[0].1["entity_path"], "Bracket.body");

    // Check value-update event
    let value_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "value-update")
        .collect();
    assert_eq!(value_events.len(), 1);
    assert_eq!(value_events[0].1["cell_id"], "Bracket.width");

    // Check constraint-update event
    let constraint_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "constraint-update")
        .collect();
    assert_eq!(constraint_events.len(), 1);
    assert_eq!(constraint_events[0].1["node_id"], "Bracket.0");

    // Check mesh-removed event
    let mesh_removed: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "mesh-removed")
        .collect();
    assert_eq!(mesh_removed.len(), 1);
    assert_eq!(mesh_removed[0].1.as_str().unwrap(), "Bracket.old_body");

    // Check value-removed event
    let value_removed: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "value-removed")
        .collect();
    assert_eq!(value_removed.len(), 1);
    assert_eq!(value_removed[0].1.as_str().unwrap(), "Bracket.old_param");

    // Check constraint-removed event
    let constraint_removed: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "constraint-removed")
        .collect();
    assert_eq!(constraint_removed.len(), 1);
    assert_eq!(
        constraint_removed[0].1.as_str().unwrap(),
        "Bracket.old_constraint"
    );
}

#[test]
fn delta_to_events_returns_empty_vec_for_empty_delta() {
    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);
    assert!(events.is_empty(), "empty delta should produce no events");
}

/// A mesh with f32::NAN vertices should produce a "serialization-error" event
/// with a structured payload containing item_type, item_id, and error fields.
/// This test fails because step-2 only logs, it doesn't emit an error event.
#[test]
fn delta_to_events_emits_serialization_error_event_on_failure() {
    let delta = StateDelta {
        changed_meshes: vec![sample_mesh("Bad.body", vec![f32::NAN, 0.0, 0.0])],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);

    // Should have exactly one "serialization-error" event
    let error_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "serialization-error")
        .collect();
    assert_eq!(
        error_events.len(),
        1,
        "expected exactly one serialization-error event; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    // The payload must have item_type, item_id, and error fields
    let payload = &error_events[0].1;
    assert_eq!(payload["item_type"], "mesh", "item_type must be \"mesh\"");
    assert_eq!(
        payload["item_id"], "Bad.body",
        "item_id must be the entity_path"
    );
    assert!(
        payload["error"].is_string() && !payload["error"].as_str().unwrap().is_empty(),
        "error must be a non-empty string"
    );

    // No mesh-update event should have been emitted for the failed mesh
    assert!(
        events.iter().all(|(n, _)| n != "mesh-update"),
        "no mesh-update event should be emitted for a mesh that failed serialization"
    );
}

/// A mesh with f32::NAN in vertices causes serde_json::to_value to fail.
/// The function must log a tracing::warn! for the failure and NOT emit a
/// "mesh-update" event for the bad mesh, while still emitting events for
/// valid items in the same delta.
#[test]
fn delta_to_events_warns_and_skips_on_serialization_failure() {
    let (subscriber, warn_count) = reify_test_support::warn_counting_subscriber();

    let delta = StateDelta {
        changed_meshes: vec![
            // NaN vertices — serde_json::to_value will return Err
            sample_mesh("Bad.body", vec![f32::NAN, 0.0, 0.0]),
            // Valid mesh — should still produce a "mesh-update" event
            sample_mesh("Good.body", vec![1.0, 2.0, 3.0]),
        ],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = tracing::subscriber::with_default(subscriber, || delta_to_events(&delta));

    // One warn should have been emitted for the NaN mesh serialization failure
    reify_test_support::assert_warn_count(
        &warn_count,
        1,
        "expected exactly one tracing::warn for the NaN mesh",
    );

    // The valid mesh should still produce its event
    let mesh_update_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "mesh-update")
        .collect();
    assert_eq!(
        mesh_update_events.len(),
        1,
        "expected exactly one mesh-update event for the valid mesh"
    );
    assert_eq!(mesh_update_events[0].1["entity_path"], "Good.body");

    // No mesh-update event for the NaN mesh
    let nan_events: Vec<_> = events
        .iter()
        .filter(|(name, val)| name == "mesh-update" && val["entity_path"] == "Bad.body")
        .collect();
    assert!(
        nan_events.is_empty(),
        "expected no mesh-update event for the NaN mesh"
    );
}

/// Two NaN meshes in the same delta both get their own warn and their own
/// "serialization-error" event. A valid value in the same delta is unaffected.
#[test]
fn delta_to_events_multiple_failures_warn_for_each() {
    let (subscriber, warn_count) = reify_test_support::warn_counting_subscriber();

    let delta = StateDelta {
        changed_meshes: vec![
            sample_mesh("Bad1.body", vec![f32::NAN, 0.0, 0.0]),
            sample_mesh("Bad2.body", vec![0.0, f32::INFINITY, 0.0]),
        ],
        changed_values: vec![sample_value("Good.width", "42")],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = tracing::subscriber::with_default(subscriber, || delta_to_events(&delta));

    // Two warnings, one per failing mesh
    reify_test_support::assert_warn_count(
        &warn_count,
        2,
        "expected exactly two tracing::warn calls",
    );

    // Two serialization-error events
    let error_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "serialization-error")
        .collect();
    assert_eq!(
        error_events.len(),
        2,
        "expected two serialization-error events; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    // The error events reference the correct item ids
    let error_ids: Vec<&str> = error_events
        .iter()
        .map(|(_, v)| v["item_id"].as_str().unwrap())
        .collect();
    assert!(
        error_ids.contains(&"Bad1.body"),
        "Bad1.body must appear in error events"
    );
    assert!(
        error_ids.contains(&"Bad2.body"),
        "Bad2.body must appear in error events"
    );

    // The valid value still produces its event
    let value_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "value-update")
        .collect();
    assert_eq!(
        value_events.len(),
        1,
        "the valid value must still produce a value-update event"
    );
    assert_eq!(value_events[0].1["cell_id"], "Good.width");

    // No mesh-update events at all (both meshes failed)
    assert!(
        events.iter().all(|(n, _)| n != "mesh-update"),
        "no mesh-update events should be emitted when all meshes failed serialization"
    );
}

/// push_serialized_event: on Ok, a single (event_name, val) tuple is pushed.
#[test]
fn push_serialized_event_pushes_update_on_ok() {
    let mut events: Vec<(String, serde_json::Value)> = Vec::new();
    let val = serde_json::json!({"x": 1});
    push_serialized_event(
        &mut events,
        "mesh-update",
        "mesh",
        "A.body",
        Ok(val.clone()),
    );
    assert_eq!(events.len(), 1, "expected exactly one event");
    assert_eq!(events[0].0, "mesh-update");
    assert_eq!(events[0].1, val);
}

/// push_serialized_event: on Err, emits exactly one warn! and pushes a
/// serialization-error event with the correct item_type, item_id, and error fields.
#[test]
fn push_serialized_event_pushes_error_and_warns_on_err() {
    let (subscriber, warn_count) = reify_test_support::warn_counting_subscriber();
    let err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
    let mut events: Vec<(String, serde_json::Value)> = Vec::new();
    tracing::subscriber::with_default(subscriber, || {
        push_serialized_event(&mut events, "value-update", "value", "V.x", Err(err));
    });
    reify_test_support::assert_warn_count(&warn_count, 1, "expected exactly 1 warn");
    assert_eq!(
        events.len(),
        1,
        "expected exactly one serialization-error event"
    );
    let (name, payload) = &events[0];
    assert_eq!(name, "serialization-error");
    assert_eq!(payload["item_type"], "value", "item_type must be \"value\"");
    assert_eq!(payload["item_id"], "V.x", "item_id must be \"V.x\"");
    assert!(
        payload["error"].is_string() && !payload["error"].as_str().unwrap().is_empty(),
        "error must be a non-empty string"
    );
}

/// diff_gui_state: identical tessellation_diagnostics in old and new → delta field is None.
#[test]
fn diff_identical_tessellation_diagnostics_returns_none() {
    let diags = vec![sample_diagnostic("Error", "geometry error: kernel failure")];
    let old = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: diags.clone(),
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: diags.clone(),
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);
    assert!(
        delta.changed_tessellation_diagnostics.is_none(),
        "expected None when diagnostics are identical, got {:?}",
        delta.changed_tessellation_diagnostics
    );
}

/// diff_gui_state: different tessellation_diagnostics → delta field is Some with the new vec.
#[test]
fn diff_changed_tessellation_diagnostics_returns_some() {
    let old_diags = vec![sample_diagnostic("Error", "geometry error: old failure")];
    let new_diags = vec![
        sample_diagnostic("Error", "geometry error: new failure"),
        sample_diagnostic("Warning", "geometry warning: suspect shape"),
    ];
    let old = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: old_diags,
        compile_diagnostics: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: new_diags.clone(),
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);
    let changed = delta
        .changed_tessellation_diagnostics
        .as_ref()
        .expect("expected Some when diagnostics changed, got None");
    assert_eq!(
        changed, &new_diags,
        "delta should carry the new diagnostics vec"
    );
}

/// diff_gui_state: non-empty → empty transition emits Some(vec![]) so
/// subscribers can clear their view.
///
/// None would swallow the clear event; subscribers must receive Some(vec![])
/// to know the list was emptied. This is distinct from StateDelta::full, which
/// collapses empty → None to avoid no-op wire traffic on initial emission.
#[test]
fn diff_clearing_tessellation_diagnostics_emits_some_empty() {
    let old = crate::types::GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![sample_diagnostic("Error", "kernel failure")],
        compile_diagnostics: vec![],
    };
    let new = crate::types::GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);
    assert_eq!(
        delta.changed_tessellation_diagnostics,
        Some(vec![]),
        "non-empty → empty transition must emit Some(vec![]) so subscribers can \
         clear their view; subscribers must receive Some(vec![]) on non-empty → \
         empty transition to clear their view; None would swallow the clear"
    );
}

/// delta_to_events: when `changed_tessellation_diagnostics` is Some(vec),
/// exactly one event named "tessellation-diagnostics" is produced with the vec
/// as its JSON payload.
#[test]
fn delta_to_events_emits_tessellation_diagnostics_event() {
    let diags = vec![
        sample_diagnostic("Error", "geometry error: kernel failure"),
        sample_diagnostic("Warning", "geometry warning: suspect shape"),
    ];
    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: Some(diags.clone()),
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);

    let tess_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "tessellation-diagnostics")
        .collect();
    assert_eq!(
        tess_events.len(),
        1,
        "expected exactly one tessellation-diagnostics event; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    let expected = serde_json::to_value(&diags).expect("failed to serialize diagnostics");
    assert_eq!(
        tess_events[0].1, expected,
        "tessellation-diagnostics payload must match the diagnostics vec"
    );
}

/// delta_to_events: when `changed_tessellation_diagnostics` is None,
/// no "tessellation-diagnostics" event is emitted.
#[test]
fn delta_to_events_omits_tessellation_diagnostics_event_when_none() {
    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);

    assert!(
        events.iter().all(|(n, _)| n != "tessellation-diagnostics"),
        "expected no tessellation-diagnostics event when field is None; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
}

// --- compile_diagnostics diff / event tests (step-5) ---

/// diff_gui_state: old empty compile_diagnostics → new non-empty produces
/// `Some(vec![...])` in the delta.
#[test]
fn diff_emits_compile_diagnostics_when_changed() {
    let old = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let new_diags = vec![sample_diagnostic("Warning", "unknown port type 'Foo'")];
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: new_diags.clone(),
    };

    let delta = diff_gui_state(&old, &new);
    let changed = delta
        .changed_compile_diagnostics
        .as_ref()
        .expect("expected Some when compile_diagnostics changed, got None");
    assert_eq!(
        changed, &new_diags,
        "delta should carry the new compile_diagnostics vec"
    );
}

/// diff_gui_state: non-empty → empty transition emits `Some(vec![])` so
/// subscribers can clear the diagnostics panel.
#[test]
fn diff_emits_compile_diagnostics_clear_on_transition_to_empty() {
    let old = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![sample_diagnostic("Warning", "unknown port type 'Foo'")],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };

    let delta = diff_gui_state(&old, &new);
    assert_eq!(
        delta.changed_compile_diagnostics,
        Some(vec![]),
        "non-empty → empty transition must emit Some(vec![]) so subscribers can clear; \
         None would swallow the clear event"
    );
}

/// delta_to_events: when `changed_compile_diagnostics` is Some(vec), exactly
/// one event named `"compile-diagnostics"` is produced with the vec as its
/// JSON payload.
#[test]
fn delta_to_events_emits_compile_diagnostics_event() {
    let diags = vec![
        sample_diagnostic("Warning", "unknown port type 'Foo'"),
        sample_diagnostic("Info", "unused import 'bar'"),
    ];
    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: Some(diags.clone()),
    };

    let events = delta_to_events(&delta);

    let compile_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "compile-diagnostics")
        .collect();
    assert_eq!(
        compile_events.len(),
        1,
        "expected exactly one compile-diagnostics event; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    let expected = serde_json::to_value(&diags).expect("failed to serialize diagnostics");
    assert_eq!(
        compile_events[0].1, expected,
        "compile-diagnostics payload must match the diagnostics vec"
    );
}

/// delta_to_events: when `changed_compile_diagnostics` is None, no
/// `"compile-diagnostics"` event is emitted.
#[test]
fn delta_to_events_omits_compile_diagnostics_event_when_none() {
    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
        changed_tessellation_diagnostics: None,
        changed_compile_diagnostics: None,
    };

    let events = delta_to_events(&delta);

    assert!(
        events.iter().all(|(n, _)| n != "compile-diagnostics"),
        "expected no compile-diagnostics event when field is None; got {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
}
