use std::sync::Arc;

use serde_json::json;

use crate::types::*;
use reify_core::{DimensionVector, Type};
use reify_ir::{DeterminacyState, FieldSourceKind, Value};

#[test]
fn gui_state_empty_serializes_with_expected_keys() {
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
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
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
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
        body: Box::new(reify_ir::CompiledExpr::literal(Value::Undef, Type::dimensionless_scalar())),
        captures: reify_ir::ValueMap::default(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        default_visible: true,
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
    use reify_ir::{ErrorRef, Freshness, ResultRef};

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
    use crate::types::{JointBinding, JointDescriptor, MechanismDescriptor};

    let joint = JointDescriptor {
        joint_index: 0,
        kind: "prismatic".to_string(),
        dimension: "length".to_string(),
        range_lower_si: Some(0.0),
        range_upper_si: Some(1.0),
        axis: Some([1.0, 0.0, 0.0]),
        driving_param_cell_id: Some("Kinematic.y_pos".to_string()),
        current_value_si: Some(0.5),
        binding: JointBinding::ParamBound {
            param_cell_id: "Kinematic.y_pos".to_string(),
            current_value_si: Some(0.5),
        },
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
    use crate::types::{JointBinding, JointDescriptor};

    let joint = JointDescriptor {
        joint_index: 2,
        kind: "fixed".to_string(),
        dimension: "dimensionless".to_string(),
        range_lower_si: None,
        range_upper_si: None,
        axis: None,
        driving_param_cell_id: None,
        current_value_si: None,
        binding: JointBinding::FixedNoMotion,
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };

    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite"),
        "expected 'non-finite' in error message: {msg}"
    );
}

// --- MeshData new-shell-field backward-compat tests (Task 3597 step-3) ---

/// When the three new shell-extract fields have their default values
/// (`element_kind: None`, `region_tags: None`, `vector_channels: HashMap::new()`),
/// they must all be absent from the JSON wire format.
///
/// This pins the manual Serialize impl's `is_some()`/`is_empty()` skip-if
/// discipline so that a future refactor that accidentally serializes a field
/// unconditionally is caught immediately.
#[test]
fn mesh_data_omits_new_shell_fields_when_default() {
    let mesh = MeshData {
        entity_path: "Bracket.body".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };
    let v = serde_json::to_value(&mesh).expect("serialize should succeed");
    assert!(
        v.get("element_kind").is_none(),
        "element_kind: None must be omitted from the wire"
    );
    assert!(
        v.get("region_tags").is_none(),
        "region_tags: None must be omitted from the wire"
    );
    assert!(
        v.get("vector_channels").is_none(),
        "empty vector_channels must be omitted from the wire"
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
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

// --- MeshData new-shell-field length-contract tests (Task 3597 steps 5–10) ---

/// `element_kind`, when `Some`, must have exactly `face_count == indices.len() / 3`
/// elements.  A length mismatch must produce `Err` with "element_kind" and
/// face-count information in the message.
///
/// Pins the length-contract enforcement added in step-6.
#[test]
fn meshdata_rejects_element_kind_with_wrong_length() {
    // 3 vertices, 2 faces (6 indices) → face_count = 2
    // element_kind has only 1 element → length mismatch
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2, 0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: Some(vec![0u8]), // length 1, face_count = 2 → mismatch
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("element_kind"),
        "expected 'element_kind' in error message: {msg}"
    );
    // Error message must contain face-count context
    assert!(
        msg.contains("face count"),
        "expected 'face count' in error message: {msg}"
    );
}

/// `region_tags`, when `Some`, must have exactly `face_count == indices.len() / 3`
/// elements.  A length mismatch must produce `Err` with "region_tags" and
/// face-count information in the message.
///
/// Pins the length-contract enforcement added in step-8.
#[test]
fn meshdata_rejects_region_tags_with_wrong_length() {
    // 3 vertices, 2 faces (6 indices) → face_count = 2
    // region_tags has only 1 element → length mismatch
    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2, 0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: Some(vec![100u32]), // length 1, face_count = 2 → mismatch
        vector_channels: std::collections::HashMap::new(),
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("region_tags"),
        "expected 'region_tags' in error message: {msg}"
    );
    // Error message must contain face-count context
    assert!(
        msg.contains("face count"),
        "expected 'face count' in error message: {msg}"
    );
}

/// A NaN value in a `vector_channels` entry must produce `Err` containing both
/// `"non-finite f32 value"` and the channel key — mirroring the existing
/// `scalar_channels` finite-value guard.  The error must also say
/// `"vector channel"` (not `"scalar channel"`) so operators can locate the
/// offending field without inspecting a stack trace.
///
/// Setup: 1 vertex (vertex_count=1), 0 faces (face_count=0).
/// Per-vertex length = 3*1 = 3 → satisfies the length contract.
/// The NaN at position 0 triggers the FiniteF32MapRef guard (step-12).
#[test]
fn vector_channels_nan_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut vc = HashMap::new();
    vc.insert("shell_normal".to_string(), vec![f32::NAN, 0.0, 0.0]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0], // 1 vertex
        indices: vec![],                // 0 faces; per-vertex len=3 satisfies contract
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in error message: {msg}"
    );
    assert!(
        msg.contains("shell_normal"),
        "expected channel key 'shell_normal' in error message: {msg}"
    );
    assert!(
        msg.contains("vector channel"),
        "expected 'vector channel' (not 'scalar channel') in error message: {msg}"
    );
}

