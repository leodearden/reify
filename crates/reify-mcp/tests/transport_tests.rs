use std::sync::Arc;

use reify_mcp::context::MockToolContext;
use reify_mcp::transport::McpServer;

#[test]
fn in_process_tools_list_returns_16_tools() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let response_str = server.handle_message(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16, "Expected 16 tools, got {}", tools.len());
}

#[test]
fn in_process_tools_call_stub_returns_not_implemented() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "reify_get_source",
            "arguments": {}
        }
    });
    let response_str = server.handle_message(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["result"]["isError"], true);
    let content = response["result"]["content"].as_array().unwrap();
    assert!(content[0]["text"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("not implemented"));
}

#[test]
fn in_process_unknown_method_returns_error() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = r#"{"jsonrpc":"2.0","id":3,"method":"bogus/method","params":{}}"#;
    let response_str = server.handle_message(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["error"]["code"], -32601);
}

#[tokio::test]
async fn stream_mode_tools_list() {
    let ctx = Arc::new(MockToolContext::default());
    let server = Arc::new(McpServer::new(ctx));

    let (client_reader, server_writer) = tokio::io::duplex(4096);
    let (server_reader, client_writer) = tokio::io::duplex(4096);

    let server_clone = server.clone();
    let handle = tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(server_reader);
        server_clone.run_on_streams(reader, server_writer).await;
    });

    // Write a tools/list request
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let message = format!("{request}\n");
    use tokio::io::AsyncWriteExt;
    let mut writer = client_writer;
    writer.write_all(message.as_bytes()).await.unwrap();
    writer.shutdown().await.unwrap();

    // Read the response
    use tokio::io::AsyncBufReadExt;
    let mut reader = tokio::io::BufReader::new(client_reader);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();

    let response: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16);

    handle.await.unwrap();
}
