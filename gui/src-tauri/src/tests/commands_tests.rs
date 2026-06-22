use std::sync::{Arc, Mutex, RwLock};

use crate::tests::test_helpers::cwd_lock;

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
        initial_file: Mutex::new(None),
        pending_solve_cancel: Arc::new(Mutex::new(None)),
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
        initial_file: Mutex::new(None),
        pending_solve_cancel: Arc::new(Mutex::new(None)),
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
        initial_file: Mutex::new(None),
        pending_solve_cancel: Arc::new(Mutex::new(None)),
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

    let result = session.export(reify_ir::ExportFormat::Step, &path);
    // MockGeometryKernel writes MOCK_EXPORT_DATA
    assert!(result.is_ok(), "export should succeed: {:?}", result.err());

    let data = std::fs::read(&path).expect("should read exported file");
    assert!(!data.is_empty(), "exported file should not be empty");
}

// --- Mutex-poison tests (task-1781) ---

/// Poison an existing `Arc<Mutex<EngineSession>>` and return it.
///
/// Used by Group-B tests to poison an already-loaded session so recovery
/// tests can verify that the impl proceeds with a consistent inner state.
fn poison_engine(engine: Arc<Mutex<EngineSession>>) -> Arc<Mutex<EngineSession>> {
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
fn get_entity_tree_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_entity_tree_impl;

    // Poison a *loaded* session — verifies that the session's data survives
    // recovery, not just that an unloaded session returns an empty default.
    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_entity_tree_impl(&engine);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    assert!(
        !result.unwrap().is_empty(),
        "loaded session entity tree should survive poison recovery"
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
    assert!(
        !tree.is_empty(),
        "entity tree should be non-empty for a loaded module"
    );
}

#[test]
fn get_entity_identity_map_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_entity_identity_map_impl;

    // Poison a *loaded* session — verifies that the session's data survives
    // recovery, not just that an unloaded session returns an empty default.
    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_entity_identity_map_impl(&engine);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    assert!(
        !result.unwrap().is_empty(),
        "loaded session identity map should survive poison recovery"
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
    assert!(
        !map.is_empty(),
        "entity identity map should be non-empty for a loaded module"
    );
}

#[test]
fn get_entity_tree_impl_returns_ok_empty_when_no_module_loaded() {
    use crate::commands::get_entity_tree_impl;

    let session = make_session();
    let engine = Mutex::new(session);

    let result = get_entity_tree_impl(&engine);
    assert!(
        result.is_ok(),
        "expected Ok with no module loaded, got {:?}",
        result
    );
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
    assert!(
        result.is_ok(),
        "expected Ok with no module loaded, got {:?}",
        result
    );
    assert!(
        result.unwrap().is_empty(),
        "entity identity map should be empty when no module is loaded"
    );
}

#[test]
fn get_containing_definition_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_containing_definition_impl;

    // Poison a *loaded* session — verifies that the session's source map
    // survives recovery and an in-bounds position still resolves correctly.
    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_containing_definition_impl(&engine, 1, 1);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let def_info = result
        .unwrap()
        .expect("position (1,1) should be inside the Bracket structure after poison recovery");
    assert_eq!(
        def_info.name, "Bracket",
        "loaded session source map should survive poison recovery"
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

// --- get_entity_at_source_location_impl tests ---

#[test]
fn get_entity_at_source_location_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_entity_at_source_location_impl;

    // Poison a *loaded* session — verifies that the session's span map survives
    // recovery and an in-bounds position still resolves to the expected entity.
    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_entity_at_source_location_impl(&engine, 2, 11);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        Some("Bracket.width".to_string()),
        "loaded session span map should survive poison recovery"
    );
}