/// Same as above for f32::INFINITY.  Also pins the `"vector channel"` label
/// (not `"scalar channel"`) in the error message.
#[test]
fn vector_channels_infinity_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut vc = HashMap::new();
    vc.insert("shell_normal".to_string(), vec![f32::INFINITY, 0.0, 0.0]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in error message: {msg}"
    );
    assert!(
        msg.contains("shell_normal"),
        "expected channel key 'shell_normal' in error message: {msg}"
    );
    assert!(
        msg.contains("vector channel"),
        "expected 'vector channel' (not 'scalar channel') in error message: {msg}"
    );
}

/// Same as above for f32::NEG_INFINITY.  Also pins the `"vector channel"` label
/// (not `"scalar channel"`) in the error message.
#[test]
fn vector_channels_neg_infinity_causes_error_with_channel_key() {
    use std::collections::HashMap;

    let mut vc = HashMap::new();
    vc.insert("shell_normal".to_string(), vec![f32::NEG_INFINITY, 0.0, 0.0]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0],
        indices: vec![],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("non-finite f32 value"),
        "expected 'non-finite f32 value' in error message: {msg}"
    );
    assert!(
        msg.contains("shell_normal"),
        "expected channel key 'shell_normal' in error message: {msg}"
    );
    assert!(
        msg.contains("vector channel"),
        "expected 'vector channel' (not 'scalar channel') in error message: {msg}"
    );
}

/// A per-vertex `vector_channels` entry (no `_per_face` suffix) must have
/// length `3 * vertex_count`.  Any other length must produce `Err` containing
/// the channel name and the vertex-count context.
///
/// Setup: 3 vertices (vertex_count=3), 1 face (face_count=1).
/// `"shell_normal"` has no `_per_face` suffix → required length = 9 (3*3).
/// An entry of length 2 is invalid.
///
/// Pins the suffix-based enforcement in the Serialize impl (step-10 +
/// amendment 2: enforce `_per_face` naming convention).
#[test]
fn meshdata_rejects_vector_channel_with_invalid_length() {
    use std::collections::HashMap;

    let mut vc = HashMap::new();
    // length 2: not 9 (per-vertex) and not 3 (per-face)
    vc.insert("shell_normal".to_string(), vec![1.0f32, 0.0]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 3 vertices
        indices: vec![0, 1, 2],                                          // 1 face
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("shell_normal"),
        "expected channel name 'shell_normal' in error message: {msg}"
    );
    // Must mention vertex-count context.  The new suffix-based enforcement
    // produces "expected length 9 (3*vertex_count) but got 2", so both
    // "vertex" and "9" appear.
    assert!(
        msg.contains("vertex") || msg.contains("9"),
        "expected vertex-count context in error message: {msg}"
    );
}

