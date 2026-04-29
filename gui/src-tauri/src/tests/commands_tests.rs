use std::sync::{Arc, Mutex, RwLock};

use reify_constraints::SimpleConstraintChecker;
use reify_mcp::SelectionInfo;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::commands::AppState;
use crate::engine::EngineSession;

fn make_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    EngineSession::new(Box::new(checker), Some(Box::new(kernel)))
}

fn make_loaded_session() -> EngineSession {
    let mut session = make_session();
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    session
}

#[test]
fn app_state_constructible() {
    let session = make_loaded_session();
    let _state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo::default())),
    };
}

#[test]
fn app_state_selection_is_accessible() {
    let session = make_loaded_session();
    let state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo {
            selected_entity: Some("Bracket".to_string()),
            selected_entities: vec![],
            hovered_entity: None,
        })),
    };
    let sel = state.selection.read().unwrap();
    assert_eq!(sel.selected_entity, Some("Bracket".to_string()));
}

#[test]
fn app_state_selection_multi() {
    let session = make_loaded_session();
    let state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo {
            selected_entity: Some("A".to_string()),
            selected_entities: vec!["A".to_string(), "B".to_string()],
            hovered_entity: None,
        })),
    };
    let sel = state.selection.read().unwrap();
    assert_eq!(sel.selected_entity, Some("A".to_string()));
    assert_eq!(
        sel.selected_entities,
        vec!["A".to_string(), "B".to_string()]
    );
}

#[test]
fn save_and_open_file_roundtrip() {
    use crate::commands::{open_file_impl, save_file_impl};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_roundtrip.ri");

    // Save
    save_file_impl(path.to_str().unwrap(), bracket_source()).expect("save should succeed");

    // Open
    let file_data = open_file_impl(path.to_str().unwrap()).expect("open should succeed");
    assert_eq!(file_data.path, path.to_str().unwrap());
    assert!(file_data.content.contains("structure Bracket"));
}

#[test]
fn constraint_violation_set_thickness_1mm() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));

    let state = {
        let mut session = engine.lock().unwrap();
        session
            .set_parameter("Bracket.thickness", "1mm")
            .expect("set thickness should succeed")
    };

    // thickness=1mm violates "thickness > 2mm"
    let thickness_gt_constraint = state.constraints.iter().find(|c| c.status == "Violated");

    assert!(
        thickness_gt_constraint.is_some(),
        "should have at least one violated constraint when thickness=1mm"
    );
}

#[test]
fn get_source_location_for_width() {
    let session = make_loaded_session();
    let loc = session
        .get_source_location("Bracket.width")
        .expect("should find width source location");

    assert_eq!(loc.file_path, "bracket.ri");
    assert!(loc.line >= 1, "line should be positive");
    assert!(loc.column >= 1, "column should be positive");
}

#[test]
fn export_writes_file() {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.step");

    let result = session.export(reify_types::ExportFormat::Step, &path);
    // MockGeometryKernel writes MOCK_EXPORT_DATA
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("should read exported file");
    assert!(!data.is_empty(), "exported file should not be empty");
}

// --- Mutex-poison tests (task-1781) ---

/// Return an `Arc<Mutex<EngineSession>>` whose mutex has already been poisoned.
///
/// Uses the standard technique: spawn a thread that panics while holding the lock,
/// then join it. After the join the mutex is in a poisoned state.
fn make_poisoned_engine() -> Arc<Mutex<EngineSession>> {
    let session = make_session();
    let engine = Arc::new(Mutex::new(session));
    let engine_clone = Arc::clone(&engine);
    let join_result = std::thread::spawn(move || {
        let _guard = engine_clone.lock().unwrap();
        panic!("poison the mutex");
    })
    .join();
    assert!(
        join_result.is_err(),
        "thread should have panicked to poison the mutex"
    );
    engine
}

#[test]
fn get_entity_tree_impl_returns_err_on_poisoned_mutex() {
    use crate::commands::get_entity_tree_impl;

    let engine = make_poisoned_engine();
    let result = get_entity_tree_impl(&engine);
    assert!(result.is_err(), "expected Err on poisoned mutex, got {:?}", result);
    assert!(
        result.unwrap_err().contains("Lock error"),
        "error message should contain 'Lock error'"
    );
}

