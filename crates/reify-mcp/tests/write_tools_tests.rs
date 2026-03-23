use reify_mcp::context::MockToolContext;
use reify_mcp::registry::ToolRegistry;
use reify_mcp::tools::register_all_tools;
use reify_mcp::types::{DiagnosticInfo, ToolError};

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

fn make_diagnostic(file_path: &str, severity: &str, message: &str) -> DiagnosticInfo {
    DiagnosticInfo {
        file_path: file_path.to_string(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 10,
        severity: severity.to_string(),
        message: message.to_string(),
        code: None,
    }
}

// === reify_update_source ===

#[test]
fn update_source_returns_success_with_diagnostics() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        diagnostics: vec![
            make_diagnostic("main.ri", "error", "undefined variable"),
            make_diagnostic("main.ri", "warning", "unused param"),
            make_diagnostic("other.ri", "error", "other file error"),
        ],
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_update_source",
            serde_json::json!({"file_path": "main.ri", "content": "param x = 10mm"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
    // Only diagnostics for the updated file
    assert_eq!(result["diagnostics_count"], 2);
    let diags = result["diagnostics"].as_array().expect("diagnostics should be array");
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0]["file_path"], "main.ri");
    assert_eq!(diags[1]["file_path"], "main.ri");
}

#[test]
fn update_source_missing_file_path_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool(
        "reify_update_source",
        serde_json::json!({"content": "param x = 10mm"}),
        &ctx,
    );

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

#[test]
fn update_source_missing_content_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool(
        "reify_update_source",
        serde_json::json!({"file_path": "main.ri"}),
        &ctx,
    );

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}
