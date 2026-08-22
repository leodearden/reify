use std::sync::{Arc, Mutex, RwLock};

use crate::tests::test_helpers::{
    assert_rigid_mass_props_determined, assert_rigid_mass_props_final,
    assert_rigid_mass_props_not_final, cwd_lock, find_moi_principal_constraint,
    rigid_mass_props_fixture_path, rigid_mass_props_session,
    rigid_mass_props_session_seeded_with_ops, visible_realization_keys,
};

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

/// Shared 3-level nested-composed fixture (task 5348). `Top` composes two `Mid`
/// subs; each `Mid` composes two `Leaf` subs; each `Leaf` owns a self-contained
/// box. This yields 4 independent leaf realizations at the composed dotted paths
/// `Top.a.p` / `Top.a.q` / `Top.b.p` / `Top.b.q` — 3 nesting levels, matching the
/// repro's `Printer.motion.head_block` depth.
///
/// Leaf geometry is self-contained (a plain `box`, no cross-sub `GeomRef`), so no
/// realization references `self.inner.body`; that would trip the documented v0.1
/// nested sub-of-sub override scope boundary (cross_sub_geometry_e2e.rs:1583-1672).
const NESTED_COMPOSED_SRC: &str = r#"pub structure Leaf {
    let g = box(10mm, 10mm, 10mm)
}
pub structure Mid {
    sub p = Leaf()
    sub q = Leaf()
}
pub structure Top {
    sub a = Mid()
    sub b = Mid()
}"#;

/// Build an `EngineSession` (MockGeometryKernel + SimpleConstraintChecker) with
/// [`NESTED_COMPOSED_SRC`] loaded — the shared nested-composed fixture for the
/// full-scene debug-read tests (task 5348).
fn make_nested_composed_session() -> EngineSession {
    let mut session = make_session();
    session
        .load_from_source(NESTED_COMPOSED_SRC, "nested_composed")
        .expect("load_from_source of NESTED_COMPOSED_SRC should succeed");
    session
}

#[test]
fn app_state_constructible() {
    let session = make_loaded_session();
    let _state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Arc::new(Mutex::new(None)),
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
        last_state: Arc::new(Mutex::new(None)),
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
        last_state: Arc::new(Mutex::new(None)),
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
    assert!(file_data.content.contains("structure def Bracket"));
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

    // bracket_source() starts with "structure def Bracket {" on line 1.
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
        viewport_layout: std::collections::HashMap::new(),
        split_ratio: 0.5,
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
        viewport_layout: std::collections::HashMap::new(),
        split_ratio: 0.5,
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

    // ── Synced: register the visible realization R0 + the displayed thickness
    //    cell through the COMMAND shim before the edit. No panel constraints, so
    //    the constraints fall OUTSIDE the observed cone (would-prune). ─────────
    let synced = Mutex::new(make_loaded_session());
    sync_observed_demand_impl(
        &synced,
        &["Bracket#realization[0]".to_string()],
        &["Bracket.thickness".to_string()],
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
        "the visible realization R0 (+ thickness cell) must be retained, got {}",
        m.observed_retained
    );
    assert_eq!(
        m.would_prune.realization, 0,
        "the visible realization R0 is observed → must NOT be in would_prune; got {}",
        m.would_prune.realization
    );
    assert!(
        would_prune_total > 0,
        "non-observed nodes (volume, constraints) must be counted as would-prune; got {:?}",
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

/// step-5 (task 4741 ε): `commands::engine_state_json` surfaces the two
/// selective-demand observability keys consumed by the debug-MCP /
/// visual-regression harness:
///
/// * `demand_prune_measurement` — the passive would-prune record produced by the
///   most recent edit (mirrors `GuiState.demand_prune_measurement`), and
/// * `last_dispatch_count_post_refresh` — the aggregate per-realization
///   geometry-kernel dispatch tally (surfaced as the sum of
///   `Engine::last_dispatch_count_by_realization`). The `_post_refresh` suffix is
///   deliberate: `engine_state_json` reads it AFTER `build_gui_state`'s internal
///   tessellate, so it reflects that refresh — slider-session attribution lives in
///   the pure-read `demand_dispatch` tool, not this key.
///
/// RED until step-6 extends `engine_state_json` with the two keys — both are
/// absent from the projection today, so the `get(..).expect(..)` lookups panic.
#[test]
fn engine_state_json_surfaces_demand_prune_measurement_and_last_dispatch_count_post_refresh() {
    use crate::commands::{engine_state_json, set_parameter_impl, sync_observed_demand_impl};

    // Drive an observed-demand slider edit through the command shim (mirrors the
    // pattern at commands_tests.rs:740): register the visible realization R0 + the
    // displayed thickness cell, then edit — the edit records the passive
    // would-prune measurement on the engine, which persists for the later
    // `engine_state_json` read (it is set only on the edit path, never cleared by
    // a subsequent `build_gui_state` tessellate).
    let synced = Mutex::new(make_loaded_session());
    sync_observed_demand_impl(
        &synced,
        &["Bracket#realization[0]".to_string()],
        &["Bracket.thickness".to_string()],
        &[],
    )
    .expect("sync_observed_demand_impl should succeed");
    set_parameter_impl(&synced, "Bracket.thickness", "2mm").expect("synced set_parameter");

    // Project the engine state through the debug-MCP helper.
    let mut session = synced
        .into_inner()
        .expect("session mutex must not be poisoned");
    let json = engine_state_json(&mut session).expect("engine_state_json should succeed");

    // (a) demand_prune_measurement: present, non-null, carrying the three
    //     would-prune/observed_retained/eval_set_size sub-fields.
    let m = json
        .get("demand_prune_measurement")
        .expect("engine_state_json must expose a 'demand_prune_measurement' key");
    assert!(
        !m.is_null(),
        "demand_prune_measurement must be populated after an observed-demand edit; got null"
    );
    assert!(
        m.get("would_prune").is_some(),
        "demand_prune_measurement must carry a 'would_prune' breakdown; got {m:?}"
    );
    assert!(
        m.get("observed_retained").is_some(),
        "demand_prune_measurement must carry 'observed_retained'; got {m:?}"
    );
    assert!(
        m.get("eval_set_size").is_some(),
        "demand_prune_measurement must carry 'eval_set_size'; got {m:?}"
    );

    // (b) last_dispatch_count_post_refresh: an unsigned integer (the aggregate
    //     per-realization geometry-kernel dispatch tally surfaced as a sum, read
    //     AFTER build_gui_state's internal tessellate). A warm cached tessellate
    //     may legitimately dispatch zero, so the contract is "present and
    //     integral", not a positive lower bound — the exact sum-vs-aggregate
    //     equality is pinned in the reify-eval unit tests.
    let dispatch = json
        .get("last_dispatch_count_post_refresh")
        .expect("engine_state_json must expose a 'last_dispatch_count_post_refresh' key");
    assert!(
        dispatch.is_u64(),
        "last_dispatch_count_post_refresh must be an unsigned integer; got {dispatch:?}"
    );
}

/// step-3 (task 5348): the debug-MCP `engine_state` tool (via `engine_state_json`)
/// must report the FULL realized scene, not the selective incremental delta, once
/// the frontend has flipped production demand to selective.
///
/// On the nested-composed fixture, `engine_state_json` before any selective sync
/// projects the complete cold full-scope scene (n meshes). After `sync_demand`
/// hides one leaf branch (flipping production demand selective), `engine_state_json`
/// must STILL report n meshes — it routes through `build_gui_state_full_scene`.
///
/// RED until step-4 switches `engine_state_json` to `build_gui_state_full_scene`:
/// today it calls the selective `build_gui_state`, so the post-sync projection is
/// the under-reporting delta (< n) — assertion-failure RED.
#[test]
fn engine_state_json_reports_full_scene_under_selective_demand() {
    use crate::commands::engine_state_json;

    let mut session = make_nested_composed_session();

    // Cold full-scope projection = the complete scene.
    let cold = engine_state_json(&mut session).expect("cold engine_state_json should succeed");
    let cold_meshes = cold["meshes"].as_array().expect("meshes must be an array");
    let n = cold_meshes.len();
    assert!(
        n >= 4,
        "nested-composed fixture must project the 4 leaf bodies; got {n}"
    );
    let all_paths: Vec<String> = cold_meshes
        .iter()
        .map(|m| {
            m["entity_path"]
                .as_str()
                .expect("each mesh must carry an entity_path string")
                .to_string()
        })
        .collect();

    // Hide one leaf branch → flip production demand SELECTIVE.
    let hidden = all_paths
        .last()
        .expect("cold scene must have at least one mesh")
        .clone();
    let visible: Vec<String> = all_paths.iter().filter(|p| **p != hidden).cloned().collect();
    session.sync_demand(&visible);

    // The engine_state tool must report the FULL scene, not the selective delta.
    let after = engine_state_json(&mut session).expect("post-sync engine_state_json should succeed");
    assert_eq!(
        after["meshes"]
            .as_array()
            .expect("meshes must be an array")
            .len(),
        n,
        "engine_state must report the full realized scene under selective demand, not the delta"
    );
}

/// step-5 (task 5348): the PRIMARY acceptance regression test — the debug-MCP
/// `mesh_stats` tool (via the extracted `commands::mesh_stats_json`) must report a
/// mesh-stats entry per rendered body == the scene's body count, on a composed
/// fixture with nested subs at 3+ levels, even under selective demand.
///
/// Mirrors step-3 but through `mesh_stats_json`, and additionally pins the
/// per-entry shape parity with the current `handle_mesh_stats` output.
///
/// RED until step-6 extracts `commands::mesh_stats_json` (compile-error RED).
#[test]
fn mesh_stats_json_reports_full_scene_under_selective_demand() {
    use crate::commands::mesh_stats_json;

    let mut session = make_nested_composed_session();

    // Cold full-scope projection = the complete scene.
    let cold = mesh_stats_json(&mut session).expect("cold mesh_stats_json should succeed");
    let cold_meshes = cold["meshes"].as_array().expect("meshes must be an array");
    let n = cold_meshes.len();
    assert!(
        n >= 4,
        "nested-composed fixture must project the 4 leaf bodies; got {n}"
    );
    let all_paths: Vec<String> = cold_meshes
        .iter()
        .map(|m| {
            m["entity_path"]
                .as_str()
                .expect("each mesh must carry an entity_path string")
                .to_string()
        })
        .collect();

    // Hide one leaf branch → flip production demand SELECTIVE.
    let hidden = all_paths
        .last()
        .expect("cold scene must have at least one mesh")
        .clone();
    let visible: Vec<String> = all_paths.iter().filter(|p| **p != hidden).cloned().collect();
    session.sync_demand(&visible);

    // mesh_stats must report the FULL scene (n entries), not the selective delta.
    let after = mesh_stats_json(&mut session).expect("post-sync mesh_stats_json should succeed");
    let after_meshes = after["meshes"].as_array().expect("meshes must be an array");
    assert_eq!(
        after_meshes.len(),
        n,
        "mesh_stats entry count must equal the scene's rendered body count under selective demand"
    );

    // Per-entry shape parity with handle_mesh_stats.
    for entry in after_meshes {
        for key in [
            "entity_path",
            "vertex_count",
            "face_count",
            "bounding_box",
            "element_kind_count",
        ] {
            assert!(
                entry.get(key).is_some(),
                "each mesh_stats entry must carry '{key}'; got {entry:?}"
            );
        }
    }
}

/// step-6 amendment (task 5348, reviewer test-coverage): the full-scene
/// regression test above only checks key *presence*, and its plain-box fixtures
/// all carry `element_kind: None`, so their `element_kind_count` is always the
/// empty object `{}`. That never exercises `mesh_stats_json`'s populated
/// byte→string-key histogram projection — a regression that mis-serialized a
/// non-empty histogram would slip through.
///
/// The FEA shell flexure fixture tessellates (under `MockGeometryKernel`) to a
/// body mesh with an all-shell `element_kind` (`vec![1; face_count]`, see
/// `build_gui_state_shell_flexure_populates_element_kind_and_von_mises_top` in
/// engine_tests.rs), so `mesh_stats_json` must emit a NON-empty, string-keyed
/// `element_kind_count` object — exactly `{"1": face_count}` — whose counts
/// partition the mesh's faces. This pins the full byte→JSON-key mapping, not just
/// the presence of the key.
#[test]
fn mesh_stats_json_emits_populated_element_kind_histogram() {
    use crate::commands::mesh_stats_json;

    let source = include_str!("../../../../examples/fea_shell_flexure.ri");
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(source, "FeaShellFlexure")
        .expect("load_from_source must succeed for fea_shell_flexure.ri");

    let stats = mesh_stats_json(&mut session).expect("mesh_stats_json should succeed");
    let meshes = stats["meshes"].as_array().expect("meshes must be an array");

    // The shell body's stats entry is the one whose histogram object is non-empty
    // (the plain-box meshes all serialize `element_kind_count` as `{}`).
    let shell_entry = meshes
        .iter()
        .find(|entry| {
            entry["element_kind_count"]
                .as_object()
                .is_some_and(|hist| !hist.is_empty())
        })
        .expect(
            "FEA shell flexure must yield a mesh_stats entry with a populated \
             element_kind histogram (all-shell body)",
        );

    let hist = shell_entry["element_kind_count"]
        .as_object()
        .expect("element_kind_count must serialize as a JSON object");
    let face_count = shell_entry["face_count"]
        .as_u64()
        .expect("face_count must be a JSON number");

    // Byte→string-key mapping contract: keys are decimal-stringified `u8` bytes,
    // values are positive counts. A mis-serialized populated histogram (wrong key
    // type, dropped/duplicated counts) fails here — the full-scene test's
    // key-presence check on box fixtures never reaches this branch.
    for (key, value) in hist {
        assert!(
            key.parse::<u8>().is_ok(),
            "element_kind_count keys must be stringified u8 bytes; got {key:?}"
        );
        assert!(
            value.as_u64().is_some_and(|c| c > 0),
            "each histogram count must be a positive JSON number; got {value:?} for {key:?}"
        );
    }

    // The histogram partitions the faces: its counts sum to face_count, and since
    // every face is a shell triangle (byte 1) it is exactly `{"1": face_count}`.
    let hist_total: u64 = hist.values().filter_map(serde_json::Value::as_u64).sum();
    assert_eq!(
        hist_total, face_count,
        "element_kind histogram counts must partition the mesh's faces (sum == face_count)"
    );
    assert_eq!(
        hist.get("1").and_then(serde_json::Value::as_u64),
        Some(face_count),
        "an all-shell body must classify every face under the '1' (shell) key; got {hist:?}"
    );

    // The shell body has vertices, so its `bounding_box` is the non-null
    // `{min, max}` branch (not the zero-vertex `null` branch) — assert its shape,
    // which the full-scene test's `bounding_box` key-presence check (satisfied
    // even by `null`) cannot distinguish.
    let bbox = shell_entry["bounding_box"]
        .as_object()
        .expect("a mesh with vertices must carry a non-null {min,max} bounding_box");
    for key in ["min", "max"] {
        assert_eq!(
            bbox[key].as_array().map(|a| a.len()),
            Some(3),
            "bounding_box.{key} must be a 3-element array"
        );
    }
}

/// Two-body, constraint-free, param-driven fixture (mirrors the engine-side
/// `SELECTIVE_MULTIBODY_SRC` at engine_tests.rs:13210 and the δ
/// `SELECTIVE_DEMAND_MULTIBODY_SRC`): `w → sa → box a (R0)` and
/// `w → sb → box b (R1)`. Hiding body_b (R1) prunes its exclusive cell `sb`.
const SELECTIVE_MULTIBODY_SRC: &str = r#"pub structure SelectiveMultiBody {
    param w : Length = 10mm
    let sa = w * 3
    let sb = w * 2
    let a = box(sa, sa, sa)
    let b = box(sb, sb, sb)
}"#;

/// step-7 (task 4741 ε): the NEW extracted command
/// `commands::demand_dispatch_json` is a PURE engine read (no `build_gui_state`,
/// so it reflects the e2e's controlled slider tessellate, design-decision-3) and
/// surfaces three selective-demand observability channels:
///
/// * `dispatch_by_realization` — an object keyed by `RealizationNodeId`
///   Display (`Entity#realization[N]`, == the `MeshData.entity_path` join key);
/// * `eval_set` — the production eval-set, each `NodeId` in Display form;
/// * `full_scope` — the cold full-scope override flag.
///
/// Headline assertion (§8 row-2 kernel-saving floor): after hiding body_b via
/// `sync_demand_impl([R0])` + a slider edit, the hidden body_b's realization is
/// dispatched ZERO times (key absent or 0) and is absent from the eval-set,
/// while the visible body_a dispatched at least once.
///
/// RED: `commands::demand_dispatch_json` does not exist yet — the gui test
/// binary fails to compile until step-8 adds it.
#[test]
fn demand_dispatch_json_attributes_zero_dispatch_to_hidden_body() {
    use crate::commands::{demand_dispatch_json, set_parameter_impl, sync_demand_impl};

    let body_a_key = "SelectiveMultiBody#realization[0]";
    let body_b_key = "SelectiveMultiBody#realization[1]";

    let mut loaded = make_session();
    loaded
        .load_from_source(SELECTIVE_MULTIBODY_SRC, "selective")
        .expect("load_from_source should succeed");
    let synced = Mutex::new(loaded);

    // Hide body_b (R1): only body_a (R0) is visible/demanded.
    sync_demand_impl(&synced, &[body_a_key.to_string()]).expect("sync_demand_impl should succeed");
    // Slider edit drives the warm selective tessellate (body_b stays pruned).
    set_parameter_impl(&synced, "SelectiveMultiBody.w", "12mm").expect("slider set_parameter");

    let mut session = synced
        .into_inner()
        .expect("session mutex must not be poisoned");
    let json = demand_dispatch_json(&mut session).expect("demand_dispatch_json should succeed");

    // (a) dispatch_by_realization: an object; hidden body_b dispatched 0 (key
    //     absent or numeric 0); visible body_a dispatched at least once.
    let dispatch = json["dispatch_by_realization"]
        .as_object()
        .expect("dispatch_by_realization must be a JSON object");
    let body_b_dispatch = dispatch.get(body_b_key).and_then(|v| v.as_u64());
    assert!(
        body_b_dispatch.unwrap_or(0) == 0,
        "hidden body_b (R1) must be dispatched ZERO times; got {body_b_dispatch:?}"
    );
    let body_a_dispatch = dispatch
        .get(body_a_key)
        .and_then(|v| v.as_u64())
        .expect("visible body_a (R0) must have a dispatch tally entry");
    assert!(
        body_a_dispatch >= 1,
        "visible body_a (R0) must be dispatched at least once; got {body_a_dispatch}"
    );

    // (b) eval_set: an array of NodeId Display strings NOT containing body_b's
    //     realization (it is pruned from the selective cone).
    let eval_set: Vec<String> = json["eval_set"]
        .as_array()
        .expect("eval_set must be a JSON array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("each eval_set entry must be a string")
                .to_string()
        })
        .collect();
    assert!(
        !eval_set.iter().any(|s| s == body_b_key),
        "hidden body_b's realization must be ABSENT from eval_set; got {eval_set:?}"
    );
    assert!(
        eval_set.iter().any(|s| s == body_a_key),
        "visible body_a's realization must be PRESENT in eval_set; got {eval_set:?}"
    );

    // (c) full_scope: the selective sync left the cold full-scope override OFF.
    assert_eq!(
        json["full_scope"],
        serde_json::Value::Bool(false),
        "a selective sync_demand must leave full_scope == false; got {:?}",
        json["full_scope"]
    );
}

