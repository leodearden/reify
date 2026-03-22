use reify_mcp::context::MockToolContext;
use reify_mcp::registry::ToolRegistry;
use reify_mcp::tools::register_all_tools;
use reify_mcp::types::ToolError;

const EXPECTED_TOOLS: &[&str] = &[
    "reify_get_source",
    "reify_get_open_files",
    "reify_get_diagnostics",
    "reify_get_parameters",
    "reify_get_constraints",
    "reify_get_eval_status",
    "reify_get_selection",
    "reify_get_source_location",
    "reify_update_source",
    "reify_set_parameter",
    "reify_open_file",
    "reify_save_file",
    "reify_export",
    "reify_focus_entity",
    "reify_navigate_to_source",
    "reify_language_reference",
];

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

#[test]
fn register_all_tools_populates_exactly_16_tools() {
    let registry = setup_registry();
    let tools = registry.list_tools();
    assert_eq!(tools.len(), 16, "Expected 16 tools, got {}", tools.len());
}

#[test]
fn all_tools_have_non_empty_descriptions() {
    let registry = setup_registry();
    for tool in registry.list_tools() {
        assert!(
            !tool.description.is_empty(),
            "Tool '{}' has empty description",
            tool.name
        );
    }
}

#[test]
fn all_tools_have_valid_object_schema() {
    let registry = setup_registry();
    for tool in registry.list_tools() {
        assert_eq!(
            tool.input_schema["type"], "object",
            "Tool '{}' schema type should be 'object', got: {}",
            tool.name, tool.input_schema["type"]
        );
    }
}

#[test]
fn all_stub_tools_return_not_implemented() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    for tool in registry.list_tools() {
        let result = registry.call_tool(&tool.name, serde_json::json!({}), &ctx);
        match result {
            Err(ToolError::NotImplemented) => {} // expected
            other => panic!(
                "Tool '{}' should return NotImplemented, got: {other:?}",
                tool.name
            ),
        }
    }
}

#[test]
fn tool_names_match_spec() {
    let registry = setup_registry();
    let tool_names: Vec<String> = registry.list_tools().iter().map(|t| t.name.clone()).collect();

    for expected in EXPECTED_TOOLS {
        assert!(
            tool_names.contains(&expected.to_string()),
            "Missing expected tool: {expected}. Found: {tool_names:?}"
        );
    }

    // Also verify no extra tools
    for name in &tool_names {
        assert!(
            EXPECTED_TOOLS.contains(&name.as_str()),
            "Unexpected tool: {name}"
        );
    }
}
