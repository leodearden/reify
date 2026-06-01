use std::sync::{Arc, Mutex, RwLock};

use reify_constraints::SimpleConstraintChecker;
use reify_mcp::SelectionInfo;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::commands::engine_state_json;
use crate::diff::compute_delta;
use crate::engine::EngineSession;
use crate::mcp_context::{TauriToolContext, mcp_tool_call_impl};
use crate::types::GuiState;

fn make_engine() -> Arc<Mutex<EngineSession>> {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    Arc::new(Mutex::new(session))
}

fn make_tauri_context() -> TauriToolContext {
    TauriToolContext::builder(make_engine()).build()
}

#[test]
fn dispatch_get_eval_status_returns_idle() {
    let ctx = make_tauri_context();
    let result = mcp_tool_call_impl("reify_get_eval_status", serde_json::json!({}), &ctx)
        .expect("dispatch should succeed");
    assert_eq!(result["phase"], "idle");
}

#[test]
fn dispatch_get_source_returns_bracket_content() {
    let ctx = make_tauri_context();
    let result = mcp_tool_call_impl("reify_get_source", serde_json::json!({}), &ctx)
        .expect("dispatch should succeed");
    assert!(
        result["content"]
            .as_str()
            .unwrap()
            .contains("structure Bracket"),
        "should contain bracket source"
    );
}

#[test]
fn dispatch_set_parameter_returns_success() {
    let ctx = make_tauri_context();
    let result = mcp_tool_call_impl(
        "reify_set_parameter",
        serde_json::json!({"cell_id": "Bracket.width", "value": "100mm"}),
        &ctx,
    )
    .expect("dispatch should succeed");
    assert_eq!(result["success"], true);
}

#[test]
fn dispatch_unknown_tool_returns_error() {
    let ctx = make_tauri_context();
    let result = mcp_tool_call_impl("nonexistent", serde_json::json!({}), &ctx);
    assert!(result.is_err(), "should return error for unknown tool");
}

#[test]
fn dispatch_get_parameters_returns_entries() {
    let ctx = make_tauri_context();
    let result = mcp_tool_call_impl("reify_get_parameters", serde_json::json!({}), &ctx)
        .expect("dispatch should succeed");
    let params = result.as_array().expect("should be an array");
    assert!(!params.is_empty(), "should have parameters");

    // Find width
    let width = params
        .iter()
        .find(|p| p["name"] == "width")
        .expect("should have width");
    assert_eq!(width["cell_id"], "Bracket.width");
    assert_eq!(width["value"], "80");
    assert_eq!(width["unit"], "mm");
}

// --- State-delta tests validating the sync pattern used by the Tauri command ---

#[test]
fn mcp_write_tool_produces_state_delta() {
    let engine = make_engine();

    // 1. Build initial GuiState and store in simulated last_state
    let initial_gui_state = engine
        .lock()
        .unwrap()
        .build_gui_state()
        .expect("initial build_gui_state");
    let last_state: Mutex<Option<GuiState>> = Mutex::new(Some(initial_gui_state));

    // 2. Perform an MCP write via mcp_tool_call_impl
    let ctx = TauriToolContext::builder(engine.clone()).build();
    let result = mcp_tool_call_impl(
        "reify_set_parameter",
        serde_json::json!({"cell_id": "Bracket.width", "value": "100mm"}),
        &ctx,
    )
    .expect("set_parameter dispatch should succeed");
    assert_eq!(result["success"], true);

    // 3. Rebuild GuiState from engine after the write
    let new_gui_state = engine
        .lock()
        .unwrap()
        .build_gui_state()
        .expect("rebuild build_gui_state");

    // 4. Compute delta against last_state
    let delta = compute_delta(&last_state, &new_gui_state);

    // 5. Assert the delta's changed_values is non-empty (width changed from 80 to 100)
    assert!(
        !delta.changed_values.is_empty(),
        "delta should have changed values after set_parameter"
    );
    let changed_width = delta
        .changed_values
        .iter()
        .find(|v| v.cell_id == "Bracket.width");
    assert!(
        changed_width.is_some(),
        "Bracket.width should appear in changed_values"
    );
    assert_eq!(changed_width.unwrap().value, "100");

    // 6. Verify last_state was updated by compute_delta
    let stored = last_state.lock().unwrap();
    assert!(stored.is_some(), "last_state should be updated");
    let stored_width = stored
        .as_ref()
        .unwrap()
        .values
        .iter()
        .find(|v| v.cell_id == "Bracket.width")
        .expect("stored state should have width");
    assert_eq!(
        stored_width.value, "100",
        "last_state should reflect the new value"
    );
}

#[test]
fn mcp_read_tool_produces_empty_delta() {
    let engine = make_engine();

    // 1. Build initial GuiState and store in simulated last_state
    let initial_gui_state = engine
        .lock()
        .unwrap()
        .build_gui_state()
        .expect("initial build_gui_state");
    let last_state: Mutex<Option<GuiState>> = Mutex::new(Some(initial_gui_state));

    // 2. Perform a read-only MCP tool call
    let ctx = TauriToolContext::builder(engine.clone()).build();
    let result = mcp_tool_call_impl("reify_get_parameters", serde_json::json!({}), &ctx)
        .expect("get_parameters dispatch should succeed");
    assert!(result.is_array(), "should return array of parameters");

    // 3. Rebuild GuiState (should be identical since no mutation occurred)
    let new_gui_state = engine
        .lock()
        .unwrap()
        .build_gui_state()
        .expect("rebuild build_gui_state");

    // 4. Compute delta — should be empty since nothing changed
    let delta = compute_delta(&last_state, &new_gui_state);

    // 5. Assert all delta fields are empty (conservative always-sync is safe for reads)
    assert!(
        delta.changed_values.is_empty(),
        "changed_values should be empty after read-only tool"
    );
    assert!(
        delta.changed_constraints.is_empty(),
        "changed_constraints should be empty after read-only tool"
    );
    assert!(
        delta.changed_meshes.is_empty(),
        "changed_meshes should be empty after read-only tool"
    );
}