/// step-11 (task 4741 ε): the headline debug-MCP §8 boundary-table e2e —
/// rows 2/3/4 — driven entirely through the GUI command shims and read back
/// through the two ε debug-MCP JSON projections
/// (`demand_dispatch_json` / `engine_state_json`).
///
/// This is the integration gate: it exercises the LANDED β (demand-scoped warm
/// tessellate) / γ (Pending-on-prune + last-substantive surfacing) / δ
/// (selective-cone maintenance + re-demand refresh) chain end-to-end at the GUI
/// boundary and confirms the §8 prune-safety scenarios hold through the
/// operator-facing debug-MCP JSON.
///
/// Fixture: [`SELECTIVE_MULTIBODY_SRC`] — `w → sa=w*3 → box a (R0)` (body_a) and
/// `w → sb=w*2 → box b (R1)` (body_b, hidden). `sb` is body_b's exclusive cell.
///
/// * **row 2 — hidden-body kernel saving.** With body_b hidden via
///   `sync_demand([R0])`, every slider tick dispatches body_b ZERO times
///   (`demand_dispatch_json`) and body_b's realization is absent from the
///   eval-set — the exact-by-construction floor (a pruned realization never
///   enters the eval set → `execute_realization_ops` is never called for it →
///   0 increments, an op-count equality, not a tolerance). The PASSIVE
///   would-prune measurement (observed-demand channel, surfaced via
///   `engine_state_json.demand_prune_measurement`) independently reports the
///   hidden body's realization as pruneable (`would_prune.realization >= 1`).
///
///   The pruneability evidence is measured on the OBSERVED channel against the
///   FULL production eval-set (a SEPARATE session with no selective
///   enforcement): under selective enforcement the hidden realization is already
///   pruned OUT of `last_eval_set` (see the engine-side
///   `per_tick_reset_hidden_body_stays_zero_ops`), so `measure_would_prune`
///   (which reads `last_eval_set`) would not count it. The two signals are
///   complementary: enforcement realizes the saving (dispatch == 0); the
///   observed measurement proves the saving was real (would_prune >= 1) — exactly
///   scenario B of the landed `selective_demand_measurement` harness, surfaced
///   here through the debug-MCP `engine_state_json` projection.
/// * **row 3 — displayed-but-pruned honesty.** `engine_state_json["values"]`'s
///   entry for `sb` carries `freshness == "pending"` and exposes its
///   last-substantive value (γ) — the GUI shows the last good number, never a
///   silently-stale recomputed one (arch §8 prune-safety scenario 3).
/// * **row 4 — un-hide refresh.** After re-admitting body_b via
///   `sync_demand([R0, R1])` and a fresh edit under the now-full cone, body_b
///   RE-REALIZES (its `demand_dispatch_json` tally climbs back to >= 1 and it
///   re-enters the eval-set) and `sb` recomputes to the CURRENT param —
///   byte-matching a fresh cold full-scope build (content oracle, mirroring the
///   δ redemand test) with no stale handle.
#[test]
fn debug_mcp_selective_demand_boundary_rows_2_3_4() {
    use crate::commands::{
        demand_dispatch_json, engine_state_json, set_parameter_impl, sync_demand_impl,
        sync_observed_demand_impl,
    };

    let body_a_key = "SelectiveMultiBody#realization[0]";
    let body_b_key = "SelectiveMultiBody#realization[1]";
    let sb_cell = "SelectiveMultiBody.sb";
    let w_cell = "SelectiveMultiBody.w";

    // Helper: load the multibody fixture into a fresh `Mutex<EngineSession>`.
    let load_session = || {
        let mut s = make_session();
        s.load_from_source(SELECTIVE_MULTIBODY_SRC, "selective")
            .expect("load_from_source should succeed");
        Mutex::new(s)
    };

    // ── Build the hidden-body slider session (body_b R1 hidden). ──────────────
    let session = load_session();
    // Production ENFORCEMENT: only body_a visible → body_b pruned.
    sync_demand_impl(&session, &[body_a_key.to_string()]).expect("sync_demand_impl");

    // ── row 2 (primary, exact floor): N-tick slider — hidden body_b dispatches
    //    0 ops EACH tick and is absent from the eval-set. ───────────────────────
    let ticks = ["12mm", "16mm", "20mm"];
    for (i, w) in ticks.iter().enumerate() {
        set_parameter_impl(&session, w_cell, w)
            .unwrap_or_else(|e| panic!("tick {i}: set_parameter({w}) must succeed: {e}"));

        // `demand_dispatch_json` is a PURE engine read reflecting set_parameter's
        // internal tessellate — read it BEFORE any re-tessellating projection so
        // body_a's freshly-dirtied dispatch is still visible (a later
        // `engine_state_json` re-tessellate would find body_a cached → 0).
        let mut guard = session.lock().expect("session lock");
        let dd = demand_dispatch_json(&mut guard).expect("demand_dispatch_json");
        drop(guard);

        let dispatch = dd["dispatch_by_realization"]
            .as_object()
            .expect("dispatch_by_realization must be an object");
        let b_count = dispatch
            .get(body_b_key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert_eq!(
            b_count, 0,
            "row2 tick {i}: hidden body_b must dispatch ZERO geometry ops; got {b_count}"
        );
        let a_count = dispatch
            .get(body_a_key)
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert!(
            a_count >= 1,
            "row2 tick {i}: visible body_a must re-realize after w changed; got {a_count}"
        );

        let eval_set: Vec<&str> = dd["eval_set"]
            .as_array()
            .expect("eval_set must be an array")
            .iter()
            .map(|v| v.as_str().expect("each eval_set entry must be a string"))
            .collect();
        assert!(
            !eval_set.contains(&body_b_key),
            "row2 tick {i}: hidden body_b must be ABSENT from eval_set; got {eval_set:?}"
        );
        assert_eq!(
            dd["full_scope"],
            serde_json::Value::Bool(false),
            "row2 tick {i}: selective sync_demand leaves full_scope == false"
        );
    }

    // ── row 3 (displayed-but-pruned honesty): after the slider session (body_b
    //    still hidden), engine_state_json's `sb` entry is Pending with its
    //    last-good value (γ surfacing read back through the debug-MCP). ─────────
    let es = {
        let mut guard = session.lock().expect("session lock");
        engine_state_json(&mut guard).expect("engine_state_json")
    };
    let values = es["values"].as_array().expect("values must be an array");
    let sb_vd = values
        .iter()
        .find(|v| v["cell_id"].as_str() == Some(sb_cell))
        .expect("sb must surface in engine_state_json values");
    assert_eq!(
        sb_vd["freshness"].as_str(),
        Some("pending"),
        "row3: hidden body_b's exclusive cell sb must be freshness==pending; got {sb_vd:?}"
    );
    assert!(
        sb_vd
            .get("last_substantive_value")
            .map(|v| !v.is_null())
            .unwrap_or(false),
        "row3: a Pending cell must expose its last-substantive (prior good) value; got {sb_vd:?}"
    );

    // ── row 2 (pruneability evidence): the PASSIVE observed-demand measurement
    //    independently reports body_b's realization as pruneable. Measured on the
    //    OBSERVED channel against the FULL production eval-set (no selective
    //    enforcement), so the hidden realization is counted in would_prune. ─────
    let obs = load_session();
    sync_observed_demand_impl(
        &obs,
        &[body_a_key.to_string()],
        &[sb_cell.to_string()],
        &[],
    )
    .expect("sync_observed_demand_impl");
    set_parameter_impl(&obs, w_cell, "12mm").expect("observed-channel edit");
    let obs_es = {
        let mut guard = obs.lock().expect("session lock");
        engine_state_json(&mut guard).expect("engine_state_json")
    };
    let m = &obs_es["demand_prune_measurement"];
    assert!(
        !m.is_null(),
        "row2: an observed-demand edit must record a demand_prune_measurement; got null"
    );
    let wp_real = m["would_prune"]["realization"]
        .as_u64()
        .expect("would_prune.realization must be an integer");
    assert!(
        wp_real >= 1,
        "row2: the hidden body_b realization must be reported pruneable \
         (would_prune.realization >= 1); got {wp_real}"
    );

    // ── row 4 (un-hide refresh): re-admit body_b → it re-realizes to the CURRENT
    //    param with no stale handle. The oracle is a FRESH cold full-scope build
    //    at the post-un-hide param (content oracle, per the δ redemand test). ───
    sync_demand_impl(
        &session,
        &[body_a_key.to_string(), body_b_key.to_string()],
    )
    .expect("un-hide sync_demand");
    // A fresh edit under the now-full cone re-realizes body_b and recomputes sb.
    set_parameter_impl(&session, w_cell, "25mm").expect("post-un-hide edit");

    // (a) body_b re-realizes (dispatch >= 1) and re-enters the eval-set.
    let dd = {
        let mut guard = session.lock().expect("session lock");
        demand_dispatch_json(&mut guard).expect("demand_dispatch_json")
    };
    let dispatch = dd["dispatch_by_realization"]
        .as_object()
        .expect("dispatch_by_realization must be an object");
    let b_count = dispatch
        .get(body_b_key)
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        b_count >= 1,
        "row4: un-hidden body_b must RE-REALIZE (dispatch >= 1); got {b_count}"
    );
    let eval_set: Vec<&str> = dd["eval_set"]
        .as_array()
        .expect("eval_set must be an array")
        .iter()
        .map(|v| v.as_str().expect("each eval_set entry must be a string"))
        .collect();
    assert!(
        eval_set.contains(&body_b_key),
        "row4: un-hidden body_b must RE-ENTER the eval_set; got {eval_set:?}"
    );

    // (b) content oracle: sb recomputes to the CURRENT param, byte-matching a
    //     fresh cold full-scope build at the same param (no stale value).
    let oracle = load_session();
    let oracle_state =
        set_parameter_impl(&oracle, w_cell, "25mm").expect("oracle full-scope edit");
    let oracle_sb = oracle_state
        .values
        .iter()
        .find(|v| v.cell_id == sb_cell)
        .expect("oracle sb must surface");

    let es2 = {
        let mut guard = session.lock().expect("session lock");
        engine_state_json(&mut guard).expect("engine_state_json")
    };
    let sb_vd2 = es2["values"]
        .as_array()
        .expect("values must be an array")
        .iter()
        .find(|v| v["cell_id"].as_str() == Some(sb_cell))
        .expect("sb must surface after un-hide");
    assert_eq!(
        sb_vd2["freshness"].as_str(),
        Some("final"),
        "row4: re-demanded sb must be refreshed to Final (no longer pending); got {sb_vd2:?}"
    );
    assert_eq!(
        sb_vd2["value"].as_str(),
        Some(oracle_sb.value.as_str()),
        "row4: re-demanded sb value must byte-match a fresh cold full-scope build \
         (no stale handle); got {} vs oracle {}",
        sb_vd2["value"],
        oracle_sb.value
    );
    assert_eq!(
        sb_vd2["unit"].as_str(),
        Some(oracle_sb.unit.as_str()),
        "row4: re-demanded sb unit must match the cold oracle; got {} vs oracle {}",
        sb_vd2["unit"],
        oracle_sb.unit
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

// ── Task 5357 step-5: run_on_large_stack composition guard ───────────────────

/// Sorted key projection of a keyed `GuiState` collection.
fn sorted_keys<T>(items: &[T], key: impl Fn(&T) -> &str) -> Vec<String> {
    let mut keys: Vec<String> = items.iter().map(|i| key(i).to_owned()).collect();
    keys.sort();
    keys
}

/// Assert that two `GuiState`s agree on everything a large-stack relocation
/// could plausibly change: the identity (and hence cardinality) of every keyed
/// collection, plus both diagnostics streams.
///
/// Deliberately NOT a whole-`GuiState` `assert_eq!`. The two states here come
/// from two independently constructed `EngineSession`s, so a full comparison
/// would also assert run-to-run determinism of every float, every vertex buffer
/// and every collection's ORDER — none of which is the property under test. If
/// any `GuiState` field ever became ordering-dependent (e.g. built from a
/// `HashMap`/`HashSet` walk) or gained a timing/counter field, a full comparison
/// would go flaky for a reason with nothing to do with `large_stack`, and the
/// failure would point at the wrong subsystem. These order-insensitive
/// projections are what actually distinguish "ran on the large-stack thread"
/// from "ran inline".
fn assert_same_salient_state(
    wrapped: &crate::types::GuiState,
    direct: &crate::types::GuiState,
    what: &str,
) {
    assert_eq!(
        sorted_keys(&wrapped.files, |f| &f.path),
        sorted_keys(&direct.files, |f| &f.path),
        "{what}: the set of loaded file paths must not depend on which thread the compile ran on"
    );
    assert_eq!(
        sorted_keys(&wrapped.meshes, |m| &m.entity_path),
        sorted_keys(&direct.meshes, |m| &m.entity_path),
        "{what}: the set of realized mesh entity_paths must be identical"
    );
    assert_eq!(
        sorted_keys(&wrapped.values, |v| &v.cell_id),
        sorted_keys(&direct.values, |v| &v.cell_id),
        "{what}: the set of value cell_ids must be identical"
    );
    assert_eq!(
        sorted_keys(&wrapped.constraints, |c| &c.node_id),
        sorted_keys(&direct.constraints, |c| &c.node_id),
        "{what}: the set of constraint node_ids must be identical"
    );
    assert_eq!(
        wrapped.compile_diagnostics.len(),
        direct.compile_diagnostics.len(),
        "{what}: compile diagnostics count must be identical; wrapped={:?} direct={:?}",
        wrapped.compile_diagnostics,
        direct.compile_diagnostics
    );
    assert_eq!(
        wrapped.tessellation_diagnostics.len(),
        direct.tessellation_diagnostics.len(),
        "{what}: tessellation diagnostics count must be identical; wrapped={:?} direct={:?}",
        wrapped.tessellation_diagnostics,
        direct.tessellation_diagnostics
    );
}

/// `open_file_engine_impl` invoked THROUGH `run_on_large_stack` returns the same
/// `Ok(GuiState)` as a direct (un-wrapped) call.
///
/// This proves the large-stack helper composes safely with the real engine /
/// `with_engine_lock` / compile path when the compile runs on a plain `std`
/// thread (no tokio-context `blocking_send` issue; identical result). It is the
/// headless proxy for the un-headless-testable `open_file_engine` / `update_source`
/// Tauri command wiring (step-6): those commands cannot be built headlessly, so
/// this exercises the exact `run_on_large_stack(|| open_file_engine_impl(..))`
/// composition they perform.
#[test]
fn open_file_engine_impl_runs_correctly_through_large_stack() {
    use crate::commands::open_file_engine_impl;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bracket.ri");
    std::fs::write(&file, bracket_source()).unwrap();
    // Absolute path → canonicalization needs no cwd change (unlike the relative
    // sibling test above), so no `cwd_lock` is required.
    let path = file.to_str().unwrap();

    // Direct (un-wrapped) call on a fresh engine.
    let engine_direct = Mutex::new(EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    ));
    let direct = open_file_engine_impl(&engine_direct, path)
        .expect("direct open_file_engine_impl should succeed");

    // The same call routed through the large-stack helper on an identically
    // initialized fresh engine. The scoped closure borrows `&engine_wrapped` /
    // `path` directly — no `Arc` clone, no `'static` bound.
    let engine_wrapped = Mutex::new(EngineSession::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    ));
    let wrapped =
        crate::large_stack::run_on_large_stack(|| open_file_engine_impl(&engine_wrapped, path))
            .expect("open_file_engine_impl through run_on_large_stack should succeed");

    // Concrete expected shape (independent of the direct half).
    assert!(
        !wrapped.files.is_empty(),
        "GuiState.files should be non-empty after loading a file on the large stack"
    );
    assert!(
        !wrapped.meshes.is_empty(),
        "GuiState.meshes should be non-empty after a clean compile on the large stack"
    );
    assert!(
        wrapped.compile_diagnostics.is_empty(),
        "a clean bracket_source compile on the large stack must emit no compile diagnostics; \
         got: {:?}",
        wrapped.compile_diagnostics
    );

    assert_same_salient_state(&wrapped, &direct, "open_file_engine_impl");
}

