use reify_mcp::context::MockToolContext;
use reify_mcp::jsonrpc::{McpDispatcher, INVALID_REQUEST};
use reify_mcp::registry::ToolRegistry;
use reify_mcp::types::ToolError;

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(
        "reify_get_source",
        "Get source code",
        serde_json::json!({"type": "object", "properties": {"file_path": {"type": "string"}}}),
        |_params, _ctx| Err(ToolError::NotImplemented),
    );
    registry.register(
        "reify_get_eval_status",
        "Get eval status",
        serde_json::json!({"type": "object", "properties": {}}),
        |_params, ctx| {
            let status = ctx.get_eval_status()?;
            Ok(serde_json::json!({"phase": status.phase}))
        },
    );
    registry
}

#[test]
fn dispatch_tools_list_returns_tools() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "reify_get_source");
}

#[test]
fn dispatch_tools_call_dispatches_to_registry() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "reify_get_eval_status",
            "arguments": {}
        }
    });
    let response_str = dispatcher.dispatch(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 2);
    // MCP tools/call returns content array
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    let text: serde_json::Value =
        serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["phase"], "idle");
}

#[test]
fn dispatch_unknown_method_returns_method_not_found() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":3,"method":"unknown/method","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 3);
    assert_eq!(response["error"]["code"], -32601);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Method not found"));
}

#[test]
fn dispatch_malformed_json_returns_parse_error() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let response_str = dispatcher.dispatch("not valid json{{{");
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["error"]["code"], -32700);
}

#[test]
fn dispatch_tools_call_unknown_tool_returns_error() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });
    let response_str = dispatcher.dispatch(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    // MCP returns tool errors as isError content, not JSON-RPC errors
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert!(content[0]["text"]
        .as_str()
        .unwrap()
        .contains("nonexistent_tool"));
}

#[test]
fn response_has_correct_jsonrpc_fields() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    // Success response
    let request = r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "abc");
    assert!(response.get("result").is_some());
    assert!(response.get("error").is_none());
}

// --- S2: JSON-RPC request validation tests ---

#[test]
fn dispatch_invalid_jsonrpc_version_returns_error() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"1.0","id":1,"method":"tools/list","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
}

#[test]
fn dispatch_null_id_returns_invalid_request() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":null,"method":"tools/list","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
}

#[test]
fn dispatch_array_id_returns_invalid_request() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":[1,2],"method":"tools/list","params":{}}"#;
    let response_str = dispatcher.dispatch(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
}