#[test]
fn get_entity_at_source_location_impl_returns_ok_on_healthy_mutex() {
    use crate::commands::get_entity_at_source_location_impl;

    let session = make_loaded_session();
    let engine = Mutex::new(session);

    // Position (2, 11) is inside the Bracket.width cell span.
    let result = get_entity_at_source_location_impl(&engine, 2, 11);
    assert_eq!(
        result,
        Ok(Some("Bracket.width".to_string())),
        "position (2,11) should resolve to Bracket.width"
    );

    // Position (16, 1) is beyond the source end → outside any template span → None.
    let result_outside = get_entity_at_source_location_impl(&engine, 16, 1);
    assert_eq!(
        result_outside,
        Ok(None),
        "position (16,1) is beyond the source and should return None"
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
    assert!(
        !actual.is_empty(),
        "expected at least one mechanism descriptor"
    );

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
fn get_mechanism_descriptors_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_mechanism_descriptors_impl;

    // Poison a *loaded* mechanism session — verifies that the session's
    // descriptor data survives recovery, not just that an empty session returns
    // an empty default.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_test_support::MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(MECHANISM_FIXTURE_SOURCE, "kinematic")
        .expect("load mechanism fixture");
    let engine = poison_engine(Arc::new(Mutex::new(session)));
    let result = get_mechanism_descriptors_impl(&engine);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    assert!(
        !result.unwrap().is_empty(),
        "loaded mechanism session descriptors should survive poison recovery"
    );
}

// --- View sidecar tests (step-7) ---

fn make_sample_persistent_state() -> crate::types::PersistentViewState {
    crate::types::PersistentViewState {
        version: "2".to_string(),
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
    assert!(
        content.contains('\n'),
        "pretty JSON should contain newlines"
    );
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
        version: "2".to_string(),
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

// --- Mutex-poison recovery tests for mutating/Result-returning impls (step-3) ---

#[test]
fn get_initial_state_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_initial_state_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_initial_state_impl(&engine);
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let state = result.unwrap();
    assert!(
        !state.values.is_empty(),
        "get_initial_state should return bracket parameters after poison recovery"
    );
}

#[test]
fn set_parameter_impl_recovers_from_poisoned_mutex() {
    use crate::commands::set_parameter_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = set_parameter_impl(&engine, "Bracket.thickness", "5mm");
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let state = result.unwrap();
    assert!(
        state
            .values
            .iter()
            .any(|v| v.cell_id == "Bracket.thickness" && v.value == "5" && v.unit == "mm"),
        "set_parameter should have applied thickness=5mm after poison recovery"
    );
}

/// step-8 (task 4532): the `sync_observed_demand` tauri command wrapper
/// (`sync_observed_demand_impl`) registers the GUI's observed-demand sources
/// through the same `&Mutex<EngineSession>` session shim the other command
/// tests use, leaves production evaluation unchanged, and the NEXT
/// `set_parameter` surfaces the passive would-prune measurement on the returned
/// `GuiState.demand_prune_measurement`.
///
/// RED until `sync_observed_demand_impl` exists (step-9).
#[test]
fn sync_observed_demand_impl_is_zero_behavior_change_and_surfaces_measurement() {
    use crate::commands::{set_parameter_impl, sync_observed_demand_impl};

    // ── Control: drive the edit through the command shim with NO sync. ────────
    let control = Mutex::new(make_loaded_session());
    let control_state =
        set_parameter_impl(&control, "Bracket.thickness", "2mm").expect("control set_parameter");

    // ── Synced: register the visible realization R0 + the displayed volume
    //    cell through the COMMAND shim before the edit. No panel constraints, so
    //    the constraints fall OUTSIDE the observed cone (would-prune).
    //
    //    NOTE: we sync `Bracket.volume` (a let-binding downstream of thickness)
    //    rather than `Bracket.thickness` (the edited param) because after θ2
    //    step-8 (#4713) Realization nodes are excluded from `last_eval_set` on the
    //    kernel-less edit path, and the source/edited param itself is also not in
    //    `last_eval_set` (it is the dirty-cone root, not a dependent).  `volume`
    //    IS in `last_eval_set` after editing `thickness` (depends on it) AND in
    //    the observed cone (direct root), so `observed_retained >= 1`. ──────────
    let synced = Mutex::new(make_loaded_session());
    sync_observed_demand_impl(
        &synced,
        &["Bracket#realization[0]".to_string()],
        &["Bracket.volume".to_string()],
        &[],
    )
    .expect("sync_observed_demand_impl should succeed");
    let synced_state =
        set_parameter_impl(&synced, "Bracket.thickness", "2mm").expect("synced set_parameter");

    // (a) Zero behavior change through the command path: parameter values are
    //     byte-identical to the no-sync control.
    assert_eq!(
        synced_state.values, control_state.values,
        "command-path observed-demand sync must NOT change GuiState parameter values"
    );

    // (b) The returned GuiState carries a populated measurement reflecting the
    //     registered sources.
    let m = synced_state
        .demand_prune_measurement
        .as_ref()
        .expect("synced GuiState must carry a demand_prune_measurement after the edit");
    let would_prune_total = m.would_prune.value
        + m.would_prune.constraint
        + m.would_prune.realization
        + m.would_prune.resolution
        + m.would_prune.compute;
    assert!(
        m.observed_retained >= 1,
        "the observed Bracket.volume cell must be retained (observed_retained >= 1), got {}",
        m.observed_retained
    );
    assert_eq!(
        m.would_prune.realization, 0,
        "Realization nodes are excluded from last_eval_set (θ2 step-8 #4713), so \
         would_prune.realization must be 0; got {}",
        m.would_prune.realization
    );
    assert!(
        would_prune_total > 0,
        "non-observed nodes (constraints) must be counted as would-prune; got {:?}",
        m.would_prune
    );
    assert_eq!(
        m.observed_retained + would_prune_total,
        m.eval_set_size,
        "invariant: observed_retained + would_prune-total == eval_set_size"
    );

    // The no-sync control surfaces a measurement too — with nothing retained and
    // the SAME production eval-set size (zero behavior change).
    let control_m = control_state
        .demand_prune_measurement
        .as_ref()
        .expect("control GuiState also carries a measurement (empty observed cone)");
    assert_eq!(
        control_m.observed_retained, 0,
        "with no observed registration, nothing is retained"
    );
    assert_eq!(
        control_m.eval_set_size, m.eval_set_size,
        "production eval-set size is identical with and without observed sync"
    );
}

#[test]
fn update_source_impl_recovers_from_poisoned_mutex() {
    use crate::commands::update_source_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = update_source_impl(&engine, "bracket", bracket_source());
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let state = result.unwrap();
    assert!(
        !state.values.is_empty(),
        "update_source should have reloaded the bracket module after poison recovery"
    );
}

#[test]
fn export_impl_recovers_from_poisoned_mutex() {
    use crate::commands::export_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery_test.step");
    let result = export_impl(&engine, "step", path.to_str().unwrap());
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    assert!(
        path.exists(),
        "export should have written the file after poison recovery"
    );
}

#[test]
fn get_source_location_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_source_location_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_source_location_impl(&engine, "Bracket.width");
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let loc = result.unwrap();
    assert_eq!(
        loc.file_path, "bracket.ri",
        "source location should point to the correct file after poison recovery"
    );
    assert!(
        loc.line >= 1,
        "source location line should be 1-based after poison recovery"
    );
}

#[test]
fn open_file_engine_impl_recovers_from_poisoned_mutex() {
    use crate::commands::open_file_engine_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_session())));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bracket.ri");
    std::fs::write(&path, bracket_source()).unwrap();
    let result = open_file_engine_impl(&engine, path.to_str().unwrap());
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let state = result.unwrap();
    assert!(
        !state.values.is_empty(),
        "open_file_engine should have loaded the bracket module after poison recovery"
    );
}