/// `reload_for_watch_impl` invoked THROUGH `run_on_large_stack` returns the same
/// salient state as a direct (un-wrapped) call.
///
/// The sibling of the guard above, for the OTHER half of the step-6 wiring.
/// TWO distinct call sites perform exactly this composition, and both are
/// un-headless-testable (they take `tauri::AppHandle` / `tauri::State`):
/// the frontend-invoked `main.rs::update_source` command, and
/// `main.rs::create_watcher`'s `FileEvent::Changed` callback — the latter being
/// the recompile reached on EVERY watch-triggered on-disk reload, i.e. the
/// highest-frequency of the wrapped compile paths.
#[test]
fn reload_for_watch_impl_runs_correctly_through_large_stack() {
    use crate::commands::reload_for_watch_impl;

    // Direct (un-wrapped) call on a freshly loaded engine.
    let engine_direct = make_test_engine_for_commands();
    let direct = reload_for_watch_impl(&engine_direct, "bracket.ri", bracket_source())
        .expect("direct reload_for_watch_impl should succeed");

    // The same call routed through the large-stack helper on an identically
    // initialized engine. The scoped closure borrows `&engine_wrapped` directly.
    let engine_wrapped = make_test_engine_for_commands();
    let wrapped = crate::large_stack::run_on_large_stack(|| {
        reload_for_watch_impl(&engine_wrapped, "bracket.ri", bracket_source())
    })
    .expect("reload_for_watch_impl through run_on_large_stack should succeed");

    assert!(
        !wrapped.meshes.is_empty(),
        "GuiState.meshes should be non-empty after a successful reload on the large stack"
    );
    assert!(
        wrapped.compile_diagnostics.is_empty(),
        "a successful reload on the large stack must emit no compile diagnostics; got: {:?}",
        wrapped.compile_diagnostics
    );

    assert_same_salient_state(&wrapped, &direct, "reload_for_watch_impl");
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
        last_state: Arc::new(Mutex::new(None)),
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
        last_state: Arc::new(Mutex::new(None)),
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
        last_state: Arc::new(Mutex::new(None)),
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
        structured_detail: vec![],
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

// ── Task #5338: Rigid mass-prop cells must survive every GUI load path ────────
//
// `TessellateResult` is an INCREMENTAL DELTA (see the DELTA CONTRACT block on
// `Engine::demand_scoped_unified_pass`). Under the frontend's SELECTIVE demand
// posture a HASH-EXEMPT realization's kernel ops are skipped, so its auto-derived
// mass-property cells arrive `Undef` even though their values are unchanged and
// still correct. These tests drive the REAL production command entry points
// headlessly (no Tauri runtime) and assert the cells never degrade.
//
// Faithfulness note: `check()` turns the cold `full_scope` override back ON
// (engine_eval.rs), and only `sync_demand` turns it off again — so every reload /
// recompile is followed here by a fresh `sync_demand_impl`, exactly as the
// frontend re-syncs demand after each re-render. Without that re-sync the
// rebuilds would run under full scope and the delta gap would be unreachable.

/// Copy the committed `examples/rigid_mass_props_smoke.ri` fixture into `dir`
/// so a reload can rewrite it without touching the tracked file. Returns the
/// path (as a `String`, the shape the watcher/`update_source` commands take)
/// alongside the original text.
fn rigid_mass_props_tempfile(dir: &std::path::Path) -> (String, String) {
    let text = std::fs::read_to_string(rigid_mass_props_fixture_path())
        .expect("the committed rigid_mass_props_smoke.ri fixture must be readable");
    let path = dir.join("rigid_mass_props_smoke.ri");
    std::fs::write(&path, &text).expect("writing the fixture copy should succeed");
    (path.to_string_lossy().into_owned(), text)
}

/// Task #5338 (step-3, RED): a watcher-driven reload must not degrade a `: Rigid`
/// body's auto-derived mass-property cells — neither on the reload build itself
/// nor on the SELECTIVE-demand re-renders that follow it, which are the states
/// the GUI actually paints.
///
/// `reload_for_watch_impl` is the watcher's exact entry point (main.rs wires the
/// notify callback straight to it). It routes through `update_source_impl` →
/// `EngineSession::update_source` → `commit_state` → `build_gui_state`.
///
/// GREEN as landed — a regression LOCK, not a reproduction. The task's plan
/// predicted this would be RED until `commit_state`'s cache clear was narrowed to
/// `FilePathUpdate::Set`, on the theory that a same-file reload leaves the
/// realization hash-exempt while the clear has just discarded the retained value.
/// MEASURED, that cannot happen: `input_cone_hash` is a field on the realization
/// node inside `eval_state.snapshot.graph`, and `check()` replaces `eval_state`
/// wholesale, so a recompile resets every hash to `None`. The first selective
/// tessellate after ANY recompile dispatches and repopulates the cache from a
/// complete delta; only the SECOND and later ones are hash-exempt, and no
/// `commit_state` runs between them. Hence the clear stays unconditional (see
/// `EngineSession::commit_state`) and this test locks the watcher path against
/// the delta-gap regression that the engine-level test reproduced.
///
/// It still earns its keep, and is not vacuous: it is the only coverage that
/// drives the watcher's REAL entry point end-to-end, and a negative control
/// (retention fallback removed, i.e. task 5194's snapshot behaviour restored)
/// puts it RED at "reload#1 re-render 2" — re-render 1 dispatches because the
/// recompile reset the hash, re-render 2 is hash-exempt.
///
/// The second reload carries CHANGED content (`depth = 250mm`) and asserts
/// `depth` reads `"250"`: that proves the fix refreshes from the fresh delta
/// rather than replaying a stale cached mass. `depth` is a plain arithmetic cell
/// resolved by the kernel-less check, not a geometry query — the only magnitude
/// asserted anywhere in this suite.
#[test]
fn rigid_mass_props_survive_watcher_reload() {
    use crate::commands::{
        get_initial_state_impl, open_file_engine_impl, reload_for_watch_impl, sync_demand_impl,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let (path, text) = rigid_mass_props_tempfile(dir.path());
    let engine = Arc::new(Mutex::new(rigid_mass_props_session()));

    // (1) Open through the shared #5193 funnel.
    let state = open_file_engine_impl(&engine, &path).expect("open_file_engine_impl should succeed");
    assert_rigid_mass_props_determined(&state, "open_file");

    // (2) The frontend's post-load handshake → SELECTIVE demand.
    let keys = visible_realization_keys(&state);
    assert!(
        !keys.is_empty(),
        "the fixture must render at least one realization mesh, else sync_demand \
         is a no-op and this test is vacuous"
    );
    sync_demand_impl(&engine, &keys).expect("sync_demand_impl should succeed");

    // (3) Watcher fires with UNCHANGED content (a touch / no-op save). The reload
    //     build itself runs under the full scope `check()` restored, so the
    //     degradation lands on the re-renders that follow it.
    let state = reload_for_watch_impl(&engine, &path, &text)
        .expect("reload_for_watch_impl with unchanged content should succeed");
    assert_rigid_mass_props_determined(&state, "reload#1 (unchanged)");

    sync_demand_impl(&engine, &visible_realization_keys(&state))
        .expect("sync_demand_impl after reload#1 should succeed");
    for i in 1..=2 {
        let state = get_initial_state_impl(&engine)
            .unwrap_or_else(|e| panic!("get_initial_state_impl #{i} after reload#1: {e}"));
        assert_rigid_mass_props_determined(&state, &format!("reload#1 re-render {i}"));
    }

    // (4) Watcher fires with CHANGED content — the edit must actually take.
    let changed = text.replace("param depth : Length = 300mm", "param depth : Length = 250mm");
    assert_ne!(
        changed, text,
        "the depth-param rewrite must actually change the fixture text; the \
         fixture's `param depth : Length = 300mm` line may have been reworded"
    );
    let state = reload_for_watch_impl(&engine, &path, &changed)
        .expect("reload_for_watch_impl with changed content should succeed");
    let depth = state
        .values
        .iter()
        .find(|v| v.name == "depth")
        .expect("expected a `depth` value cell after the changed reload");
    assert_eq!(
        depth.value, "250",
        "depth must read 250 (mm) after the changed reload — proving the reload \
         took effect rather than replaying a stale cached state; got {:?}",
        depth.value
    );
    assert_rigid_mass_props_determined(&state, "reload#2 (depth=250mm)");

    // (5) …and the re-renders after the changed reload must hold too.
    sync_demand_impl(&engine, &visible_realization_keys(&state))
        .expect("sync_demand_impl after reload#2 should succeed");
    for i in 1..=2 {
        let state = get_initial_state_impl(&engine)
            .unwrap_or_else(|e| panic!("get_initial_state_impl #{i} after reload#2: {e}"));
        assert_rigid_mass_props_determined(&state, &format!("reload#2 re-render {i}"));
    }
}

/// Task #5338 (step-5): the startup-argv funnel must deliver the SAME contract as
/// the File-Open funnel.
///
/// `main()`'s argv block used to call `EngineSession::load_file` inline and
/// DISCARD the returned `GuiState`, so `UnresolvedGuiState::resolve` never ran on
/// that path. Three contract points follow, each asserted below:
///
/// (a) the mass-prop cells surface — the argv launch is the entry point the
///     2026-07-22 dogfood retest reported as broken;
/// (b) every `files[].path` in the RETURNED state is a canonical ABSOLUTE path —
///     the #5193 identity contract, produced only by `UnresolvedGuiState::resolve`;
/// (c) the state agrees with `open_file_engine_impl`'s for the same file — same
///     `files[].path` set and same value-cell determinacy map — so the two entry
///     points cannot silently drift apart again.
///
/// WHAT (b) DOES NOT SAY. This pins the contract at the FUNNEL boundary, not the
/// pixels of an argv-launched GUI. `resolve` mutates only the returned `GuiState`,
/// and `main()` uses that return value for its `Err` arm alone; the frontend's
/// startup path is `initApp` → `get_initial_state` → `build_gui_state`, which
/// rebuilds `files[]` from the stem-only `source_map()` keys. So this test stays
/// green whether or not an argv launch ever paints absolute paths — read it as
/// "the two funnels agree", never as "the #5193 split is closed end to end". See
/// `commands::load_initial_file_impl`'s docs for the follow-up that would close it.
///
/// RED before the accompanying impl: `commands::load_initial_file_impl` does not
/// exist, so this does not compile.
#[test]
fn load_initial_file_impl_matches_open_file_engine_impl_contract() {
    use crate::commands::{load_initial_file_impl, open_file_engine_impl};

    let dir = tempfile::tempdir().expect("tempdir");
    let (path, _text) = rigid_mass_props_tempfile(dir.path());
    let canonical = std::fs::canonicalize(&path).expect("fixture copy must canonicalize");

    // ── argv path ──
    let argv_engine = Arc::new(Mutex::new(rigid_mass_props_session()));
    let argv_state = load_initial_file_impl(&argv_engine, &canonical)
        .expect("load_initial_file_impl should succeed for the fixture copy");

    // (a) the headline defect.
    assert_rigid_mass_props_determined(&argv_state, "argv");

    // (b) the #5193 identity contract, AS PINNED AT THIS FUNNEL's boundary — see
    //     the "WHAT (b) DOES NOT SAY" paragraph above before reading this as an
    //     end-to-end guarantee for an argv-launched GUI.
    assert!(
        !argv_state.files.is_empty(),
        "an argv load must report at least one file entry"
    );
    for f in &argv_state.files {
        let p = std::path::Path::new(&f.path);
        assert!(
            p.is_absolute(),
            "the `files[].path` RETURNED by load_initial_file_impl must be an absolute \
             canonical path (the #5193 contract, produced by UnresolvedGuiState::resolve); \
             got {:?}",
            f.path
        );
        assert_eq!(
            p,
            canonical.as_path(),
            "the returned `files[].path` must equal the canonicalized fixture path; got {:?}",
            f.path
        );
    }

    // ── File-Open path, same file, freshly seeded session ──
    let open_engine = Arc::new(Mutex::new(rigid_mass_props_session()));
    let open_state = open_file_engine_impl(&open_engine, &path)
        .expect("open_file_engine_impl should succeed for the same fixture copy");

    // (c) the two entry points must agree. Mesh vertex data is excluded: it is
    //     kernel output, not part of the load contract under test.
    let argv_files: Vec<&str> = argv_state.files.iter().map(|f| f.path.as_str()).collect();
    let open_files: Vec<&str> = open_state.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        argv_files, open_files,
        "argv and File-Open must report the SAME files[].path set for the same file"
    );

    let determinacy_map = |s: &crate::types::GuiState| -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = s
            .values
            .iter()
            .map(|c| {
                (
                    format!("{}.{}", c.entity_path, c.name),
                    c.determinacy.clone(),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        determinacy_map(&argv_state),
        determinacy_map(&open_state),
        "argv and File-Open must produce the SAME value-cell determinacy map"
    );
}

/// Task #5338 (step-5): a CWD-relative argv spelling must round-trip through
/// `resolve_initial_file_path` → `load_initial_file_impl`.
///
/// This is the full production argv chain: `main()` takes the raw
/// `std::env::args().nth(1)` string, canonicalises it with
/// `resolve_initial_file_path`, and hands the result to `load_initial_file_impl`.
/// Serialised on `cwd_lock()` per the `main_helpers_tests.rs` idiom, since it
/// mutates the process CWD.
#[test]
fn resolve_initial_file_path_then_load_initial_file_impl_round_trips_relative_argv() {
    use crate::commands::{load_initial_file_impl, resolve_initial_file_path};

    let dir = tempfile::tempdir().expect("tempdir");
    let (path, _text) = rigid_mass_props_tempfile(dir.path());
    let expected = std::fs::canonicalize(&path).expect("fixture copy must canonicalize");

    let engine = Arc::new(Mutex::new(rigid_mass_props_session()));

    let _guard = cwd_lock().lock().unwrap();
    let original = std::env::current_dir().expect("current_dir");
    std::env::set_current_dir(dir.path()).expect("set_current_dir into the tempdir");
    // Resolve while the CWD is the tempdir; restore immediately so a panic in the
    // assertions below cannot leave the process in the temp directory.
    let resolved = resolve_initial_file_path("rigid_mass_props_smoke.ri");
    std::env::set_current_dir(&original).expect("restore current_dir");

    let resolved = resolved.expect("a CWD-relative .ri argv spelling must resolve to Some(path)");
    assert_eq!(
        resolved, expected,
        "resolve_initial_file_path must canonicalise the relative argv spelling"
    );

    let state = load_initial_file_impl(&engine, &resolved)
        .expect("load_initial_file_impl should succeed for the resolved relative argv path");
    assert_rigid_mass_props_determined(&state, "argv (relative spelling)");
}

/// Task #5338 (step-7) — the headline deliverable: ONE regression matrix over
/// every GUI entry point that can load or rebuild a `: Rigid` body.
///
/// Four rows, each on its own freshly-seeded session and its own tempdir copy of
/// the fixture, so no row can mask another:
///
/// | row              | entry point                                       |
/// |------------------|---------------------------------------------------|
/// | `argv`           | `load_initial_file_impl`                          |
/// | `open_file`      | `open_file_engine_impl`                           |
/// | `watcher`        | open, then `reload_for_watch_impl`                |
/// | `warm edit_param`| open, then `set_parameter_impl(depth, 250mm)`     |
///
/// The last row covers the path task 5194's own details flagged as unverified.
///
/// Every row then runs the IDENTICAL post-condition sweep:
///
/// 1. the state the entry point itself returned;
/// 2. after `sync_demand_impl` with the keys derived from that state, THREE
///    successive `get_initial_state_impl` calls — the selective-demand
///    re-renders the GUI actually paints, and where the hash-exempt delta gap
///    bites (the second and later ones);
/// 3. `build_gui_state_full_scene` — the projection the debug-MCP `engine_state`
///    tool reads (commands.rs `engine_state_json`), i.e. the surface the
///    2026-07-22 dogfood retest observed. It must agree with the panel.
///
/// Each row names itself in the assertion context, so a failure identifies its
/// entry point without a bisect.
#[test]
fn rigid_mass_props_determined_across_all_gui_load_paths() {
    use crate::commands::{
        get_initial_state_impl, load_initial_file_impl, open_file_engine_impl,
        reload_for_watch_impl, set_parameter_impl, sync_demand_impl,
    };

    /// How a row gets its first `GuiState`. Each variant is a real production
    /// entry point; none of them is a test-only shortcut.
    enum Entry {
        Argv,
        OpenFile,
        Watcher,
        WarmEditParam,
    }

    for (row, entry) in [
        ("argv", Entry::Argv),
        ("open_file", Entry::OpenFile),
        ("watcher", Entry::Watcher),
        ("warm edit_param", Entry::WarmEditParam),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, text) = rigid_mass_props_tempfile(dir.path());
        let engine = Arc::new(Mutex::new(rigid_mass_props_session()));

        // ── (1) the entry point itself ──
        let state = match entry {
            Entry::Argv => {
                let canonical = std::fs::canonicalize(&path).expect("canonicalize");
                load_initial_file_impl(&engine, &canonical)
                    .unwrap_or_else(|e| panic!("[{row}] load_initial_file_impl: {e}"))
            }
            Entry::OpenFile => open_file_engine_impl(&engine, &path)
                .unwrap_or_else(|e| panic!("[{row}] open_file_engine_impl: {e}")),
            Entry::Watcher => {
                open_file_engine_impl(&engine, &path)
                    .unwrap_or_else(|e| panic!("[{row}] open before reload: {e}"));
                reload_for_watch_impl(&engine, &path, &text)
                    .unwrap_or_else(|e| panic!("[{row}] reload_for_watch_impl: {e}"))
            }
            Entry::WarmEditParam => {
                open_file_engine_impl(&engine, &path)
                    .unwrap_or_else(|e| panic!("[{row}] open before edit: {e}"));
                let edited = set_parameter_impl(&engine, "RigidMassSmoke.depth", "250mm")
                    .unwrap_or_else(|e| panic!("[{row}] set_parameter_impl: {e}"));
                let depth = edited
                    .values
                    .iter()
                    .find(|v| v.name == "depth")
                    .unwrap_or_else(|| panic!("[{row}] expected a `depth` cell after the edit"));
                assert_eq!(
                    depth.value, "250",
                    "[{row}] depth must read 250 (mm) after the warm edit, proving the \
                     edit took effect rather than replaying a stale state; got {:?}",
                    depth.value
                );
                edited
            }
        };
        assert_rigid_mass_props_determined(&state, &format!("{row}: entry state"));

        // ── (2) the selective-demand re-renders the GUI actually paints ──
        let keys = visible_realization_keys(&state);
        assert!(
            !keys.is_empty(),
            "[{row}] the entry state must render at least one realization mesh, else \
             sync_demand is a no-op and this row is vacuous"
        );
        sync_demand_impl(&engine, &keys)
            .unwrap_or_else(|e| panic!("[{row}] sync_demand_impl: {e}"));

        for i in 1..=3 {
            let state = get_initial_state_impl(&engine)
                .unwrap_or_else(|e| panic!("[{row}] get_initial_state_impl #{i}: {e}"));
            assert_rigid_mass_props_determined(&state, &format!("{row}: re-render {i}"));
        }

        // ── (3) the debug-MCP `engine_state` projection must agree with the panel ──
        let full_scene = crate::engine_lock::with_engine_lock(&engine, |s| {
            s.build_gui_state_full_scene()
        })
        .and_then(std::convert::identity)
        .unwrap_or_else(|e| panic!("[{row}] build_gui_state_full_scene: {e}"));
        assert_rigid_mass_props_determined(&full_scene, &format!("{row}: full-scene debug read"));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task #5338 amendment — the OTHER half of the retention mechanism.
//
// The tests above all call `sync_demand` with the COMPLETE visible key set, so
// they exercise only RETENTION. These four exercise the two ways retention must
// NOT fire: the `sync_demand` prune (a hidden entity's cached cells are dropped,
// arch §8) and the dispatch discriminator (a realization that re-ran and
// resolved to nothing must not replay its previous value). Without them a
// regression that deleted the `retain` line, inverted its predicate, or made
// retention unconditional on `Undef` would leave the whole suite green.
// ─────────────────────────────────────────────────────────────────────────────

/// Two INDEPENDENT `: Rigid` bodies — two entities, one realization each — so a
/// `sync_demand` that names only `RigidBodyA`'s key hides `RigidBodyB` WHOLE.
/// That is the granularity the prune is exact at (see `EngineSession::sync_demand`
/// docs), and the granularity a `: Rigid` body's entity-level mass-prop cells
/// actually live at.
///
/// Body A's `depth` is hoisted into a defaulted `param` so a warm `set_parameter`
/// can re-dispatch body A while body B stays hash-exempt
/// (`warm_edit_does_not_collaterally_drop_another_bodys_retained_mass_props`). It
/// is INERT for the consumers that never edit: at load `depth = 300mm`, so the box
/// receives the same scalar args a literal would give it, and
/// `compute_realization_upstream_values_hash_from_ops` folds the op's arg VALUES —
/// the input cone only moves once `set_parameter` is actually called. That is why
/// one fixture serves all three consumers; an earlier round carried a second,
/// literal-armed copy of this source on a constant-cone premise that hoisting does
/// not in fact disturb.
const RIGID_TWO_BODY_SRC: &str = r#"structure def RigidBodyA : Rigid {
    param depth : Length = 300mm
    param geometry : Solid = box(100mm, 100mm, depth)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}

structure def RigidBodyB : Rigid {
    param geometry : Solid = box(50mm, 50mm, 150mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}"#;

/// ONE `: Rigid` entity carrying TWO realizations (the `Rigid`-flavoured twin of
/// `SELECTIVE_MULTIBODY_SRC`): `geometry` realizes as `#realization[0]`, the
/// extra `let` box as `#realization[1]`. Hiding just `[1]` is the PARTIAL hide
/// the entity-granular prune deliberately does not catch.
const RIGID_TWO_REALIZATION_SRC: &str = r#"structure def RigidTwoRealizations : Rigid {
    param geometry : Solid = box(100mm, 100mm, 300mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    let aux = box(50mm, 50mm, 50mm)
}"#;

/// The subset of `visible_realization_keys` belonging to `entity` — what the
/// frontend sends after the user hides everything else.
fn realization_keys_for(state: &crate::types::GuiState, entity: &str) -> Vec<String> {
    let prefix = format!("{entity}#realization[");
    let keys: Vec<String> = visible_realization_keys(state)
        .into_iter()
        .filter(|k| k.starts_with(&prefix))
        .collect();
    assert!(
        !keys.is_empty(),
        "expected at least one `{prefix}N]` mesh key; a vacuous key list would make \
         sync_demand a no-op and the calling test meaningless. Have: {:?}",
        visible_realization_keys(state)
    );
    keys
}

/// Task #5338 amendment (reviewer suggestion 3) — pins the `sync_demand` PRUNE
/// predicate, not just the retention it guards.
///
/// Sequence: load both bodies, `sync_demand` with BOTH keys and rebuild so BOTH
/// bodies' mass-prop cells land in `geometry_derived_cache`, then `sync_demand`
/// with ONLY body A's key and rebuild. Body B is now hidden, so its realization
/// is neither dispatched nor demanded — arch §8 ("a pruned realization's cached
/// result is never served as Final") requires its cached cells be dropped rather
/// than re-surfaced as `determined` / `final`.
///
/// Body A must stay Final throughout: the prune must drop the hidden entity's
/// entries WITHOUT collaterally dropping the visible one's, which is what
/// distinguishes a correct predicate from `retain(|_, _| false)`.
#[test]
fn hidden_rigid_body_mass_props_are_not_served_as_final() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_TWO_BODY_SRC, "two_body")
        .expect("load_from_source should succeed for the two-Rigid-body source");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "cold load");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "cold load");

    // (1) BOTH visible: both bodies' cells are cached.
    session.sync_demand(&visible_realization_keys(&state));
    let state = session.build_gui_state().expect("rebuild, both visible");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "both visible");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "both visible");

    // (2) Hide body B. The prune must drop ITS entries and keep body A's.
    let a_keys = realization_keys_for(&state, "RigidBodyA");
    session.sync_demand(&a_keys);
    let state = session.build_gui_state().expect("rebuild, body B hidden");
    assert_rigid_mass_props_not_final(&state, "RigidBodyB", "body B hidden");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "body B hidden");
}

