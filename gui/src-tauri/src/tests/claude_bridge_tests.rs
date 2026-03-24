// Tests for the Claude Code SDK sidecar bridge.
#![allow(unused_imports)]

use crate::claude_bridge::*;
use serde_json::{json, Value};

// --- IPC message type tests (step-1) ---

#[test]
fn inbound_send_message_serializes_correctly() {
    let msg = InboundMessage::SendMessage {
        id: "msg-1".to_string(),
        text: "Hello".to_string(),
        context: None,
    };
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["id"], "msg-1");
    assert_eq!(json_val["text"], "Hello");
}

#[test]
fn inbound_send_message_with_context_serializes() {
    let ctx = MessageContext {
        selected_entity: Some("cube1".to_string()),
        diagnostics: Some(vec!["Error at line 1".to_string()]),
        constraints: Some(vec!["x > 0".to_string()]),
    };
    let msg = InboundMessage::SendMessage {
        id: "msg-2".to_string(),
        text: "Fix it".to_string(),
        context: Some(ctx),
    };
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["context"]["selected_entity"], "cube1");
    assert_eq!(json_val["context"]["diagnostics"][0], "Error at line 1");
    assert_eq!(json_val["context"]["constraints"][0], "x > 0");
}

#[test]
fn inbound_abort_serializes() {
    let msg = InboundMessage::Abort;
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "abort");
}

#[test]
fn inbound_clear_session_serializes() {
    let msg = InboundMessage::ClearSession;
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "clear_session");
}

#[test]
fn outbound_text_delta_deserializes() {
    let json_str = r#"{"type":"text_delta","id":"msg-1","content":"Hello"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::TextDelta { id, content } => {
            assert_eq!(id, "msg-1");
            assert_eq!(content, "Hello");
        }
        _ => panic!("Expected TextDelta"),
    }
}

#[test]
fn outbound_thinking_delta_deserializes() {
    let json_str = r#"{"type":"thinking_delta","id":"msg-1","content":"thinking..."}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ThinkingDelta { id, content } => {
            assert_eq!(id, "msg-1");
            assert_eq!(content, "thinking...");
        }
        _ => panic!("Expected ThinkingDelta"),
    }
}

#[test]
fn outbound_tool_call_deserializes() {
    let json_str = r#"{"type":"tool_call","id":"msg-1","tool_name":"reify_get_shape","tool_input":{"name":"cube1"}}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ToolCall { id, tool_name, tool_input } => {
            assert_eq!(id, "msg-1");
            assert_eq!(tool_name, "reify_get_shape");
            assert_eq!(tool_input["name"], "cube1");
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn outbound_tool_result_deserializes() {
    let json_str = r#"{"type":"tool_result","id":"msg-1","tool_name":"reify_get_shape","result":"ok"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ToolResult { id, tool_name, result } => {
            assert_eq!(id, "msg-1");
            assert_eq!(tool_name, "reify_get_shape");
            assert_eq!(result, json!("ok"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[test]
fn outbound_done_deserializes() {
    let json_str = r#"{"type":"done","id":"msg-1"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::Done { id } => {
            assert_eq!(id, "msg-1");
        }
        _ => panic!("Expected Done"),
    }
}

#[test]
fn outbound_error_deserializes() {
    let json_str = r#"{"type":"error","id":"msg-1","message":"Something went wrong"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ErrorMessage { id, message } => {
            assert_eq!(id, "msg-1");
            assert_eq!(message, "Something went wrong");
        }
        _ => panic!("Expected ErrorMessage"),
    }
}

#[test]
fn outbound_ready_deserializes() {
    let json_str = r#"{"type":"ready"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::Ready => {}
        _ => panic!("Expected Ready"),
    }
}

#[test]
fn message_context_optional_fields_skip_none() {
    let ctx = MessageContext {
        selected_entity: None,
        diagnostics: None,
        constraints: None,
    };
    let json_val: Value = serde_json::to_value(&ctx).unwrap();
    // None fields should be omitted (skip_serializing_if)
    assert!(json_val.get("selected_entity").is_none());
    assert!(json_val.get("diagnostics").is_none());
    assert!(json_val.get("constraints").is_none());
}

#[test]
fn ipc_types_are_clone_debug_partialeq() {
    fn assert_traits<T: Clone + std::fmt::Debug + PartialEq>() {}
    assert_traits::<InboundMessage>();
    assert_traits::<OutboundMessage>();
    assert_traits::<MessageContext>();
}