#[test]
fn get_def_preview_impl_recovers_from_poisoned_mutex() {
    use crate::commands::get_def_preview_impl;

    let engine = poison_engine(Arc::new(Mutex::new(make_loaded_session())));
    let result = get_def_preview_impl(&engine, "Bracket");
    assert!(
        result.is_ok(),
        "expected Ok recovery from poisoned mutex, got {:?}",
        result
    );
    let state = result.unwrap();
    assert!(
        !state.values.is_empty(),
        "get_def_preview should return Bracket parameters after poison recovery"
    );
}

// --- open_file_impl canonicalisation tests (step-3) ---

/// (a) opening a file via its CWD-relative path returns FileData.path equal to
/// the canonical absolute realpath of that file.
#[test]
fn open_file_impl_returns_canonical_path_for_relative_input() {
    use crate::commands::open_file_impl;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.ri");
    std::fs::write(&file, "structure Test {}").unwrap();
    let expected = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let result = open_file_impl("test.ri");

    std::env::set_current_dir(&original).unwrap();

    let file_data = result.expect("open_file_impl should succeed for existing file");
    assert_eq!(
        file_data.path, expected,
        "FileData.path should be the canonical absolute realpath"
    );
}

/// (b) two open_file_impl calls using two different spellings of the same file
/// (relative vs absolute) return IDENTICAL path strings.
#[test]
fn open_file_impl_same_path_for_relative_and_absolute() {
    use crate::commands::open_file_impl;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("myfile.ri");
    std::fs::write(&file, "structure MyFile {}").unwrap();
    let abs_path = file.to_str().unwrap().to_string();

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let by_relative = open_file_impl("myfile.ri").expect("relative open should succeed");
    let by_absolute = open_file_impl(&abs_path).expect("absolute open should succeed");

    std::env::set_current_dir(&original).unwrap();

    assert_eq!(
        by_relative.path, by_absolute.path,
        "relative and absolute spellings of the same file should produce identical FileData.path"
    );
}