/// Task #5338 amendment (reviewer suggestion 1) — `build_gui_state_full_scene`
/// must not leak a HIDDEN entity's freshly-resolved cells into the retention
/// cache.
///
/// The debug-MCP `engine_state` / `mesh_stats` projection forces `full_scope` for
/// one build, so `tessellate_snapshot` dispatches EVERY realization — including
/// hidden ones. Those values bypass the `sync_demand` prune chokepoint entirely.
/// If they persisted past the read, the very next SELECTIVE rebuild would find
/// the hidden body's cell `Undef` in the delta, hit the leaked entry, and paint
/// it `determined` / `final` — the arch §8 violation the prune exists to prevent.
///
/// The final assertion is deliberately AFTER the debug read, which is exactly
/// where `rigid_mass_props_determined_across_all_gui_load_paths` stops and
/// therefore cannot observe the after-effect.
#[test]
fn full_scene_debug_read_does_not_leak_hidden_cells_into_the_retention_cache() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_TWO_BODY_SRC, "two_body")
        .expect("load_from_source should succeed for the two-Rigid-body source");

    // Hide body B from the start, so nothing legitimately caches its cells.
    let a_keys = realization_keys_for(&state, "RigidBodyA");
    session.sync_demand(&a_keys);
    let state = session.build_gui_state().expect("selective rebuild");
    assert_rigid_mass_props_not_final(&state, "RigidBodyB", "before the debug read");

    // The debug read DOES see body B — that is its whole purpose, and asserting
    // it here proves the full-scope override really did dispatch the hidden
    // realization (i.e. the leak this test guards against is genuinely reachable).
    let full_scene = session
        .build_gui_state_full_scene()
        .expect("build_gui_state_full_scene");
    assert_rigid_mass_props_final(&full_scene, "RigidBodyB", "full-scene debug read");

    // …but it must leave no trace in the production posture.
    let state = session
        .build_gui_state()
        .expect("rebuild after the debug read");
    assert_rigid_mass_props_not_final(&state, "RigidBodyB", "after the debug read");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "after the debug read");
}