#[test]
fn get_entity_tree_impl_returns_ok_on_healthy_mutex() {
    use crate::commands::get_entity_tree_impl;

    let session = make_loaded_session();
    let engine = Mutex::new(session);

    let result = get_entity_tree_impl(&engine);
    assert!(result.is_ok(), "expected Ok on healthy mutex");
    let tree = result.unwrap();
    assert!(!tree.is_empty(), "entity tree should be non-empty for a loaded module");
}

#[test]
fn get_entity_identity_map_impl_returns_err_on_poisoned_mutex() {
    use crate::commands::get_entity_identity_map_impl;

    let engine = make_poisoned_engine();
    let result = get_entity_identity_map_impl(&engine);
    assert!(result.is_err(), "expected Err on poisoned mutex, got {:?}", result);
    assert!(
        result.unwrap_err().contains("Lock error"),
        "error message should contain 'Lock error'"
    );
}

#[test]
fn get_entity_identity_map_impl_returns_ok_on_healthy_mutex() {
    use crate::commands::get_entity_identity_map_impl;

    let session = make_loaded_session();
    let engine = Mutex::new(session);

    let result = get_entity_identity_map_impl(&engine);
    assert!(result.is_ok(), "expected Ok on healthy mutex");
    let map = result.unwrap();
    assert!(!map.is_empty(), "entity identity map should be non-empty for a loaded module");
}

#[test]
fn get_entity_tree_impl_returns_ok_empty_when_no_module_loaded() {
    use crate::commands::get_entity_tree_impl;

    let session = make_session();
    let engine = Mutex::new(session);

    let result = get_entity_tree_impl(&engine);
    assert!(result.is_ok(), "expected Ok with no module loaded, got {:?}", result);
    assert!(
        result.unwrap().is_empty(),
        "entity tree should be empty when no module is loaded"
    );
}

#[test]
fn get_entity_identity_map_impl_returns_ok_empty_when_no_module_loaded() {
    use crate::commands::get_entity_identity_map_impl;

    let session = make_session();
    let engine = Mutex::new(session);

    let result = get_entity_identity_map_impl(&engine);
    assert!(result.is_ok(), "expected Ok with no module loaded, got {:?}", result);
    assert!(
        result.unwrap().is_empty(),
        "entity identity map should be empty when no module is loaded"
    );
}

#[test]
fn get_containing_definition_impl_returns_err_on_poisoned_mutex() {
    use crate::commands::get_containing_definition_impl;

    let engine = make_poisoned_engine();
    let result = get_containing_definition_impl(&engine, 1, 1);
    assert!(result.is_err(), "expected Err on poisoned mutex, got {:?}", result);
    assert!(
        result.unwrap_err().contains("Lock error"),
        "error message should contain 'Lock error'"
    );
}

#[test]
fn get_containing_definition_impl_returns_ok_on_healthy_mutex() {
    use crate::commands::get_containing_definition_impl;

    let session = make_loaded_session();
    let engine = Mutex::new(session);

    // bracket_source() starts with "structure Bracket {" on line 1.
    // Position (1, 1) is the first character of that declaration → inside Bracket.
    let result = get_containing_definition_impl(&engine, 1, 1);
    let def_info = result
        .expect("healthy mutex should return Ok")
        .expect("position (1,1) should be inside the Bracket structure");
    assert_eq!(def_info.name, "Bracket");
    assert_eq!(def_info.kind, "structure");

    // bracket_source() has 15 lines; line 16 is beyond the source → outside any definition.
    let result_outside = get_containing_definition_impl(&engine, 16, 1);
    assert_eq!(
        result_outside,
        Ok(None),
        "position (16,1) is beyond the source and should be outside any definition"
    );
}

// --- Integration tests (step-11) ---

#[test]
fn constraint_violation_and_recovery() {
    let mut session = make_loaded_session();

    // Set thickness=1mm → violates "thickness > 2mm"
    let state = session
        .set_parameter("Bracket.thickness", "1mm")
        .expect("set thickness=1mm");

    let violated_count = state
        .constraints
        .iter()
        .filter(|c| c.status == "Violated")
        .count();
    assert!(
        violated_count >= 1,
        "thickness=1mm should violate at least 1 constraint"
    );

    // Some constraints should still be satisfied
    let satisfied_count = state
        .constraints
        .iter()
        .filter(|c| c.status == "Satisfied")
        .count();
    assert!(
        satisfied_count >= 1,
        "some constraints should still be satisfied"
    );

    // Set back to 5mm → all satisfied again
    let state = session
        .set_parameter("Bracket.thickness", "5mm")
        .expect("set thickness=5mm");

    for c in &state.constraints {
        assert_eq!(
            c.status, "Satisfied",
            "all constraints should be satisfied after restoring thickness=5mm, but {} is {}",
            c.node_id, c.status
        );
    }
}