/// A `vector_channels` entry whose name ends in `_per_face` must have length
/// `3 * face_count`.  Using a per-vertex-sized length (3*vertex_count) when
/// `vertex_count ≠ face_count` must produce `Err` mentioning both the channel
/// name and the `_per_face` convention.
///
/// Setup: 3 vertices (vertex_count=3), 1 face (face_count=1).
/// `"data_per_face"` requires length 3 (3*face_count), but the inserted
/// slice has length 9 (3*vertex_count).
///
/// Pins the `_per_face`-suffix enforcement introduced in amendment 2
/// (reviewer suggestion 2).
#[test]
fn vector_channels_per_face_suffix_enforces_face_count_length() {
    use std::collections::HashMap;

    let mut vc = HashMap::new();
    // length 9 = 3*vertex_count; _per_face requires 3*face_count = 3
    vc.insert("data_per_face".to_string(), vec![0.0f32; 9]);

    let mesh = MeshData {
        entity_path: "test".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // 3 vertices
        indices: vec![0, 1, 2],                                          // 1 face
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let err = serde_json::to_value(&mesh).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("data_per_face"),
        "expected channel name 'data_per_face' in error message: {msg}"
    );
    assert!(
        msg.contains("_per_face"),
        "expected '_per_face' convention mention in error message: {msg}"
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
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
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

    // See doc comment above for production wire path.
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

// --- MeshData new shell-extract fields (Task 3597) ---

/// Serializing a MeshData with `element_kind: Some(vec![0, 1])` must produce
/// a JSON field `element_kind` containing the byte values `[0, 1]`.
///
/// Fails to compile until `element_kind: Option<Vec<u8>>` is added to `MeshData`.
#[test]
fn mesh_data_element_kind_some_serializes_with_field() {
    // 3 vertices, 2 faces (6 indices) → face_count = 2
    let mesh = MeshData {
        entity_path: "Bracket.shell".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2, 0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: Some(vec![0u8, 1u8]),
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };
    let v = serde_json::to_value(&mesh).unwrap();
    let ek = v.get("element_kind").expect("element_kind must be present in JSON");
    assert!(ek.is_array(), "element_kind must serialize as a JSON array");
    let arr = ek.as_array().unwrap();
    assert_eq!(arr.len(), 2, "element_kind array must have 2 elements");
    assert_eq!(arr[0], serde_json::json!(0), "element_kind[0] must be 0");
    assert_eq!(arr[1], serde_json::json!(1), "element_kind[1] must be 1");
}

/// Serializing a MeshData with `region_tags: Some(vec![100, 200])` must produce
/// a JSON field `region_tags` containing the u32 values `[100, 200]`.
///
/// Fails to compile until `region_tags: Option<Vec<u32>>` is added to `MeshData`.
#[test]
fn mesh_data_region_tags_some_serializes_with_field() {
    // 3 vertices, 2 faces (6 indices) → face_count = 2
    let mesh = MeshData {
        entity_path: "Bracket.shell".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2, 0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: Some(vec![100u32, 200u32]),
        vector_channels: std::collections::HashMap::new(),
    };
    let v = serde_json::to_value(&mesh).unwrap();
    let rt = v.get("region_tags").expect("region_tags must be present in JSON");
    assert!(rt.is_array(), "region_tags must serialize as a JSON array");
    let arr = rt.as_array().unwrap();
    assert_eq!(arr.len(), 2, "region_tags array must have 2 elements");
    assert_eq!(arr[0], serde_json::json!(100), "region_tags[0] must be 100");
    assert_eq!(arr[1], serde_json::json!(200), "region_tags[1] must be 200");
}

/// Serializing a MeshData with a populated `vector_channels` entry must produce
/// a JSON object `vector_channels` containing the channel key and its float values.
///
/// Fails to compile until `vector_channels: HashMap<String, Vec<f32>>` is added to `MeshData`.
#[test]
fn mesh_data_vector_channels_populated_serializes_with_field() {
    // 3 vertices, 2 faces (6 indices) → per-face length = 3*2 = 6
    let mut vc = std::collections::HashMap::new();
    vc.insert(
        "shell_normal_per_face".to_string(),
        vec![0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0],
    );
    let mesh = MeshData {
        entity_path: "Bracket.shell".to_string(),
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2, 0, 1, 2],
        normals: None,
        scalar_channels: std::collections::HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        vector_channels: vc,
    };
    let v = serde_json::to_value(&mesh).unwrap();
    let vc_json = v.get("vector_channels").expect("vector_channels must be present in JSON");
    assert!(vc_json.is_object(), "vector_channels must serialize as a JSON object");
    let ch = vc_json.get("shell_normal_per_face")
        .expect("shell_normal_per_face key must be present");
    assert!(ch.is_array(), "vector channel must serialize as a JSON array");
    assert_eq!(
        ch.as_array().unwrap().len(),
        6,
        "shell_normal_per_face must have 6 elements (3 * face_count=2)"
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
        element_kind: None,
        region_tags: None,
        vector_channels: std::collections::HashMap::new(),
    };

    let v = serde_json::to_value(&mesh).expect("should serialize successfully");
    assert_eq!(
        v["scalar_channels"]["vonMises"].as_array().unwrap().len(),
        3,
        "vonMises must have 3 elements"
    );
}

// ── Task 3541 step-7: WarmPoolEvent IPC struct serialization ─────────────────

/// Pin the PRD §2.2 / PRD §3.2 wire format for the `warm-pool-event` channel.
///
/// Exact JSON shape: `{"kind":"evicted"|"donated","size_bytes":<u64>,"node_id":<string>}`.
/// No `serde(rename_all)` — field names match the TS interface exactly.
///
/// Mirrors `auto_resolve_iteration_serializes_with_expected_field_set` (line 1662).
#[test]
fn warm_pool_event_serializes_with_expected_field_set() {
    use crate::types::WarmPoolEvent;
    use serde_json::json;

    // (a) Evicted variant serializes to the expected flat shape.
    let evicted = WarmPoolEvent {
        kind: "evicted".to_string(),
        size_bytes: 1024,
        node_id: "Body.thickness".to_string(),
    };
    let v = serde_json::to_value(&evicted).expect("WarmPoolEvent must serialize");
    assert_eq!(v, json!({"kind": "evicted", "size_bytes": 1024, "node_id": "Body.thickness"}),
        "evicted: exact JSON shape mismatch");
    // Top-level key set must be exactly {kind, size_bytes, node_id} — no extras.
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 3, "evicted must have exactly 3 top-level keys, got {:?}", obj.keys().collect::<Vec<_>>());

    // (b) Donated variant serializes identically (only kind differs).
    let donated = WarmPoolEvent {
        kind: "donated".to_string(),
        size_bytes: 4096,
        node_id: "Plate.width".to_string(),
    };
    let v2 = serde_json::to_value(&donated).expect("WarmPoolEvent must serialize");
    assert_eq!(v2, json!({"kind": "donated", "size_bytes": 4096, "node_id": "Plate.width"}),
        "donated: exact JSON shape mismatch");

    // (c) Round-trip via serde_json::from_value preserves all fields.
    let rt: WarmPoolEvent = serde_json::from_value(v2.clone()).expect("must deserialize");
    assert_eq!(rt.kind, "donated");
    assert_eq!(rt.size_bytes, 4096);
    assert_eq!(rt.node_id, "Plate.width");

    // (d) from_engine_event: WarmPoolEvent::Evicted maps to kind="evicted", correct size_bytes
    //     and node_id stringified via NodeId Display.
    use reify_eval::cache::NodeId;
    use reify_eval::warm_pool::WarmPoolEvent as EngineWarmPoolEvent;
    use reify_core::ValueCellId;

    let victim = NodeId::Value(ValueCellId::new("Body", "thickness"));
    let eng_ev = EngineWarmPoolEvent::Evicted {
        node_id: victim,
        size_bytes: 512,
    };
    let ipc = WarmPoolEvent::from_engine_event(&eng_ev);
    assert_eq!(ipc.kind, "evicted");
    assert_eq!(ipc.size_bytes, 512u64);
    // node_id is NodeId::Display — must be non-empty and contain "thickness"
    assert!(!ipc.node_id.is_empty(), "node_id must not be empty");
    assert!(ipc.node_id.contains("thickness"), "node_id must contain 'thickness', got: {}", ipc.node_id);

    // (e) from_engine_event: WarmPoolEvent::Donated maps to kind="donated".
    let donor = NodeId::Value(ValueCellId::new("Plate", "width"));
    let eng_donated = EngineWarmPoolEvent::Donated {
        node_id: donor,
        size_bytes: 8192,
    };
    let ipc2 = WarmPoolEvent::from_engine_event(&eng_donated);
    assert_eq!(ipc2.kind, "donated");
    assert_eq!(ipc2.size_bytes, 8192u64);
    assert!(ipc2.node_id.contains("width"), "node_id must contain 'width', got: {}", ipc2.node_id);
}

