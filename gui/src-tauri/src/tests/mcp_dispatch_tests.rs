use std::sync::{Arc, Mutex, RwLock};

use reify_constraints::SimpleConstraintChecker;
use reify_mcp::SelectionInfo;
use reify_test_support::{MockGeometryKernel, bracket_source};

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
