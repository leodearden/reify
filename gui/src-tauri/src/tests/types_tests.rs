use serde_json::json;

use crate::types::*;

#[test]
fn gui_state_empty_serializes_with_expected_keys() {
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(v.get("meshes").unwrap().is_array());
    assert!(v.get("values").unwrap().is_array());
    assert!(v.get("constraints").unwrap().is_array());
    assert!(v.get("files").unwrap().is_array());
}

#[test]
fn mesh_data_serializes_with_expected_fields() {
    let mesh = MeshData {
        entity_path: "Bracket.body".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: Some(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
    };
    let v = serde_json::to_value(&mesh).unwrap();
    assert_eq!(v["entity_path"], json!("Bracket.body"));
    assert_eq!(v["vertices"].as_array().unwrap().len(), 9);
    assert_eq!(v["indices"].as_array().unwrap().len(), 3);
    assert_eq!(v["normals"].as_array().unwrap().len(), 9);
}

#[test]
fn value_data_serializes_with_expected_fields() {
    let val = ValueData {
        cell_id: "Bracket.width".to_string(),
        name: "width".to_string(),
        value: "80".to_string(),
        unit: "mm".to_string(),
        determinacy: "Determined".to_string(),
        entity_path: "Bracket".to_string(),
        kind: "Param".to_string(),
    };
    let v = serde_json::to_value(&val).unwrap();
    assert_eq!(v["cell_id"], json!("Bracket.width"));
    assert_eq!(v["name"], json!("width"));
    assert_eq!(v["value"], json!("80"));
    assert_eq!(v["unit"], json!("mm"));
    assert_eq!(v["determinacy"], json!("Determined"));
    assert_eq!(v["entity_path"], json!("Bracket"));
    assert_eq!(v["kind"], json!("Param"));
}

#[test]
fn constraint_data_serializes_with_expected_fields() {
    let c = ConstraintData {
        node_id: "Bracket.0".to_string(),
        expression: "thickness > 2mm".to_string(),
        status: "Satisfied".to_string(),
        label: None,
        parameter_ids: vec!["Bracket.thickness".to_string()],
    };
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["node_id"], json!("Bracket.0"));
    assert_eq!(v["expression"], json!("thickness > 2mm"));
    assert_eq!(v["status"], json!("Satisfied"));
    assert!(v["label"].is_null());
    assert_eq!(v["parameter_ids"].as_array().unwrap().len(), 1);
}

#[test]
fn source_location_serializes_with_expected_fields() {
    let loc = SourceLocation {
        file: "bracket.ri".to_string(),
        line: 3,
        column: 4,
        end_line: 3,
        end_column: 30,
    };
    let v = serde_json::to_value(&loc).unwrap();
    assert_eq!(v["file"], json!("bracket.ri"));
    assert_eq!(v["line"], json!(3));
    assert_eq!(v["column"], json!(4));
    assert_eq!(v["end_line"], json!(3));
    assert_eq!(v["end_column"], json!(30));
}

#[test]
fn file_data_serializes_with_expected_fields() {
    let f = FileData {
        path: "bracket.ri".to_string(),
        content: "structure Bracket { }".to_string(),
    };
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["path"], json!("bracket.ri"));
    assert_eq!(v["content"], json!("structure Bracket { }"));
}

#[test]
fn evaluation_status_serializes_with_phase_and_optional_progress() {
    // EvaluationStatus with no progress should omit the progress field
    let status = EvaluationStatus {
        phase: "idle".to_string(),
        progress: None,
    };
    let v = serde_json::to_value(&status).unwrap();
    assert_eq!(v["phase"], json!("idle"));
    assert!(v.get("progress").is_none(), "progress should be omitted when None");

    // EvaluationStatus with progress should include it
    let status = EvaluationStatus {
        phase: "evaluating".to_string(),
        progress: Some(0.5),
    };
    let v = serde_json::to_value(&status).unwrap();
    assert_eq!(v["phase"], json!("evaluating"));
    assert_eq!(v["progress"], json!(0.5));
}