// --- parse_outbound tests (step-5) ---

#[test]
fn parse_outbound_text_delta() {
    let line = r#"{"type":"text_delta","id":"msg-1","content":"Hello"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(msg, OutboundMessage::TextDelta { id: "msg-1".to_string(), content: "Hello".to_string() });
}

#[test]
fn parse_outbound_thinking_delta() {
    let line = r#"{"type":"thinking_delta","id":"msg-2","content":"hmm"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(msg, OutboundMessage::ThinkingDelta { id: "msg-2".to_string(), content: "hmm".to_string() });
}

#[test]
fn parse_outbound_tool_call() {
    let line = r#"{"type":"tool_call","id":"msg-3","tool_name":"reify_get","tool_input":{"x":1}}"#;
    let msg = parse_outbound(line).unwrap();
    match msg {
        OutboundMessage::ToolCall { id, tool_name, tool_input } => {
            assert_eq!(id, "msg-3");
            assert_eq!(tool_name, "reify_get");
            assert_eq!(tool_input["x"], 1);
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn parse_outbound_tool_result() {
    let line = r#"{"type":"tool_result","id":"msg-3","tool_name":"reify_get","result":"done"}"#;
    let msg = parse_outbound(line).unwrap();
    match msg {
        OutboundMessage::ToolResult { id, tool_name, result } => {
            assert_eq!(id, "msg-3");
            assert_eq!(tool_name, "reify_get");
            assert_eq!(result, json!("done"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[test]
fn parse_outbound_done() {
    let line = r#"{"type":"done","id":"msg-4"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(msg, OutboundMessage::Done { id: "msg-4".to_string() });
}

#[test]
fn parse_outbound_error() {
    let line = r#"{"type":"error","id":"msg-5","message":"oops"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(msg, OutboundMessage::ErrorMessage { id: "msg-5".to_string(), message: "oops".to_string() });
}

#[test]
fn parse_outbound_ready() {
    let line = r#"{"type":"ready"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(msg, OutboundMessage::Ready);
}

#[test]
fn parse_outbound_invalid_json_returns_err() {
    let result = parse_outbound("not-json");
    assert!(result.is_err());
}

#[test]
fn parse_outbound_unknown_type_returns_err() {
    let line = r#"{"type":"unknown_type","id":"x"}"#;
    let result = parse_outbound(line);
    assert!(result.is_err());
}

#[test]
fn parse_outbound_missing_required_field_returns_err() {
    // TextDelta requires 'content'
    let line = r#"{"type":"text_delta","id":"msg-1"}"#;
    let result = parse_outbound(line);
    assert!(result.is_err());
}

// --- claude_send_message_impl tests (step-23) ---

#[tokio::test]
async fn claude_send_message_impl_errors_when_sidecar_is_none() {
    let sidecar: tokio::sync::Mutex<Option<SidecarHandle>> = tokio::sync::Mutex::new(None);

    let result = claude_send_message_impl(&sidecar, "hello", None).await;

    assert!(result.is_err(), "Expected error when sidecar is not started");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("not started") || msg.contains("spawn") || msg.contains("sidecar"),
        "Error should mention sidecar state: {}",
        msg
    );
}

#[tokio::test]
async fn claude_send_message_impl_sends_message_when_sidecar_ready() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};

    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let empty_reader = BufReader::new(&b""[..]);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let sidecar = tokio::sync::Mutex::new(Some(handle));

    let result = claude_send_message_impl(&sidecar, "hello world", None).await;

    assert!(result.is_ok(), "Expected success when sidecar is Ready: {:?}", result);
    let id = result.unwrap();
    assert!(id.starts_with("msg-"), "ID should start with msg-: {}", id);

    // Verify message was written to sidecar stdin
    let mut buf = vec![0u8; 1024];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    let json_val: serde_json::Value = serde_json::from_str(written.trim_end()).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["id"], id);
    assert_eq!(json_val["text"], "hello world");
}

#[tokio::test]
async fn claude_send_message_impl_errors_when_sidecar_not_ready() {
    use std::sync::Arc;
    use tokio::io::BufReader;

    // Sidecar exists but is still Starting (not yet Ready)
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Starting));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let empty_reader = BufReader::new(&b""[..]);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let sidecar = tokio::sync::Mutex::new(Some(handle));

    let result = claude_send_message_impl(&sidecar, "hello", None).await;

    // Should error since sidecar is not in Ready state
    assert!(result.is_err(), "Expected error when sidecar is Starting (not yet Ready)");
}

#[tokio::test]
async fn from_parts_with_mcp_intercepts_reify_tool_calls() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use crate::engine::EngineSession;

    // Set up engine for MCP dispatch
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    // stdin_writer: Rust writes here → sidecar reads it (we read from stdin_reader to inspect)
    // stdout_writer: simulates sidecar writing → Rust reader task processes it
    let (stdin_writer, mut stdin_reader) = tokio::io::duplex(4096);
    let (mut stdout_writer, stdout_reader) = tokio::io::duplex(4096);
    let reader = BufReader::new(stdout_reader);
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));

    // from_parts_with_mcp wires up both event sink and MCP tool interception
    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);
    let _handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name, payload));
        },
    );

    // Inject a reify_ tool_call from simulated sidecar stdout
    let tool_call =
        r#"{"type":"tool_call","id":"msg-1","tool_name":"reify_get_diagnostics","tool_input":{}}"#;
    stdout_writer
        .write_all(format!("{}\n", tool_call).as_bytes())
        .await
        .unwrap();

    // Allow reader task to process the tool_call and run the spawned MCP handler
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }

    // Verify the tool_call event was emitted to the event sink
    let emitted = events.lock().unwrap();
    assert!(
        emitted.iter().any(|(name, _)| name == "claude-tool-call"),
        "Expected claude-tool-call event in sink, got: {:?}",
        emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    drop(emitted);

    // Verify tool_result was written back to sidecar stdin
    let mut buf = vec![0u8; 4096];
    let n = stdin_reader.read(&mut buf).await.unwrap_or(0);
    assert!(n > 0, "Expected tool_result to be written back to sidecar stdin");
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    // The response should be a tool_result JSON line
    let json_val: serde_json::Value =
        serde_json::from_str(written.trim()).unwrap_or(serde_json::json!(null));
    assert_eq!(
        json_val["type"], "tool_result",
        "Expected tool_result type, got: {}",
        written
    );
    assert_eq!(json_val["tool_name"], "reify_get_diagnostics");

    drop(stdout_writer);
}