/// (c) when the file does not exist, the existing "Error reading …" error is
/// still surfaced (regression check on the fallback / error branch).
#[test]
fn open_file_impl_errors_for_nonexistent_file() {
    use crate::commands::open_file_impl;

    let result = open_file_impl("/tmp/__reify_nonexistent_xyzzy_99999.ri");
    assert!(result.is_err(), "should return Err for nonexistent file");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("Error reading"),
        "error message should contain 'Error reading', got: {msg}"
    );
}

// --- open_file_engine_impl canonicalisation tests (step-5) ---
//
// The plan's step 5 description states that GuiState.files[0].path should be
// the canonical absolute path after calling open_file_engine_impl with a
// relative input.  engine::source_map() always stores keys as module_key =
// "{name}.ri" (see engine.rs commit_state:275-277), so this requires
// open_file_engine_impl to post-process the returned GuiState.files paths
// (see step-6 implementation for how this is done).  The test is written to
// the observable contract: files[0].path == canonical absolute path.

/// Opening a file via its CWD-relative path causes GuiState.files[0].path to
/// equal the canonical absolute realpath (not the bare filename / module key).
#[test]
fn open_file_engine_impl_files_path_is_canonical_absolute() {
    use crate::commands::open_file_engine_impl;
    use reify_test_support::bracket_source;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bracket.ri");
    std::fs::write(&file, bracket_source()).unwrap();
    let expected = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_test_support::MockGeometryKernel::new();
    let session = crate::engine::EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = std::sync::Mutex::new(session);

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let result = open_file_engine_impl(&engine, "bracket.ri");

    std::env::set_current_dir(&original).unwrap();

    let state = result.expect("open_file_engine_impl should succeed for existing file");
    assert!(
        !state.files.is_empty(),
        "GuiState.files should be non-empty after loading a file"
    );
    assert_eq!(
        state.files[0].path, expected,
        "GuiState.files[0].path should be the canonical absolute realpath, not a module-key form"
    );
}

// ── Task 3543 step-9: cancel_solve_impl command tests (GR-016 ζ) ─────────────

/// `cancel_solve_impl` calls `.cancel()` on the published handle, clears the
/// slot, and returns `Ok(())`.
///
/// Verifies the PRD §11 Q2 resolution: the `cancel_solve` Tauri command reads
/// `AppState::pending_solve_cancel`, cancels the handle if present, and clears
/// the slot so it is not double-cancelled by a follow-on command invocation.
#[test]
fn cancel_solve_impl_fires_published_handle_and_clears_slot() {
    use reify_eval::CancellationHandle;
    use crate::commands::cancel_solve_impl;

    let session = make_session();
    let handle = CancellationHandle::new();
    let handle_clone = handle.clone();

    let state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo::default())),
        initial_file: Mutex::new(None),
        pending_solve_cancel: Arc::new(Mutex::new(Some(handle_clone))),
    };

    let result = cancel_solve_impl(&state);
    assert!(result.is_ok(), "cancel_solve_impl must return Ok; got: {:?}", result);
    assert!(handle.is_cancelled(), "CancellationHandle must be cancelled after cancel_solve_impl");
    let slot = state.pending_solve_cancel.lock().unwrap();
    assert!(slot.is_none(), "pending_solve_cancel slot must be cleared after cancel_solve_impl");
}

/// `cancel_solve_impl` returns `Ok(())` when the slot is empty (no solve in flight).
///
/// A no-op is the correct outcome — there is nothing to cancel.
#[test]
fn cancel_solve_impl_returns_ok_when_slot_empty() {
    use crate::commands::cancel_solve_impl;

    let session = make_session();
    let state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo::default())),
        initial_file: Mutex::new(None),
        pending_solve_cancel: Arc::new(Mutex::new(None)),
    };

    let result = cancel_solve_impl(&state);
    assert!(result.is_ok(), "cancel_solve_impl must return Ok when slot is empty; got: {:?}", result);
}

// ── Task 4086 step-7: RED — production sink + consumer interplay ──
//
// Verifies PendingSolveCancelSink (the production SolveCancellationSink impl):
//   (a) solve_started writes the handle into the shared slot
//   (b) solve_finished clears the slot
//   (c) cancel_solve_impl (the existing consumer) fires the handle and clears
//       the slot — producer→consumer contract
//
// Fails with compile error until step-8 adds PendingSolveCancelSink to commands.rs.

