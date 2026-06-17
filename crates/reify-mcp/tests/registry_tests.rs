use reify_mcp::context::{MockToolContext, ReifyToolContext};
use reify_mcp::registry::ToolRegistry;
use reify_mcp::types::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, SelectionInfo,
    SetParamResult, SourceContent, SourceLocationInfo, ToolError, ToolInfo, UpdateResult,
};

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
        assert!(
            !display.is_empty(),
            "Display should not be empty for {err:?}"
        );
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

// --- PartialEq derive tests (S1) ---

#[test]
fn source_content_partial_eq() {
    let a = SourceContent {
        content: "x = 1".to_string(),
        file_path: "main.ri".to_string(),
    };
    let b = SourceContent {
        content: "x = 1".to_string(),
        file_path: "main.ri".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn open_file_info_partial_eq() {
    let a = OpenFileInfo {
        path: "/a.ri".to_string(),
        language: "reify".to_string(),
        dirty: false,
    };
    let b = OpenFileInfo {
        path: "/a.ri".to_string(),
        language: "reify".to_string(),
        dirty: false,
    };
    assert_eq!(a, b);
}

#[test]
fn diagnostic_info_partial_eq() {
    let a = DiagnosticInfo {
        file_path: "a.ri".to_string(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 5,
        severity: "error".to_string(),
        message: "bad".to_string(),
        code: None,
        has_location: true,
    };
    let b = DiagnosticInfo {
        file_path: "a.ri".to_string(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 5,
        severity: "error".to_string(),
        message: "bad".to_string(),
        code: None,
        has_location: true,
    };
    assert_eq!(a, b);
}

#[test]
fn parameter_info_partial_eq() {
    let a = ParameterInfo {
        cell_id: "c1".to_string(),
        name: "width".to_string(),
        value: "10".to_string(),
        unit: "mm".to_string(),
        kind: "real".to_string(),
        entity_path: "/box".to_string(),
        determinacy: "determined".to_string(),
        reason: None,
    };
    let b = ParameterInfo {
        cell_id: "c1".to_string(),
        name: "width".to_string(),
        value: "10".to_string(),
        unit: "mm".to_string(),
        kind: "real".to_string(),
        entity_path: "/box".to_string(),
        determinacy: "determined".to_string(),
        reason: None,
    };
    assert_eq!(a, b);
}

#[test]
fn constraint_info_partial_eq() {
    let a = ConstraintInfo {
        node_id: "n1".to_string(),
        expression: "x > 0".to_string(),
        status: "satisfied".to_string(),
        label: Some("pos".to_string()),
        parameter_ids: vec!["c1".to_string()],
    };
    let b = ConstraintInfo {
        node_id: "n1".to_string(),
        expression: "x > 0".to_string(),
        status: "satisfied".to_string(),
        label: Some("pos".to_string()),
        parameter_ids: vec!["c1".to_string()],
    };
    assert_eq!(a, b);
}

#[test]
fn eval_status_info_partial_eq() {
    let a = EvalStatusInfo {
        phase: "idle".to_string(),
        progress: Some(1.0),
        dirty_count: 0,
    };
    let b = EvalStatusInfo {
        phase: "idle".to_string(),
        progress: Some(1.0),
        dirty_count: 0,
    };
    assert_eq!(a, b);
}

#[test]
fn selection_info_partial_eq() {
    let a = SelectionInfo {
        selected_entity: Some("box".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    };
    let b = SelectionInfo {
        selected_entity: Some("box".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    };
    assert_eq!(a, b);
}

#[test]
fn source_location_info_partial_eq() {
    let a = SourceLocationInfo {
        file_path: "a.ri".to_string(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 5,
    };
    let b = SourceLocationInfo {
        file_path: "a.ri".to_string(),
        line: 1,
        column: 0,
        end_line: 1,
        end_column: 5,
    };
    assert_eq!(a, b);
}

#[test]
fn source_location_info_serializes_file_path_key() {
    let loc = SourceLocationInfo {
        file_path: "src/main.ri".to_string(),
        line: 10,
        column: 3,
        end_line: 10,
        end_column: 15,
    };
    let json = serde_json::to_value(&loc).unwrap();
    assert_eq!(
        json["file_path"], "src/main.ri",
        "SourceLocationInfo must serialize the file field as 'file_path' JSON key"
    );
    assert!(
        json.get("file").is_none(),
        "SourceLocationInfo must not have a 'file' JSON key"
    );
}

#[test]
fn update_result_partial_eq() {
    let a = UpdateResult {
        success: true,
        diagnostics_count: 0,
    };
    let b = UpdateResult {
        success: true,
        diagnostics_count: 0,
    };
    assert_eq!(a, b);
}

#[test]
fn set_param_result_partial_eq() {
    let a = SetParamResult {
        success: true,
        new_value: "10".to_string(),
        unit: "mm".to_string(),
    };
    let b = SetParamResult {
        success: true,
        new_value: "10".to_string(),
        unit: "mm".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn tool_info_partial_eq() {
    let a = ToolInfo {
        name: "test".to_string(),
        description: "desc".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    };
    let b = ToolInfo {
        name: "test".to_string(),
        description: "desc".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    };
    assert_eq!(a, b);
}

#[test]
fn tool_error_partial_eq() {
    assert_eq!(ToolError::NotImplemented, ToolError::NotImplemented);
    assert_eq!(
        ToolError::InvalidParams("x".to_string()),
        ToolError::InvalidParams("x".to_string())
    );
    assert_eq!(
        ToolError::InternalError("y".to_string()),
        ToolError::InternalError("y".to_string())
    );
    assert_eq!(
        ToolError::EngineError("z".to_string()),
        ToolError::EngineError("z".to_string())
    );
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
    assert!(
        files.is_empty(),
        "Default mock should return empty open files"
    );

    let status = mock.get_eval_status().unwrap();
    assert_eq!(status.phase, "idle");
}

// --- ToolRegistry tests ---

#[test]
fn registry_register_and_list_returns_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(
        "test_tool",
        "A test tool",
        serde_json::json!({"type": "object"}),
        |_params, _ctx| Ok(serde_json::json!({"result": "ok"})),
    );

    let tools = registry.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "test_tool");
    assert_eq!(tools[0].description, "A test tool");
    assert_eq!(tools[0].input_schema["type"], "object");
}

#[test]
fn registry_call_tool_returns_result() {
    let mut registry = ToolRegistry::new();
    registry.register(
        "echo",
        "Echo tool",
        serde_json::json!({"type": "object"}),
        |params, _ctx| Ok(params),
    );

    let ctx = MockToolContext::default();
    let params = serde_json::json!({"message": "hello"});
    let result = registry.call_tool("echo", params.clone(), &ctx).unwrap();
    assert_eq!(result, params);
}

#[test]
fn registry_call_unknown_tool_returns_error() {
    let registry = ToolRegistry::new();
    let ctx = MockToolContext::default();
    let result = registry.call_tool("nonexistent", serde_json::json!({}), &ctx);
    assert!(result.is_err());
    match result.unwrap_err() {
        ToolError::InvalidParams(msg) => {
            assert!(
                msg.contains("nonexistent"),
                "Error should mention tool name: {msg}"
            );
        }
        other => panic!("Expected InvalidParams, got: {other:?}"),
    }
}

#[test]
fn registry_multiple_tools_preserves_order() {
    let mut registry = ToolRegistry::new();
    for name in ["alpha", "beta", "gamma"] {
        registry.register(
            name,
            &format!("{name} tool"),
            serde_json::json!({"type": "object"}),
            |_params, _ctx| Ok(serde_json::json!(null)),
        );
    }

    let tools = registry.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

// --- S4: Duplicate registration test ---

#[test]
#[should_panic(expected = "duplicate")]
fn registry_duplicate_name_panics() {
    let mut registry = ToolRegistry::new();
    registry.register(
        "alpha",
        "First alpha",
        serde_json::json!({"type": "object"}),
        |_params, _ctx| Ok(serde_json::json!(null)),
    );
    registry.register(
        "alpha",
        "Second alpha",
        serde_json::json!({"type": "object"}),
        |_params, _ctx| Ok(serde_json::json!(null)),
    );
}

#[test]
fn registry_handler_receives_params_and_context() {
    let mut registry = ToolRegistry::new();
    registry.register(
        "get_status",
        "Get eval status from context",
        serde_json::json!({"type": "object"}),
        |_params, ctx: &dyn ReifyToolContext| {
            let status = ctx.get_eval_status()?;
            Ok(serde_json::json!({"phase": status.phase}))
        },
    );

    let ctx = MockToolContext::default();
    let result = registry
        .call_tool("get_status", serde_json::json!({}), &ctx)
        .unwrap();
    assert_eq!(result["phase"], "idle");
}

// --- Re-export identity tests ---

#[test]
fn diagnostic_info_is_reexported_from_reify_types() {
    use std::any::TypeId;
    // After reify-mcp re-exports DiagnosticInfo from reify-types, both paths must
    // resolve to the *same* type (same TypeId). This guards against a newtype wrapper
    // accidentally breaking the identity.
    assert_eq!(
        TypeId::of::<reify_mcp::DiagnosticInfo>(),
        TypeId::of::<reify_core::DiagnosticInfo>(),
        "reify_mcp::DiagnosticInfo must be the same type as reify_core::DiagnosticInfo"
    );
}

#[test]
fn source_location_info_is_reexported_from_reify_types() {
    use std::any::TypeId;
    // After reify-mcp re-exports SourceLocationInfo from reify-types, both paths must
    // resolve to the *same* type (same TypeId).
    assert_eq!(
        TypeId::of::<reify_mcp::SourceLocationInfo>(),
        TypeId::of::<reify_core::SourceLocationInfo>(),
        "reify_mcp::SourceLocationInfo must be the same type as reify_core::SourceLocationInfo"
    );
}