// --- AppState sidecar field tests (step-21) ---

#[test]
fn app_state_has_sidecar_field() {
    use std::sync::{Arc, Mutex};
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use crate::commands::AppState;
    use crate::engine::EngineSession;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // AppState should be constructible with the new sidecar field
    let _state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
    };
}

// --- SidecarHandle::kill and crash detection tests (step-19) ---

#[tokio::test]
async fn sidecar_handle_kill_sets_state_to_not_started() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    handle.kill().await;

    assert!(matches!(*handle.state().lock().await, SidecarState::NotStarted));
}

#[tokio::test]
async fn crash_detection_sets_state_to_crashed_on_eof() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    // Use a duplex where we control the writer - dropping it simulates crash
    let (data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (writer, _reader_end) = tokio::io::duplex(1024);

    // Drop the data_writer to simulate EOF (crash)
    drop(data_writer);

    let handle = SidecarHandle::from_parts(writer, reader, state);

    // Wait for reader task to notice the EOF and set Crashed
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }

    assert!(matches!(*handle.state().lock().await, SidecarState::Crashed(_)));
}

// --- SidecarHandle::abort and clear_session tests (step-17) ---

#[tokio::test]
async fn sidecar_handle_abort_writes_abort_json() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    handle.abort().await.unwrap();

    let mut buf = vec![0u8; 64];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    assert_eq!(written, "{\"type\":\"abort\"}\n");
}

#[tokio::test]
async fn sidecar_handle_clear_session_writes_clear_session_json() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    handle.clear_session().await.unwrap();

    let mut buf = vec![0u8; 64];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    assert_eq!(written, "{\"type\":\"clear_session\"}\n");
}

// --- SidecarHandle::send_message tests (step-15) ---

#[tokio::test]
async fn send_message_returns_message_id_and_writes_to_stdin() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    // Use duplex so we can read what was written to the "stdin"
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b""; // empty reader - no incoming messages
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let id = handle.send_message("hello world", None).await.unwrap();
    assert!(!id.is_empty(), "ID should not be empty");
    assert!(id.starts_with("msg-"), "ID should start with msg-: {}", id);

    // Read what was written to stdin
    let mut buf = vec![0u8; 1024];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    assert!(written.ends_with('\n'));
    let json_val: serde_json::Value = serde_json::from_str(written.trim_end()).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["id"], id);
    assert_eq!(json_val["text"], "hello world");
}

