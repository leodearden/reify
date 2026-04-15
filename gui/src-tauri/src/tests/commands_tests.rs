use std::sync::{Arc, Mutex, RwLock};

use reify_constraints::SimpleConstraintChecker;
use reify_mcp::SelectionInfo;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::commands::AppState;
use crate::engine::EngineSession;

fn make_loaded_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
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
            hovered_entity: None,
        })),
    };
    let sel = state.selection.read().unwrap();
    assert_eq!(sel.selected_entity, Some("Bracket".to_string()));
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
    let session = EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    let engine = Arc::new(Mutex::new(session));
    let engine_clone = Arc::clone(&engine);
    let _ = std::thread::spawn(move || {
        let _guard = engine_clone.lock().unwrap();
        panic!("poison the mutex");
    })
    .join();
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