/// Task #5338 amendment (reviewer suggestion 2) — pins the DOCUMENTED
/// entity-granular approximation in `EngineSession::sync_demand`.
///
/// `ValueCellId` is `(entity, member)` and carries no realization index, so the
/// prune joins on the entity half only: an entity keeps its cached cells while
/// ANY of its realizations is visible. Here `RigidTwoRealizations#realization[1]`
/// (the `aux` box) is hidden while `[0]` stays visible, so the entity's mass-prop
/// cells are retained.
///
/// That is CORRECT for this shape — the mass props derive from `geometry`, i.e.
/// realization `[0]`, which is still visible and demanded — but it is retention
/// by entity-level approximation, not by proof that the owning realization is
/// live. This test exists so the approximation is executable rather than prose:
/// a future change to realization-granular association should make it fail and
/// force a conscious update, and `sync_demand`'s "Known limitation" section
/// should be revised with it.
#[test]
fn multi_realization_partial_hide_retains_at_entity_granularity() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_TWO_REALIZATION_SRC, "two_realizations")
        .expect("load_from_source should succeed for the two-realization source");

    let keys = visible_realization_keys(&state);
    assert!(
        keys.len() >= 2,
        "the fixture must render at least TWO realizations of the one entity, else \
         this test does not exercise a PARTIAL hide at all; got {keys:?}"
    );
    session.sync_demand(&keys);
    let state = session.build_gui_state().expect("rebuild, all visible");
    assert_rigid_mass_props_final(&state, "RigidTwoRealizations", "all realizations visible");

    // Hide exactly one realization of the entity; the entity itself stays visible.
    let kept: Vec<String> = keys.iter().take(1).cloned().collect();
    session.sync_demand(&kept);
    let state = session
        .build_gui_state()
        .expect("rebuild, one realization hidden");
    assert_rigid_mass_props_final(
        &state,
        "RigidTwoRealizations",
        "one realization hidden (entity still visible)",
    );
}

