use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{bracket_source, MockGeometryKernel};

use crate::engine::EngineSession;
use crate::mcp_context::{mcp_tool_call_impl, TauriToolContext};

fn make_tauri_context() -> TauriToolContext {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    let engine = Arc::new(Mutex::new(session));
    TauriToolContext::new(engine)
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