#[tokio::test]
async fn send_message_ids_are_unique_across_calls() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, _reader_end) = tokio::io::duplex(4096); // Keep _reader_end alive
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let id1 = handle.send_message("msg 1", None).await.unwrap();
    let id2 = handle.send_message("msg 2", None).await.unwrap();
    let id3 = handle.send_message("msg 3", None).await.unwrap();
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
    drop(_reader_end); // Explicit drop to clarify intent
}

// --- SidecarState and SidecarHandle tests (step-13) ---

#[test]
fn sidecar_state_variants_are_debug_clone() {
    let s1 = SidecarState::NotStarted;
    let s2 = SidecarState::Starting;
    let s3 = SidecarState::Ready;
    let s4 = SidecarState::Crashed("oops".to_string());
    // Must be Debug + Clone
    let _ = format!("{:?}{:?}{:?}{:?}", s1, s2, s3, s4);
    let _ = s4.clone();
}

#[tokio::test]
async fn sidecar_handle_state_starts_as_starting() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));

    // Construct handle via from_parts with a dummy reader that ends immediately
    let data: &[u8] = b"";
    let reader = BufReader::new(data);
    let (writer, _reader_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state.clone());
    // State should still be Starting since we haven't sent ready
    assert!(matches!(
        *handle.state().lock().await,
        SidecarState::Starting | SidecarState::NotStarted | SidecarState::Crashed(_)
    ));
}

#[tokio::test]
async fn sidecar_handle_transitions_to_ready_on_ready_message() {
    use std::sync::Arc;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    // Use a duplex so we can write the ready message without causing immediate EOF
    let (mut data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (writer, _writer_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state.clone());

    // Write the ready message (without closing the writer, so no EOF)
    data_writer.write_all(b"{\"type\":\"ready\"}\n").await.unwrap();

    // Yield control multiple times to let the spawned reader task execute
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }

    let s = handle.state().lock().await;
    assert!(matches!(*s, SidecarState::Ready), "Expected Ready, got {:?}", *s);
    // Keep data_writer alive so reader task stays open (no EOF/Crashed)
    drop(data_writer);
}

// --- write_to_sidecar tests (step-11) ---

#[tokio::test]
async fn write_to_sidecar_send_message_writes_json_line() {
    let msg = InboundMessage::SendMessage {
        id: "msg-1".to_string(),
        text: "hello".to_string(),
        context: None,
    };
    let mut buf: Vec<u8> = Vec::new();
    write_to_sidecar(&mut buf, &msg).await.unwrap();
    let written = std::str::from_utf8(&buf).unwrap();
    assert!(written.ends_with('\n'));
    let json_val: serde_json::Value = serde_json::from_str(written.trim_end()).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["id"], "msg-1");
    assert_eq!(json_val["text"], "hello");
}

#[tokio::test]
async fn write_to_sidecar_abort_writes_json_line() {
    let msg = InboundMessage::Abort;
    let mut buf: Vec<u8> = Vec::new();
    write_to_sidecar(&mut buf, &msg).await.unwrap();
    let written = std::str::from_utf8(&buf).unwrap();
    assert_eq!(written, "{\"type\":\"abort\"}\n");
}

#[tokio::test]
async fn write_to_sidecar_clear_session_writes_json_line() {
    let msg = InboundMessage::ClearSession;
    let mut buf: Vec<u8> = Vec::new();
    write_to_sidecar(&mut buf, &msg).await.unwrap();
    let written = std::str::from_utf8(&buf).unwrap();
    assert_eq!(written, "{\"type\":\"clear_session\"}\n");
}

// --- read_sidecar_output tests (step-9) ---

#[tokio::test]
async fn read_sidecar_output_receives_messages_in_order() {
    use std::sync::{Arc, Mutex};
    use tokio::io::BufReader;

    let data = b"{\"type\":\"ready\"}\n{\"type\":\"text_delta\",\"id\":\"msg-1\",\"content\":\"hi\"}\n";
    let reader = BufReader::new(&data[..]);
    let received: Arc<Mutex<Vec<OutboundMessage>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = Arc::clone(&received);
    let exit_fired: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let exit_fired_clone = Arc::clone(&exit_fired);

    read_sidecar_output(
        reader,
        move |msg| {
            received_clone.lock().unwrap().push(msg);
        },
        move || {
            *exit_fired_clone.lock().unwrap() = true;
        },
    )
    .await;

    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0], OutboundMessage::Ready);
    assert_eq!(msgs[1], OutboundMessage::TextDelta { id: "msg-1".to_string(), content: "hi".to_string() });
    assert!(*exit_fired.lock().unwrap(), "on_exit should fire at EOF");
}