/// (a) + (b): PendingSolveCancelSink sets the slot on solve_started and clears
/// it on solve_finished — the Some-during/None-after lifecycle.
///
/// Constructs a shared slot directly, builds PendingSolveCancelSink from it,
/// and drives the two lifecycle calls manually without a full EngineSession.
#[test]
fn pending_solve_cancel_sink_sets_then_clears_slot() {
    use reify_eval::CancellationHandle;
    use crate::commands::PendingSolveCancelSink;
    use crate::engine::SolveCancellationSink;

    let slot: Arc<Mutex<Option<CancellationHandle>>> = Arc::new(Mutex::new(None));
    let sink = PendingSolveCancelSink::new(slot.clone());

    let handle = CancellationHandle::new();
    let handle_clone = handle.clone();

    // solve_started must write the handle into the slot.
    sink.solve_started(handle_clone);
    let slot_after_start = slot.lock().unwrap();
    assert!(
        slot_after_start.is_some(),
        "slot must be Some after solve_started"
    );
    // Verify the handle in the slot is the same one we published (shares Arc).
    let stored = slot_after_start.clone().unwrap();
    assert!(
        !stored.is_cancelled(),
        "stored handle must not be cancelled immediately after solve_started"
    );
    drop(slot_after_start);

    // solve_finished must clear the slot.
    sink.solve_finished();
    let slot_after_finish = slot.lock().unwrap();
    assert!(
        slot_after_finish.is_none(),
        "slot must be None after solve_finished"
    );
}

/// (c): After solve_started publishes a handle, cancel_solve_impl fires it
/// and clears the slot — the producer→consumer contract.
///
/// Uses an AppState built with the shared slot so the consumer reads the same
/// Arc as the producer.
#[test]
fn pending_solve_cancel_cancelled_by_consumer_during_solve() {
    use reify_eval::CancellationHandle;
    use crate::commands::{cancel_solve_impl, PendingSolveCancelSink};
    use crate::engine::SolveCancellationSink;

    let slot: Arc<Mutex<Option<CancellationHandle>>> = Arc::new(Mutex::new(None));
    let sink = PendingSolveCancelSink::new(slot.clone());

    // Simulate solve_started: publish a handle into the slot.
    let handle = CancellationHandle::new();
    let handle_clone = handle.clone();
    sink.solve_started(handle_clone);

    // Build AppState with the SAME slot Arc so cancel_solve_impl reads it.
    let session = make_session();
    let state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo::default())),
        initial_file: Mutex::new(None),
        pending_solve_cancel: slot.clone(),
    };

    // cancel_solve_impl must: (1) cancel the handle, (2) clear the slot.
    let result = cancel_solve_impl(&state);
    assert!(result.is_ok(), "cancel_solve_impl must return Ok; got: {:?}", result);
    assert!(
        handle.is_cancelled(),
        "CancellationHandle must be cancelled after cancel_solve_impl fires it"
    );
    let slot_after_cancel = state.pending_solve_cancel.lock().unwrap();
    assert!(
        slot_after_cancel.is_none(),
        "slot must be cleared after cancel_solve_impl"
    );
}

// ---------------------------------------------------------------------------
// Hot-reload staleness recording at the update_source_impl chokepoint (task 4153)
// ---------------------------------------------------------------------------

/// Make a fresh engine with bracket source pre-loaded.
fn make_test_engine_for_commands() -> Arc<Mutex<EngineSession>> {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");
    Arc::new(Mutex::new(session))
}

/// (step-4 GREEN-a) update_source_impl must record staleness when update_source
/// returns Err (here: compile error from invalid source syntax).
///
/// NOTE: The original step-3 plan used `set_panic_on_eval_for_test`, but that
/// mechanism injects panics caught *inside* the eval loop (engine_eval.rs:3677
/// catches per-cell panics via catch_unwind), so `update_source` still returns Ok.
/// A compile error via invalid source is the correct proxy for triggering
/// `update_source → Err` at the commands layer.  This test therefore covers the
/// **compile-error** staleness path, not the check()-panic path.
///
/// The check()-panic path (compile_failure=None, last_reload_error=Some, synthetic
/// DiagnosticInfo emitted) is exercised at the unit level via
/// `record_reload_error` in `engine_tests.rs` (e.g.
/// `build_gui_state_appends_synth_diagnostic_when_stale`).  An end-to-end
/// integration test that triggers a true check()-panic through `update_source`
/// would require a language-level construct that causes a panic after successful
/// compilation — none exists today, so unit-level coverage is the accepted approach.
///
/// RED until step-4 adds the `record_reload_error` call inside `update_source_impl`.
#[test]
fn update_source_impl_records_staleness_on_compile_error() {
    let engine = make_test_engine_for_commands();

    // Use invalid source to trigger a compile error — the reliable path for
    // update_source to return Err at the commands layer.
    let result = crate::commands::update_source_impl(&engine, "bracket.ri", "invalid syntax $$$");
    assert!(
        result.is_err(),
        "update_source_impl must return Err for invalid source; got Ok"
    );

    // The session must now be stale — is_stale() is true and reload_error() is Some.
    // This assertion is RED until step-4 adds `record_reload_error` inside update_source_impl.
    let is_stale = crate::engine_lock::with_engine_lock(&engine, |s| s.is_stale())
        .expect("with_engine_lock should not panic");
    assert!(
        is_stale,
        "session must be stale after update_source_impl returns Err; \
         this assertion is RED in step-3 and turns GREEN in step-4"
    );
    let has_reload_error =
        crate::engine_lock::with_engine_lock(&engine, |s| s.reload_error().is_some())
            .expect("with_engine_lock should not panic");
    assert!(
        has_reload_error,
        "reload_error() must be Some after update_source_impl returns Err; \
         this assertion is RED in step-3 and turns GREEN in step-4"
    );
}

