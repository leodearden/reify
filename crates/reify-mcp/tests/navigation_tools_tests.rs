use reify_mcp::context::MockToolContext;
use reify_mcp::registry::ToolRegistry;
use reify_mcp::tools::register_all_tools;
use reify_mcp::types::{SourceLocationInfo, ToolError};

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

// === reify_focus_entity ===

#[test]
fn focus_entity_returns_success() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_focus_entity",
            serde_json::json!({"entity_path": "bracket/body"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
}

#[test]
fn focus_entity_missing_entity_path_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool("reify_focus_entity", serde_json::json!({}), &ctx);

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

// === reify_navigate_to_source ===

#[test]
fn navigate_to_source_with_known_entity_returns_location() {
    let registry = setup_registry();
    let mut locations = std::collections::HashMap::new();
    locations.insert(
        "bracket/body".to_string(),
        SourceLocationInfo {
            file_path: "main.ri".to_string(),
            line: 5,
            column: 3,
            end_line: 20,
            end_column: 1,
        },
    );
    let ctx = MockToolContext {
        source_locations: locations,
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_navigate_to_source",
            serde_json::json!({"entity_path": "bracket/body"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
    let loc = &result["location"];
    assert_eq!(loc["file_path"], "main.ri");
    assert_eq!(loc["line"], 5);
    assert_eq!(loc["column"], 3);
    assert_eq!(loc["end_line"], 20);
    assert_eq!(loc["end_column"], 1);
}

#[test]
fn navigate_to_source_unknown_entity_returns_false_with_null_location() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_navigate_to_source",
            serde_json::json!({"entity_path": "nonexistent/entity"}),
            &ctx,
        )
        .expect("should succeed (not error)");

    assert_eq!(result["success"], false);
    assert!(result["location"].is_null());
}

#[test]
fn navigate_to_source_missing_entity_path_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool("reify_navigate_to_source", serde_json::json!({}), &ctx);

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}
