use std::sync::Arc;

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
        tessellation_diagnostics: vec![],
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(v.get("meshes").unwrap().is_array());
    assert!(v.get("values").unwrap().is_array());
    assert!(v.get("constraints").unwrap().is_array());
    assert!(v.get("files").unwrap().is_array());
}

#[test]
fn gui_state_serializes_tessellation_diagnostics_field() {
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(
        v.get("tessellation_diagnostics").unwrap().is_array(),
        "tessellation_diagnostics must serialize as a JSON array"
    );
    assert_eq!(
        v["tessellation_diagnostics"].as_array().unwrap().len(),
        0,
        "empty tessellation_diagnostics should serialize as an empty array"
    );
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
        lambda: Arc::new(Value::Undef),
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

// --- serialize_finite_f32_vec characterization tests ---
// These tests exercise serialize_finite_f32_vec (private fn) through MeshData's
// Serialize impl, covering both happy-path and error paths, characterizing the merged single-pass loop behavior.

#[test]
fn serialize_finite_f32_vec_all_finite_values_round_trip() {
    // Exact float values must survive the serialize→deserialize round-trip unchanged.
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![1.5, -2.25, 3.0],
        indices: vec![],
        normals: None,
    };
    let v = serde_json::to_value(&mesh).unwrap();
    let arr = v["vertices"].as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0].as_f64().unwrap(), 1.5);
    assert_eq!(arr[1].as_f64().unwrap(), -2.25);
    assert_eq!(arr[2].as_f64().unwrap(), 3.0);
}

#[test]
fn serialize_finite_f32_vec_empty_vec_serializes_to_empty_array() {
    // Edge case: an empty vertices slice must produce an empty JSON array,
    // not an error — the merged single-pass loop must handle zero iterations.
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![],
        indices: vec![],
        normals: None,
    };
    let v = serde_json::to_value(&mesh).unwrap();
    let arr = v["vertices"].as_array().unwrap();
    assert_eq!(arr.len(), 0);
}

#[test]
fn serialize_finite_f32_vec_nan_causes_error_with_non_finite_and_nan() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![f32::NAN],
        indices: vec![],
        normals: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("non-finite"), "expected 'non-finite' in: {msg}");
    assert!(msg.contains("NaN"), "expected 'NaN' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_infinity_causes_error_with_non_finite_and_inf() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![f32::INFINITY],
        indices: vec![],
        normals: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("non-finite"), "expected 'non-finite' in: {msg}");
    assert!(msg.contains("inf"), "expected 'inf' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_neg_infinity_causes_error_with_non_finite_and_neg_inf() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![f32::NEG_INFINITY],
        indices: vec![],
        normals: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("non-finite"), "expected 'non-finite' in: {msg}");
    assert!(msg.contains("-inf"), "expected '-inf' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_nan_in_normals_causes_error() {
    // serialize_finite_f32_vec_opt must delegate to serialize_finite_f32_vec,
    // so a NaN in normals triggers the same error path.
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![0],
        normals: Some(vec![0.0, 0.0, f32::NAN]),
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("non-finite"), "expected 'non-finite' in: {msg}");
    assert!(msg.contains("NaN"), "expected 'NaN' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_non_finite_at_later_position_still_causes_error() {
    // A non-finite value at position > 0 must still cause an error.
    // This verifies fail-fast semantics hold regardless of where the bad value sits
    // in the single-pass merged loop.
    //
    // Partial-output safety: although the loop writes earlier elements (1.0, 2.0)
    // to the serializer's internal state before detecting the NaN, `serde_json::to_value`
    // builds an in-memory `Value` that is simply dropped on `Err` — no partial output is
    // observable by the caller.  For streaming-serializer callers see the `# Note` in
    // `serialize_finite_f32_vec` (types.rs).
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![1.0, 2.0, f32::NAN],
        indices: vec![],
        normals: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("non-finite"), "expected 'non-finite' in: {msg}");
}

// --- PersistentViewState serde tests (step-7) ---

