use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

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
fn in_process_tools_call_missing_params_returns_error() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "reify_update_source",
            "arguments": {}
        }
    });
    let response_str = server.handle_message(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["result"]["isError"], true);
    let content = response["result"]["content"].as_array().unwrap();
    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("invalid parameters")
    );
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
        server_clone
            .run_on_streams(reader, server_writer)
            .await
            .unwrap();
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

// --- Integration tests: full round-trip ---

#[test]
fn integration_initialize_returns_capabilities() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let response_str = server.handle_message(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert_eq!(response["result"]["serverInfo"]["name"], "reify-mcp");
}

#[test]
fn integration_tools_list_has_all_16_correct_names() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let response_str = server.handle_message(request);
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 16);

    let expected_names = [
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
    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in &expected_names {
        assert!(tool_names.contains(expected), "Missing tool: {expected}");
    }
}

#[test]
fn integration_tools_call_missing_params_error() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "reify_update_source",
            "arguments": {}
        }
    });
    let response_str = server.handle_message(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.to_lowercase().contains("invalid parameters"),
        "Expected 'invalid parameters' in error text: {text}"
    );
}

#[test]
fn integration_tools_call_nonexistent_tool() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });
    let response_str = server.handle_message(&request.to_string());
    let response: serde_json::Value = serde_json::from_str(&response_str).unwrap();

    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("nonexistent_tool"),
        "Error should mention tool name: {text}"
    );
}

// --- S5: Transport error handling tests ---

/// A writer that always fails on write_all.
struct FailingWriter;

impl tokio::io::AsyncWrite for FailingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "test write error",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn stream_mode_write_error_returns_err() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    // A reader with one valid request
    let request = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}\n";
    let reader = tokio::io::BufReader::new(&request[..]);
    let writer = FailingWriter;

    let result = server.run_on_streams(reader, writer).await;
    assert!(result.is_err(), "Expected Err from write failure, got Ok");
}

#[tokio::test]
async fn stream_mode_graceful_eof_returns_ok() {
    let ctx = Arc::new(MockToolContext::default());
    let server = McpServer::new(ctx);

    // Empty reader — immediate EOF
    let reader = tokio::io::BufReader::new(&b""[..]);
    let (_, writer) = tokio::io::duplex(4096);

    let result = server.run_on_streams(reader, writer).await;
    assert!(
        result.is_ok(),
        "Expected Ok on graceful EOF, got Err: {:?}",
        result.err()
    );
}