/// Task #5338 amendment (reviewer suggestion 5) — a realization that RE-RAN and
/// resolved to nothing must not have its previous value replayed as Final.
///
/// MEASURED on this branch: the delta encodes a hash-exempt gap and a genuine
/// degeneration IDENTICALLY, as an explicit `Value::Undef` entry (never an absent
/// key), so `Undef` alone cannot discriminate. `surface_geometry_derived_cells`
/// therefore keys on `result.meshes` — the delta's own dispatch record — and
/// retains only when the cell's realization did NOT run this pass.
///
/// The degeneration is induced without OCCT by NARROWING the
/// `MockGeometryKernel` seed: the mock's `next_id` is monotonic and never reset,
/// each dispatch of the box allocates the next id, and seeding `1..=2` leaves the
/// warm `set_parameter` dispatch UNANSWERED. Its geometry queries then fail with
/// an `OpContractViolation` — exactly the shape of a failing kernel query or
/// degenerate geometry in production, and encoded in the delta exactly as a
/// hash-exempt gap is.
///
/// Reviewer suggestion 4: seed-range starvation couples this test to the number
/// of kernel dispatches the production path happens to perform, which is a
/// fragile way to say "the geometry query failed". The knob that would fix it
/// properly (`with_volume_error(h, ..)` / `fail_after_n_dispatches` on
/// `MockGeometryKernel`) lives in `crates/reify-test-support`, outside this task's
/// locked scope, so the coupling is instead made EXPLICIT: step (3) asserts,
/// against the mock's own operation log, that the post-edit rebuild really did
/// dispatch an op whose handle is past the seeded ceiling. A drift in dispatch
/// count now fails with a message naming the handles it saw, instead of failing
/// downstream on a determinacy assertion. The injectable form is filed under
/// ticket `tkt_0RSRP1HKTPG0E9XB0YWQVC0RT0`.
///
/// Pre-amendment this test FAILS: the edited rebuild's `Undef` was read as a
/// delta gap and the pre-edit mass was re-surfaced `determined` / `final` /
/// `reason = None`, i.e. a stale value presented as fresh and authoritative.
#[test]
fn degenerate_geometry_after_rebuild_clears_the_retained_mass_props() {
    /// The mock seed ceiling: a dispatched op that allocates a handle above this
    /// has no volume / centroid / inertia reply, so its geometry queries fail.
    const SEED_CEILING: u64 = 2;

    let dir = tempfile::tempdir().expect("tempdir");
    let (path, _text) = rigid_mass_props_tempfile(dir.path());
    let (session, ops) = rigid_mass_props_session_seeded_with_ops(1..=SEED_CEILING);
    let engine = Arc::new(Mutex::new(session));

    // (1) Cold load: the box realizes to seeded handle 1, so the mass props
    //     resolve and enter the retention cache.
    let state = crate::commands::open_file_engine_impl(&engine, &path)
        .expect("open_file_engine_impl should succeed");
    assert_rigid_mass_props_final(&state, "RigidMassSmoke", "cold load (handle 1)");

    // (2) Selective re-renders with NO edit: the first re-dispatches to seeded
    //     handle 2, the rest are hash-exempt and served from retention. This half
    //     must keep working — the amendment NARROWS retention, it does not remove
    //     it — and it is what makes step (3) a real change of verdict rather than
    //     a cell that was never Final to begin with.
    let keys = visible_realization_keys(&state);
    crate::commands::sync_demand_impl(&engine, &keys).expect("sync_demand_impl");
    for i in 1..=3 {
        let state = crate::commands::get_initial_state_impl(&engine)
            .unwrap_or_else(|e| panic!("re-render #{i}, no edit: {e}"));
        assert_rigid_mass_props_final(
            &state,
            "RigidMassSmoke",
            &format!("re-render {i}, no edit (retention still live)"),
        );
    }

    // (3) Warm edit: the realization's input-cone hash CHANGES, so it is
    //     dispatched — and its geometry queries now hit an unseeded handle.
    let ops_before = ops.lock().expect("mock op log").len();
    let state = crate::commands::set_parameter_impl(&engine, "RigidMassSmoke.depth", "250mm")
        .expect("set_parameter_impl");
    // State the precondition directly rather than trusting the dispatch count: the
    // edit must have re-executed geometry, and at least one of those ops must have
    // landed past the seeded ceiling, which is what makes its queries fail. Both
    // halves matter — no new op at all would mean the realization stayed
    // hash-exempt (a different scenario, covered by
    // `warm_edit_of_a_non_op_arg_mass_input_does_not_replay_a_stale_mass`), and a
    // new op INSIDE the seeded range would mean the queries were answered and
    // nothing degenerated.
    let new_handles: Vec<u64> = ops
        .lock()
        .expect("mock op log")
        .iter()
        .skip(ops_before)
        .map(|rec| rec.result_handle.0)
        .collect();
    assert!(
        !new_handles.is_empty(),
        "the edit must have re-DISPATCHED the realization — no new kernel op means \
         it stayed hash-exempt, which is a different scenario than the one this \
         test induces"
    );
    assert!(
        new_handles.iter().any(|h| *h > SEED_CEILING),
        "the post-edit dispatch must allocate a handle past the seeded ceiling \
         ({SEED_CEILING}) so its volume / centroid / inertia queries go unanswered \
         — that unanswered query IS the degeneration under test. Got new handles \
         {new_handles:?}; if they are all seeded, the production path's dispatch \
         count drifted and the seed range needs re-narrowing"
    );
    let depth = state
        .values
        .iter()
        .find(|v| v.name == "depth")
        .expect("expected a `depth` cell after the edit");
    assert_eq!(
        depth.value, "250",
        "the edit must have taken effect, else this test proves nothing about the \
         post-dispatch path; got {:?}",
        depth.value
    );
    assert_rigid_mass_props_not_final(&state, "RigidMassSmoke", "after the degenerating edit");

    // (4) …and it must STAY cleared on subsequent re-renders, rather than the
    //     stale pre-edit value creeping back in on the next hash-exempt pass.
    for i in 1..=2 {
        let state = crate::commands::get_initial_state_impl(&engine)
            .unwrap_or_else(|e| panic!("re-render #{i} after the degenerating edit: {e}"));
        assert_rigid_mass_props_not_final(
            &state,
            "RigidMassSmoke",
            &format!("re-render {i} after the degenerating edit"),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task #5338 amendment round 2 — the WARM-EDIT half of retention invalidation.
//
// `degenerate_geometry_after_rebuild_clears_the_retained_mass_props` above covers
// the warm edit that DOES re-dispatch (an op-arg edit whose realization then
// resolves to nothing). The pair below covers the warm edit that does NOT: an
// edit to a mass input that never reaches a geometry op's scalar args, so the
// realization stays HASH-EXEMPT, is never dispatched, and the delta can carry no
// fresh value at all. There the "a fresh delta always wins" half of the retention
// contract is structurally unavailable, and only an explicit invalidation can stop
// the pre-edit value being replayed as `determined` / `final`.
//
// The second test pins the SCOPE of that invalidation: it must be entity-scoped,
// never a blunt full clear.
// ─────────────────────────────────────────────────────────────────────────────

/// A `: Rigid` body whose `material.density` — and therefore its auto-derived
/// `mass` (`volume(geometry) * material.density`) and `moment_of_inertia`
/// (`moment_of_inertia(geometry, body_density)`) — is driven by a param that is
/// NOT a scalar argument of any geometry op.
///
/// `density_scale : Real` rather than a `Density`-typed param is deliberate:
/// `parse_value_string` (engine.rs) only knows `UNIT_TABLE`'s deg/rad/mm/cm/m, so
/// a `Density`-typed param is not editable through `set_parameter` at all. A plain
/// `Real` multiplier is the reachable spelling of the same defect, and the shape a
/// user actually authors (a `param body_density` folded into `Material(...)`).
///
/// At load `density_scale = 1.0`, so the density is bits-exact `7850.0` — which is
/// what `with_inertia_tensor_result` keys on (test_helpers.rs) — and the cold load
/// resolves normally.
const RIGID_DENSITY_SCALE_SRC: &str = r#"structure def RigidDensityScale : Rigid {
    param depth : Length = 300mm
    param density_scale : Real = 1.0
    param geometry : Solid = box(100mm, 100mm, depth)
    param material : Material = Material(name: "steel", density: 7850kg/m^3 * density_scale, youngs_modulus: 200GPa)
    constraint depth > 0mm
}"#;

/// Task #5338 amendment round 2 (reviewer blocking issue) — a warm edit of a
/// mass input that is not a geometry-op arg must not replay the PRE-EDIT mass as
/// a fresh, authoritative value.
///
/// RED before the accompanying `engine.rs` change, and the chain is entirely
/// measurable on this branch:
///
///  * `set_parameter` (engine.rs) commits through `core.commit_check`, which
///    touches only `last_check`. The `geometry_derived_cache.clear()` lives in
///    `commit_state`, which a warm edit NEVER reaches — so retention survives the
///    edit untouched.
///  * `edit_param` mutates the EXISTING snapshot graph in place rather than
///    rebuilding it, so `RealizationNodeData.input_cone_hash` survives too; and
///    that hash is a fold over the realization's own geometry-op scalar args ONLY
///    (`compute_realization_upstream_values_hash_from_ops`, engine_build.rs).
///    `density_scale` reaches `material.density`, never an op arg.
///  * Unchanged hash ⇒ HASH-EXEMPT ⇒ dropped from the scheduled seed ⇒ kernel ops
///    skipped ⇒ no mesh in `result.meshes`, and an explicit `Value::Undef` in
///    `result.values`.
///  * In `surface_geometry_derived_cells` the
///    `Some(Value::Undef) if dispatched_entities.contains(..)` degeneration guard
///    therefore does NOT fire (the entity was not dispatched), and the
///    `_ => cache.get(&id)` arm replays the pre-edit mass as `determined` /
///    `freshness = "final"` / `reason = None`.
///
/// That is strictly worse than the pre-#5338 behaviour, where the cell read
/// `Undef`. Degrading to `Undef` is the fail-safe direction and is what this test
/// requires.
///
/// The PD-constraint assertion is not redundant with the cell assertion: a
/// re-check driven off a retained pre-edit `moi_principal` is the same stale value
/// wearing a constraint badge, and `surface_geometry_derived_cells` explicitly
/// overlays cache-sourced cells into the re-check input.
#[test]
fn warm_edit_of_a_non_op_arg_mass_input_does_not_replay_a_stale_mass() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_DENSITY_SCALE_SRC, "rigid_density_scale")
        .expect("load_from_source should succeed for the density-scale source");
    assert_rigid_mass_props_final(&state, "RigidDensityScale", "cold load");

    // (1) The first SELECTIVE rebuild — the pass that stores the realization's
    //     `input_cone_hash` AND populates the retention cache. Both halves of the
    //     defect's precondition are established here; without it the edit below
    //     would simply re-dispatch under full scope and prove nothing.
    session.sync_demand(&visible_realization_keys(&state));
    let state = session
        .build_gui_state()
        .expect("first selective rebuild should succeed");
    assert_rigid_mass_props_final(&state, "RigidDensityScale", "first selective rebuild");

    // (2) The warm edit. `density_scale` doubles the body's density, so every
    //     mass prop is now wrong by construction — but no geometry op arg moved.
    let state = session
        .set_parameter("RigidDensityScale.density_scale", "2.0")
        .expect("set_parameter should succeed for the Real density multiplier");
    let scale = state
        .values
        .iter()
        .find(|v| v.entity_path == "RigidDensityScale" && v.name == "density_scale")
        .expect("expected a `density_scale` value cell after the edit");
    let scale_value: f64 = scale.value.parse().unwrap_or_else(|_| {
        panic!(
            "expected a numeric `density_scale` cell after the edit; got {:?}",
            scale.value
        )
    });
    assert_eq!(
        scale_value, 2.0,
        "the edit must have taken effect, else this test proves nothing about the \
         post-edit path; got {:?}",
        scale.value
    );
    assert_rigid_mass_props_not_final(
        &state,
        "RigidDensityScale",
        "after the non-op-arg density edit",
    );
    assert_ne!(
        find_moi_principal_constraint(&state).status,
        "Satisfied",
        "the `moi_principal[0] > 0` PD constraint must not be re-checked to \
         Satisfied off a RETAINED pre-edit `moi_principal` — that is the same stale \
         value wearing a constraint badge"
    );

    // (3) …and the dropped value must not creep back on the next hash-exempt pass.
    let state = session
        .build_gui_state()
        .expect("rebuild after the density edit should succeed");
    assert_rigid_mass_props_not_final(&state, "RigidDensityScale", "rebuild after the density edit");
}

