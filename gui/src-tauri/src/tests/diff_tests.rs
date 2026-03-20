use crate::diff::diff_gui_state;
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
        determinacy: "Determined".to_string(),
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
