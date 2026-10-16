use reify_mcp::context::MockToolContext;
use reify_mcp::jsonrpc::{INVALID_PARAMS, INVALID_REQUEST, JsonRpcResponse, McpDispatcher};
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

fn reply(dispatcher: &McpDispatcher<'_>, request: &str) -> serde_json::Value {
    let response = dispatcher
        .dispatch(request)
        .expect("a request carrying an id must get a reply");
    serde_json::from_str(&response).unwrap()
}

#[test]
fn dispatch_tools_list_returns_tools() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let response = reply(&dispatcher, request);

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
    let response = reply(&dispatcher, &request.to_string());

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
    let response = reply(&dispatcher, request);

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 3);
    assert_eq!(response["error"]["code"], -32601);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Method not found")
    );
}

#[test]
fn dispatch_malformed_json_returns_parse_error() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let response = reply(&dispatcher, "not valid json{{{");

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
    let response = reply(&dispatcher, &request.to_string());

    // MCP returns tool errors as isError content, not JSON-RPC errors
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .contains("nonexistent_tool")
    );
}

#[test]
fn response_has_correct_jsonrpc_fields() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    // Success response
    let request = r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list","params":{}}"#;
    let response = reply(&dispatcher, request);

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
    let response = reply(&dispatcher, request);

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
}

#[test]
fn dispatch_null_id_returns_invalid_request() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":null,"method":"tools/list","params":{}}"#;
    let response = reply(&dispatcher, request);

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
    assert!(response["id"].is_null());
}

#[test]
fn dispatch_array_id_returns_invalid_request() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"2.0","id":[1,2],"method":"tools/list","params":{}}"#;
    let response = reply(&dispatcher, request);

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
}

// --- Notifications: an id-less message is never answered ---

#[test]
fn dispatch_initialized_notification_produces_no_reply() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    assert_eq!(dispatcher.dispatch(notification), None);
}

#[test]
fn dispatch_unknown_method_notification_produces_no_reply() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let notification = r#"{"jsonrpc":"2.0","method":"bogus/notify","params":{}}"#;

    assert_eq!(dispatcher.dispatch(notification), None);
}

#[test]
fn dispatch_notification_with_wrong_jsonrpc_version_replies_invalid_request() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = r#"{"jsonrpc":"1.0","method":"notifications/initialized"}"#;
    let response = reply(&dispatcher, request);

    assert_eq!(response["error"]["code"], INVALID_REQUEST);
    assert!(response["id"].is_null());
}

// --- S2+S3: tools/call input validation tests ---

#[test]
fn dispatch_tools_call_missing_name_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "arguments": {}
        }
    });
    let response = reply(&dispatcher, &request.to_string());

    // Should be an MCP isError content response
    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("name"),
        "Error should mention 'name', got: {text}"
    );
}

#[test]
fn dispatch_tools_call_numeric_name_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": 123,
            "arguments": {}
        }
    });
    let response = reply(&dispatcher, &request.to_string());

    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("name"),
        "Error should mention 'name', got: {text}"
    );
}

#[test]
fn dispatch_tools_call_array_arguments_returns_invalid_params() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();
    let dispatcher = McpDispatcher::new(&registry, &ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "reify_get_source",
            "arguments": [1, 2]
        }
    });
    let response = reply(&dispatcher, &request.to_string());

    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("arguments"),
        "Error should mention 'arguments', got: {text}"
    );
}

// --- Response envelope: result XOR error ---

#[test]
fn response_ok_serializes_result_without_error() {
    let response = JsonRpcResponse::ok(serde_json::json!(7), serde_json::json!({"k": 1}));

    assert_eq!(
        serde_json::to_value(response).unwrap(),
        serde_json::json!({"jsonrpc": "2.0", "id": 7, "result": {"k": 1}})
    );
}

#[test]
fn response_err_serializes_error_without_result() {
    let response = JsonRpcResponse::err(serde_json::json!("a"), INVALID_PARAMS, "bad");

    assert_eq!(
        serde_json::to_value(response).unwrap(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "a",
            "error": {"code": -32602, "message": "bad"}
        })
    );
}
