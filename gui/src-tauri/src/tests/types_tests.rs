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
        compile_diagnostics: vec![],
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
        compile_diagnostics: vec![],
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
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
        freshness: "final".to_string(),
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in: {msg}"
    );
    assert!(msg.contains("NaN"), "expected 'NaN' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_infinity_causes_error_with_non_finite_and_inf() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![f32::INFINITY],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in: {msg}"
    );
    assert!(msg.contains("inf"), "expected 'inf' in: {msg}");
}

#[test]
fn serialize_finite_f32_vec_neg_infinity_causes_error_with_non_finite_and_neg_inf() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![f32::NEG_INFINITY],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in: {msg}"
    );
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in: {msg}"
    );
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
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in: {msg}"
    );
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
        version: "2".to_string(),
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
        version: "2".to_string(),
        active_view_id: "auto:default".to_string(),
        user_views: vec![],
        explicit: std::collections::HashMap::new(),
        viewport_cameras: std::collections::HashMap::new(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
    };

    let v = serde_json::to_value(&state).expect("serialise should succeed");
    // Keys must be camelCase to match the TypeScript PersistentViewState interface.
    assert!(
        v.get("activeViewId").is_some(),
        "activeViewId key must be present"
    );
    assert!(
        v.get("userViews").is_some(),
        "userViews key must be present"
    );
    assert!(
        v.get("viewportCameras").is_some(),
        "viewportCameras key must be present"
    );
    // Snake_case equivalents must NOT appear.
    assert!(
        v.get("active_view_id").is_none(),
        "snake_case active_view_id must not appear"
    );
    assert!(
        v.get("user_views").is_none(),
        "snake_case user_views must not appear"
    );
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

// ---- MechanismDescriptor / JointDescriptor IPC contract (step-1) ----------

#[test]
fn mechanism_descriptor_ipc_contract() {
    use super::assert_ipc_contract;
    use crate::types::{JointDescriptor, MechanismDescriptor};
    assert_ipc_contract::<MechanismDescriptor>();
    assert_ipc_contract::<JointDescriptor>();
}

#[test]
fn mechanism_descriptor_round_trips_through_serde_json_with_snake_case_keys() {
    use crate::types::{JointDescriptor, MechanismDescriptor};

    let joint = JointDescriptor {
        joint_index: 0,
        kind: "prismatic".to_string(),
        dimension: "length".to_string(),
        range_lower_si: Some(0.0),
        range_upper_si: Some(1.0),
        axis: Some([1.0, 0.0, 0.0]),
        driving_param_cell_id: Some("Kinematic.y_pos".to_string()),
        current_value_si: Some(0.5),
    };

    let descriptor = MechanismDescriptor {
        cell_id: "Kinematic.m".to_string(),
        entity_path: "Kinematic".to_string(),
        name: "m".to_string(),
        bodies_count: 2,
        joints: vec![joint],
    };

    let v = serde_json::to_value(&descriptor).expect("serialize");

    // Snake-case keys must be present
    assert!(v.get("cell_id").is_some(), "expected 'cell_id' key");
    assert!(v.get("entity_path").is_some(), "expected 'entity_path' key");
    assert!(
        v.get("bodies_count").is_some(),
        "expected 'bodies_count' key"
    );
    assert!(v.get("joints").is_some(), "expected 'joints' key");

    let joints_arr = v["joints"].as_array().expect("joints is array");
    assert_eq!(joints_arr.len(), 1);
    let j = &joints_arr[0];
    assert!(j.get("joint_index").is_some(), "expected 'joint_index' key");
    assert!(
        j.get("range_lower_si").is_some(),
        "expected 'range_lower_si' key"
    );
    assert!(
        j.get("range_upper_si").is_some(),
        "expected 'range_upper_si' key"
    );
    assert!(
        j.get("driving_param_cell_id").is_some(),
        "expected 'driving_param_cell_id' key"
    );
    assert!(
        j.get("current_value_si").is_some(),
        "expected 'current_value_si' key"
    );

    // Round-trip
    let back: MechanismDescriptor = serde_json::from_value(v).expect("deserialize");
    assert_eq!(back.cell_id, "Kinematic.m");
    assert_eq!(back.bodies_count, 2);
    assert_eq!(back.joints.len(), 1);
    assert_eq!(back.joints[0].kind, "prismatic");
    assert_eq!(back.joints[0].range_lower_si, Some(0.0));
    assert_eq!(back.joints[0].range_upper_si, Some(1.0));
    assert_eq!(
        back.joints[0].driving_param_cell_id,
        Some("Kinematic.y_pos".to_string())
    );
}

