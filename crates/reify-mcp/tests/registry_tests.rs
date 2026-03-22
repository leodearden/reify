use reify_mcp::types::{ToolError, ToolInfo};

#[test]
fn tool_error_not_implemented_has_display() {
    let err = ToolError::NotImplemented;
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("not implemented"),
        "Expected 'not implemented' in display, got: {msg}"
    );
}

#[test]
fn tool_error_invalid_params_carries_message() {
    let err = ToolError::InvalidParams("missing field 'name'".to_string());
    let msg = format!("{err}");
    assert!(
        msg.contains("missing field 'name'"),
        "Expected custom message in display, got: {msg}"
    );
}

#[test]
fn tool_error_all_variants_roundtrip_display() {
    let variants: Vec<ToolError> = vec![
        ToolError::NotImplemented,
        ToolError::InvalidParams("bad params".to_string()),
        ToolError::InternalError("internal".to_string()),
        ToolError::EngineError("engine failed".to_string()),
    ];

    for err in &variants {
        let display = format!("{err}");
        assert!(!display.is_empty(), "Display should not be empty for {err:?}");
    }
}

#[test]
fn tool_info_serializes_to_json_with_required_fields() {
    let info = ToolInfo {
        name: "reify_get_source".to_string(),
        description: "Get source code".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" }
            }
        }),
    };

    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["name"], "reify_get_source");
    assert_eq!(json["description"], "Get source code");
    assert_eq!(json["inputSchema"]["type"], "object");
}
