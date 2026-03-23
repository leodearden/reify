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

/// Write tools with their minimal valid params.
const WRITE_TOOLS: &[(&str, &str)] = &[
    ("reify_update_source", r#"{"file_path": "main.ri", "content": "param x = 10mm"}"#),
    ("reify_set_parameter", r#"{"cell_id": "Bracket.width", "value": "120mm"}"#),
    ("reify_open_file", r#"{"file_path": "main.ri"}"#),
    ("reify_save_file", r#"{}"#),
    ("reify_export", r#"{"format": "step", "output_path": "/tmp/out.step"}"#),
];

/// Navigation tools with their minimal valid params.
const NAV_TOOLS: &[(&str, &str)] = &[
    ("reify_focus_entity", r#"{"entity_path": "bracket/body"}"#),
    ("reify_navigate_to_source", r#"{"entity_path": "bracket/body"}"#),
];

/// Reference tools with their minimal valid params.
const REF_TOOLS: &[(&str, &str)] = &[
    ("reify_language_reference", r#"{"topic": "syntax"}"#),
];

#[test]
fn all_non_read_tools_return_ok_with_valid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let all_tools: Vec<(&str, &str)> = WRITE_TOOLS
        .iter()
        .chain(NAV_TOOLS.iter())
        .chain(REF_TOOLS.iter())
        .copied()
        .collect();

    for (name, params_str) in &all_tools {
        let params: serde_json::Value = serde_json::from_str(params_str)
            .unwrap_or_else(|e| panic!("bad test params for '{name}': {e}"));
        let result = registry.call_tool(name, params, &ctx);
        assert!(
            result.is_ok(),
            "Tool '{}' should return Ok with valid params, got: {:?}",
            name,
            result.err()
        );
    }
}

#[test]
fn no_tools_return_not_implemented() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    // Use minimal params that avoid InvalidParams for required-param tools
    let minimal_params: std::collections::HashMap<&str, serde_json::Value> = [
        ("reify_update_source", serde_json::json!({"file_path": "a.ri", "content": ""})),
        ("reify_set_parameter", serde_json::json!({"cell_id": "x", "value": "1"})),
        ("reify_open_file", serde_json::json!({"file_path": "a.ri"})),
        ("reify_export", serde_json::json!({"format": "step", "output_path": "/tmp/o"})),
        ("reify_focus_entity", serde_json::json!({"entity_path": "x"})),
        ("reify_navigate_to_source", serde_json::json!({"entity_path": "x"})),
        ("reify_get_source_location", serde_json::json!({"entity_path": "x"})),
    ]
    .into_iter()
    .collect();

    for tool in registry.list_tools() {
        let params = minimal_params
            .get(tool.name.as_str())
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let result = registry.call_tool(&tool.name, params, &ctx);
        match result {
            Err(ToolError::NotImplemented) => panic!(
                "Tool '{}' still returns NotImplemented — should be implemented",
                tool.name
            ),
            _ => {} // Any other result (Ok or non-NotImplemented error) is fine
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