#[test]
fn joint_descriptor_optional_fields_serialize_as_null() {
    use crate::types::JointDescriptor;

    let joint = JointDescriptor {
        joint_index: 2,
        kind: "fixed".to_string(),
        dimension: "dimensionless".to_string(),
        range_lower_si: None,
        range_upper_si: None,
        axis: None,
        driving_param_cell_id: None,
        current_value_si: None,
    };

    let v = serde_json::to_value(&joint).expect("serialize");
    assert!(
        v["range_lower_si"].is_null(),
        "range_lower_si should be null when None"
    );
    assert!(
        v["range_upper_si"].is_null(),
        "range_upper_si should be null when None"
    );
    assert!(v["axis"].is_null(), "axis should be null when None");
    assert!(
        v["driving_param_cell_id"].is_null(),
        "driving_param_cell_id should be null when None"
    );
    assert!(
        v["current_value_si"].is_null(),
        "current_value_si should be null when None"
    );
}

// --- scalar_channels IPC wire tests (task 2959, step-1) ---

/// A `MeshData` with a populated `scalar_channels` map serializes to a JSON
/// object with the expected channel keys and values, and round-trips through
/// `serde_json::to_value` / `from_value` preserving the map contents.
#[test]
fn mesh_data_scalar_channels_round_trips() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    channels.insert("vonMises".to_string(), vec![10.0_f32, 20.0, 30.0]);

    let mesh = MeshData {
        entity_path: "Test.body".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let v = serde_json::to_value(&mesh).expect("serialize should succeed");

    // (a) JSON must contain a scalar_channels object with key "vonMises"
    let sc = v
        .get("scalar_channels")
        .expect("scalar_channels must be present");
    assert!(sc.is_object(), "scalar_channels must be a JSON object");
    let arr = sc["vonMises"]
        .as_array()
        .expect("vonMises must be an array");
    assert_eq!(arr.len(), 3, "vonMises array must have 3 elements");
    assert_eq!(arr[0].as_f64().unwrap(), 10.0);
    assert_eq!(arr[1].as_f64().unwrap(), 20.0);
    assert_eq!(arr[2].as_f64().unwrap(), 30.0);

    // (b) Deserializing back yields the same struct
    let back: MeshData = serde_json::from_value(v).expect("deserialize should succeed");
    assert_eq!(back.entity_path, "Test.body");
    assert_eq!(
        back.scalar_channels.get("vonMises").unwrap(),
        &vec![10.0_f32, 20.0, 30.0]
    );
}

/// A `MeshData` with an empty `scalar_channels` HashMap must omit the field
/// entirely from the serialized JSON (validates `skip_serializing_if = "HashMap::is_empty"`).
#[test]
fn mesh_data_empty_scalar_channels_omitted_from_wire() {
    use std::collections::HashMap;

    let mesh = MeshData {
        entity_path: "Test.body".to_string(),
        vertices: vec![],
        indices: vec![],
        normals: None,
        scalar_channels: HashMap::new(),
        displaced_positions: None,
    };

    let v = serde_json::to_value(&mesh).expect("serialize should succeed");

    // Empty scalar_channels must not appear in the wire format
    assert!(
        v.get("scalar_channels").is_none(),
        "empty scalar_channels must be omitted from the wire"
    );
}

// --- scalar_channels NaN/Inf rejection tests (task 2959, step-3) ---

/// `serialize_finite_f32_map` must reject NaN values in a channel with an error
/// message containing both the channel key ("vonMises") and "non-finite f32 value".
#[test]
fn serialize_finite_f32_map_nan_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    channels.insert("vonMises".to_string(), vec![f32::NAN]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in: {msg}"
    );
    assert!(
        msg.contains("vonMises"),
        "expected channel key 'vonMises' in error message: {msg}"
    );
}

/// `serialize_finite_f32_map` must reject +Inf values with a message containing
/// both the channel key and "non-finite f32 value".
#[test]
fn serialize_finite_f32_map_infinity_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    channels.insert("vonMises".to_string(), vec![f32::INFINITY]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in: {msg}"
    );
    assert!(
        msg.contains("vonMises"),
        "expected channel key 'vonMises' in error message: {msg}"
    );
}

/// `serialize_finite_f32_map` must reject -Inf values with a message containing
/// both the channel key and "non-finite f32 value".
#[test]
fn serialize_finite_f32_map_neg_infinity_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    channels.insert("vonMises".to_string(), vec![f32::NEG_INFINITY]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in: {msg}"
    );
    assert!(
        msg.contains("vonMises"),
        "expected channel key 'vonMises' in error message: {msg}"
    );
}