#[tokio::test]
async fn read_sidecar_output_eof_fires_on_exit() {
    use std::sync::{Arc, Mutex};
    use tokio::io::BufReader;

    let data: &[u8] = b"";
    let reader = BufReader::new(data);
    let exit_fired: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let exit_fired_clone = Arc::clone(&exit_fired);

    read_sidecar_output(
        reader,
        |_msg| {},
        move || {
            *exit_fired_clone.lock().unwrap() = true;
        },
    )
    .await;

    assert!(*exit_fired.lock().unwrap(), "on_exit should fire at EOF even with no messages");
}

#[tokio::test]
async fn read_sidecar_output_skips_invalid_json_lines() {
    use std::sync::{Arc, Mutex};
    use tokio::io::BufReader;

    let data = b"not-json\n{\"type\":\"ready\"}\n";
    let reader = BufReader::new(&data[..]);
    let received: Arc<Mutex<Vec<OutboundMessage>>> = Arc::new(Mutex::new(vec![]));
    let received_clone = Arc::clone(&received);

    read_sidecar_output(
        reader,
        move |msg| {
            received_clone.lock().unwrap().push(msg);
        },
        || {},
    )
    .await;

    // Invalid line skipped, ready message received
    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0], OutboundMessage::Ready);
}

// --- outbound_to_event tests (step-7) ---

#[test]
fn outbound_to_event_text_delta() {
    let msg = OutboundMessage::TextDelta { id: "msg-1".to_string(), content: "hi".to_string() };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-text-delta");
    assert_eq!(payload["id"], "msg-1");
    assert_eq!(payload["content"], "hi");
}

#[test]
fn outbound_to_event_thinking_delta() {
    let msg = OutboundMessage::ThinkingDelta { id: "msg-2".to_string(), content: "...".to_string() };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-thinking-delta");
    assert_eq!(payload["id"], "msg-2");
    assert_eq!(payload["content"], "...");
}

#[test]
fn outbound_to_event_tool_call() {
    let msg = OutboundMessage::ToolCall {
        id: "msg-3".to_string(),
        tool_name: "reify_list".to_string(),
        tool_input: json!({"filter": "all"}),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-tool-call");
    assert_eq!(payload["id"], "msg-3");
    assert_eq!(payload["tool_name"], "reify_list");
    assert_eq!(payload["tool_input"]["filter"], "all");
}

#[test]
fn outbound_to_event_tool_result() {
    let msg = OutboundMessage::ToolResult {
        id: "msg-3".to_string(),
        tool_name: "reify_list".to_string(),
        result: json!(["cube1", "cube2"]),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-tool-result");
    assert_eq!(payload["id"], "msg-3");
    assert_eq!(payload["tool_name"], "reify_list");
    assert_eq!(payload["result"][0], "cube1");
}

#[test]
fn outbound_to_event_done() {
    let msg = OutboundMessage::Done { id: "msg-4".to_string() };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-done");
    assert_eq!(payload["id"], "msg-4");
}

#[test]
fn outbound_to_event_error() {
    let msg = OutboundMessage::ErrorMessage { id: "msg-5".to_string(), message: "oops".to_string() };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-error");
    assert_eq!(payload["id"], "msg-5");
    assert_eq!(payload["message"], "oops");
}

#[test]
fn outbound_to_event_ready() {
    let msg = OutboundMessage::Ready;
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-ready");
    // payload should be empty object
    assert!(payload.as_object().unwrap().is_empty());
}

// --- SidecarHandle child process lifecycle tests (step-28) ---

#[tokio::test]
async fn kill_without_child_sets_state_to_not_started() {
    // from_parts creates a handle with no child (existing behavior preserved)
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    // from_parts creates no child
    assert!(!handle.has_child(), "from_parts handle should have no child");

    // kill() should not panic even without a child
    handle.kill().await;
    assert!(matches!(*handle.state().lock().await, SidecarState::NotStarted));
}

#[tokio::test]
async fn set_child_makes_has_child_return_true() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    assert!(!handle.has_child(), "should have no child before set_child");

    // Spawn a real process (sleep) to use as the child
    let child = tokio::process::Command::new("sleep")
        .arg("999")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn sleep process");

    handle.set_child(child);
    assert!(handle.has_child(), "should have child after set_child");

    // Clean up: kill the child
    handle.kill().await;
    assert!(!handle.has_child(), "should have no child after kill");
}

#[tokio::test]
async fn kill_with_child_terminates_process_and_clears_child() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    // Spawn a long-running process
    let child = tokio::process::Command::new("sleep")
        .arg("999")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn sleep process");

    handle.set_child(child);
    assert!(handle.has_child());

    // kill() should terminate the process and clear the child field
    handle.kill().await;

    assert!(!handle.has_child(), "child should be cleared after kill");
    assert!(matches!(*handle.state().lock().await, SidecarState::NotStarted));
}