/// (step-4 GREEN-b) After a previously-recorded staleness, a successful
/// update_source_impl must clear the stale flag (commit_state already clears it).
///
/// Depends on step-4a passing (staleness recorded via compile-error) and
/// commit_state clearing last_reload_error on the subsequent successful reload.
#[test]
fn update_source_impl_clears_staleness_on_successful_reload() {
    let engine = make_test_engine_for_commands();

    // Trigger compile error to set staleness.
    let _ = crate::commands::update_source_impl(&engine, "bracket.ri", "invalid syntax $$$");

    // Second call: valid source — update_source_impl should succeed and clear staleness.
    let result = crate::commands::update_source_impl(&engine, "bracket.ri", bracket_source());
    assert!(
        result.is_ok(),
        "second update_source_impl (valid source) must return Ok; got: {:?}",
        result.err()
    );

    let is_stale = crate::engine_lock::with_engine_lock(&engine, |s| s.is_stale())
        .expect("with_engine_lock should not panic");
    assert!(
        !is_stale,
        "staleness must be cleared after a successful update_source_impl; \
         commit_state clears last_reload_error next to compile_failure"
    );
}

// ---------------------------------------------------------------------------
// Hot-reload watch helper tests (task 4153, step-5 RED)
// ---------------------------------------------------------------------------

/// (step-5 RED-a) reload_for_watch_impl on success must return Ok(GuiState) with
/// non-empty meshes and empty compile_diagnostics.
///
/// RED until step-6 adds `reload_for_watch_impl` to commands.rs.
#[test]
fn reload_for_watch_impl_success_returns_ok_with_fresh_state() {
    let engine = make_test_engine_for_commands();

    // Successful reload with valid source.
    let result = crate::commands::reload_for_watch_impl(&engine, "bracket.ri", bracket_source());
    assert!(
        result.is_ok(),
        "reload_for_watch_impl must return Ok on valid source; got: {:?}",
        result.err()
    );
    let gui_state = result.unwrap();
    assert!(
        !gui_state.meshes.is_empty(),
        "GuiState.meshes must be non-empty after a successful reload"
    );
    assert!(
        gui_state.compile_diagnostics.is_empty(),
        "GuiState.compile_diagnostics must be empty after a successful reload; \
         got: {:?}",
        gui_state.compile_diagnostics
    );
}