#[test]
fn end_to_end_get_source_location() {
    let session = make_loaded_session();

    // Should find all params
    for param in &["Bracket.width", "Bracket.height", "Bracket.thickness"] {
        let loc = session.get_source_location(param);
        assert!(loc.is_some(), "should find location for {}", param);
        let loc = loc.unwrap();
        assert_eq!(loc.file_path, "bracket.ri");
        assert!(
            loc.line >= 1 && loc.line <= 15,
            "line should be within bracket.ri"
        );
    }

    // Non-existent should return None
    assert!(session.get_source_location("Nonexistent.param").is_none());
}

#[test]
fn end_to_end_export_via_impl() {
    use crate::commands::export_impl;

    let session = make_loaded_session();
    let engine = Mutex::new(session);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e2e_test.step");

    export_impl(&engine, "step", path.to_str().unwrap()).expect("export should succeed");
    assert!(path.exists(), "exported file should exist");
}

#[test]
fn module_structure_all_public_types() {
    // Verify all public types are accessible from the crate
    use crate::types::{ConstraintData, FileData, GuiState, MeshData, ValueData};
    use reify_mcp::SourceLocationInfo;
    // Verify full IPC contract (Serialize + DeserializeOwned + Clone + Debug + PartialEq)
    super::assert_ipc_contract::<GuiState>();
    super::assert_ipc_contract::<MeshData>();
    super::assert_ipc_contract::<ValueData>();
    super::assert_ipc_contract::<ConstraintData>();
    super::assert_ipc_contract::<SourceLocationInfo>();
    super::assert_ipc_contract::<FileData>();
}

// --- Mechanism descriptor command tests (step-13) ---

/// A 1-body mechanism with a prismatic joint bound to a param via snapshot().
/// Matches SNAPSHOT_PARAM_BIND_SOURCE in engine_tests.rs; duplicated here to keep
/// commands_tests self-contained.
const MECHANISM_FIXTURE_SOURCE: &str = r#"
structure Kinematic {
    param y_pos: Length = 100mm
    let y_axis = prismatic(vec3(1, 0, 0), 0mm .. 800mm)
    let m0     = mechanism()
    let m1     = body(m0, "solid_a", y_axis)
    let snap   = snapshot(m1, [bind(y_axis, y_pos)])
}
"#;

#[test]
fn get_mechanism_descriptors_impl_round_trips() {
    use crate::commands::get_mechanism_descriptors_impl;

    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_test_support::MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(MECHANISM_FIXTURE_SOURCE, "kinematic")
        .expect("load mechanism fixture");

    // Capture the expected descriptors via the EngineSession method directly.
    let expected = session.get_mechanism_descriptors();

    // Now wrap the same session in a Mutex and call through the impl helper.
    let engine = Mutex::new(session);
    let result = get_mechanism_descriptors_impl(&engine);
    assert!(
        result.is_ok(),
        "get_mechanism_descriptors_impl should return Ok; got {:?}",
        result
    );
    let actual = result.unwrap();

    assert_eq!(
        actual, expected,
        "impl round-trip should return the same descriptors as EngineSession::get_mechanism_descriptors()"
    );

    // Sanity: the fixture has m0 (0 bodies) and m1 (1 body); both are mechanisms, so 2 descriptors.
    // The impl should return at least one descriptor with bodies_count=1.
    assert!(!actual.is_empty(), "expected at least one mechanism descriptor");

    // Find the descriptor for m1 (1-body mechanism) — same approach as the engine_tests step-11.
    let m1_desc = actual
        .iter()
        .find(|d| d.bodies_count == 1)
        .expect("expected a descriptor with bodies_count=1 (m1)");
    assert_eq!(m1_desc.joints.len(), 1, "m1 should have exactly one joint");
    assert_eq!(
        m1_desc.joints[0].driving_param_cell_id,
        Some("Kinematic.y_pos".to_string()),
        "driving param should be resolved via impl round-trip"
    );
}