#[test]
fn fea_case_changed_serializes_to_expected_json_shape() {
    // Pins PRD §3.2 field-name-exactness: no rename_all, field names match TS exactly.
    let payload = crate::types::FeaCaseChanged {
        active_case_id: "operating".into(),
        available_cases: vec![
            "operating".into(),
            "overload".into(),
            "transport".into(),
        ],
    };
    let v = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "active_case_id": "operating",
            "available_cases": ["operating", "overload", "transport"]
        })
    );
}

// ---- JointDescriptor.binding field IPC round-trip tests (task 3783, step-3) --

/// The new `binding: JointBinding` field on `JointDescriptor` serializes and
/// deserializes correctly, with the nested `kind` discriminator visible in the
/// JSON wire format.
#[test]
fn joint_descriptor_binding_field_round_trips_via_serde() {
    use crate::types::{JointBinding, JointDescriptor};

    let joint = JointDescriptor {
        joint_index: 1,
        kind: "prismatic".to_string(),
        dimension: "length".to_string(),
        range_lower_si: Some(0.0),
        range_upper_si: Some(0.8),
        axis: Some([1.0, 0.0, 0.0]),
        driving_param_cell_id: None,
        current_value_si: None,
        binding: JointBinding::LiteralBound {
            synth_param_name: "__joint_y_axis_v".to_string(),
            initial_value_si: Some(0.05),
            scrubbable: true,
        },
    };

    let v = serde_json::to_value(&joint).expect("serialize JointDescriptor with binding");

    // The `binding` key must be present and contain the kind discriminator.
    assert!(
        v.get("binding").is_some(),
        "expected 'binding' key in JointDescriptor wire format"
    );
    assert_eq!(
        v["binding"]["kind"], "literal_bound",
        "binding.kind must be 'literal_bound'; got {:?}",
        v["binding"]["kind"]
    );
    assert_eq!(
        v["binding"]["synth_param_name"], "__joint_y_axis_v",
        "binding.synth_param_name must be '__joint_y_axis_v'; got {:?}",
        v["binding"]["synth_param_name"]
    );
    assert_eq!(
        v["binding"]["initial_value_si"], 0.05,
        "binding.initial_value_si must be 0.05; got {:?}",
        v["binding"]["initial_value_si"]
    );
    assert_eq!(
        v["binding"]["scrubbable"], true,
        "binding.scrubbable must be true; got {:?}",
        v["binding"]["scrubbable"]
    );

    // Round-trip: must restore the full descriptor including the binding.
    let back: JointDescriptor = serde_json::from_value(v).expect("deserialize JointDescriptor");
    assert_eq!(
        back, joint,
        "JointDescriptor must round-trip through serde without data loss"
    );
}

// ---- JointBinding enum IPC contract tests (task 3783, step-1) ----------------

/// Compile-time IPC contract for `JointBinding`: Serialize + DeserializeOwned +
/// Clone + Debug + PartialEq.
#[test]
fn joint_binding_ipc_contract() {
    use super::assert_ipc_contract;
    use crate::types::JointBinding;
    assert_ipc_contract::<JointBinding>();
}