// --- displaced_positions IPC wire tests (task 2959, step-5) ---

/// (a) A `MeshData` with `displaced_positions: Some(vec![...])` serializes to
/// JSON with a 3-element `"displaced_positions"` array.
#[test]
fn mesh_data_displaced_positions_some_serializes_to_array() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: Some(vec![1.0_f32, 2.0, 3.0]),
    };

    let v = serde_json::to_value(&mesh).expect("serialize should succeed");
    let arr = v
        .get("displaced_positions")
        .expect("displaced_positions must be present");
    let arr = arr
        .as_array()
        .expect("displaced_positions must be an array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0].as_f64().unwrap(), 1.0);
    assert_eq!(arr[1].as_f64().unwrap(), 2.0);
    assert_eq!(arr[2].as_f64().unwrap(), 3.0);
}

/// (b) `displaced_positions: None` must be omitted from the wire
/// (validates `skip_serializing_if = "Option::is_none"`).
#[test]
fn mesh_data_displaced_positions_none_omitted_from_wire() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
    };

    let v = serde_json::to_value(&mesh).expect("serialize should succeed");
    assert!(
        v.get("displaced_positions").is_none(),
        "displaced_positions: None must be omitted from the wire"
    );
}

/// (c) Round-trip: `Some(vec![1.0, 2.0, 3.0])` survives serde serialize/deserialize.
#[test]
fn mesh_data_displaced_positions_round_trips() {
    let mesh = MeshData {
        entity_path: "dp-test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: Some(vec![1.0_f32, 2.0, 3.0]),
    };

    let v = serde_json::to_value(&mesh).expect("serialize should succeed");
    let back: MeshData = serde_json::from_value(v).expect("deserialize should succeed");
    assert_eq!(back.displaced_positions, Some(vec![1.0_f32, 2.0, 3.0]));
}

/// (d) NaN in a `Some(...)` displaced_positions triggers a serialization error
/// mentioning "non-finite f32" — reusing the existing serialize_finite_f32_vec_opt semantics.
///
/// Note: displaced_positions must have the same length as vertices (length contract).
/// Use a matching-length vector so the length check passes and the NaN check fires.
#[test]
fn mesh_data_displaced_positions_nan_causes_error() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0], // 1 vertex → 3 floats
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: Some(vec![f32::NAN, 0.0, 0.0]), // 3 floats, length matches
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in error message: {msg}"
    );
}

// --- MeshData length contract tests (amendment, suggestion 3) ---

/// Serializing a MeshData whose scalar_channels entry has a different length
/// than the vertex count must return `Err` with the channel name and "vertex count"
/// in the error message.  This pins the length contract ("contract in production
/// code rather than relying on test coverage", task 2544).
#[test]
fn meshdata_rejects_scalar_channel_with_wrong_length() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    // 3-vertex mesh (9 floats), but vonMises has only 2 values
    channels.insert("vonMises".to_string(), vec![10.0_f32, 20.0]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 3 vertices
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("vonMises"),
        "expected channel name 'vonMises' in error message: {msg}"
    );
    assert!(
        msg.contains("vertex count"),
        "expected 'vertex count' in error message: {msg}"
    );
}

/// Serializing a MeshData whose displaced_positions length differs from vertices
/// length must return `Err` mentioning both "displaced_positions" and "vertices".
#[test]
fn meshdata_rejects_displaced_positions_with_wrong_length() {
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 9 floats
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: Some(vec![0.1_f32, 0.2, 0.3, 0.4, 0.5, 0.6]), // 6 floats ≠ 9
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("displaced_positions"),
        "expected 'displaced_positions' in error message: {msg}"
    );
    assert!(
        msg.contains("vertices"),
        "expected 'vertices' in error message: {msg}"
    );
}

/// `GuiState` must carry a `compile_diagnostics` JSON field that serializes as
/// an array. Mirrors `gui_state_serializes_tessellation_diagnostics_field`.
/// Fails until `compile_diagnostics: Vec<DiagnosticInfo>` is added to `GuiState`.
#[test]
fn gui_state_serializes_compile_diagnostics_field() {
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(
        v.get("compile_diagnostics").unwrap().is_array(),
        "compile_diagnostics must serialize as a JSON array"
    );
    assert_eq!(
        v["compile_diagnostics"].as_array().unwrap().len(),
        0,
        "empty compile_diagnostics should serialize as an empty array"
    );
}

// --- AutoResolveIteration / AutoResolveParameterValue / AutoResolveConstraintProgress serde tests ---

