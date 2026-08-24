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
        has_location: true,
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
    let diags = result["diagnostics"]
        .as_array()
        .expect("diagnostics should be array");
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

// === reify_set_parameter ===

#[test]
fn set_parameter_returns_success_with_diagnostics() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        diagnostics: vec![make_diagnostic(
            "main.ri",
            "warning",
            "constraint near limit",
        )],
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_set_parameter",
            serde_json::json!({"cell_id": "Bracket.width", "value": "120mm"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
    assert!(result["new_value"].is_string());
    assert!(result["unit"].is_string());
    let diags = result["diagnostics"]
        .as_array()
        .expect("diagnostics should be array");
    assert_eq!(diags.len(), 1);
}

#[test]
fn set_parameter_missing_cell_id_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool(
        "reify_set_parameter",
        serde_json::json!({"value": "120mm"}),
        &ctx,
    );

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

#[test]
fn set_parameter_missing_value_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool(
        "reify_set_parameter",
        serde_json::json!({"cell_id": "Bracket.width"}),
        &ctx,
    );

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

// === reify_open_file ===

#[test]
fn open_file_returns_success_with_source() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        source: reify_mcp::types::SourceContent {
            content: "param x = 10mm".to_string(),
            file_path: "main.ri".to_string(),
        },
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_open_file",
            serde_json::json!({"file_path": "main.ri"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
    assert_eq!(result["source"], "param x = 10mm");
}

#[test]
fn open_file_missing_file_path_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool("reify_open_file", serde_json::json!({}), &ctx);

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

// === reify_set_parameter (continued) ===

#[test]
fn set_parameter_context_error_propagates() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        set_param_error: Some(ToolError::EngineError("param not found".to_string())),
        ..Default::default()
    };

    let result = registry.call_tool(
        "reify_set_parameter",
        serde_json::json!({"cell_id": "Bracket.width", "value": "120mm"}),
        &ctx,
    );

    match result {
        Err(ToolError::EngineError(msg)) => assert!(msg.contains("param not found")),
        other => panic!("expected EngineError, got: {other:?}"),
    }
}

// === reify_save_file ===

#[test]
fn save_file_with_path_returns_success() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_save_file",
            serde_json::json!({"file_path": "main.ri"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
}

#[test]
fn save_file_without_path_saves_active_file() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool("reify_save_file", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result["success"], true);
}

// === reify_export ===

#[test]
fn export_step_returns_success() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_export",
            serde_json::json!({"format": "step", "output_path": "/tmp/out.step"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
    assert_eq!(result["path"], "/tmp/out.step");
}

#[test]
fn export_stl_returns_success() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_export",
            serde_json::json!({"format": "stl", "output_path": "/tmp/out.stl"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["success"], true);
}

#[test]
fn export_missing_format_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool(
        "reify_export",
        serde_json::json!({"output_path": "/tmp/out.step"}),
        &ctx,
    );

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

#[test]
fn export_missing_output_path_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool("reify_export", serde_json::json!({"format": "step"}), &ctx);

    match result {
        Err(ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}

// === reify_set_parameter schema: the unit contract (task #6184) ===

/// `reify_set_parameter`'s `value` schema description must DECLARE the unit
/// contract it silently relies on (INV-AD-4;
/// `docs/prds/v0_6/angle-dimension-completion.md`, §9 B7).
///
/// This is a WIRE-VISIBLE CONTRACT, not a source docstring. The schema JSON is
/// data served to every MCP client over `tools/list`, and its silence IS the
/// defect: `mcp_context.rs` parses a bare `f64` out of this field and installs
/// it as the cell's SI-coherent magnitude — for an Angle cell, RADIANS —
/// applying no conversion and parsing no unit suffix. A client sending
/// `1.5708` gets 1.5708 rad, not 1.5708°, and nothing on the wire says so.
///
/// The assertion is deliberately SMALL and semantic: two word-boundary token
/// checks plus an additive check that the field still describes a string
/// expression. It does NOT pin the sentence, regex the prose, or assert on any
/// Rust doc comment — a wording-fragile pin here would be exactly the
/// documentation meta-test this repo avoids.
///
/// Word boundaries matter, and are not pedantry: a naive
/// `description.contains("si")` is satisfied by the CURRENT text, because
/// "expre**si**on" contains it. That check would be green on arrival and would
/// pin nothing.
#[test]
fn set_parameter_schema_declares_the_si_unit_contract() {
    let registry = setup_registry();
    let tools = registry.list_tools();
    let info = tools
        .iter()
        .find(|t| t.name == "reify_set_parameter")
        .expect("reify_set_parameter must be registered");

    let description = info.input_schema["properties"]["value"]["description"]
        .as_str()
        .expect("reify_set_parameter's `value` property must carry a description string");
    let lowered = description.to_lowercase();

    // Word-boundary tokens, so substring accidents cannot satisfy the pin.
    let words: Vec<&str> = lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    assert!(
        words.contains(&"si"),
        "the `value` schema description must name the SI contract as a word (the value is \
         installed as the cell's SI-coherent magnitude with no conversion), but it reads: \
         {description:?}"
    );
    assert!(
        lowered.contains("radian"),
        "the `value` schema description must name RADIANS — the angular crossing this contract \
         exists for, where a client sending 1.5708 gets 1.5708 rad and not 1.5708 degrees — but \
         it reads: {description:?}"
    );

    // Additive, not a rewrite: the field is still a string expression.
    assert!(
        words.contains(&"string") && words.contains(&"expression"),
        "the unit contract must be ADDED to the `value` description, not replace what it already \
         told clients (that the value is a string expression), but it reads: {description:?}"
    );
}
