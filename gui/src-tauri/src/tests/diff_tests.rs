use crate::diff::{diff_gui_state, StateDelta};
use crate::types::*;

fn empty_state() -> GuiState {
    GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
    }
}

fn sample_value(cell_id: &str, value: &str) -> ValueData {
    ValueData {
        cell_id: cell_id.to_string(),
        name: cell_id.split('.').last().unwrap_or(cell_id).to_string(),
        value: value.to_string(),
        unit: "mm".to_string(),
        determinacy: "determined".to_string(),
        entity_path: cell_id.split('.').next().unwrap_or("").to_string(),
        kind: "Param".to_string(),
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
    }
}

#[test]
fn diff_identical_states_returns_empty_delta() {
    let state = GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0])],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![sample_constraint("Bracket.0", "Satisfied")],
        files: vec![],
    };

    let delta = diff_gui_state(&state, &state);

    assert!(delta.changed_meshes.is_empty(), "no meshes changed");
    assert!(delta.changed_values.is_empty(), "no values changed");
    assert!(delta.changed_constraints.is_empty(), "no constraints changed");
    assert!(delta.removed_mesh_paths.is_empty(), "no meshes removed");
    assert!(delta.removed_value_ids.is_empty(), "no values removed");
    assert!(delta.removed_constraint_ids.is_empty(), "no constraints removed");
}

#[test]
fn diff_detects_changed_value() {
    let old = GuiState {
        meshes: vec![],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![],
        files: vec![],
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![sample_value("Bracket.width", "120")],
        constraints: vec![],
        files: vec![],
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
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![sample_constraint("Bracket.0", "Violated")],
        files: vec![],
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
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("Bracket.body", vec![2.0, 2.0, 2.0]), // changed
            sample_mesh("Bracket.hole", vec![1.0, 1.0, 1.0]), // unchanged
        ],
        values: vec![],
        constraints: vec![],
        files: vec![],
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
    };
    let new = GuiState {
        meshes: vec![],
        values: vec![
            sample_value("Bracket.width", "80"),
            sample_value("Bracket.new_param", "20"), // added
        ],
        constraints: vec![],
        files: vec![],
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
    use std::sync::Mutex;
    use crate::diff::compute_delta;

    let last_state: Mutex<Option<GuiState>> = Mutex::new(None);

    let state1 = GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![0.0, 0.0, 0.0])],
        values: vec![sample_value("Bracket.width", "80")],
        constraints: vec![],
        files: vec![],
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
    };

    let events = delta_to_events(&delta);

    // Should have 6 events: 3 changes + 3 removals
    assert_eq!(events.len(), 6, "expected 6 events, got {}", events.len());

    // Check mesh-update event
    let mesh_events: Vec<_> = events.iter().filter(|(name, _)| name == "mesh-update").collect();
    assert_eq!(mesh_events.len(), 1);
    assert_eq!(mesh_events[0].1["entity_path"], "Bracket.body");

    // Check value-update event
    let value_events: Vec<_> = events.iter().filter(|(name, _)| name == "value-update").collect();
    assert_eq!(value_events.len(), 1);
    assert_eq!(value_events[0].1["cell_id"], "Bracket.width");

    // Check constraint-update event
    let constraint_events: Vec<_> = events.iter().filter(|(name, _)| name == "constraint-update").collect();
    assert_eq!(constraint_events.len(), 1);
    assert_eq!(constraint_events[0].1["node_id"], "Bracket.0");

    // Check mesh-removed event
    let mesh_removed: Vec<_> = events.iter().filter(|(name, _)| name == "mesh-removed").collect();
    assert_eq!(mesh_removed.len(), 1);
    assert_eq!(mesh_removed[0].1.as_str().unwrap(), "Bracket.old_body");

    // Check value-removed event
    let value_removed: Vec<_> = events.iter().filter(|(name, _)| name == "value-removed").collect();
    assert_eq!(value_removed.len(), 1);
    assert_eq!(value_removed[0].1.as_str().unwrap(), "Bracket.old_param");

    // Check constraint-removed event
    let constraint_removed: Vec<_> = events.iter().filter(|(name, _)| name == "constraint-removed").collect();
    assert_eq!(constraint_removed.len(), 1);
    assert_eq!(constraint_removed[0].1.as_str().unwrap(), "Bracket.old_constraint");
}

#[test]
fn delta_to_events_returns_empty_vec_for_empty_delta() {
    use crate::diff::delta_to_events;

    let delta = StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
    };

    let events = delta_to_events(&delta);
    assert!(events.is_empty(), "empty delta should produce no events");
}