/// Step-1: pin the JSON key-set and nested value types for a fully-populated
/// `AutoResolveIteration` with `driving_metric` and `driving_metric_value` present.
#[test]
fn auto_resolve_iteration_serializes_with_expected_field_set() {
    use crate::types::{
        AutoResolveConstraintProgress, AutoResolveIteration, AutoResolveParameterValue,
    };
    use std::collections::HashMap;

    let mut parameters = HashMap::new();
    parameters.insert(
        "Bracket.thickness".to_string(),
        AutoResolveParameterValue {
            value: 4.2,
            unit: "mm".to_string(),
            display: "4.2mm".to_string(),
        },
    );

    let mut constraints = HashMap::new();
    constraints.insert(
        "max_von_mises".to_string(),
        AutoResolveConstraintProgress {
            name: "max_von_mises".to_string(),
            value: Some(180.0),
            unit: Some("MPa".to_string()),
            target_lower: None,
            target_upper: Some(200.0),
            satisfied: true,
        },
    );

    let iter = AutoResolveIteration {
        iteration: 0,
        parameters,
        constraints,
        driving_metric: Some("max_von_mises".to_string()),
        driving_metric_value: Some(180.0),
    };

    let v = serde_json::to_value(&iter).unwrap();

    // Top-level keys must exist
    assert!(v.get("iteration").is_some(), "iteration key must be present");
    assert!(v.get("parameters").is_some(), "parameters key must be present");
    assert!(v.get("constraints").is_some(), "constraints key must be present");
    assert_eq!(
        v["iteration"],
        json!(0),
        "iteration must be 0"
    );
    assert_eq!(
        v["driving_metric"],
        json!("max_von_mises"),
        "driving_metric must be present"
    );
    assert_eq!(
        v["driving_metric_value"],
        json!(180.0),
        "driving_metric_value must be present"
    );

    // parameters must be a JSON object
    assert!(
        v["parameters"].is_object(),
        "parameters must be a JSON object"
    );
    let param = &v["parameters"]["Bracket.thickness"];
    assert_eq!(param["value"], json!(4.2), "parameter value must be 4.2");
    assert_eq!(param["unit"], json!("mm"), "parameter unit must be 'mm'");
    assert_eq!(
        param["display"],
        json!("4.2mm"),
        "parameter display must be '4.2mm'"
    );

    // constraints must be a JSON object
    assert!(
        v["constraints"].is_object(),
        "constraints must be a JSON object"
    );
    let constraint = &v["constraints"]["max_von_mises"];
    assert_eq!(
        constraint["name"],
        json!("max_von_mises"),
        "constraint name must match"
    );
    assert_eq!(
        constraint["value"],
        json!(180.0),
        "constraint value must be 180.0 when Some"
    );
    assert_eq!(
        constraint["unit"],
        json!("MPa"),
        "constraint unit must be 'MPa'"
    );
    assert_eq!(
        constraint["target_upper"],
        json!(200.0),
        "constraint target_upper must be 200.0"
    );
    assert_eq!(
        constraint["satisfied"],
        json!(true),
        "constraint satisfied must be true"
    );
}

/// Step-2a: driving_metric and driving_metric_value must be ABSENT from JSON
/// when set to None (validates #[serde(skip_serializing_if = "Option::is_none")]).
#[test]
fn auto_resolve_iteration_omits_optional_when_none() {
    use crate::types::AutoResolveIteration;
    use std::collections::HashMap;

    let iter = AutoResolveIteration {
        iteration: 0,
        parameters: HashMap::new(),
        constraints: HashMap::new(),
        driving_metric: None,
        driving_metric_value: None,
    };

    let v = serde_json::to_value(&iter).unwrap();
    assert!(
        v.get("driving_metric").is_none(),
        "driving_metric must be absent from JSON when None"
    );
    assert!(
        v.get("driving_metric_value").is_none(),
        "driving_metric_value must be absent from JSON when None"
    );
}

/// Step-2b: value, unit, target_lower, and target_upper must be ABSENT from JSON
/// when set to None (validates #[serde(skip_serializing_if = "Option::is_none")]).
/// `value: None` is the v1 representation — the kernel does not yet expose
/// per-constraint observed scalars, so omitting is better than emitting 0.0.
#[test]
fn auto_resolve_constraint_progress_omits_unset_targets_and_unit() {
    use crate::types::AutoResolveConstraintProgress;

    let c = AutoResolveConstraintProgress {
        name: "stress_limit".to_string(),
        value: None,
        unit: None,
        target_lower: None,
        target_upper: None,
        satisfied: false,
    };

    let v = serde_json::to_value(&c).unwrap();
    assert!(
        v.get("value").is_none(),
        "value must be absent from JSON when None"
    );
    assert!(
        v.get("unit").is_none(),
        "unit must be absent from JSON when None"
    );
    assert!(
        v.get("target_lower").is_none(),
        "target_lower must be absent from JSON when None"
    );
    assert!(
        v.get("target_upper").is_none(),
        "target_upper must be absent from JSON when None"
    );
    // Required fields must still be present
    assert_eq!(v["name"], json!("stress_limit"));
    assert_eq!(v["satisfied"], json!(false));
}