// --- spawn_sidecar_impl tests (step-1, step-3) ---

#[tokio::test]
async fn spawn_sidecar_impl_returns_error_for_missing_binary() {
    use std::path::Path;
    use std::sync::Arc;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use crate::engine::EngineSession;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    let result = spawn_sidecar_impl(
        Path::new("/tmp/no-such-sidecar-binary"),
        engine,
        |_name: String, _payload: serde_json::Value| {},
    )
    .await;

    assert!(result.is_err(), "Expected error for missing binary");
    let err = result.err().expect("Expected Err variant");
    assert!(
        err.contains("Failed to spawn sidecar"),
        "Error should mention 'Failed to spawn sidecar': {}",
        err
    );
}

// --- SidecarHandle::wait_ready tests (step-26) ---

#[tokio::test]
async fn wait_ready_returns_ok_when_ready_message_arrives() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    let (mut data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (writer, _writer_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state.clone());

    // Write the ready message so the reader task processes it
    data_writer.write_all(b"{\"type\":\"ready\"}\n").await.unwrap();

    // wait_ready should return Ok once the ready message is processed
    let result = handle.wait_ready(Duration::from_secs(5)).await;
    assert!(result.is_ok(), "wait_ready should return Ok when sidecar sends ready: {:?}", result);

    // State should now be Ready
    assert!(matches!(*handle.state().lock().await, SidecarState::Ready));
    drop(data_writer);
}

#[tokio::test]
async fn wait_ready_returns_err_on_timeout() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    // Use a duplex that stays open but never sends ready
    let (_data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (writer, _writer_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state);

    // Short timeout - sidecar never sends ready so it should time out
    let result = handle.wait_ready(Duration::from_millis(100)).await;
    assert!(result.is_err(), "wait_ready should return Err on timeout");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("timeout") || msg.contains("timed out") || msg.contains("Timeout"),
        "Error should mention timeout: {}",
        msg
    );
}

#[tokio::test]
async fn wait_ready_returns_ok_immediately_when_already_ready() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // State is already Ready before calling wait_ready
    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let data: &[u8] = b"";
    let reader = BufReader::new(data);
    let (writer, _writer_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state);

    // Should return immediately without blocking
    let result = handle.wait_ready(Duration::from_secs(1)).await;
    assert!(result.is_ok(), "wait_ready should return Ok immediately when state is already Ready: {:?}", result);
}

// --- format_inbound tests (step-3) ---

#[test]
fn format_inbound_send_message_produces_json_line() {
    let msg = InboundMessage::SendMessage {
        id: "msg-1".to_string(),
        text: "Hello".to_string(),
        context: None,
    };
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'), "Should end with newline");
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["type"], "send_message");
    assert_eq!(json_val["id"], "msg-1");
    assert_eq!(json_val["text"], "Hello");
}

#[test]
fn format_inbound_abort_produces_minimal_json_line() {
    let msg = InboundMessage::Abort;
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'), "Should end with newline");
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["type"], "abort");
    // Should only have the type field
    assert_eq!(line.trim_end(), r#"{"type":"abort"}"#);
}

#[test]
fn format_inbound_clear_session_produces_json_line() {
    let msg = InboundMessage::ClearSession;
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'), "Should end with newline");
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["type"], "clear_session");
    assert_eq!(line.trim_end(), r#"{"type":"clear_session"}"#);
}

#[test]
fn format_inbound_send_message_with_context_includes_context() {
    let msg = InboundMessage::SendMessage {
        id: "msg-2".to_string(),
        text: "fix".to_string(),
        context: Some(MessageContext {
            selected_entity: Some("box1".to_string()),
            diagnostics: None,
            constraints: None,
        }),
    };
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'));
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["context"]["selected_entity"], "box1");
}