/// `JointBinding::ParamBound` round-trips through `serde_json::to_value` /
/// `from_value` and serializes with `kind: "param_bound"` plus the expected
/// payload keys.
#[test]
fn joint_binding_param_bound_round_trips() {
    use crate::types::JointBinding;

    let binding = JointBinding::ParamBound {
        param_cell_id: "Kinematic.y_pos".to_string(),
        current_value_si: Some(0.1),
    };
    let v = serde_json::to_value(&binding).expect("serialize ParamBound");

    assert_eq!(
        v["kind"], "param_bound",
        "ParamBound must serialize with kind=\"param_bound\"; got {:?}",
        v["kind"]
    );
    assert!(
        v.get("param_cell_id").is_some(),
        "expected 'param_cell_id' key in ParamBound"
    );
    assert!(
        v.get("current_value_si").is_some(),
        "expected 'current_value_si' key in ParamBound"
    );

    let back: JointBinding = serde_json::from_value(v).expect("deserialize ParamBound");
    assert_eq!(back, binding, "ParamBound must round-trip without data loss");
}

/// `JointBinding::LiteralBound` round-trips through `serde_json::to_value` /
/// `from_value` and serializes with `kind: "literal_bound"` plus the expected
/// payload keys.
#[test]
fn joint_binding_literal_bound_round_trips() {
    use crate::types::JointBinding;

    let binding = JointBinding::LiteralBound {
        synth_param_name: "__joint_y_axis_v".to_string(),
        initial_value_si: Some(0.05),
        scrubbable: true,
    };
    let v = serde_json::to_value(&binding).expect("serialize LiteralBound");

    assert_eq!(
        v["kind"], "literal_bound",
        "LiteralBound must serialize with kind=\"literal_bound\"; got {:?}",
        v["kind"]
    );
    assert!(
        v.get("synth_param_name").is_some(),
        "expected 'synth_param_name' key in LiteralBound"
    );
    assert!(
        v.get("initial_value_si").is_some(),
        "expected 'initial_value_si' key in LiteralBound"
    );
    assert!(
        v.get("scrubbable").is_some(),
        "expected 'scrubbable' key in LiteralBound"
    );

    let back: JointBinding = serde_json::from_value(v).expect("deserialize LiteralBound");
    assert_eq!(
        back, binding,
        "LiteralBound must round-trip without data loss"
    );
}

/// `JointBinding::CouplingDerived` round-trips through `serde_json::to_value` /
/// `from_value` and serializes with `kind: "coupling_derived"`.
#[test]
fn joint_binding_coupling_derived_round_trips() {
    use crate::types::JointBinding;

    let binding = JointBinding::CouplingDerived {
        source_joint: "j_drive".to_string(),
    };
    let v = serde_json::to_value(&binding).expect("serialize CouplingDerived");

    assert_eq!(
        v["kind"], "coupling_derived",
        "CouplingDerived must serialize with kind=\"coupling_derived\"; got {:?}",
        v["kind"]
    );
    assert!(
        v.get("source_joint").is_some(),
        "expected 'source_joint' key in CouplingDerived"
    );

    let back: JointBinding = serde_json::from_value(v).expect("deserialize CouplingDerived");
    assert_eq!(
        back, binding,
        "CouplingDerived must round-trip without data loss"
    );
}

/// `JointBinding::FixedNoMotion` round-trips through `serde_json::to_value` /
/// `from_value` as the unit variant `{"kind": "fixed_no_motion"}`.
#[test]
fn joint_binding_fixed_no_motion_round_trips() {
    use crate::types::JointBinding;

    let binding = JointBinding::FixedNoMotion;
    let v = serde_json::to_value(&binding).expect("serialize FixedNoMotion");

    assert_eq!(
        v["kind"], "fixed_no_motion",
        "FixedNoMotion must serialize as {{\"kind\": \"fixed_no_motion\"}}; got {:?}",
        v
    );
    // Unit variant: only the `kind` discriminator key should be present.
    assert_eq!(
        v.as_object().map(|m| m.len()),
        Some(1),
        "FixedNoMotion wire must be exactly {{\"kind\": \"fixed_no_motion\"}} (one key); got {:?}",
        v
    );

    let back: JointBinding = serde_json::from_value(v).expect("deserialize FixedNoMotion");
    assert_eq!(
        back, binding,
        "FixedNoMotion must round-trip without data loss"
    );
}

// ── T0b: TensegrityWireData serde wire-shape tests ────────────────────────────

/// `TensegrityWireData` must serialize to JSON with snake_case keys:
/// `entity_path`, `kind`, `x1`, `y1`, `z1`, `x2`, `y2`, `z2` — all present and
/// typed correctly (string for entity_path/kind, number for the six coords).
///
/// RED until `TensegrityWireData` is added to `types.rs`.
#[test]
fn tensegrity_wire_data_serializes_with_expected_keys() {
    let wire = TensegrityWireData {
        entity_path: "TPrism".to_string(),
        kind: "strut".to_string(),
        x1: 1.0,
        y1: 0.0,
        z1: 1.0,
        x2: 0.866,
        y2: 0.5,
        z2: 0.0,
    };
    let v = serde_json::to_value(&wire).unwrap();
    assert_eq!(v["entity_path"], json!("TPrism"), "entity_path must be 'TPrism'");
    assert_eq!(v["kind"], json!("strut"), "kind must be 'strut'");
    assert_eq!(v["x1"], json!(1.0), "x1 must be 1.0");
    assert_eq!(v["y1"], json!(0.0), "y1 must be 0.0");
    assert_eq!(v["z1"], json!(1.0), "z1 must be 1.0");
    assert_eq!(v["x2"], json!(0.866), "x2 must be 0.866");
    assert_eq!(v["y2"], json!(0.5), "y2 must be 0.5");
    assert_eq!(v["z2"], json!(0.0), "z2 must be 0.0");
}