#[test]
fn dispatch_get_selection_returns_selected_entity() {
    let engine = make_engine();
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    }));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection)
        .build();
    let result = mcp_tool_call_impl("reify_get_selection", serde_json::json!({}), &ctx)
        .expect("dispatch should succeed");
    assert_eq!(
        result["selected_entity"], "Bracket",
        "selected_entity should be Bracket"
    );
    assert!(
        result["hovered_entity"].is_null(),
        "hovered_entity should be null"
    );
}

#[test]
fn dispatch_get_selection_returns_both_fields() {
    let engine = make_engine();
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: Some("Bracket.width".to_string()),
    }));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection)
        .build();
    let result = mcp_tool_call_impl("reify_get_selection", serde_json::json!({}), &ctx)
        .expect("dispatch should succeed");
    assert_eq!(
        result["selected_entity"], "Bracket",
        "selected_entity should be Bracket"
    );
    assert_eq!(
        result["hovered_entity"], "Bracket.width",
        "hovered_entity should be Bracket.width"
    );
}

// ---------------------------------------------------------------------------
// engine_state_json helper tests (task 4153, step-7 RED)
// ---------------------------------------------------------------------------

/// (step-7 RED-a) engine_state_json on a freshly-loaded engine must include the
/// existing meshes/values/constraints/files keys AND the new stale/reload_error/
/// compile_diagnostics keys with clean-state values.
///
/// RED until step-8 extracts engine_state_json from handle_engine_state.
#[test]
fn engine_state_json_clean_engine_has_expected_shape() {
    let engine = make_engine();
    let result = {
        let mut session = engine.lock().unwrap();
        engine_state_json(&mut session).expect("engine_state_json should succeed on clean engine")
    };

    // Regression guard: existing keys must still be present.
    assert!(
        result.get("meshes").is_some(),
        "result must contain 'meshes' key"
    );
    assert!(
        result.get("values").is_some(),
        "result must contain 'values' key"
    );
    assert!(
        result.get("constraints").is_some(),
        "result must contain 'constraints' key"
    );
    assert!(
        result.get("files").is_some(),
        "result must contain 'files' key"
    );

    // New staleness fields on a clean (non-stale) engine.
    assert_eq!(
        result["stale"],
        serde_json::Value::Bool(false),
        "stale must be false for a freshly-loaded engine"
    );
    assert!(
        result["reload_error"].is_null(),
        "reload_error must be null for a freshly-loaded engine; got: {:?}",
        result["reload_error"]
    );
    let compile_diagnostics = result["compile_diagnostics"]
        .as_array()
        .expect("compile_diagnostics must be an array");
    assert!(
        compile_diagnostics.is_empty(),
        "compile_diagnostics must be empty for a freshly-loaded engine; got: {:?}",
        compile_diagnostics
    );

    // Meshes must be non-empty (bracket source produces geometry).
    let meshes = result["meshes"].as_array().expect("meshes must be an array");
    assert!(
        !meshes.is_empty(),
        "meshes must be non-empty for a freshly-loaded engine"
    );
}

/// (step-7 RED-b) After record_reload_error, engine_state_json must expose
/// stale=true, a non-null reload_error containing the error string, a non-empty
/// compile_diagnostics array with an Error-severity entry, and still-non-empty
/// meshes (last-good retained).
///
/// RED until step-8 extracts engine_state_json.
#[test]
fn engine_state_json_after_record_reload_error_exposes_staleness() {
    let engine = make_engine();

    // Record a synthetic reload error.
    {
        let mut session = engine.lock().unwrap();
        session.record_reload_error("panic in engine: test_panic_x".to_string());
    }

    let result = {
        let mut session = engine.lock().unwrap();
        engine_state_json(&mut session)
            .expect("engine_state_json should succeed even when session is stale")
    };

    // Staleness flags.
    assert_eq!(
        result["stale"],
        serde_json::Value::Bool(true),
        "stale must be true after record_reload_error"
    );
    let reload_error = result["reload_error"]
        .as_str()
        .expect("reload_error must be a string after record_reload_error");
    assert!(
        reload_error.contains("panic"),
        "reload_error must contain 'panic'; got: {reload_error:?}"
    );

    // compile_diagnostics must contain at least one Error-severity entry.
    let compile_diagnostics = result["compile_diagnostics"]
        .as_array()
        .expect("compile_diagnostics must be an array");
    let has_error = compile_diagnostics
        .iter()
        .any(|d| d["severity"].as_str() == Some("Error"));
    assert!(
        has_error,
        "compile_diagnostics must contain an Error-severity entry after record_reload_error; \
         got: {:?}",
        compile_diagnostics
    );

    // Meshes must still be the last-good non-empty set.
    let meshes = result["meshes"].as_array().expect("meshes must be an array");
    assert!(
        !meshes.is_empty(),
        "meshes must be non-empty (last-good retained) after record_reload_error"
    );
}