#[test]
fn get_mechanism_descriptors_impl_returns_err_on_poisoned_mutex() {
    use crate::commands::get_mechanism_descriptors_impl;

    let engine = make_poisoned_engine();
    let result = get_mechanism_descriptors_impl(&engine);
    assert!(result.is_err(), "expected Err on poisoned mutex, got {:?}", result);
    assert!(
        result.unwrap_err().contains("Lock error"),
        "error message should contain 'Lock error'"
    );
}

// --- View sidecar tests (step-7) ---

fn make_sample_persistent_state() -> crate::types::PersistentViewState {
    crate::types::PersistentViewState {
        version: "1".to_string(),
        active_view_id: "auto:default".to_string(),
        user_views: vec![],
        explicit: std::collections::HashMap::new(),
        viewport_cameras: std::collections::HashMap::new(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
    }
}

#[test]
fn read_view_sidecar_returns_none_when_absent() {
    use crate::commands::read_view_sidecar_impl;

    let dir = tempfile::tempdir().unwrap();
    let ri_path = dir.path().join("test.ri");
    // The .ri file itself doesn't need to exist — only the sidecar matters.
    let result = read_view_sidecar_impl(ri_path.to_str().unwrap());
    assert!(result.is_ok(), "should return Ok when sidecar is absent");
    assert!(
        result.unwrap().is_none(),
        "should return None when sidecar is absent"
    );
}

#[test]
fn write_view_sidecar_creates_file_next_to_ri_with_pretty_json() {
    use crate::commands::write_view_sidecar_impl;

    let dir = tempfile::tempdir().unwrap();
    let ri_path = dir.path().join("bracket.ri");
    let state = make_sample_persistent_state();

    write_view_sidecar_impl(ri_path.to_str().unwrap(), &state).expect("write should succeed");

    // Sidecar should be next to the .ri with .views.json appended.
    let sidecar_path = format!("{}.views.json", ri_path.to_str().unwrap());
    assert!(
        std::path::Path::new(&sidecar_path).exists(),
        "sidecar file should exist at {sidecar_path}"
    );

    let content = std::fs::read_to_string(&sidecar_path).unwrap();
    // Pretty JSON contains newlines and the version field.
    assert!(content.contains('\n'), "pretty JSON should contain newlines");
    assert!(
        content.contains("\"version\""),
        "pretty JSON should contain version key"
    );
}

// Note: a separate "returns_some_when_file_exists" test was removed — the
// `view_sidecar_roundtrip` test below asserts field equality on the loaded
// value, which strictly subsumes the weaker is_some() check.

#[test]
fn read_view_sidecar_returns_err_on_malformed_json() {
    use crate::commands::read_view_sidecar_impl;

    let dir = tempfile::tempdir().unwrap();
    let ri_path = dir.path().join("bracket.ri");
    let sidecar_path = format!("{}.views.json", ri_path.to_str().unwrap());

    std::fs::write(&sidecar_path, b"not-valid-json").unwrap();

    let result = read_view_sidecar_impl(ri_path.to_str().unwrap());
    assert!(
        result.is_err(),
        "should return Err on malformed JSON, not panic"
    );
}

#[test]
fn view_sidecar_roundtrip() {
    use crate::commands::{read_view_sidecar_impl, write_view_sidecar_impl};
    use crate::types::{CameraStateData, ViewDefinitionData};

    let dir = tempfile::tempdir().unwrap();
    let ri_path = dir.path().join("bracket.ri");

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

    let user_views = vec![ViewDefinitionData {
        id: "user:my-view".to_string(),
        name: "My View".to_string(),
        auto: false,
        visibility: visibility.clone(),
        modified: Some(true),
    }];

    let mut explicit = std::collections::HashMap::new();
    explicit.insert("Bracket.body".to_string(), "ghost".to_string());

    let state = crate::types::PersistentViewState {
        version: "1".to_string(),
        active_view_id: "user:my-view".to_string(),
        user_views,
        explicit,
        viewport_cameras: cameras,
        timestamp: "2026-04-22T12:00:00Z".to_string(),
    };

    write_view_sidecar_impl(ri_path.to_str().unwrap(), &state).unwrap();
    let loaded = read_view_sidecar_impl(ri_path.to_str().unwrap())
        .unwrap()
        .expect("should load state");

    assert_eq!(loaded, state, "round-trip should preserve all fields");
}