/// `GuiState.tensegrity_wires` must serialize as a JSON array.
/// - Non-empty case: array length and element kind preserved.
/// - Empty case: empty array (not absent).
///
/// RED until `GuiState.tensegrity_wires` field is added.
#[test]
fn gui_state_tensegrity_wires_serializes_as_array() {
    // Non-empty case
    let wire = TensegrityWireData {
        entity_path: "TPrism".to_string(),
        kind: "cable".to_string(),
        x1: 1.0, y1: 0.0, z1: 1.0,
        x2: -0.5, y2: 0.866, z2: 1.0,
    };
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![wire],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(
        v.get("tensegrity_wires").unwrap().is_array(),
        "tensegrity_wires must serialize as a JSON array"
    );
    assert_eq!(
        v["tensegrity_wires"].as_array().unwrap().len(),
        1,
        "non-empty tensegrity_wires must have 1 element"
    );
    assert_eq!(
        v["tensegrity_wires"][0]["kind"],
        json!("cable"),
        "wire kind must be 'cable'"
    );

    // Empty case — must serialize as [] not be absent
    let empty_state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
    };
    let ev = serde_json::to_value(&empty_state).unwrap();
    assert!(
        ev.get("tensegrity_wires").unwrap().is_array(),
        "empty tensegrity_wires must still serialize as a JSON array (not be absent)"
    );
    assert_eq!(
        ev["tensegrity_wires"].as_array().unwrap().len(),
        0,
        "empty tensegrity_wires must be an empty array"
    );
}

// ── GR-016 ζ step-5: SolverProgress IPC struct serialization ─────────────────

/// Pin the PRD §2.2 / PRD §3.2 wire format for the `solver-progress` event channel.
///
/// Three sub-cases:
/// (a) Full payload (eta_ms present) — exact JSON shape + 4-key assertion.
/// (b) eta_ms-None payload — exact JSON shape + 3-key assertion (pins
///     `skip_serializing_if = "Option::is_none"` directive).
/// (c) Round-trip from the 3-key wire shape preserves None for eta_ms.
///
/// No `serde(rename_all)` — field names match the TS interface exactly
/// (PRD §3.2 field-name-exactness convention).
///
/// Mirrors `warm_pool_event_serializes_with_expected_field_set` (line 2041).
#[test]
fn solver_progress_serializes_to_expected_json_shape() {
    use crate::types::SolverProgress;
    use serde_json::json;

    // (a) Full payload: eta_ms present → 4 top-level keys.
    let full = SolverProgress {
        solver_kind: "cg".to_string(),
        iter: 7,
        residual: 1.234e-6_f64,
        eta_ms: Some(2500),
    };
    let v = serde_json::to_value(&full).expect("SolverProgress (full) must serialize");
    assert_eq!(
        v,
        json!({
            "solver_kind": "cg",
            "iter": 7,
            "residual": 1.234e-6_f64,
            "eta_ms": 2500
        }),
        "full payload: exact JSON shape mismatch"
    );
    let obj = v.as_object().unwrap();
    assert_eq!(
        obj.len(),
        4,
        "full payload must have exactly 4 top-level keys; got {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // (b) eta_ms-None payload → 3 top-level keys (skip_serializing_if omits it).
    let no_eta = SolverProgress {
        solver_kind: "cg".to_string(),
        iter: 3,
        residual: 5.0e-4_f64,
        eta_ms: None,
    };
    let v2 = serde_json::to_value(&no_eta).expect("SolverProgress (no eta_ms) must serialize");
    assert_eq!(
        v2,
        json!({
            "solver_kind": "cg",
            "iter": 3,
            "residual": 5.0e-4_f64
        }),
        "no-eta payload: exact JSON shape mismatch (eta_ms must be absent)"
    );
    let obj2 = v2.as_object().unwrap();
    assert_eq!(
        obj2.len(),
        3,
        "no-eta payload must have exactly 3 top-level keys; got {:?}",
        obj2.keys().collect::<Vec<_>>()
    );

    // (c) Round-trip from the 3-key wire shape (no eta_ms) preserves None.
    let rt: SolverProgress = serde_json::from_value(v2.clone()).expect("must deserialize 3-key shape");
    assert_eq!(rt.solver_kind, "cg");
    assert_eq!(rt.iter, 3);
    assert!((rt.residual - 5.0e-4_f64).abs() < f64::EPSILON * 100.0,
        "residual round-trip mismatch: {} vs {}", rt.residual, 5.0e-4_f64);
    assert_eq!(rt.eta_ms, None, "eta_ms must be None when absent from wire shape");
}

// ── task-3458 step-3: ModeShapeFrame IPC struct serde round-trip ─────────────

/// Pin the wire format for the `mode-shape-frame` Tauri event channel (GR-024 Phase 9).
///
/// Three assertions:
/// (a) Serialize→deserialize identity: a `ModeShapeFrame` value must round-trip
///     without data loss.
/// (b) Exact JSON key names — `"mode_index"`, `"phase"`, `"displaced_positions"` —
///     matching the TypeScript `ModeShapeFrame` interface verbatim.
///     No `serde(rename_all)` on the struct (field names cross the wire unchanged).
/// (c) Exactly 3 top-level keys in the serialized object (no hidden fields).
///
/// Mirrors `solver_progress_serializes_to_expected_json_shape` (line 2433).
///
/// **RED at step-3**: fails until `ModeShapeFrame` is added to `types.rs` in step-4.
#[test]
fn mode_shape_frame_serde_round_trip_with_exact_key_names() {
    use crate::types::ModeShapeFrame;
    use serde_json::json;

    let frame = ModeShapeFrame {
        mode_index: 2,
        phase: 0.75_f32,
        displaced_positions: vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0],
        eigenvalue: None, // base-frame case; None must be omitted from wire (step-1)
    };

    // (a) Serialize.
    let v = serde_json::to_value(&frame)
        .expect("ModeShapeFrame must serialize without error");

    // (b) Exact JSON key names (no rename_all).
    assert_eq!(
        v,
        json!({
            "mode_index": 2_u8,
            "phase": 0.75_f32,
            "displaced_positions": [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0]
        }),
        "ModeShapeFrame JSON shape mismatch — check field names match TS interface"
    );

    // (c) Exactly 3 top-level keys.
    let obj = v.as_object().unwrap();
    assert_eq!(
        obj.len(),
        3,
        "ModeShapeFrame must have exactly 3 top-level JSON keys; got {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // Confirm each key is present with the exact name.
    assert!(obj.contains_key("mode_index"),          "key 'mode_index' must be present");
    assert!(obj.contains_key("phase"),               "key 'phase' must be present");
    assert!(obj.contains_key("displaced_positions"), "key 'displaced_positions' must be present");

    // (d) Deserialize back → identity.
    let rt: ModeShapeFrame = serde_json::from_value(v.clone())
        .expect("ModeShapeFrame must deserialize from its own JSON");
    assert_eq!(rt.mode_index, 2, "mode_index must survive round-trip");
    assert!((rt.phase - 0.75_f32).abs() < f32::EPSILON * 10.0,
        "phase round-trip mismatch: {} vs 0.75", rt.phase);
    assert_eq!(
        rt.displaced_positions,
        vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0],
        "displaced_positions must survive round-trip"
    );
}