// --- NaN-sentinel serialization contract test (Task 3648 amendment, suggestion 4) ---

/// Pins the wire-level JSON serialization of the NaN sentinel for
/// `AutoResolveParameterValue` on the real Tauri emit path.
///
/// **Production wire path:**
/// `emit_typed(&self.app, "auto-resolve-iteration", &iter)` (main.rs)
/// → `tauri::AppHandle::emit` (event_bus.rs)
/// → `serde_json::to_string(&iter)` (Tauri's internal serialization step).
/// The `AutoResolveIteration` payload — with the NaN-bearing
/// `AutoResolveParameterValue` nested under `parameters: HashMap<String, _>`
/// — is the literal bytes Tauri puts on the wire.
///
/// `serde_json::to_string` (like `to_value`) maps `f64::NAN` to JSON `null`.
/// This test approximates the un-unit-testable real `AppHandle::emit` by
/// calling `serde_json::to_string` directly; no live Tauri `AppHandle` or
/// webview is available in this unit-test module.
///
/// **Frontend contract (action required in gui/src/types.ts — out of scope for this task):**
/// The TypeScript `AutoResolveParameterValue` interface must type `value` as
/// `number | null` (not `number`) so the auto-resolve panel can render an error chip
/// when `value === null` rather than displaying `NaN` or crashing.
///
/// This test pins the current `serde_json::to_string` wire format so any future
/// serde_json upgrade that changes NaN handling (e.g. from null to an error) will
/// be caught immediately.
#[test]
fn auto_resolve_parameter_value_nan_sentinel_serializes_value_field_as_null() {
    use crate::types::{AutoResolveIteration, AutoResolveParameterValue};
    use std::collections::HashMap;

    let param = AutoResolveParameterValue {
        value: f64::NAN,
        unit: String::new(),
        display: "<non-scalar>".to_string(),
    };

    // Legacy: serde_json::to_value shares the same underlying f64 serializer — also maps NaN → null.
    assert!(serde_json::to_value(&param).unwrap()["value"].is_null());

    // Wire path: serde_json::to_string of the full AutoResolveIteration payload —
    // the faithful production proxy.
    // emit_typed(&app, "auto-resolve-iteration", &iter) → tauri::AppHandle::emit →
    // serde_json::to_string(&iter) is the actual Tauri wire path for the emitted event.
    // The NaN-bearing AutoResolveParameterValue is nested under parameters: HashMap<String, _>.
    let param_key = "Bracket.thickness";
    let mut parameters = HashMap::new();
    parameters.insert(param_key.to_string(), param);

    let iter = AutoResolveIteration {
        iteration: 0,
        parameters,
        constraints: HashMap::new(),
        driving_metric: None,
        driving_metric_value: None,
    };

    let wire = serde_json::to_string(&iter).expect(
        "AutoResolveIteration with NaN value must serialize without error via the Tauri to_string wire path",
    );
    let v: serde_json::Value = serde_json::from_str(&wire).expect("wire JSON must reparse");

    assert_eq!(v["iteration"], json!(0), "iteration must be 0");
    assert_eq!(
        v["parameters"][param_key]["value"],
        serde_json::Value::Null,
        "NaN sentinel nested in AutoResolveIteration must serialize as JSON null on the to_string wire path"
    );
    assert_eq!(
        v["parameters"][param_key]["unit"],
        json!(""),
        "unit must be empty string in nested payload"
    );
    assert_eq!(
        v["parameters"][param_key]["display"],
        json!("<non-scalar>"),
        "display must be '<non-scalar>' in nested payload"
    );
}

/// Positive case: a correct-length scalar_channels entry serializes successfully.
#[test]
fn meshdata_accepts_matching_scalar_channel_length() {
    use std::collections::HashMap;

    let mut channels = HashMap::new();
    channels.insert("vonMises".to_string(), vec![10.0_f32, 20.0, 30.0]); // 3 values = 3 vertices

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 3 vertices
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: channels,
        displaced_positions: None,
    };

    let v = serde_json::to_value(&mesh).expect("should serialize successfully");
    assert_eq!(
        v["scalar_channels"]["vonMises"].as_array().unwrap().len(),
        3,
        "vonMises must have 3 elements"
    );
}
