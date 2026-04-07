use serde_json::json;

use crate::types::*;
use reify_types::{DeterminacyState, DimensionVector, FieldSourceKind, Type, Value};

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
        determinacy: "determined".to_string(),
        entity_path: "Bracket".to_string(),
        kind: "Param".to_string(),
    };
    let v = serde_json::to_value(&val).unwrap();
    assert_eq!(v["cell_id"], json!("Bracket.width"));
    assert_eq!(v["name"], json!("width"));
    assert_eq!(v["value"], json!("80"));
    assert_eq!(v["unit"], json!("mm"));
    assert_eq!(v["determinacy"], json!("determined"));
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
    let loc = reify_mcp::SourceLocationInfo {
        file_path: "bracket.ri".to_string(),
        line: 3,
        column: 4,
        end_line: 3,
        end_column: 30,
    };
    let v = serde_json::to_value(&loc).unwrap();
    assert_eq!(v["file_path"], json!("bracket.ri"));
    assert_eq!(v["line"], json!(3));
    assert_eq!(v["column"], json!(4));
    assert_eq!(v["end_line"], json!(3));
    assert_eq!(v["end_column"], json!(30));
    assert!(v.get("file").is_none(), "should not serialize as 'file'");
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
    assert!(
        v.get("progress").is_none(),
        "progress should be omitted when None"
    );

    // EvaluationStatus with progress should include it
    let status = EvaluationStatus {
        phase: "evaluating".to_string(),
        progress: Some(0.5),
    };
    let v = serde_json::to_value(&status).unwrap();
    assert_eq!(v["phase"], json!("evaluating"));
    assert_eq!(v["progress"], json!(0.5));
}

#[test]
fn format_determinacy_returns_lowercase_strings() {
    // The frontend expects lowercase determinacy strings (e.g. 'determined', not 'Determined')
    assert_eq!(
        format_determinacy(DeterminacyState::Determined),
        "determined"
    );
    assert_eq!(
        format_determinacy(DeterminacyState::Undetermined),
        "undetermined"
    );
    assert_eq!(
        format_determinacy(DeterminacyState::Provisional),
        "provisional"
    );
    assert_eq!(format_determinacy(DeterminacyState::Auto), "auto");
}

// --- format_value characterization tests ---
// These tests exercise Value::format_display_pair() through the GUI format_value()
// thin wrapper, covering composite types that have no other test coverage.

#[test]
fn format_value_int() {
    assert_eq!(
        format_value(&Value::Int(42)),
        ("42".to_string(), String::new())
    );
}

#[test]
fn format_value_real() {
    assert_eq!(
        format_value(&Value::Real(3.125)),
        ("3.125".to_string(), String::new())
    );
}

#[test]
fn format_value_bool() {
    assert_eq!(
        format_value(&Value::Bool(true)),
        ("true".to_string(), String::new())
    );
    assert_eq!(
        format_value(&Value::Bool(false)),
        ("false".to_string(), String::new())
    );
}

#[test]
fn format_value_scalar_length() {
    let v = Value::Scalar {
        si_value: 0.08,
        dimension: DimensionVector::LENGTH,
    };
    assert_eq!(format_value(&v), ("80".to_string(), "mm".to_string()));
}

#[test]
fn format_value_point() {
    let v = Value::Point(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    assert_eq!(
        format_value(&v),
        ("point(0, 0, 0)".to_string(), String::new())
    );
}

#[test]
fn format_value_vector() {
    let v = Value::Vector(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    assert_eq!(
        format_value(&v),
        ("vec(0, 0, 0)".to_string(), String::new())
    );
}

#[test]
fn format_value_orientation() {
    let v = Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    assert_eq!(
        format_value(&v),
        ("[1, 0, 0, 0]q".to_string(), String::new())
    );
}

#[test]
fn format_value_frame() {
    let origin = Value::Point(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let basis = Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let v = Value::Frame {
        origin: Box::new(origin),
        basis: Box::new(basis),
    };
    assert_eq!(
        format_value(&v),
        (
            "frame(point(0, 0, 0), [1, 0, 0, 0]q)".to_string(),
            String::new()
        )
    );
}

#[test]
fn format_value_transform() {
    let rotation = Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let translation = Value::Vector(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let v = Value::Transform {
        rotation: Box::new(rotation),
        translation: Box::new(translation),
    };
    assert_eq!(
        format_value(&v),
        (
            "transform([1, 0, 0, 0]q, vec(0, 0, 0))".to_string(),
            String::new()
        )
    );
}

#[test]
fn format_value_plane() {
    let origin = Value::Point(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let normal = Value::Vector(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
    ]);
    let v = Value::Plane {
        origin: Box::new(origin),
        normal: Box::new(normal),
    };
    assert_eq!(
        format_value(&v),
        (
            "plane(point(0, 0, 0), vec(0, 0, 1))".to_string(),
            String::new()
        )
    );
}

#[test]
fn format_value_axis() {
    let origin = Value::Point(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let direction = Value::Vector(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
    ]);
    let v = Value::Axis {
        origin: Box::new(origin),
        direction: Box::new(direction),
    };
    assert_eq!(
        format_value(&v),
        (
            "axis(point(0, 0, 0), vec(0, 0, 1))".to_string(),
            String::new()
        )
    );
}

#[test]
fn format_value_bbox() {
    let min = Value::Point(vec![
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let max = Value::Point(vec![
        Value::Scalar {
            si_value: 0.001,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.001,
            dimension: DimensionVector::LENGTH,
        },
        Value::Scalar {
            si_value: 0.001,
            dimension: DimensionVector::LENGTH,
        },
    ]);
    let v = Value::BoundingBox {
        min: Box::new(min),
        max: Box::new(max),
    };
    assert_eq!(
        format_value(&v),
        (
            "bbox(point(0, 0, 0), point(1, 1, 1))".to_string(),
            String::new()
        )
    );
}

#[test]
fn format_value_range() {
    let v = Value::Range {
        lower: Some(Box::new(Value::Int(1))),
        upper: Some(Box::new(Value::Int(5))),
        lower_inclusive: true,
        upper_inclusive: true,
    };
    assert_eq!(format_value(&v), ("[1..5]".to_string(), String::new()));
}

#[test]
fn format_value_matrix() {
    let v = Value::Matrix(vec![
        vec![Value::Int(1), Value::Int(0)],
        vec![Value::Int(0), Value::Int(1)],
    ]);
    assert_eq!(
        format_value(&v),
        ("[[1, 0], [0, 1]]".to_string(), String::new())
    );
}

#[test]
fn format_value_field() {
    let v = Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Analytical,
        lambda: Box::new(Value::Undef),
    };
    assert_eq!(
        format_value(&v),
        ("Field<Real, Real>(Analytical)".to_string(), String::new())
    );
}

#[test]
fn format_value_lambda() {
    let v = Value::Lambda {
        params: vec![],
        body: Box::new(reify_types::CompiledExpr::literal(Value::Undef, Type::Real)),
        captures: reify_types::ValueMap::default(),
    };
    assert_eq!(format_value(&v), ("<lambda>".to_string(), String::new()));
}

#[test]
fn format_value_undef() {
    assert_eq!(
        format_value(&Value::Undef),
        ("undefined".to_string(), String::new())
    );
}