// ── task-4072 step-1: ModeShapeFrame eigenvalue field serde contract ──────────

/// (a) PEAK frame: `eigenvalue: Some(1234.5)` must serialize with the
/// `"eigenvalue"` key present and the value must be exactly 4 top-level keys.
/// Also round-trips back to an equal struct.
///
/// **RED at step-1**: compile-fails because `ModeShapeFrame` has no `eigenvalue`
/// field yet. GREEN after step-2 adds the field to `types.rs`.
#[test]
fn mode_shape_frame_peak_eigenvalue_serializes_with_4_keys() {
    use crate::types::ModeShapeFrame;
    use serde_json::json;

    let frame = ModeShapeFrame {
        mode_index: 1,
        phase: 1.0_f32,
        displaced_positions: vec![0.1_f32, 0.0, 0.0],
        eigenvalue: Some(1234.5_f64),
    };

    let v = serde_json::to_value(&frame)
        .expect("ModeShapeFrame (peak) must serialize without error");

    // "eigenvalue" key present with correct value.
    assert_eq!(
        v["eigenvalue"],
        json!(1234.5_f64),
        "eigenvalue must be serialized as a JSON number"
    );

    // Exactly 4 top-level keys (mode_index, phase, displaced_positions, eigenvalue).
    let obj = v.as_object().unwrap();
    assert_eq!(
        obj.len(),
        4,
        "peak ModeShapeFrame must have exactly 4 JSON keys; got {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // Round-trip.
    let rt: ModeShapeFrame = serde_json::from_value(v)
        .expect("peak ModeShapeFrame must deserialize from its own JSON");
    assert_eq!(rt.eigenvalue, Some(1234.5_f64), "eigenvalue must survive round-trip");
    assert_eq!(rt.mode_index, 1);
    assert!((rt.phase - 1.0_f32).abs() < f32::EPSILON);
}

/// (b) BASE frame: `eigenvalue: None` must serialize WITHOUT the `"eigenvalue"`
/// key (exactly 3 top-level keys) due to `#[serde(skip_serializing_if = "Option::is_none")]`.
/// Deserializing a 3-key payload back must yield `eigenvalue: None` via
/// `#[serde(default)]`.
///
/// **RED at step-1**: compile-fails for the same reason as (a).
#[test]
fn mode_shape_frame_base_eigenvalue_none_omits_key() {
    use crate::types::ModeShapeFrame;

    let frame = ModeShapeFrame {
        mode_index: 0,
        phase: 0.0_f32,
        displaced_positions: vec![0.0_f32, 0.0, 0.0],
        eigenvalue: None,
    };

    let v = serde_json::to_value(&frame)
        .expect("base ModeShapeFrame must serialize without error");

    // The "eigenvalue" key must be absent.
    let obj = v.as_object().unwrap();
    assert!(
        !obj.contains_key("eigenvalue"),
        "eigenvalue: None must be omitted from wire (skip_serializing_if); got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // Exactly 3 keys.
    assert_eq!(
        obj.len(),
        3,
        "base ModeShapeFrame must have exactly 3 JSON keys; got {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    // Round-trip: a 3-key JSON payload must deserialize with eigenvalue=None.
    let rt: ModeShapeFrame = serde_json::from_value(v)
        .expect("base ModeShapeFrame must deserialize from 3-key JSON");
    assert_eq!(rt.eigenvalue, None, "absent 'eigenvalue' key must deserialize to None");
}

// ── Tensegrity-β step-1: TensegritySurfaceData serde wire-shape tests ────────

/// `TensegritySurfaceData` must serialize to JSON with snake_case keys:
/// `entity_path`, `kind`, `i0`, `i1`, `i2`, `x0`, `y0`, `z0`, `x1`, `y1`,
/// `z1`, `x2`, `y2`, `z2` — all present and typed correctly (string for
/// entity_path/kind, integer for i0/i1/i2, number for the nine coords).
///
/// RED until `TensegritySurfaceData` is added to `types.rs`.
#[test]
fn tensegrity_surface_data_serializes_with_expected_keys() {
    let surface = TensegritySurfaceData {
        entity_path: "TPatch".to_string(),
        kind: "membrane".to_string(),
        i0: 0,
        i1: 1,
        i2: 2,
        x0: 0.0,
        y0: 0.0,
        z0: 0.0,
        x1: 1.0,
        y1: 0.0,
        z1: 0.0,
        x2: 0.5,
        y2: 0.866,
        z2: 0.0,
    };
    let v = serde_json::to_value(&surface).unwrap();
    assert_eq!(v["entity_path"], json!("TPatch"), "entity_path must be 'TPatch'");
    assert_eq!(v["kind"], json!("membrane"), "kind must be 'membrane'");
    assert_eq!(v["i0"], json!(0), "i0 must be 0");
    assert_eq!(v["i1"], json!(1), "i1 must be 1");
    assert_eq!(v["i2"], json!(2), "i2 must be 2");
    assert_eq!(v["x0"], json!(0.0), "x0 must be 0.0");
    assert_eq!(v["y0"], json!(0.0), "y0 must be 0.0");
    assert_eq!(v["z0"], json!(0.0), "z0 must be 0.0");
    assert_eq!(v["x1"], json!(1.0), "x1 must be 1.0");
    assert_eq!(v["y1"], json!(0.0), "y1 must be 0.0");
    assert_eq!(v["z1"], json!(0.0), "z1 must be 0.0");
    assert_eq!(v["x2"], json!(0.5), "x2 must be 0.5");
    assert_eq!(v["y2"], json!(0.866), "y2 must be 0.866");
    assert_eq!(v["z2"], json!(0.0), "z2 must be 0.0");
}

/// `GuiState.tensegrity_surfaces` must serialize as a JSON array.
/// - Non-empty case: array length and element kind preserved.
/// - Empty case: empty array (not absent).
///
/// RED until `GuiState.tensegrity_surfaces` field is added.
#[test]
fn gui_state_tensegrity_surfaces_serializes_as_array() {
    // Non-empty case
    let surface = TensegritySurfaceData {
        entity_path: "TPatch".to_string(),
        kind: "membrane".to_string(),
        i0: 0, i1: 1, i2: 2,
        x0: 0.0, y0: 0.0, z0: 0.0,
        x1: 1.0, y1: 0.0, z1: 0.0,
        x2: 0.5, y2: 0.866, z2: 0.0,
    };
    let state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![surface],
        demand_prune_measurement: None,
    };
    let v = serde_json::to_value(&state).unwrap();
    assert!(
        v.get("tensegrity_surfaces").unwrap().is_array(),
        "tensegrity_surfaces must serialize as a JSON array"
    );
    assert_eq!(
        v["tensegrity_surfaces"].as_array().unwrap().len(),
        1,
        "non-empty tensegrity_surfaces must have 1 element"
    );
    assert_eq!(
        v["tensegrity_surfaces"][0]["kind"],
        json!("membrane"),
        "surface kind must be 'membrane'"
    );

    // Empty case — must serialize as [] not be absent
    let empty_state = GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
    };
    let ev = serde_json::to_value(&empty_state).unwrap();
    assert!(
        ev.get("tensegrity_surfaces").unwrap().is_array(),
        "empty tensegrity_surfaces must still serialize as a JSON array (not be absent)"
    );
    assert_eq!(
        ev["tensegrity_surfaces"].as_array().unwrap().len(),
        0,
        "empty tensegrity_surfaces must be an empty array"
    );
}