/// Task #5338 amendment round 2 — pins the SCOPE of the warm-edit invalidation.
///
/// Expected GREEN both before and after the accompanying `engine.rs` change: it
/// exists to forbid the obvious over-correction. A blunt
/// `geometry_derived_cache.clear()` on every `set_parameter` would drop every
/// UNAFFECTED entity's retention too, and those entities stay hash-exempt until
/// the next recompile — so their mass-prop cells would read `Undef` indefinitely,
/// which is #5338 itself, re-opened for every body the user did not touch. This
/// test fails at step (3)'s `RigidBodyB` if that shortcut is ever taken.
///
/// Body A's `depth` IS an op arg, so its input-cone hash changes, it dispatches,
/// and the delta carries fresh values for it — the assertion there is that a
/// correct invalidation does not break the ordinary re-dispatch path either.
#[test]
fn warm_edit_does_not_collaterally_drop_another_bodys_retained_mass_props() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_TWO_BODY_SRC, "two_body")
        .expect("load_from_source should succeed for the two-Rigid-body source");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "cold load");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "cold load");

    // (1) Both bodies visible and demanded: both land in the retention cache.
    session.sync_demand(&visible_realization_keys(&state));
    let state = session
        .build_gui_state()
        .expect("first selective rebuild should succeed");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "first selective rebuild");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "first selective rebuild");

    // (2) Edit ONE body's op arg.
    let state = session
        .set_parameter("RigidBodyA.depth", "250mm")
        .expect("set_parameter should succeed for body A's depth");
    let depth = state
        .values
        .iter()
        .find(|v| v.entity_path == "RigidBodyA" && v.name == "depth")
        .expect("expected a `RigidBodyA.depth` value cell after the edit");
    assert_eq!(
        depth.value, "250",
        "the edit must have taken effect; got {:?}",
        depth.value
    );
    assert_rigid_mass_props_final(&state, "RigidBodyA", "edited body (re-dispatched)");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "untouched body (hash-exempt)");

    // (3) …and both must survive the following hash-exempt rebuild.
    let state = session
        .build_gui_state()
        .expect("rebuild after the depth edit should succeed");
    assert_rigid_mass_props_final(&state, "RigidBodyA", "edited body, next rebuild");
    assert_rigid_mass_props_final(&state, "RigidBodyB", "untouched body, next rebuild");
}

// ─────────────────────────────────────────────────────────────────────────────
// Task #5338 amendment round 3 — the CONTAINED sub-part shape.
//
// Every fixture above is a flat, root-level template, where the frontend's mesh
// key (`Entity#realization[N]`) and the panel cell's `entity_path` are the same
// string. For a `: Rigid` body reached through containment they are NOT: the mesh
// key carries the composed CONTAINMENT path (`Asm.part#realization[0]`, built by
// reify-eval's `surface_realizations` from the dotted path prefix) while the cell
// keeps the TEMPLATE name (`RigidPart`, since `ValueData.entity_path` is
// `cell.id.entity` and value cells are template-level). The pair below pins what
// that mismatch actually costs, measured rather than assumed.
// ─────────────────────────────────────────────────────────────────────────────

/// A `: Rigid` body reached through containment: `Asm` holds one placed `sub part`
/// whose type is the `: Rigid` template. Surfaces as mesh key
/// `Asm.part#realization[0]` with mass-prop cells on entity `RigidPart`.
///
/// The placement mirrors `get_entity_tree_aux_sub_inherits_default_visible_false`
/// (engine_tests.rs), the existing composed-path fixture.
const RIGID_CONTAINED_SUB_SRC: &str = r#"structure def RigidPart : Rigid {
    param geometry : Solid = box(100mm, 100mm, 300mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}