/// (step-5 RED-b) reload_for_watch_impl on failure must return Ok(GuiState) — NOT Err —
/// whose meshes are the LAST-GOOD non-empty set and whose compile_diagnostics
/// contains at least one Error-severity entry.  After the call, is_stale() is true.
///
/// This validates that the watcher always has a state to emit (never silent).
///
/// RED until step-6 adds `reload_for_watch_impl` to commands.rs.
#[test]
fn reload_for_watch_impl_failure_returns_ok_with_diagnostic_and_staleness() {
    let engine = make_test_engine_for_commands();

    // Record the mesh count from the pre-failure good state.
    let good_mesh_count = crate::engine_lock::with_engine_lock(&engine, |s| {
        s.build_gui_state()
            .map(|gs| gs.meshes.len())
            .unwrap_or(0)
    })
    .expect("with_engine_lock should not panic");
    assert!(good_mesh_count > 0, "test fixture must have non-empty meshes");

    // Force a failure with invalid source (compile error — reliable Err path).
    let result =
        crate::commands::reload_for_watch_impl(&engine, "bracket.ri", "invalid syntax $$$");

    // Must return Ok, NOT Err — the watcher must always have a state to emit.
    assert!(
        result.is_ok(),
        "reload_for_watch_impl must return Ok even on failure (watcher needs state to emit); \
         got Err: {:?}",
        result.err()
    );
    let gui_state = result.unwrap();

    // Meshes must be the last-good (pre-failure) set.
    assert_eq!(
        gui_state.meshes.len(),
        good_mesh_count,
        "GuiState.meshes count must equal the pre-failure count (last-good retained)"
    );

    // compile_diagnostics must contain at least one Error-severity entry.
    let has_error_diag = gui_state
        .compile_diagnostics
        .iter()
        .any(|d| d.severity == "Error");
    assert!(
        has_error_diag,
        "GuiState.compile_diagnostics must contain at least one Error-severity entry \
         after a failed reload; got: {:?}",
        gui_state.compile_diagnostics
    );

    // Assert the no-dup contract: the compile-error path sets compile_failure
    // (LiveEdit) so build_gui_state gates the synthetic reload-error diagnostic
    // on compile_failure.is_none() and must NOT produce a `hot-reload-error`
    // code entry.  The structured LiveEdit diags are the only Error entries here.
    // A regression that removed the is_none() gate would cause double-reporting
    // and this assertion would catch it (engine.rs:2190-2196).
    let has_hot_reload_error_synthetic = gui_state
        .compile_diagnostics
        .iter()
        .any(|d| d.code.as_deref() == Some("hot-reload-error"));
    assert!(
        !has_hot_reload_error_synthetic,
        "compile-error path must NOT produce a 'hot-reload-error' synthetic diagnostic \
         (compile_failure is Some(LiveEdit) so build_gui_state skips the synthesis); \
         got: {:?}",
        gui_state.compile_diagnostics
    );

    // The session must be stale.
    let is_stale = crate::engine_lock::with_engine_lock(&engine, |s| s.is_stale())
        .expect("with_engine_lock should not panic");
    assert!(
        is_stale,
        "session must be stale after reload_for_watch_impl returns a failure state"
    );
}

// ---------------------------------------------------------------------------
// Watcher delta surfacing test (task 4153, step-9 RED)
// ---------------------------------------------------------------------------

