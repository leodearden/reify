use reify_mcp::context::ReifyToolContext;
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

// --- ReifyToolContext trait tests ---

#[test]
fn context_trait_is_object_safe() {
    // This compiles only if the trait is object-safe
    fn _accepts_dyn(_ctx: &dyn ReifyToolContext) {}
}

#[test]
fn context_trait_is_send_sync() {
    fn _assert_send_sync<T: Send + Sync>() {}
    _assert_send_sync::<Box<dyn ReifyToolContext>>();
}

#[test]
fn mock_context_returns_canned_data() {
    use reify_mcp::context::MockToolContext;

    let mock = MockToolContext::default();
    let files = mock.get_open_files().unwrap();
    assert!(files.is_empty(), "Default mock should return empty open files");

    let status = mock.get_eval_status().unwrap();
    assert_eq!(status.phase, "idle");
}