structure Asm {
    sub part : RigidPart at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
}"#;

/// Task #5338 amendment round 3 (reviewer suggestion 1) — a contained `: Rigid`
/// sub-part under the frontend's REAL key does not reach the retention path at
/// all, and the degradation is fail-safe.
///
/// MEASURED on this branch, and the reason this is a lock rather than a fix:
/// feeding `sync_demand` the key the frontend actually holds
/// (`Asm.part#realization[0]`, straight out of `state.meshes`) leaves the pass
/// dispatching NOTHING — `state.meshes` is EMPTY on the very first selective
/// rebuild, where the flat fixtures emit every visible body's mesh (see
/// `contained_rigid_sub_part_retention_works_once_the_demand_key_resolves`, which
/// runs the SAME source and cold load and differs only in the demand key). So the
/// composed containment path does not resolve to a realization node in the demand
/// graph: the cone is empty and the sub-part is not rendered.
///
/// The `sync_demand` prune ALSO drops the cached cells there — `visible_entities`
/// holds `Asm.part`, the cache keys on `RigidPart` — and that is the CORRECT
/// outcome, not a second bug to fix independently. Retaining them would paint a
/// `determined` / `final` mass for a body the pass never demanded and the viewport
/// never drew, i.e. exactly the arch §8 violation ("a pruned realization's cached
/// result is never served as Final") the prune exists to discharge. Repairing the
/// join alone, without the upstream key resolution, would therefore make this
/// WORSE, not better.
///
/// The residual is that #5338's retention never applies to contained bodies, which
/// costs nothing while their selective demand resolves to an empty scene anyway.
/// Both halves live upstream of this crate (reify-eval's realization-node keying
/// vs. the composed mesh path), so closing them is filed under ticket
/// `tkt_0RSRP0RVHF2SMG12S7QB1F9VHT` rather than smuggled into an amendment pass.
#[test]
fn contained_rigid_sub_part_is_not_served_as_final_under_the_composed_key() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_CONTAINED_SUB_SRC, "contained_rigid")
        .expect("load_from_source should succeed for the contained-sub source");
    // The cold load is full-scope, so the sub-part's mass props resolve normally —
    // establishing that the cells exist and CAN be Final, which is what makes the
    // assertions below a change of verdict rather than a cell that never resolved.
    assert_rigid_mass_props_final(&state, "RigidPart", "cold load (full scope)");

    // The join-key mismatch itself: the mesh key is the composed containment path,
    // the cells sit on the template name. Pinned explicitly — if reify-eval ever
    // makes these agree, this assertion is the first thing to go red and the
    // `sync_demand` limitation doc above it can be retired.
    let keys = visible_realization_keys(&state);
    assert_eq!(
        keys,
        vec!["Asm.part#realization[0]".to_string()],
        "the frontend's key for a contained sub-part is the composed containment \
         path, while its mass-prop cells key on the template name `RigidPart` — \
         that mismatch is this test's whole premise"
    );

    session.sync_demand(&keys);
    for i in 1..=3 {
        let state = session
            .build_gui_state()
            .unwrap_or_else(|e| panic!("re-render #{i} under the composed key: {e}"));
        assert!(
            state.meshes.is_empty(),
            "[re-render {i}] MEASURED: the composed key resolves to no realization \
             node, so the pass dispatches nothing and the scene is empty; got {:?}. \
             A non-empty scene here means the upstream key gap closed — re-read the \
             `sync_demand` known-limitation doc before touching the prune",
            state
                .meshes
                .iter()
                .map(|m| m.entity_path.as_str())
                .collect::<Vec<_>>()
        );
        assert_rigid_mass_props_not_final(
            &state,
            "RigidPart",
            &format!("re-render {i} under the composed key (nothing demanded)"),
        );
    }
}

/// The positive twin: the retention mechanism is NOT containment-blind — feed
/// `sync_demand` a key that resolves and a contained sub-part behaves exactly like
/// a root body.
///
/// Same source and same cold load as
/// `contained_rigid_sub_part_is_not_served_as_final_under_the_composed_key`; the
/// ONLY difference is the demand key (`RigidPart#realization[0]`, the template-level
/// form, instead of the composed `Asm.part#realization[0]`). Re-render 1 dispatches
/// and emits the mesh; re-renders 2-3 are hash-exempt and are served from the
/// retention cache. That isolates the defect to the KEY, not to the cache's
/// entity join or to anything containment-specific in `surface_geometry_derived_cells`
/// — so a future fix belongs at the key seam, and this test says what "fixed" looks
/// like.
#[test]
fn contained_rigid_sub_part_retention_works_once_the_demand_key_resolves() {
    let mut session = rigid_mass_props_session();
    let state = session
        .load_from_source(RIGID_CONTAINED_SUB_SRC, "contained_rigid")
        .expect("load_from_source should succeed for the contained-sub source");
    assert_rigid_mass_props_final(&state, "RigidPart", "cold load (full scope)");

    session.sync_demand(&["RigidPart#realization[0]".to_string()]);

    let state = session
        .build_gui_state()
        .expect("first selective rebuild under the template-level key");
    assert!(
        !state.meshes.is_empty(),
        "the template-level key must resolve to a demand root — an empty scene here \
         would make the retention assertions below vacuous"
    );
    assert_rigid_mass_props_final(&state, "RigidPart", "first selective rebuild (dispatched)");

    for i in 2..=3 {
        let state = session
            .build_gui_state()
            .unwrap_or_else(|e| panic!("re-render #{i} under the template-level key: {e}"));
        assert_rigid_mass_props_final(
            &state,
            "RigidPart",
            &format!("re-render {i} (hash-exempt, served from retention)"),
        );
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// Task #5338 amendment round 3 — the CROSS-MODULE collision `commit_state`'s
// unconditional clear is documented as guarding against.
//
// `commit_state` clears `geometry_derived_cache` on EVERY recompile, and its doc
// argues that narrowing that to `FilePathUpdate::Set` would be actively unsafe
// because `load_from_source` also commits with `Preserve` and can carry a
// DIFFERENT module whose entities collide on `ValueCellId` (entity+member) — two
// sources each declaring a `Body : Rigid` both key `Body.mass`. Nothing in the
// suite exercised that, so the argument was prose a maintainer could not check.
// ─────────────────────────────────────────────────────────────────────────────

/// Module A's `Body : Rigid`: density 7850kg/m^3, the value
/// `with_inertia_tensor_result` is seeded on (test_helpers.rs), so ALL FOUR
/// mass-prop cells resolve and enter the retention cache.
const COLLIDING_BODY_SRC_A: &str = r#"structure def Body : Rigid {
    param geometry : Solid = box(100mm, 100mm, 300mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}"#;

/// Module B's `Body : Rigid` — same entity name, hence the same `ValueCellId`s,
/// but a different body: half the density, so its `mass` differs from module A's
/// by construction, and its inertia query is UNANSWERED (the mock keys the
/// inertia tensor on a bits-exact 7850.0), so its `moment_of_inertia` /
/// `moi_principal` resolve to `Undef`.
///
/// Both halves matter: the first catches a stale `mass` value being replayed, the
/// second catches a stale tensor being served where module B has no value at all.
const COLLIDING_BODY_SRC_B: &str = r#"structure def Body : Rigid {
    param geometry : Solid = box(50mm, 50mm, 50mm)
    param material : Material = Material(name: "alu", density: 3925kg/m^3, youngs_modulus: 70GPa)
}"#;

/// The formatted `Body.mass` cell, or a panic naming what was there instead.
fn mass_value_of(state: &crate::types::GuiState, entity: &str, ctx: &str) -> String {
    state
        .values
        .iter()
        .find(|v| v.entity_path == entity && v.name == "mass")
        .unwrap_or_else(|| panic!("[{ctx}] expected a `{entity}.mass` cell"))
        .value
        .clone()
}

/// Task #5338 amendment round 3 (reviewer suggestion 6) — loading a second module
/// whose entities collide on `ValueCellId` must not replay the first module's
/// mass-property values.
///
/// This is the executable form of the collision `commit_state`'s unconditional
/// `geometry_derived_cache.clear()` is documented against: `load_from_source`
/// commits with `Preserve`, so a clear narrowed to `FilePathUpdate::Set` would
/// carry module A's `Body.mass` / `Body.moment_of_inertia` into module B's panel,
/// where both cells key identically.
///
/// Read what it pins accurately: it locks the OUTCOME (no cross-module replay),
/// not the clear specifically. MEASURED on this branch — with that `clear()`
/// deleted this test still passes, because a recompile resets every
/// `input_cone_hash`, so module B's colliding realization dispatches on the pass
/// right after the load and the dispatched-entity discriminator drops the retained
/// entry. The clear is the outer guard for the case the discriminator cannot see
/// (a colliding realization that dispatches but emits no mesh); that case is not
/// reachable through this crate's API with the mock kernel, which is why this test
/// stops where it does. See the `commit_state` comment for the full division of
/// labour before narrowing anything there.
#[test]
fn a_colliding_second_module_does_not_replay_the_first_modules_mass_props() {
    let mut session = rigid_mass_props_session();

    // (1) Module A, driven all the way into the retention cache: cold load, then
    //     the selective rebuild that is the only thing that populates it.
    let state = session
        .load_from_source(COLLIDING_BODY_SRC_A, "module_a")
        .expect("load_from_source should succeed for module A");
    assert_rigid_mass_props_final(&state, "Body", "module A cold load");
    session.sync_demand(&visible_realization_keys(&state));
    let state = session
        .build_gui_state()
        .expect("module A's selective rebuild should succeed");
    assert_rigid_mass_props_final(&state, "Body", "module A selective rebuild");
    let mass_a = mass_value_of(&state, "Body", "module A selective rebuild");

    // (2) Module B, same entity name, different body.
    let state = session
        .load_from_source(COLLIDING_BODY_SRC_B, "module_b")
        .expect("load_from_source should succeed for module B");

    let mass_b = mass_value_of(&state, "Body", "module B cold load");
    assert_ne!(
        mass_b, mass_a,
        "module B's `Body.mass` must be its OWN value, not module A's replayed \
         through the colliding `ValueCellId` — module B is half the density, so an \
         equal reading means one module's mass was surfaced onto another's panel"
    );

    // Module B's inertia query is unanswered, so it HAS no tensor this pass. The
    // cells must degrade rather than serve module A's.
    for name in ["moment_of_inertia", "moi_principal"] {
        let cell = state
            .values
            .iter()
            .find(|v| v.entity_path == "Body" && v.name == name)
            .unwrap_or_else(|| panic!("expected a `Body.{name}` cell after module B's load"));
        assert!(
            !(cell.determinacy == "determined" && cell.freshness == "final"),
            "`Body.{name}` must not be served as a fresh Final value under module B, \
             which has no tensor for it; got determinacy={:?}, freshness={:?}, \
             value={:?}",
            cell.determinacy,
            cell.freshness,
            cell.value
        );
    }

    // (3) …and the following rebuild must not resurrect them either.
    let state = session
        .build_gui_state()
        .expect("the rebuild after module B's load should succeed");
    assert_eq!(
        mass_value_of(&state, "Body", "module B rebuild"),
        mass_b,
        "module B's `Body.mass` must be stable across the next rebuild rather than \
         drifting back toward module A's retained value"
    );
}