/// (step-9 RED) Prove that the watcher's failure path surfaces the Error-severity
/// diagnostic to the frontend via the `compile-diagnostics` Tauri event.
///
/// Drive a forced compile-error reload, take the GuiState from
/// `reload_for_watch_impl` (failure path → last-good + reload-error diagnostic),
/// run it through `diff::compute_delta` then `diff::delta_to_events`, and assert
/// the resulting events contain a `("compile-diagnostics", payload)` tuple whose
/// payload array includes at least one Error-severity entry.
///
/// This validates the full chain: failure → last-good state with diagnostic →
/// delta computation → Tauri event the frontend already listens for.
///
/// The plan says "RED until reload_for_watch_impl returns the diagnostic-bearing
/// last-good state (step-6) and build_gui_state synthesis (step-2) are both in
/// place."  Both are done, so this test should pass immediately after being written.
#[test]
fn watcher_failure_surfaces_compile_diagnostics_event() {
    let engine = make_test_engine_for_commands();

    // Capture the clean GuiState before the failed reload.
    let prev_good_state = crate::commands::get_initial_state_impl(&engine)
        .expect("get_initial_state_impl should succeed on clean engine");
    assert!(
        !prev_good_state.meshes.is_empty(),
        "prev_good_state must have non-empty meshes (test fixture)"
    );
    assert!(
        prev_good_state.compile_diagnostics.is_empty(),
        "prev_good_state must have no compile_diagnostics before the reload"
    );

    // Drive a failed reload.
    let failure_state =
        crate::commands::reload_for_watch_impl(&engine, "bracket.ri", "invalid syntax $$$")
            .expect("reload_for_watch_impl must return Ok even on failure");

    // The failure state must carry at least one Error-severity diagnostic.
    assert!(
        failure_state
            .compile_diagnostics
            .iter()
            .any(|d| d.severity == "Error"),
        "failure GuiState.compile_diagnostics must contain an Error-severity entry"
    );

    // Run the state through the watcher's delta pipeline.
    let last_state_mutex = Mutex::new(Some(prev_good_state));
    let delta = crate::diff::compute_delta(&last_state_mutex, &failure_state);
    let events = crate::diff::delta_to_events(&delta);

    // Assert there is a "compile-diagnostics" event with an Error-severity entry.
    let compile_diag_event = events
        .iter()
        .find(|(name, _)| name == "compile-diagnostics");
    assert!(
        compile_diag_event.is_some(),
        "delta_to_events must produce a 'compile-diagnostics' event after a failed reload; \
         got events: {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    let payload = &compile_diag_event.unwrap().1;
    let diags = payload
        .as_array()
        .expect("compile-diagnostics payload must be an array");
    let has_error = diags
        .iter()
        .any(|d| d["severity"].as_str() == Some("Error"));
    assert!(
        has_error,
        "compile-diagnostics payload must contain an Error-severity entry; \
         got: {:?}",
        diags
    );
}

// ── Task 3026 step-5: RED — set_active_fea_case_impl / get_active_fea_case_impl ──
//
// Tests over a Mutex<EngineSession>:
//   (a) get_active_fea_case_impl returns Ok(None) initially (lex-first default).
//   (b) set_active_fea_case_impl(engine, "overload") returns Ok(GuiState).
//   (c) Subsequent get_active_fea_case_impl returns Ok(Some("overload")).
//   (d) Unknown case name is handled deterministically (falls back to lex-first;
//       does not return Err).
//
// Fails to COMPILE until step-6 adds:
//   - set_active_fea_case_impl(&Mutex<EngineSession>, name) -> Result<GuiState, String>
//   - get_active_fea_case_impl(&Mutex<EngineSession>) -> Result<Option<String>, String>

/// Build a ValueMap containing a MultiCaseResult with "operating" and "overload" cases.
///
/// Uses simple Value::Int payloads (not real ElasticResult) so the test focuses on
/// the command-layer getter/setter contract; channel content is verified in engine_tests.
fn make_simple_multi_case_values() -> reify_ir::ValueMap {
    use reify_ir::Value;
    use reify_test_support::multi_case_result_value;
    let mcr = multi_case_result_value(&[
        ("operating", Value::Int(1)),
        ("overload", Value::Int(2)),
    ]);
    let mut map = reify_ir::ValueMap::new();
    map.insert(reify_core::ValueCellId::new("Bracket", "result"), mcr);
    map
}

/// set_active_fea_case_impl / get_active_fea_case_impl command-layer contract.
#[test]
fn set_and_get_active_fea_case_impl_contract() {
    use reify_eval::CheckResult;
    use crate::commands::{get_active_fea_case_impl, set_active_fea_case_impl}; // FAILS TO COMPILE

    // Build a loaded session and inject a multi-case CheckResult.
    let mut session = make_loaded_session();
    let check = CheckResult {
        values: make_simple_multi_case_values(),
        constraint_results: vec![],
        diagnostics: vec![],
        resolved_params: std::collections::HashMap::new(),
    };
    session.inject_check_for_test(check);

    let engine = Mutex::new(session);

    // (a) Initial active case is None (lex-first default).
    let initial = get_active_fea_case_impl(&engine) // FAILS TO COMPILE
        .expect("get_active_fea_case_impl must succeed");
    assert_eq!(initial, None, "initial active case must be None");

    // (b) Switch to "overload" → Ok(GuiState).
    // The command-layer contract is that set returns Ok for a valid case name.
    // Mesh-from-cache content is verified in engine_tests; this layer tests only
    // the Ok-return contract.
    let _state_overload = set_active_fea_case_impl(&engine, "overload") // FAILS TO COMPILE
        .expect("set_active_fea_case_impl('overload') must succeed");

    // (c) Subsequent get returns Some("overload").
    let active_after = get_active_fea_case_impl(&engine) // FAILS TO COMPILE
        .expect("get_active_fea_case_impl must succeed after set");
    assert_eq!(
        active_after,
        Some("overload".to_string()),
        "active case must be 'overload' after set_active_fea_case_impl"
    );

    // (d) Unknown case name does not return Err (falls back to lex-first).
    let _state_unknown = set_active_fea_case_impl(&engine, "nonexistent_case") // FAILS TO COMPILE
        .expect("set_active_fea_case_impl with unknown case must not return Err (falls back to lex-first)");
    // After setting an unknown case, get returns Some("nonexistent_case")
    // (the name is stored as-is; apply_fea_channels uses lex-first as the fallback).
    let active_unknown = get_active_fea_case_impl(&engine) // FAILS TO COMPILE
        .expect("get_active_fea_case_impl must succeed after unknown-case set");
    assert_eq!(
        active_unknown,
        Some("nonexistent_case".to_string()),
        "active case stored as given even if not found in cases map"
    );
}