#[test]
fn persistent_view_state_serde_roundtrip() {
    use crate::types::{CameraStateData, PersistentViewState, ViewDefinitionData};

    let mut cameras = std::collections::HashMap::new();
    cameras.insert(
        "design".to_string(),
        CameraStateData {
            position: [1.0, 2.0, 3.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            zoom: 1.5,
        },
    );

    let mut visibility = std::collections::HashMap::new();
    visibility.insert("Bracket.flange".to_string(), "show".to_string());

    let mut explicit = std::collections::HashMap::new();
    explicit.insert("Bracket.body".to_string(), "ghost".to_string());

    let state = PersistentViewState {
        version: "1".to_string(),
        active_view_id: "user:my-view".to_string(),
        user_views: vec![ViewDefinitionData {
            id: "user:my-view".to_string(),
            name: "My View".to_string(),
            auto: false,
            visibility,
            modified: Some(true),
        }],
        explicit,
        viewport_cameras: cameras,
        timestamp: "2026-04-22T12:00:00Z".to_string(),
    };

    // Serialise to JSON and back.
    let json = serde_json::to_string_pretty(&state).expect("serialise should succeed");
    let loaded: PersistentViewState =
        serde_json::from_str(&json).expect("deserialise should succeed");

    assert_eq!(loaded, state, "round-trip should preserve all fields");
}

#[test]
fn persistent_view_state_json_uses_camel_case_keys() {
    use crate::types::PersistentViewState;

    let state = PersistentViewState {
        version: "1".to_string(),
        active_view_id: "auto:default".to_string(),
        user_views: vec![],
        explicit: std::collections::HashMap::new(),
        viewport_cameras: std::collections::HashMap::new(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
    };

    let v = serde_json::to_value(&state).expect("serialise should succeed");
    // Keys must be camelCase to match the TypeScript PersistentViewState interface.
    assert!(v.get("activeViewId").is_some(), "activeViewId key must be present");
    assert!(v.get("userViews").is_some(), "userViews key must be present");
    assert!(v.get("viewportCameras").is_some(), "viewportCameras key must be present");
    // Snake_case equivalents must NOT appear.
    assert!(v.get("active_view_id").is_none(), "snake_case active_view_id must not appear");
    assert!(v.get("user_views").is_none(), "snake_case user_views must not appear");
}

#[test]
fn persistent_view_state_ipc_contract() {
    use crate::types::PersistentViewState;
    super::assert_ipc_contract::<PersistentViewState>();
}

/// `ValueData` must carry a `freshness: String` JSON field.
///
/// The field defaults to `"final"` (matching `Freshness::default() = Final`).
/// This pin guards the wire-shape contract added by task #2337.
#[test]
fn value_data_serializes_with_freshness_field() {
    let val = ValueData {
        cell_id: "Bracket.width".to_string(),
        name: "width".to_string(),
        value: "80".to_string(),
        unit: "mm".to_string(),
        determinacy: "determined".to_string(),
        entity_path: "Bracket".to_string(),
        kind: "Param".to_string(),
        freshness: "final".to_string(),
    };
    let v = serde_json::to_value(&val).unwrap();
    assert_eq!(
        v["freshness"],
        serde_json::json!("final"),
        "ValueData must serialize freshness field as 'final'"
    );
}

/// `EntityTreeNode` must carry a `freshness: String` JSON field.
///
/// The field defaults to `"final"` per the task #2337 design decision on
/// `Freshness::default() = Final`. This pin guards the wire-shape contract.
#[test]
fn entity_tree_node_serializes_with_freshness_field() {
    let node = EntityTreeNode {
        entity_path: "Bracket".to_string(),
        kind: "structure".to_string(),
        type_name: None,
        display_name: None,
        has_mesh: false,
        trait_geometry: false,
        children: vec![],
        freshness: "final".to_string(),
    };
    let v = serde_json::to_value(&node).unwrap();
    assert_eq!(
        v["freshness"],
        serde_json::json!("final"),
        "EntityTreeNode must serialize freshness field as 'final'"
    );
}

/// `format_freshness` must return lowercase wire strings matching the
/// pattern already established by `format_determinacy`. The frontend reads
/// these strings as CSS `data-freshness` attribute values (task #2337
/// design decision: tag-only string collapse).
#[test]
fn format_freshness_returns_lowercase_strings() {
    use crate::types::format_freshness;
    use reify_types::{ErrorRef, Freshness, ResultRef};

    // Final → "final"
    assert_eq!(format_freshness(&Freshness::Final), "final");

    // Intermediate (generation ignored at wire layer) → "intermediate"
    assert_eq!(
        format_freshness(&Freshness::Intermediate { generation: 7 }),
        "intermediate"
    );

    // Pending (last_substantive ignored at wire layer) → "pending"
    assert_eq!(
        format_freshness(&Freshness::Pending {
            last_substantive: ResultRef::none()
        }),
        "pending"
    );

    // Failed (error payload ignored at wire layer) → "failed"
    assert_eq!(
        format_freshness(&Freshness::Failed {
            error: ErrorRef::new("test-error-x")
        }),
        "failed"
    );
}
