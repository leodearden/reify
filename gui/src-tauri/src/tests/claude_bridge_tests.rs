// Tests for the Claude Code SDK sidecar bridge.
#![allow(unused_imports)]

use crate::claude_bridge::*;
use serde_json::{Value, json};

// --- Test helpers (task-452/step-3) ---

/// Create a `SidecarHandle` in `Starting` state with a live duplex data stream.
///
/// Returns `(handle, data_writer)`.  The caller **must** hold `data_writer` alive
/// for the duration of the test — dropping it causes EOF on the reader task, which
/// transitions state to `Crashed` and invalidates the test.
fn make_starting_handle() -> (SidecarHandle, tokio::io::DuplexStream) {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    let (data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
    let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
    (handle, data_writer)
}

/// Create a `SidecarHandle` in `Ready` state with a live duplex data stream.
///
/// Returns `(handle, data_writer)`.  Caller must hold the returned `DuplexStream`
/// alive — dropping it causes EOF on the reader task which transitions state to
/// `Crashed`.
///
/// Suitable for tests that verify state without interacting with stdin or the
/// reader task.
fn make_ready_handle() -> (SidecarHandle, tokio::io::DuplexStream) {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
    let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
    (handle, data_writer)
}

/// Async body for a standard ready-signaling `spawn_fn`.
///
/// Creates a `Starting` handle, writes `{"type":"ready"}` so the reader task
/// will transition to `Ready`, and sends the `data_writer` over `writer_tx`
/// to keep it alive (preventing EOF → Crashed).
///
/// Usage:
/// ```ignore
/// let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();
/// let spawn_fn = || make_ready_spawn_fn(writer_tx);
/// let result = ensure_sidecar_ready(&sidecar, spawn_fn, timeout).await;
/// let _held = writer_rx.await.ok(); // keep data_writer alive
/// ```
async fn make_ready_spawn_fn(
    writer_tx: tokio::sync::oneshot::Sender<tokio::io::DuplexStream>,
) -> Result<SidecarHandle, String> {
    use std::sync::Arc;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    let (mut data_writer, data_reader) = tokio::io::duplex(1024);
    data_writer
        .write_all(b"{\"type\":\"ready\"}\n")
        .await
        .map_err(|e| e.to_string())?;
    let reader = BufReader::new(data_reader);
    let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
    let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
    writer_tx
        .send(data_writer)
        .expect("writer_tx receiver dropped");
    Ok(handle)
}

// --- make_ready_spawn_fn error-handling tests ---

/// Verify that `make_ready_spawn_fn` panics when the oneshot receiver is dropped
/// before the send.  This guards against silent error suppression in the helper.
#[tokio::test]
#[should_panic(expected = "writer_tx receiver dropped")]
async fn make_ready_spawn_fn_panics_when_receiver_dropped() {
    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel::<tokio::io::DuplexStream>();
    drop(writer_rx);
    let _ = make_ready_spawn_fn(writer_tx).await;
}

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
        current_file: None,
        attached_contexts: None,
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
    let json_str = r#"{"type":"tool_call","id":"msg-1","tool_name":"reify_get_shape","tool_input":{"name":"cube1"},"tool_use_id":"tu-9"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ToolCall {
            id,
            tool_name,
            tool_input,
            tool_use_id,
        } => {
            assert_eq!(id, "msg-1");
            assert_eq!(tool_name, "reify_get_shape");
            assert_eq!(tool_input["name"], "cube1");
            assert_eq!(tool_use_id, "tu-9");
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn outbound_tool_result_deserializes() {
    let json_str =
        r#"{"type":"tool_result","id":"msg-1","tool_name":"reify_get_shape","result":"ok"}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::ToolResult {
            id,
            tool_name,
            result,
        } => {
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
        current_file: None,
        attached_contexts: None,
    };
    let json_val: Value = serde_json::to_value(&ctx).unwrap();
    // None fields should be omitted (skip_serializing_if)
    assert!(json_val.get("selected_entity").is_none());
    assert!(json_val.get("diagnostics").is_none());
    assert!(json_val.get("constraints").is_none());
    assert!(json_val.get("current_file").is_none());
    assert!(json_val.get("attached_contexts").is_none());
}

#[test]
fn message_context_with_all_fields_serializes() {
    let ctx = MessageContext {
        selected_entity: Some("cube1".to_string()),
        diagnostics: Some(vec!["Error at line 1".to_string()]),
        constraints: Some(vec!["x > 0".to_string()]),
        current_file: Some("bracket.ri".to_string()),
        attached_contexts: Some(vec!["design-spec.md".to_string(), "notes.txt".to_string()]),
    };
    let json_val: Value = serde_json::to_value(&ctx).unwrap();
    assert_eq!(json_val["selected_entity"], "cube1");
    assert_eq!(json_val["diagnostics"][0], "Error at line 1");
    assert_eq!(json_val["constraints"][0], "x > 0");
    assert_eq!(json_val["current_file"], "bracket.ri");
    assert_eq!(json_val["attached_contexts"][0], "design-spec.md");
    assert_eq!(json_val["attached_contexts"][1], "notes.txt");
}

#[test]
fn ipc_types_satisfy_full_contract() {
    super::assert_ipc_contract::<InboundMessage>();
    super::assert_ipc_contract::<OutboundMessage>();
    super::assert_ipc_contract::<MessageContext>();
}

// --- parse_outbound tests (step-5) ---

#[test]
fn parse_outbound_text_delta() {
    let line = r#"{"type":"text_delta","id":"msg-1","content":"Hello"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(
        msg,
        OutboundMessage::TextDelta {
            id: "msg-1".to_string(),
            content: "Hello".to_string()
        }
    );
}

#[test]
fn parse_outbound_thinking_delta() {
    let line = r#"{"type":"thinking_delta","id":"msg-2","content":"hmm"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(
        msg,
        OutboundMessage::ThinkingDelta {
            id: "msg-2".to_string(),
            content: "hmm".to_string()
        }
    );
}

#[test]
fn parse_outbound_tool_call() {
    let line = r#"{"type":"tool_call","id":"msg-3","tool_name":"reify_get","tool_input":{"x":1},"tool_use_id":"tu-9"}"#;
    let msg = parse_outbound(line).unwrap();
    match msg {
        OutboundMessage::ToolCall {
            id,
            tool_name,
            tool_input,
            tool_use_id,
        } => {
            assert_eq!(id, "msg-3");
            assert_eq!(tool_name, "reify_get");
            assert_eq!(tool_input["x"], 1);
            assert_eq!(tool_use_id, "tu-9");
        }
        _ => panic!("Expected ToolCall"),
    }
}

#[test]
fn parse_outbound_tool_call_without_tool_use_id_defaults_to_empty() {
    // Stale-sidecar payload: ToolCall JSON missing tool_use_id.
    // After #[serde(default)] is applied, this should parse Ok with tool_use_id == "".
    let line = r#"{"type":"tool_call","id":"msg-1","tool_name":"reify_get","tool_input":{"x":1}}"#;
    let result = parse_outbound(line);
    match result {
        Ok(OutboundMessage::ToolCall {
            id,
            tool_name,
            tool_input,
            tool_use_id,
        }) => {
            assert_eq!(id, "msg-1");
            assert_eq!(tool_name, "reify_get");
            assert_eq!(tool_input["x"], 1);
            assert_eq!(
                tool_use_id, "",
                "missing tool_use_id must default to empty string"
            );
        }
        Ok(other) => panic!("Expected ToolCall, got {:?}", other),
        Err(e) => panic!(
            "Expected Ok(ToolCall with empty tool_use_id), got Err: {}",
            e
        ),
    }
}

#[test]
fn parse_outbound_tool_result() {
    let line = r#"{"type":"tool_result","id":"msg-3","tool_name":"reify_get","result":"done"}"#;
    let msg = parse_outbound(line).unwrap();
    match msg {
        OutboundMessage::ToolResult {
            id,
            tool_name,
            result,
        } => {
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
    assert_eq!(
        msg,
        OutboundMessage::Done {
            id: "msg-4".to_string()
        }
    );
}

#[test]
fn parse_outbound_error() {
    let line = r#"{"type":"error","id":"msg-5","message":"oops"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(
        msg,
        OutboundMessage::ErrorMessage {
            id: "msg-5".to_string(),
            message: "oops".to_string()
        }
    );
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

    assert!(
        result.is_err(),
        "Expected error when sidecar is not started"
    );
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

    assert!(
        result.is_ok(),
        "Expected success when sidecar is Ready: {:?}",
        result
    );
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
    assert!(
        result.is_err(),
        "Expected error when sidecar is Starting (not yet Ready)"
    );
}

#[tokio::test]
async fn from_parts_with_mcp_intercepts_reify_tool_calls() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

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
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));
    let _handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name, payload));
        },
        selection,
    );

    // Inject a reify_ tool_call from simulated sidecar stdout
    let tool_call = r#"{"type":"tool_call","id":"msg-1","tool_name":"reify_get_diagnostics","tool_input":{},"tool_use_id":"tu-diag"}"#;
    stdout_writer
        .write_all(format!("{}\n", tool_call).as_bytes())
        .await
        .unwrap();

    // Allow reader task to process the tool_call and run the spawned MCP handler
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }

    // Verify the tool_call event was emitted to the event sink
    {
        let emitted = events.lock().unwrap();
        assert!(
            emitted.iter().any(|(name, _)| name == "claude-tool-call"),
            "Expected claude-tool-call event in sink, got: {:?}",
            emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }

    // Verify tool_result was written back to sidecar stdin
    let mut buf = vec![0u8; 4096];
    let n = stdin_reader.read(&mut buf).await.unwrap_or(0);
    assert!(
        n > 0,
        "Expected tool_result to be written back to sidecar stdin"
    );
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
    assert_eq!(
        json_val["tool_use_id"], "tu-diag",
        "tool_use_id must be echoed from the tool_call outbound"
    );

    drop(stdout_writer);
}

#[tokio::test]
async fn from_parts_with_mcp_threads_selection_into_tool_result() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    let (stdin_writer, mut stdin_reader) = tokio::io::duplex(4096);
    let (mut stdout_writer, stdout_reader) = tokio::io::duplex(4096);
    let reader = BufReader::new(stdout_reader);
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));

    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);

    // Pre-populate selection with concrete values
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: Some("Bracket.width".to_string()),
    }));
    let _handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name, payload));
        },
        selection,
    );

    // Inject a reify_get_selection tool_call
    let tool_call = r#"{"type":"tool_call","id":"msg-sel","tool_name":"reify_get_selection","tool_input":{},"tool_use_id":"tu-sel"}"#;
    stdout_writer
        .write_all(format!("{}\n", tool_call).as_bytes())
        .await
        .unwrap();

    for _ in 0..100 {
        tokio::task::yield_now().await;
    }

    // Verify tool_result contains the pre-populated selection data
    let mut buf = vec![0u8; 4096];
    let n = stdin_reader.read(&mut buf).await.unwrap_or(0);
    assert!(
        n > 0,
        "Expected tool_result to be written back to sidecar stdin"
    );
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    let json_val: serde_json::Value =
        serde_json::from_str(written.trim()).unwrap_or(serde_json::json!(null));
    assert_eq!(
        json_val["type"], "tool_result",
        "Expected tool_result type, got: {}",
        written
    );
    assert_eq!(json_val["tool_name"], "reify_get_selection");
    assert_eq!(
        json_val["tool_use_id"], "tu-sel",
        "tool_use_id must be echoed from the tool_call outbound"
    );

    let selection_result = &json_val["result"];
    assert_eq!(
        selection_result["selected_entity"], "Bracket",
        "Selection should contain pre-populated selected_entity, got: {}",
        json_val
    );
    assert_eq!(
        selection_result["hovered_entity"], "Bracket.width",
        "Selection should contain pre-populated hovered_entity, got: {}",
        json_val
    );

    drop(stdout_writer);
}

#[tokio::test]
async fn from_parts_with_mcp_wires_event_emitter_into_tool_context() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::sync::Arc;
    use tokio::io::{AsyncWriteExt, BufReader};

    // Set up engine for MCP dispatch
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    // stdin_writer: Rust writes tool_result here (we don't read it in this test)
    // stdout_writer: simulates sidecar writing tool_call → Rust reader task processes it
    let (stdin_writer, _stdin_reader) = tokio::io::duplex(4096);
    let (mut stdout_writer, stdout_reader) = tokio::io::duplex(4096);
    let reader = BufReader::new(stdout_reader);
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));

    // Collect emitted events so we can assert on navigation events.
    // A Notify signals deterministically when focus-entity arrives, avoiding
    // wall-clock polling that can be flaky under CI load.
    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_clone = Arc::clone(&notify);
    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));
    let _handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name.clone(), payload));
            if name == "focus-entity" {
                notify_clone.notify_one();
            }
        },
        selection,
    );

    // Register the waiter BEFORE writing the tool call to avoid a race where
    // notify_one() fires before notified() starts listening.
    let notified = notify.notified();

    // Inject a reify_focus_entity tool_call from simulated sidecar stdout
    let tool_call = r#"{"type":"tool_call","id":"msg-focus","tool_name":"reify_focus_entity","tool_input":{"entity_path":"Bracket"},"tool_use_id":"tu-focus"}"#;
    stdout_writer
        .write_all(format!("{}\n", tool_call).as_bytes())
        .await
        .unwrap();

    // Wait deterministically for focus-entity to arrive (5 s is generous for CI).
    tokio::time::timeout(std::time::Duration::from_secs(5), notified)
        .await
        .expect("timed out waiting for focus-entity event from MCP tool");

    // Verify the outbound claude-tool-call event was emitted (sanity: proves reader processed it)
    {
        let emitted = events.lock().unwrap();
        assert!(
            emitted.iter().any(|(name, _)| name == "claude-tool-call"),
            "Expected claude-tool-call event in sink, got: {:?}",
            emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }

    // The regression guard: focus-entity event must reach the events sink via the wired emitter.
    // This fails when TauriToolContext is built without .with_event_emitter(...).
    {
        let emitted = events.lock().unwrap();
        assert!(
            emitted.iter().any(|(name, payload)| name == "focus-entity"
                && payload == &serde_json::json!("Bracket")),
            "Expected focus-entity event with payload \"Bracket\" in sink, got: {:?}",
            emitted
                .iter()
                .map(|(n, p)| format!("({n}, {p})"))
                .collect::<Vec<_>>()
        );
    }

    drop(stdout_writer);
}

#[tokio::test]
async fn tool_result_write_failure_emits_claude_error_event() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::sync::Arc;
    use tokio::io::{AsyncWriteExt, BufReader};

    // Set up engine for MCP dispatch
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    // stdin_writer: Rust writes tool_result here → sidecar would read it.
    // Drop stdin_reader immediately so any write to stdin_writer will fail (broken pipe).
    let (stdin_writer, stdin_reader) = tokio::io::duplex(4096);
    drop(stdin_reader);

    // stdout_writer: simulates sidecar writing tool_call → Rust reader task processes it.
    let (mut stdout_writer, stdout_reader) = tokio::io::duplex(4096);
    let reader = BufReader::new(stdout_reader);
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));

    let events = Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));

    let _handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name, payload));
        },
        selection,
    );

    // Inject a reify_ tool_call from simulated sidecar stdout.
    // The MCP handler will try to write the tool_result back to stdin_writer,
    // which will fail because stdin_reader was dropped.
    let tool_call = r#"{"type":"tool_call","id":"msg-fail","tool_name":"reify_get_diagnostics","tool_input":{},"tool_use_id":"tu-fail"}"#;
    stdout_writer
        .write_all(format!("{}\n", tool_call).as_bytes())
        .await
        .unwrap();

    // Allow reader task + spawned MCP handler to execute and observe the write failure.
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }

    // Verify the event sink contains a claude-error event with a relevant message.
    let emitted = events.lock().unwrap();
    let error_event = emitted.iter().find(|(name, _)| name == "claude-error");
    assert!(
        error_event.is_some(),
        "Expected claude-error event in sink after write failure, got: {:?}",
        emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );
    let (_, payload) = error_event.unwrap();
    let msg = payload["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("failed to send tool result to sidecar"),
        "Expected error message to contain 'failed to send tool result to sidecar', got: {:?}",
        msg
    );
    assert_eq!(
        payload["id"].as_str().unwrap(),
        "msg-fail",
        "error event should carry the original tool_call id"
    );

    drop(stdout_writer);
}

// --- AppState sidecar field tests (step-21) ---

#[test]
fn app_state_has_sidecar_field() {
    use crate::commands::AppState;
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_mcp::SelectionInfo;
    use reify_test_support::MockGeometryKernel;
    use std::sync::{Arc, Mutex, RwLock};

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));

    // AppState should be constructible with the new sidecar field
    let _state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(SelectionInfo::default())),
        initial_file: Mutex::new(None),
        pending_solve_cancel: Mutex::new(None),
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

    assert!(matches!(
        *handle.state().lock().await,
        SidecarState::NotStarted
    ));
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

    // Poll under timeout: the spawned reader task must notice EOF and set Crashed.
    // A fixed yield count is flaky on loaded CI runners — same race as the wiring
    // test (`from_parts_with_mcp_emits_sidecar_crashed_on_eof`) below.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if matches!(*handle.state().lock().await, SidecarState::Crashed(_)) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Timed out waiting for SidecarState::Crashed after EOF");
}

// Wiring test: from_parts_with_mcp → EOF → on_sidecar_exit → emitter
//
// The three on_sidecar_exit_* unit tests verify the helper in isolation.
// This test pins the production wiring so that a future refactor that forgets
// to clone one of the three Arcs (state_for_crash, notify_for_crash,
// event_emitter_for_exit) into the on_exit closure would be caught here.
#[tokio::test]
async fn from_parts_with_mcp_emits_sidecar_crashed_on_eof() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::sync::Arc;
    use tokio::io::BufReader;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));

    let events: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>> =
        Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);

    let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
    let (data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));

    let handle = SidecarHandle::from_parts_with_mcp(
        stdin_writer,
        reader,
        state,
        engine,
        move |name: String, payload: serde_json::Value| {
            events_clone.lock().unwrap().push((name, payload));
        },
        selection,
    );

    // Drop data_writer to simulate sidecar crash (EOF on reader).
    drop(data_writer);

    // Poll under timeout: the spawned on_exit task must acquire the state mutex
    // and emit the event; a fixed yield count is flaky on loaded CI runners.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if matches!(*handle.state().lock().await, SidecarState::Crashed(_)) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Timed out waiting for SidecarState::Crashed after EOF");

    let emitted = events.lock().unwrap();
    let crashed = emitted
        .iter()
        .find(|(name, _)| name == "claude-sidecar-crashed")
        .map(|(_, p)| p)
        .expect("Expected claude-sidecar-crashed event in sink");
    assert!(
        crashed["reason"].is_string() && !crashed["reason"].as_str().unwrap().is_empty(),
        "Expected non-empty 'reason' in claude-sidecar-crashed payload, got: {crashed:?}"
    );
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
async fn sidecar_handle_abort_returns_ok_when_stdin_pipe_is_broken() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // State=Ready so the state pre-check does NOT short-circuit;
    // this test must exercise the BrokenPipe path specifically.
    let state = Arc::new(Mutex::new(SidecarState::Ready));
    // Drop the read half immediately so any write to writer returns BrokenPipe.
    let (writer, stdin_reader) = tokio::io::duplex(1024);
    drop(stdin_reader);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let result = handle.abort().await;
    assert!(
        result.is_ok(),
        "expected abort() against a closed stdin to be a no-op success, got: {:?}",
        result
    );
}

#[tokio::test]
async fn sidecar_handle_abort_short_circuits_when_state_is_crashed() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio::sync::Mutex;

    // State=Crashed so the upcoming state pre-check should short-circuit before writing.
    let state = Arc::new(Mutex::new(SidecarState::Crashed("simulated".to_string())));
    // Keep the read half alive so writes would succeed if attempted.
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let result = handle.abort().await;
    assert!(
        result.is_ok(),
        "expected abort() in Crashed state to return Ok, got: {:?}",
        result
    );

    // Assert no bytes were written to stdin — if the short-circuit fires, the pipe
    // should be silent. A read with a short timeout should time out (not return bytes).
    let mut buf = vec![0u8; 64];
    let read_result =
        tokio::time::timeout(Duration::from_millis(50), reader_end.read(&mut buf)).await;
    assert!(
        read_result.is_err(),
        "expected no write to stdin when state is Crashed, but read bytes: {:?}",
        read_result.map(|r| r.map(|n| std::str::from_utf8(&buf[..n])
            .unwrap_or("<invalid utf8>")
            .to_string()))
    );
}

#[tokio::test]
async fn sidecar_handle_abort_short_circuits_when_state_is_not_started() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio::sync::Mutex;

    // State=NotStarted so the state pre-check should short-circuit before writing.
    let state = Arc::new(Mutex::new(SidecarState::NotStarted));
    // Keep the read half alive so writes would succeed if attempted.
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let mut handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let result = handle.abort().await;
    assert!(
        result.is_ok(),
        "expected abort() in NotStarted state to return Ok, got: {:?}",
        result
    );

    // Assert no bytes were written to stdin — if the short-circuit fires, the pipe
    // should be silent. A read with a short timeout should time out (not return bytes).
    let mut buf = vec![0u8; 64];
    let read_result =
        tokio::time::timeout(Duration::from_millis(50), reader_end.read(&mut buf)).await;
    assert!(
        read_result.is_err(),
        "expected no write to stdin when state is NotStarted, but read bytes: {:?}",
        read_result.map(|r| r.map(|n| std::str::from_utf8(&buf[..n])
            .unwrap_or("<invalid utf8>")
            .to_string()))
    );
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
    data_writer
        .write_all(b"{\"type\":\"ready\"}\n")
        .await
        .unwrap();

    // Yield control multiple times to let the spawned reader task execute
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }

    let s = handle.state().lock().await;
    assert!(
        matches!(*s, SidecarState::Ready),
        "Expected Ready, got {:?}",
        *s
    );
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

    let data =
        b"{\"type\":\"ready\"}\n{\"type\":\"text_delta\",\"id\":\"msg-1\",\"content\":\"hi\"}\n";
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
    assert_eq!(
        msgs[1],
        OutboundMessage::TextDelta {
            id: "msg-1".to_string(),
            content: "hi".to_string()
        }
    );
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

    assert!(
        *exit_fired.lock().unwrap(),
        "on_exit should fire at EOF even with no messages"
    );
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

// --- read_sidecar_output warn tests (task-2819) ---

/// A stale sidecar that omits `tool_use_id` from a ToolCall event must still
/// deliver the message to `on_message` (not silently drop it), AND must emit
/// exactly one WARN so operators know to rebuild the sidecar.
#[tokio::test(flavor = "current_thread")]
async fn read_sidecar_output_warns_when_tool_call_missing_tool_use_id() {
    use std::sync::{Arc, Mutex};
    use tokio::io::BufReader;

    let (_guard, warn_counter) = reify_test_support::warn_counting_guard();

    // Stale-sidecar payload: ToolCall without tool_use_id.
    let data = b"{\"type\":\"tool_call\",\"id\":\"msg-1\",\"tool_name\":\"reify_get\",\"tool_input\":{}}\n";
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

    // (a) Message was delivered (not silently dropped).
    let msgs = received.lock().unwrap();
    assert_eq!(
        msgs.len(),
        1,
        "stale ToolCall must be delivered to on_message"
    );
    assert!(
        matches!(
            &msgs[0],
            OutboundMessage::ToolCall { tool_use_id, .. } if tool_use_id.is_empty()
        ),
        "expected ToolCall with empty tool_use_id, got {:?}",
        msgs[0]
    );

    // (b) Exactly one WARN must be emitted for the version-skew.
    reify_test_support::assert_warn_count(
        &warn_counter,
        1,
        "missing tool_use_id must emit exactly one WARN",
    );
}

/// Parse failures (e.g. invalid JSON) must emit a WARN and NOT deliver a
/// message to `on_message`.
#[tokio::test(flavor = "current_thread")]
async fn read_sidecar_output_warns_on_parse_failure() {
    use std::sync::{Arc, Mutex};
    use tokio::io::BufReader;

    let (_guard, warn_counter) = reify_test_support::warn_counting_guard();

    // Structurally-invalid OutboundMessage (unknown discriminant) — distinct from
    // `read_sidecar_output_skips_invalid_json_lines`, which feeds raw `not-json`.
    // Both shapes route through the same `parse_outbound -> Err` branch, so the
    // WARN behavior we assert below is the same; the goal here is to widen
    // invalid-input coverage.
    let data = b"{\"type\":\"unknown_kind\"}\n";
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

    // (a) No message delivered (existing drop-on-parse-failure behavior preserved).
    let msgs = received.lock().unwrap();
    assert_eq!(
        msgs.len(),
        0,
        "invalid JSON must not be delivered to on_message"
    );

    // (b) Exactly one WARN must be emitted so operators can diagnose the failure.
    reify_test_support::assert_warn_count(
        &warn_counter,
        1,
        "parse failure must emit exactly one WARN",
    );
}

/// `warn_if_stale_tool_call` must emit exactly one WARN for a stale ToolCall
/// (empty tool_use_id), and zero WARNs for a valid ToolCall or a non-ToolCall
/// variant.  Three branches tested in a single subscriber session so the
/// assert is on the cumulative count.
#[test]
fn warn_if_stale_tool_call_emits_one_warn_only_for_stale_tool_call() {
    let (_guard, counter) = reify_test_support::warn_counting_guard();

    // (i) Stale ToolCall — empty tool_use_id → must bump the counter by 1.
    let stale = OutboundMessage::ToolCall {
        id: "msg-1".to_string(),
        tool_name: "reify_get".to_string(),
        tool_input: json!({}),
        tool_use_id: String::new(),
    };
    warn_if_stale_tool_call(&stale);

    // (ii) Valid ToolCall — non-empty tool_use_id → counter unchanged.
    let valid = OutboundMessage::ToolCall {
        id: "msg-2".to_string(),
        tool_name: "reify_get".to_string(),
        tool_input: json!({}),
        tool_use_id: "tu-1".to_string(),
    };
    warn_if_stale_tool_call(&valid);

    // (iii) Non-ToolCall variant → counter unchanged.
    warn_if_stale_tool_call(&OutboundMessage::Ready);

    reify_test_support::assert_warn_count(&counter, 1, "only the stale ToolCall must warn");
}

// --- outbound_to_event tests (step-7) ---

#[test]
fn outbound_to_event_text_delta() {
    let msg = OutboundMessage::TextDelta {
        id: "msg-1".to_string(),
        content: "hi".to_string(),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-text-delta");
    assert_eq!(payload["id"], "msg-1");
    assert_eq!(payload["content"], "hi");
}

#[test]
fn outbound_to_event_thinking_delta() {
    let msg = OutboundMessage::ThinkingDelta {
        id: "msg-2".to_string(),
        content: "...".to_string(),
    };
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
        tool_use_id: "tu-payload".to_string(),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-tool-call");
    assert_eq!(payload["id"], "msg-3");
    assert_eq!(payload["tool_name"], "reify_list");
    assert_eq!(payload["tool_input"]["filter"], "all");
    assert_eq!(payload["tool_use_id"], "tu-payload");
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
    let msg = OutboundMessage::Done {
        id: "msg-4".to_string(),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-done");
    assert_eq!(payload["id"], "msg-4");
}

#[test]
fn outbound_to_event_error() {
    let msg = OutboundMessage::ErrorMessage {
        id: "msg-5".to_string(),
        message: "oops".to_string(),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-error");
    assert_eq!(payload["id"], "msg-5");
    assert_eq!(payload["message"], "oops");
}

#[test]
fn outbound_to_event_notice() {
    let msg = OutboundMessage::Notice {
        id: "msg-6".to_string(),
        code: "degraded_turn_boundary".to_string(),
        message: "assistant event missing message.id; turn-boundary detection disabled".to_string(),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-notice");
    assert_eq!(payload["id"], "msg-6");
    assert_eq!(payload["code"], "degraded_turn_boundary");
    assert_eq!(
        payload["message"],
        "assistant event missing message.id; turn-boundary detection disabled"
    );
}

#[test]
fn parse_outbound_notice() {
    let line = r#"{"type":"notice","id":"msg-n1","code":"degraded_turn_boundary","message":"assistant event missing message.id"}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(
        msg,
        OutboundMessage::Notice {
            id: "msg-n1".to_string(),
            code: "degraded_turn_boundary".to_string(),
            message: "assistant event missing message.id".to_string(),
        }
    );
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
    assert!(
        !handle.has_child(),
        "from_parts handle should have no child"
    );

    // kill() should not panic even without a child
    handle.kill().await;
    assert!(matches!(
        *handle.state().lock().await,
        SidecarState::NotStarted
    ));
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
    assert!(matches!(
        *handle.state().lock().await,
        SidecarState::NotStarted
    ));
}

// --- shutdown_sidecar tests (step-5) ---

#[tokio::test]
async fn shutdown_sidecar_kills_and_clears_handle() {
    use std::sync::Arc;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // Build a live handle in Ready state using from_parts (duplex I/O)
    let state = Arc::new(Mutex::new(SidecarState::Ready));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);

    let sidecar: tokio::sync::Mutex<Option<SidecarHandle>> = Mutex::new(Some(handle));

    // Before: slot is Some
    assert!(
        sidecar.lock().await.is_some(),
        "Expected Some before shutdown"
    );

    shutdown_sidecar(&sidecar).await;

    // After: slot should be None
    assert!(
        sidecar.lock().await.is_none(),
        "Expected None after shutdown_sidecar"
    );
}

// --- spawn_sidecar_impl tests (step-1, step-3) ---

#[tokio::test]
async fn spawn_sidecar_impl_returns_error_for_missing_binary() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::path::Path;
    use std::sync::Arc;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));
    let result = spawn_sidecar_impl(
        Path::new("/tmp/no-such-sidecar-binary"),
        engine,
        |_name: String, _payload: serde_json::Value| {},
        selection,
        Path::new("/tmp/test-ws"),
        None,
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

#[tokio::test]
async fn spawn_sidecar_impl_returns_handle_for_valid_binary() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::path::Path;
    use std::sync::Arc;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));

    // /bin/cat keeps stdin open and produces no unexpected stdout — ideal minimal live process
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));
    let result = spawn_sidecar_impl(
        Path::new("/bin/cat"),
        engine,
        |_name: String, _payload: serde_json::Value| {},
        selection,
        Path::new("/tmp/test-ws"),
        None,
    )
    .await;

    assert!(result.is_ok(), "Expected Ok for /bin/cat binary");
    let mut handle = result.expect("Expected handle");
    assert!(
        handle.has_child(),
        "Handle should have a child process after spawn"
    );

    // Clean up: kill the spawned cat process
    handle.kill().await;
    assert!(!handle.has_child(), "Child should be gone after kill");
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
    data_writer
        .write_all(b"{\"type\":\"ready\"}\n")
        .await
        .unwrap();

    // wait_ready should return Ok once the ready message is processed
    let result = handle.wait_ready(Duration::from_secs(5)).await;
    assert!(
        result.is_ok(),
        "wait_ready should return Ok when sidecar sends ready: {:?}",
        result
    );

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
    assert!(
        result.is_ok(),
        "wait_ready should return Ok immediately when state is already Ready: {:?}",
        result
    );
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
            current_file: None,
            attached_contexts: None,
        }),
    };
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'));
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["context"]["selected_entity"], "box1");
}

#[test]
fn format_inbound_tool_result_includes_tool_use_id() {
    let msg = InboundMessage::ToolResult {
        id: "msg-tr1".to_string(),
        tool_name: "reify_get_diagnostics".to_string(),
        result: serde_json::json!([]),
        tool_use_id: "tu-echo-1".to_string(),
    };
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'), "Should end with newline");
    let json_val: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["type"], "tool_result");
    assert_eq!(json_val["id"], "msg-tr1");
    assert_eq!(json_val["tool_name"], "reify_get_diagnostics");
    assert_eq!(
        json_val["tool_use_id"], "tu-echo-1",
        "tool_use_id must be echoed so the sidecar can use id-based correlation"
    );
}

// --- shutdown_sidecar edge-case tests (task-353/step-1) ---

#[tokio::test]
async fn shutdown_sidecar_noop_on_empty_slot() {
    use tokio::sync::Mutex;

    // Slot is already None — the `if let Some` guard handles this gracefully.
    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);

    // Should not panic or error.
    shutdown_sidecar(&sidecar).await;

    // Slot must remain None.
    assert!(
        sidecar.lock().await.is_none(),
        "Expected None after shutdown_sidecar on empty slot"
    );
}

// --- wait_ready crash-during-wait test (task-353/step-3) ---

#[tokio::test]
async fn wait_ready_returns_err_on_crash_during_wait() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let state = Arc::new(Mutex::new(SidecarState::Starting));
    // Use duplex instead of b"" to prevent the reader task from seeing
    // immediate EOF and firing on_exit with "sidecar exited unexpectedly".
    // Keeping _data_writer alive means the reader blocks on read, not EOF.
    let (_data_writer, data_reader) = tokio::io::duplex(1024);
    let reader = BufReader::new(data_reader);
    let (writer, _writer_end) = tokio::io::duplex(1024);

    let handle = SidecarHandle::from_parts(writer, reader, state.clone());

    // Clone notify and state so a spawned task can trigger a crash.
    let notify = Arc::clone(handle.ready_notify());
    let state_for_crash = Arc::clone(handle.state());

    // Spawn a task that simulates a crash after wait_ready has subscribed.
    // In #[tokio::test] (single-threaded current_thread runtime), yield_now
    // enqueues this task at the back of the run queue — a single deterministic
    // scheduling order where wait_ready reaches its notified.await first.
    tokio::spawn(async move {
        // Yield enough times for wait_ready to reach its notified.await.
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        *state_for_crash.lock().await = SidecarState::Crashed("test crash".to_string());
        notify.notify_waiters();
    });

    let result = handle.wait_ready(Duration::from_secs(5)).await;
    assert!(
        result.is_err(),
        "wait_ready should return Err when sidecar crashes during wait"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("test crash"),
        "Error should contain the exact crash reason from the test task, \
         not the reader's on_exit handler: {}",
        msg
    );
}

// --- ensure_sidecar_ready tests (task-353/steps-5,7,9,11) ---

#[tokio::test]
async fn ensure_sidecar_ready_spawns_and_waits_when_none() {
    use std::time::Duration;
    use tokio::sync::Mutex;

    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();
    let spawn_fn = || make_ready_spawn_fn(writer_tx);

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;
    let _held = writer_rx.await.ok();

    assert!(
        result.is_ok(),
        "Expected Ok from ensure_sidecar_ready: {:?}",
        result
    );
    assert!(
        sidecar.lock().await.is_some(),
        "Expected sidecar slot to contain Some(handle) after ensure_sidecar_ready"
    );
}

#[tokio::test]
async fn ensure_sidecar_ready_skips_spawn_when_already_some() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Pre-populate the sidecar slot with a Ready handle.
    // _data_writer must stay alive to prevent EOF → Crashed race.
    let (handle, _data_writer) = make_ready_handle();
    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(Some(handle));

    // Track whether spawn_fn was invoked at all.
    let spawn_called = Arc::new(AtomicBool::new(false));
    let spawn_called_clone = Arc::clone(&spawn_called);

    let spawn_fn = move || {
        // This code runs synchronously when the closure is called.
        spawn_called_clone.store(true, Ordering::SeqCst);
        async { Err::<SidecarHandle, String>("should not be called".to_string()) }
    };

    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;

    assert!(
        result.is_ok(),
        "Expected Ok when sidecar is already Some: {:?}",
        result
    );
    assert!(
        !spawn_called.load(Ordering::SeqCst),
        "spawn_fn should NOT be called when sidecar slot is already Some"
    );
}

#[tokio::test]
async fn ensure_sidecar_ready_propagates_spawn_error() {
    use std::time::Duration;
    use tokio::sync::Mutex;

    let spawn_fn = || async { Err::<SidecarHandle, String>("spawn failed".to_string()) };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;

    assert!(result.is_err(), "Expected Err when spawn_fn returns error");
    assert_eq!(result.unwrap_err(), "spawn failed");
    assert!(
        sidecar.lock().await.is_none(),
        "Sidecar slot should remain None after spawn error"
    );
}

#[tokio::test]
async fn ensure_sidecar_ready_timeout_when_no_ready_signal() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // Hold data_writer alive so the duplex stays open (no EOF → no crash detection).
    let held_writer: Arc<Mutex<Option<tokio::io::DuplexStream>>> = Arc::new(Mutex::new(None));
    let held_clone = Arc::clone(&held_writer);

    let spawn_fn = move || {
        let held = Arc::clone(&held_clone);
        async move {
            let state = Arc::new(Mutex::new(SidecarState::Starting));
            // Open duplex but never write the "ready" message.
            let (data_writer, data_reader) = tokio::io::duplex(1024);
            let reader = BufReader::new(data_reader);
            let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
            let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
            // Keep data_writer alive so reader sees no EOF (stays in Starting state).
            *held.lock().await = Some(data_writer);
            Ok::<SidecarHandle, String>(handle)
        }
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    // Use a short timeout so the test finishes quickly.
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(100)).await;

    assert!(result.is_err(), "Expected Err on timeout");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("timeout") || msg.contains("Timeout") || msg.contains("ready"),
        "Error should mention timeout: {}",
        msg
    );
}

// --- post-review fix tests (task-353/steps-15..24) ---

/// After a timeout error the sidecar slot must be cleared so the next call can
/// re-enter the spawn path.  Without cleanup a timed-out handle persists in the
/// slot and every subsequent call returns Ok(()) without re-spawning.
#[tokio::test]
async fn ensure_sidecar_ready_clears_slot_on_timeout() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // Hold the writer alive so the duplex stays open (no EOF → no crash).
    let held_writer: Arc<Mutex<Option<tokio::io::DuplexStream>>> = Arc::new(Mutex::new(None));
    let held_clone = Arc::clone(&held_writer);

    let spawn_fn = move || {
        let held = Arc::clone(&held_clone);
        async move {
            let state = Arc::new(Mutex::new(SidecarState::Starting));
            // Open duplex but never write the ready message.
            let (data_writer, data_reader) = tokio::io::duplex(1024);
            let reader = BufReader::new(data_reader);
            let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
            let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
            *held.lock().await = Some(data_writer);
            Ok::<SidecarHandle, String>(handle)
        }
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(100)).await;

    assert!(result.is_err(), "Expected timeout error");

    // *** Key assertion: slot must be None so the next call can re-spawn. ***
    assert!(
        sidecar.lock().await.is_none(),
        "Sidecar slot must be cleared after timeout so recovery is possible"
    );
}

/// After a crash error the sidecar slot must be cleared so the next call can
/// re-enter the spawn path.
#[tokio::test]
async fn ensure_sidecar_ready_clears_slot_on_crash() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let spawn_fn = || async move {
        let state = Arc::new(Mutex::new(SidecarState::Starting));
        // Immediately drop data_writer → EOF → reader task sets Crashed.
        let (data_writer, data_reader) = tokio::io::duplex(1024);
        drop(data_writer);
        let reader = BufReader::new(data_reader);
        let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
        let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
        Ok::<SidecarHandle, String>(handle)
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;

    assert!(result.is_err(), "Expected crash error: {:?}", result);
    let msg = result.unwrap_err();
    assert!(
        msg.contains("crashed") || msg.contains("not ready"),
        "Error should mention crash or not-ready: {}",
        msg
    );

    // *** Key assertion: slot must be None so the next call can re-spawn. ***
    assert!(
        sidecar.lock().await.is_none(),
        "Sidecar slot must be cleared after crash so recovery is possible"
    );
}

/// A handle in `SidecarState::Crashed` in the fast path should be cleared and
/// re-spawned on the next call.
#[tokio::test]
async fn ensure_sidecar_ready_rejects_crashed_existing_handle() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // Pre-populate the slot with a Crashed handle.
    let crashed_state = Arc::new(Mutex::new(SidecarState::Crashed("pre-crash".to_string())));
    let data: &[u8] = b"";
    let empty_reader = BufReader::new(data);
    let (writer, _writer_end) = tokio::io::duplex(1024);
    let crashed_handle = SidecarHandle::from_parts(writer, empty_reader, crashed_state);
    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(Some(crashed_handle));

    // Working spawn_fn that produces a Ready handle.
    let spawn_called = Arc::new(AtomicBool::new(false));
    let spawn_called_clone = Arc::clone(&spawn_called);
    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();

    let spawn_fn = move || {
        spawn_called_clone.store(true, Ordering::SeqCst);
        make_ready_spawn_fn(writer_tx)
    };

    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;
    let _held = writer_rx.await.ok();

    assert!(
        result.is_ok(),
        "Should succeed after clearing crashed handle and re-spawning: {:?}",
        result
    );
    assert!(
        spawn_called.load(Ordering::SeqCst),
        "spawn_fn must be called to replace the Crashed handle"
    );
    assert!(
        sidecar.lock().await.is_some(),
        "Slot should contain a new handle after successful re-spawn"
    );
}

/// A handle in `SidecarState::Starting` (stale from a cancelled previous attempt)
/// should be cleared and re-spawned.
#[tokio::test]
async fn ensure_sidecar_ready_respawns_starting_stale_handle() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Pre-populate with a Starting handle that will never become Ready.
    let (stale_handle, _stale_writer_keeper) = make_starting_handle();
    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(Some(stale_handle));

    let spawn_called = Arc::new(AtomicBool::new(false));
    let spawn_called_clone = Arc::clone(&spawn_called);
    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();

    let spawn_fn = move || {
        spawn_called_clone.store(true, Ordering::SeqCst);
        make_ready_spawn_fn(writer_tx)
    };

    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;
    let _held = writer_rx.await.ok();

    assert!(
        result.is_ok(),
        "Should succeed after clearing stale Starting handle: {:?}",
        result
    );
    assert!(
        spawn_called.load(Ordering::SeqCst),
        "spawn_fn must be called to replace the stale Starting handle"
    );
}

// --- ensure_sidecar_ready kill-on-eviction test (task-353/step-25) ---

/// Verify that when a stale (non-Ready) handle is evicted from the sidecar slot
/// during Phase 1, `kill()` is called on it rather than just dropping it.
///
/// `SidecarHandle::kill()` sets state to `SidecarState::NotStarted` and aborts
/// the reader task. A bare drop leaves state unchanged (no `Drop` impl). The test
/// clones the evicted handle's state Arc before storing the handle, then verifies
/// the state is `NotStarted` after `ensure_sidecar_ready` returns — proving
/// `kill()` was called, not just `drop()`.
#[tokio::test]
async fn ensure_sidecar_ready_kills_evicted_stale_handle() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    // Build a stale Crashed handle. Keep a clone of the state Arc so we can
    // verify it transitions to NotStarted after eviction.
    let old_state = Arc::new(Mutex::new(SidecarState::Crashed("pre-crash".to_string())));
    let (stale_data_writer, stale_data_reader) = tokio::io::duplex(1024);
    let stale_reader = BufReader::new(stale_data_reader);
    let (stale_stdin_writer, _stale_stdin_reader) = tokio::io::duplex(1024);
    let stale_handle =
        SidecarHandle::from_parts(stale_stdin_writer, stale_reader, Arc::clone(&old_state));
    // Keep stale_data_writer alive so the reader task does not get EOF and
    // overwrite state via on_exit.
    let _stale_writer_keeper = stale_data_writer;

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(Some(stale_handle));

    // Working spawn_fn that produces a Ready handle.
    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();
    let spawn_fn = || make_ready_spawn_fn(writer_tx);

    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;
    let _held = writer_rx.await.ok();

    assert!(
        result.is_ok(),
        "ensure_sidecar_ready should succeed after evicting stale handle: {:?}",
        result
    );

    // The evicted handle must have been killed (not just dropped).
    // kill() sets state to NotStarted; a bare drop leaves state as Crashed.
    let evicted_state = old_state.lock().await.clone();
    assert!(
        matches!(evicted_state, SidecarState::NotStarted),
        "Evicted stale handle must be killed (state = NotStarted), got: {:?}",
        evicted_state
    );
}

// --- ensure_sidecar_ready kill-on-error-cleanup tests (task-353/steps-27,28) ---

/// Verify that when `ensure_sidecar_ready` times out waiting for the ready
/// notification, `kill()` is called on the stored handle rather than just
/// clearing the slot.
///
/// `kill()` sets state to `SidecarState::NotStarted`. A bare `*guard = None`
/// leaves state as `Starting`. The test shares the state Arc with spawn_fn
/// and checks for `NotStarted` after the timeout Err is returned.
#[tokio::test]
async fn ensure_sidecar_ready_kills_handle_on_timeout_cleanup() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    // Shared state Arc — spawn_fn passes it to SidecarHandle so we can check
    // it after ensure_sidecar_ready returns.
    let shared_state = Arc::new(Mutex::new(SidecarState::Starting));
    let shared_state_clone = Arc::clone(&shared_state);

    // held_writer keeps the reader task alive (no EOF → stays in Starting state).
    let held_writer: Arc<Mutex<Option<tokio::io::DuplexStream>>> = Arc::new(Mutex::new(None));
    let held_clone = Arc::clone(&held_writer);

    let spawn_fn = move || {
        let state = Arc::clone(&shared_state_clone);
        let held = Arc::clone(&held_clone);
        async move {
            let (data_writer, data_reader) = tokio::io::duplex(1024);
            let reader = BufReader::new(data_reader);
            let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
            let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
            // Keep writer alive so no EOF → reader stays open in Starting state.
            *held.lock().await = Some(data_writer);
            Ok(handle)
        }
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(100)).await;

    assert!(result.is_err(), "Expected timeout error: {:?}", result);

    // The handle must have been killed (not just cleared from the slot).
    // kill() sets state to NotStarted; a bare clear leaves state as Starting.
    let state_after = shared_state.lock().await.clone();
    assert!(
        matches!(state_after, SidecarState::NotStarted),
        "Handle must be killed on timeout cleanup (state = NotStarted), got: {:?}",
        state_after
    );
}

/// Verify that when the sidecar crashes before becoming ready,
/// `kill()` is called on the stored handle during cleanup.
///
/// `kill()` sets state to `SidecarState::NotStarted`. A bare `*guard = None`
/// leaves state as `Crashed`. The test shares the state Arc with spawn_fn
/// and verifies `NotStarted` after the crash Err is returned.
#[tokio::test]
async fn ensure_sidecar_ready_kills_handle_on_crash_cleanup() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    let shared_state = Arc::new(Mutex::new(SidecarState::Starting));
    let shared_state_clone = Arc::clone(&shared_state);

    let spawn_fn = move || {
        let state = Arc::clone(&shared_state_clone);
        async move {
            let (data_writer, data_reader) = tokio::io::duplex(1024);
            // Drop data_writer immediately → EOF → reader task sets Crashed.
            drop(data_writer);
            let reader = BufReader::new(data_reader);
            let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
            let handle = SidecarHandle::from_parts(stdin_writer, reader, state);
            Ok::<SidecarHandle, String>(handle)
        }
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_secs(5)).await;

    assert!(result.is_err(), "Expected crash error: {:?}", result);
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("crashed") || err_msg.contains("not ready"),
        "Error should mention crash: {}",
        err_msg
    );

    // The handle must have been killed during crash cleanup.
    // kill() sets state to NotStarted; a bare clear leaves state as Crashed.
    let state_after = shared_state.lock().await.clone();
    assert!(
        matches!(state_after, SidecarState::NotStarted),
        "Handle must be killed on crash cleanup (state = NotStarted), got: {:?}",
        state_after
    );
}

// --- shutdown unblocked during spawn test (task-353/step-30) ---

/// Verify that `shutdown_sidecar` can complete while `ensure_sidecar_ready`
/// is blocked inside `spawn_fn`.
///
/// With the current code the sidecar Mutex is held for the entire duration of
/// `spawn_fn().await?`, so `shutdown_sidecar` blocks until spawn finishes.
/// After step-31 (spawn outside the lock), shutdown acquires the lock
/// immediately while `spawn_fn` is still running — no timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_not_blocked_during_ensure_sidecar_ready_spawn() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Atomic flag set by spawn_fn when it has started blocking.
    let spawn_entered = Arc::new(AtomicBool::new(false));
    let spawn_entered_clone = Arc::clone(&spawn_entered);

    // Oneshot channel: the test signals spawn_fn to unblock after shutdown.
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let sidecar: Arc<Mutex<Option<SidecarHandle>>> = Arc::new(Mutex::new(None));
    let sidecar_for_spawn = Arc::clone(&sidecar);
    let sidecar_for_shutdown = Arc::clone(&sidecar);

    let spawn_fn = move || {
        let entered = Arc::clone(&spawn_entered_clone);
        async move {
            // Signal that we have entered spawn_fn (Phase 1's lock is released).
            entered.store(true, Ordering::SeqCst);
            // Block until the test unblocks us.
            rx.await.ok();
            Err::<SidecarHandle, String>("cancelled".to_string())
        }
    };

    // Start ensure_sidecar_ready in the background.
    let ensure_handle = tokio::spawn(async move {
        ensure_sidecar_ready(&sidecar_for_spawn, spawn_fn, Duration::from_secs(5)).await
    });

    // Wait until spawn_fn has been entered (the sidecar lock has been released).
    while !spawn_entered.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }

    // shutdown_sidecar should NOT block — the lock is free because spawn_fn
    // runs outside the lock after step-31.  With the current code (lock held
    // during spawn), this times out.
    let shutdown_result = tokio::time::timeout(
        Duration::from_millis(200),
        shutdown_sidecar(&sidecar_for_shutdown),
    )
    .await;
    assert!(
        shutdown_result.is_ok(),
        "shutdown_sidecar must not block while ensure_sidecar_ready is in spawn_fn"
    );

    // Unblock spawn_fn so ensure_sidecar_ready can finish (returns Err — no slot
    // to place the handle since shutdown cleared it, but spawn_fn returned Err
    // anyway so ensure_sidecar_ready propagates Err directly).
    let _ = tx.send(());
    let _ = ensure_handle.await;
}

// --- multi-thread race regression test for wait_ready (task-363/step-1) ---

/// Regression test for the wait_ready() subscribe-before-check pattern.
///
/// On a multi-thread runtime, the reader task can process `{"type":"ready"}`
/// and call `notify_waiters()` on a different worker thread during the window
/// between `notified()` creation and the first poll of the Notified future.
/// With `enable()` called eagerly, the waiter is registered before re-check
/// and the notification is never lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_ready_notified_race_on_multithread() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    // Run 20 iterations to increase probability of hitting timing-dependent races.
    for i in 0..20 {
        let state = Arc::new(Mutex::new(SidecarState::Starting));
        let (mut data_writer, data_reader) = tokio::io::duplex(1024);

        // Write ready BEFORE creating the handle so the reader task can
        // process it immediately when scheduled on the second thread.
        data_writer
            .write_all(b"{\"type\":\"ready\"}\n")
            .await
            .unwrap();
        let reader = BufReader::new(data_reader);
        let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
        let mut handle = SidecarHandle::from_parts(stdin_writer, reader, state);

        // Hold the writer alive so the reader doesn't see EOF.
        let _data_writer = data_writer;

        let result = handle.wait_ready(Duration::from_millis(500)).await;
        assert!(
            result.is_ok(),
            "iteration {}: wait_ready should return Ok \
             (race condition causes spurious timeout without enable()): {:?}",
            i,
            result
        );

        // Abort the reader task immediately instead of leaving it detached
        // until the writer drops at end-of-iteration.
        handle.kill().await;
    }
}

// --- multi-thread race regression test (task-353/step-15) ---

/// Regression guard for the subscribe-before-check pattern in ensure_sidecar_ready().
///
/// On a multi-thread executor, the reader task can call `notify_waiters()` in
/// the window between `notified()` creation and its first poll. This test
/// writes `{"type":"ready"}` before handle construction so the reader can
/// process it immediately on a second worker thread, exercising that window.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_sidecar_ready_notified_race_on_multithread() {
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Run 20 iterations to increase failure probability with the buggy code.
    for i in 0..20 {
        let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();
        let spawn_fn = || make_ready_spawn_fn(writer_tx);

        let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
        let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(500)).await;
        let _held = writer_rx.await.ok();

        assert!(
            result.is_ok(),
            "iteration {}: ensure_sidecar_ready should return Ok \
             (race condition causes spurious timeout with buggy code): {:?}",
            i,
            result
        );

        // Kill the handle stored in the sidecar slot to abort the reader task
        // immediately, preventing task accumulation across iterations.
        if let Some(mut h) = sidecar.lock().await.take() {
            h.kill().await;
        }
    }
}

// --- wait_ready enable() race test (task-407/step-1) ---

/// Reproduce the race condition in `wait_ready` where `self.ready_notify.notified()`
/// creates a Notified future (line 314) but the waiter is NOT registered until first
/// poll. Between creation and the first poll at `tokio::time::timeout` (line 321),
/// there is an await point at `self.state.lock().await` (line 317). On a multi-thread
/// runtime the following interleaving causes a lost notification:
///
///   1. wait_ready: `notified()` created — waiter NOT registered
///   2. wait_ready: acquires state lock, sees Starting, releases lock
///   3. notifier task: acquires state lock, sets Ready, calls `notify_waiters()` — lost
///   4. wait_ready: polls `notified` — registers waiter, but notification already fired
///
/// The fix adds `tokio::pin!(notified)` + `notified.as_mut().enable()` so the waiter
/// is eagerly registered before the intervening await point.
///
/// NOTE: This test exercises the race probabilistically — some iterations may succeed
/// via `enable()` capturing the notification, others via the re-check at line 353
/// (`state.lock().await` sees Ready before the timeout poll). Both paths are correct
/// defense-in-depth behavior; the test validates that at least one succeeds on every
/// iteration.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_ready_enable_prevents_missed_notification_race() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::BufReader;
    use tokio::sync::Mutex;

    for i in 0..50 {
        let (mut handle, _data_writer) = make_starting_handle();

        let notify_arc = Arc::clone(handle.ready_notify());
        let state_clone = Arc::clone(handle.state());

        // Spawn a task that yields minimally then sets Ready + notifies.
        // On a multi-thread runtime this can preempt wait_ready between
        // notified() creation and the first poll, triggering the race.
        tokio::spawn(async move {
            // Yield to give wait_ready a chance to reach the race window.
            tokio::task::yield_now().await;
            let mut s = state_clone.lock().await;
            *s = SidecarState::Ready;
            drop(s);
            notify_arc.notify_waiters();
        });

        let result = handle.wait_ready(Duration::from_millis(500)).await;
        assert!(
            result.is_ok(),
            "iteration {}: wait_ready should succeed (race: notify_waiters \
             fires between notified() creation and first poll): {:?}",
            i,
            result
        );

        // Abort the reader task immediately instead of leaving it detached
        // until the writer drops at end-of-iteration.
        handle.kill().await;
    }
}

// --- ensure_sidecar_ready enable() race test (task-407/step-4) ---

/// Reproduce the post-creation race window in `ensure_sidecar_ready` where
/// `notify_arc.notified()` (line 625) creates a Notified future whose waiter
/// is NOT registered until first polled. Between creation and the first poll at
/// `tokio::time::timeout(ready_timeout, notified)` (line 666), the Phase 3
/// await points form a race window:
///   - `sidecar.lock().await` (line 630)
///   - `state_arc.lock().await` (line 636)
///   - `h.kill().await` (line 650) — evicting a concurrent non-ready handle
///
/// Pre-populating the sidecar slot with a non-ready (Starting) handle adds
/// Phase 1 async work: the state check at line 593 and `h.kill().await` at
/// line 600 execute before spawn_fn, widening the pre-creation window
/// (more time before `notified()` is even created at line 625).  On a
/// multi-thread runtime the reader task can fire `notify_waiters()` during
/// either window, producing a lost wakeup and a spurious timeout.
///
/// The fix adds `std::pin::pin!(notified)` + `notified.as_mut().enable()` so
/// the waiter is eagerly registered before any Phase 3 await points (handling
/// the post-creation window). The re-check at line 661 handles the
/// pre-creation window. Together they provide defense-in-depth; this test
/// exercises the race probabilistically — some iterations may succeed via
/// `enable()` and others via the re-check.
///
/// This is distinct from `ensure_sidecar_ready_notified_race_on_multithread`
/// which was written for the earlier fix (moving `notified()` before the lock)
/// and uses only 20 iterations without a pre-populated sidecar slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_sidecar_ready_enable_prevents_missed_notification_race() {
    use std::time::Duration;
    use tokio::sync::Mutex;

    for i in 0..50 {
        let (writer_tx, writer_rx) = tokio::sync::oneshot::channel();
        let spawn_fn = || make_ready_spawn_fn(writer_tx);

        // Pre-populate the sidecar slot with a non-ready (Starting) handle.
        // This triggers the h.kill().await in Phase 1, adding async work
        // before notified() creation and widening the race window.
        let (stale_handle, _stale_writer) = make_starting_handle();

        let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(Some(stale_handle));
        let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(500)).await;
        let _held = writer_rx.await.ok();

        assert!(
            result.is_ok(),
            "iteration {}: ensure_sidecar_ready should return Ok \
             (enable() race: notify_waiters fires during Phase 3 await \
             points between notified() creation and first poll): {:?}",
            i,
            result
        );

        // Kill the handle stored in the sidecar slot to abort the reader task
        // immediately, preventing task accumulation across iterations.
        if let Some(mut h) = sidecar.lock().await.take() {
            h.kill().await;
        }
    }
}

// --- deterministic re-check path test ---

/// Deterministic test for the post-spawn re-check path (line 661) in
/// `ensure_sidecar_ready`.
///
/// Uses a single-threaded `current_thread` runtime so task scheduling is
/// deterministic: `tokio::spawn`'d tasks only run when the current task
/// yields.  The `spawn_fn` writes `{"type":"ready"}` and yields until the
/// reader task processes it (sets state to `Ready` and calls
/// `notify_waiters()`), all before returning the handle.
///
/// This means `notify_waiters()` fires BEFORE `notified()` is created on
/// line 625 — the notification is irretrievably lost and `enable()` captures
/// nothing.  Phase 3 finds no concurrent caller.  The ONLY path to `Ok` is
/// the re-check at line 661 (`state.lock().await` sees `Ready`).
///
/// A shared `AtomicBool` flag records that state was `Ready` before
/// `spawn_fn` returned, proving the notification window was pre-creation
/// (category (a) in the re-check comment), not post-creation.
#[tokio::test(flavor = "current_thread")]
async fn ensure_sidecar_ready_returns_ok_via_recheck_when_ready_during_spawn() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    // Shared flag: set to true when spawn_fn confirms state is Ready
    // BEFORE returning the handle.
    let ready_before_return = Arc::new(AtomicBool::new(false));
    let ready_flag_clone = Arc::clone(&ready_before_return);

    let (writer_tx, writer_rx) = tokio::sync::oneshot::channel::<tokio::io::DuplexStream>();

    let spawn_fn = move || {
        let flag = Arc::clone(&ready_flag_clone);
        async move {
            let state = Arc::new(Mutex::new(SidecarState::Starting));
            let (mut data_writer, data_reader) = tokio::io::duplex(1024);
            data_writer
                .write_all(b"{\"type\":\"ready\"}\n")
                .await
                .map_err(|e| e.to_string())?;
            let reader = BufReader::new(data_reader);
            let (stdin_writer, _stdin_reader) = tokio::io::duplex(1024);
            let handle = SidecarHandle::from_parts(stdin_writer, reader, state.clone());

            // Yield until the reader task processes the ready message.
            // On current_thread, spawned tasks run only when we yield.
            // Chain: reader task reads line → spawns state-setter task →
            // state-setter sets Ready + calls notify_waiters().
            // 2-task chain (reader → state-setter); 10 yields is
            // conservative headroom — typically converges in 2-4 yields.
            for _ in 0..10 {
                tokio::task::yield_now().await;
                if matches!(*state.lock().await, SidecarState::Ready) {
                    break;
                }
            }

            // Verify Ready was set — this proves notify_waiters() already
            // fired, so the notification is lost by the time notified()
            // is created on line 625 after spawn_fn returns.
            assert!(
                matches!(*state.lock().await, SidecarState::Ready),
                "state should be Ready after yielding in spawn_fn"
            );
            flag.store(true, Ordering::SeqCst);

            // Keep data_writer alive so the reader doesn't see EOF.
            writer_tx
                .send(data_writer)
                .expect("writer_tx receiver dropped");
            Ok(handle)
        }
    };

    let sidecar: Mutex<Option<SidecarHandle>> = Mutex::new(None);
    let result = ensure_sidecar_ready(&sidecar, spawn_fn, Duration::from_millis(500)).await;
    let _held = writer_rx.await.ok();

    assert!(
        ready_before_return.load(Ordering::SeqCst),
        "spawn_fn must have confirmed state was Ready before returning"
    );
    assert!(
        result.is_ok(),
        "ensure_sidecar_ready must succeed via re-check path (line 661): {:?}",
        result
    );

    // Kill the handle stored in the sidecar slot to abort the reader task
    // immediately, preventing task accumulation across iterations.
    if let Some(mut h) = sidecar.lock().await.take() {
        h.kill().await;
    }
}

// --- Permission-prompt wire types (task-3206/step-7) ---

/// (a) OutboundMessage::PermissionRequest deserializes from wire JSON.
#[test]
fn outbound_permission_request_deserializes() {
    let json_str = r#"{"type":"permission_request","id":"msg-1","request_id":"r1","tool_name":"Write","tool_input":{"path":"/tmp/x"}}"#;
    let msg: OutboundMessage = serde_json::from_str(json_str).unwrap();
    match msg {
        OutboundMessage::PermissionRequest {
            id,
            request_id,
            tool_name,
            tool_input,
        } => {
            assert_eq!(id, "msg-1");
            assert_eq!(request_id, "r1");
            assert_eq!(tool_name, "Write");
            assert_eq!(tool_input["path"], "/tmp/x");
        }
        _ => panic!("Expected PermissionRequest"),
    }
}

/// (a) parse_outbound also handles permission_request lines correctly.
#[test]
fn parse_outbound_permission_request() {
    let line = r#"{"type":"permission_request","id":"msg-1","request_id":"r1","tool_name":"Write","tool_input":{"path":"/tmp/x"}}"#;
    let msg = parse_outbound(line).unwrap();
    assert_eq!(
        msg,
        OutboundMessage::PermissionRequest {
            id: "msg-1".to_string(),
            request_id: "r1".to_string(),
            tool_name: "Write".to_string(),
            tool_input: json!({"path": "/tmp/x"}),
        }
    );
}

/// (b) InboundMessage::PermissionDecision serializes with snake_case keys and
/// skips None optional fields (message, updated_input, remember).
#[test]
fn inbound_permission_decision_minimal_serializes() {
    let msg = InboundMessage::PermissionDecision {
        request_id: "r1".to_string(),
        behavior: "allow".to_string(),
        message: None,
        updated_input: None,
        remember: None,
    };
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "permission_decision");
    assert_eq!(json_val["request_id"], "r1");
    assert_eq!(json_val["behavior"], "allow");
    // None fields must be absent — skip_serializing_if = Option::is_none
    assert!(
        json_val.get("message").is_none(),
        "None message should be absent"
    );
    assert!(
        json_val.get("updated_input").is_none(),
        "None updated_input should be absent"
    );
    assert!(
        json_val.get("remember").is_none(),
        "None remember should be absent"
    );
}

/// (b) InboundMessage::PermissionDecision with all optional fields present serializes all.
#[test]
fn inbound_permission_decision_full_serializes() {
    let msg = InboundMessage::PermissionDecision {
        request_id: "r2".to_string(),
        behavior: "deny".to_string(),
        message: Some("no".to_string()),
        updated_input: Some(json!({"path": "/safe/path"})),
        remember: Some(true),
    };
    let json_val: Value = serde_json::to_value(&msg).unwrap();
    assert_eq!(json_val["type"], "permission_decision");
    assert_eq!(json_val["request_id"], "r2");
    assert_eq!(json_val["behavior"], "deny");
    assert_eq!(json_val["message"], "no");
    assert_eq!(json_val["updated_input"]["path"], "/safe/path");
    assert_eq!(json_val["remember"], true);
}

/// (b) format_inbound(PermissionDecision) round-trips through JSON as a line.
#[test]
fn format_inbound_permission_decision_produces_json_line() {
    let msg = InboundMessage::PermissionDecision {
        request_id: "r3".to_string(),
        behavior: "allow".to_string(),
        message: None,
        updated_input: None,
        remember: Some(false),
    };
    let line = format_inbound(&msg);
    assert!(line.ends_with('\n'), "Should end with newline");
    let json_val: Value = serde_json::from_str(line.trim_end()).unwrap();
    assert_eq!(json_val["type"], "permission_decision");
    assert_eq!(json_val["request_id"], "r3");
    assert_eq!(json_val["behavior"], "allow");
    assert_eq!(json_val["remember"], false);
    assert!(json_val.get("message").is_none());
    assert!(json_val.get("updated_input").is_none());
}

/// (c) outbound_to_event maps PermissionRequest to "claude-permission-request"
/// with the full { id, request_id, tool_name, tool_input } payload.
#[test]
fn outbound_to_event_permission_request() {
    let msg = OutboundMessage::PermissionRequest {
        id: "msg-1".to_string(),
        request_id: "r1".to_string(),
        tool_name: "Write".to_string(),
        tool_input: json!({"path": "/tmp/x"}),
    };
    let (name, payload) = outbound_to_event(&msg);
    assert_eq!(name, "claude-permission-request");
    assert_eq!(payload["id"], "msg-1");
    assert_eq!(payload["request_id"], "r1");
    assert_eq!(payload["tool_name"], "Write");
    assert_eq!(payload["tool_input"]["path"], "/tmp/x");
}

/// (d) claude_permission_decision_impl returns Err when sidecar is None.
#[tokio::test]
async fn claude_permission_decision_impl_errors_when_sidecar_is_none() {
    let sidecar: tokio::sync::Mutex<Option<SidecarHandle>> = tokio::sync::Mutex::new(None);
    let decision = PermissionDecisionArgs {
        request_id: "r1".to_string(),
        behavior: "allow".to_string(),
        message: None,
        updated_input: None,
        remember: None,
    };
    let result = claude_permission_decision_impl(&sidecar, decision).await;
    assert!(
        result.is_err(),
        "Expected error when sidecar is None, got: {:?}",
        result
    );
}

/// (d) claude_permission_decision_impl writes the correct JSON line to sidecar stdin
/// when the sidecar is Ready, with optional fields absent for None values.
#[tokio::test]
async fn claude_permission_decision_impl_writes_json_when_ready() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};

    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let empty_reader = BufReader::new(&b""[..]);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);
    let sidecar = tokio::sync::Mutex::new(Some(handle));

    let decision = PermissionDecisionArgs {
        request_id: "r1".to_string(),
        behavior: "allow".to_string(),
        message: None,
        updated_input: None,
        remember: None,
    };
    let result = claude_permission_decision_impl(&sidecar, decision).await;
    assert!(
        result.is_ok(),
        "Expected success when sidecar is Ready: {:?}",
        result
    );

    let mut buf = vec![0u8; 1024];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    let json_val: Value = serde_json::from_str(written.trim_end()).unwrap();
    assert_eq!(json_val["type"], "permission_decision");
    assert_eq!(json_val["request_id"], "r1");
    assert_eq!(json_val["behavior"], "allow");
    // None fields must be absent (skip_serializing_if = Option::is_none)
    assert!(
        json_val.get("message").is_none(),
        "None message must be absent from wire"
    );
    assert!(
        json_val.get("updated_input").is_none(),
        "None updated_input must be absent from wire"
    );
    assert!(
        json_val.get("remember").is_none(),
        "None remember must be absent from wire"
    );
}

/// (d) All optional fields present in the decision are forwarded to the wire.
#[tokio::test]
async fn claude_permission_decision_impl_writes_all_optional_fields() {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};

    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Ready));
    let (writer, mut reader_end) = tokio::io::duplex(1024);
    let empty_reader = BufReader::new(&b""[..]);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);
    let sidecar = tokio::sync::Mutex::new(Some(handle));

    let decision = PermissionDecisionArgs {
        request_id: "r2".to_string(),
        behavior: "deny".to_string(),
        message: Some("not allowed".to_string()),
        updated_input: None,
        remember: Some(true),
    };
    let result = claude_permission_decision_impl(&sidecar, decision).await;
    assert!(result.is_ok(), "Expected success: {:?}", result);

    let mut buf = vec![0u8; 1024];
    let n = reader_end.read(&mut buf).await.unwrap();
    let written = std::str::from_utf8(&buf[..n]).unwrap();
    let json_val: Value = serde_json::from_str(written.trim_end()).unwrap();
    assert_eq!(json_val["request_id"], "r2");
    assert_eq!(json_val["behavior"], "deny");
    assert_eq!(json_val["message"], "not allowed");
    assert_eq!(json_val["remember"], true);
    assert!(json_val.get("updated_input").is_none());
}

/// (d) claude_permission_decision_impl returns Err when sidecar is not in Ready state.
#[tokio::test]
async fn claude_permission_decision_impl_errors_when_sidecar_not_ready() {
    use std::sync::Arc;
    use tokio::io::BufReader;

    let state = Arc::new(tokio::sync::Mutex::new(SidecarState::Starting));
    let (writer, _reader_end) = tokio::io::duplex(1024);
    let empty_reader = BufReader::new(&b""[..]);
    let handle = SidecarHandle::from_parts(writer, empty_reader, state);
    let sidecar = tokio::sync::Mutex::new(Some(handle));

    let decision = PermissionDecisionArgs {
        request_id: "r1".to_string(),
        behavior: "allow".to_string(),
        message: None,
        updated_input: None,
        remember: None,
    };
    let result = claude_permission_decision_impl(&sidecar, decision).await;
    assert!(
        result.is_err(),
        "Expected error when sidecar is Starting: {:?}",
        result
    );
}

/// (e) PermissionDecisionArgs satisfies the full IPC contract (Serialize +
/// DeserializeOwned + Clone + Debug + PartialEq). This is a compile-time assertion.
#[test]
fn permission_decision_args_satisfies_ipc_contract() {
    super::assert_ipc_contract::<PermissionDecisionArgs>();
}

/// Regression test: make_ready_handle's empty b"" reader causes immediate EOF,
/// which triggers the on_exit callback setting state to Crashed.  On a
/// multi_thread runtime the spawned on_exit task can run between yield points,
/// causing intermittent Ready→Crashed transitions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn make_ready_handle_stays_ready_on_multi_thread() {
    let (handle, _data_writer) = make_ready_handle();
    let state = std::sync::Arc::clone(handle.state());

    // Yield to allow the spawned on_exit task (if any) to execute.
    tokio::task::yield_now().await;
    // Brief sleep gives the on_exit task time to acquire the state lock.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let current = state.lock().await.clone();
    assert!(
        matches!(current, SidecarState::Ready),
        "Expected SidecarState::Ready after yield, got {:?} — \
         the empty-reader EOF race caused a Crashed transition",
        current,
    );
}

// --- workspace_resolution tests (task 3210 step-13) ---

mod workspace_resolution {
    use crate::claude_bridge::{MessageContext, resolve_workspace_dir};
    use std::path::{Path, PathBuf};

    fn ctx_with_file(path: &str) -> MessageContext {
        MessageContext {
            selected_entity: None,
            diagnostics: None,
            constraints: None,
            current_file: Some(path.to_string()),
            attached_contexts: None,
        }
    }

    #[test]
    fn current_file_returns_its_parent() {
        let ctx = ctx_with_file("/proj/main.ri");
        let fallback = PathBuf::from("/fallback");
        let result = resolve_workspace_dir(Some(&ctx), None, &fallback);
        assert_eq!(result, PathBuf::from("/proj"));
    }

    #[test]
    fn initial_file_used_when_no_current_file() {
        let ctx = MessageContext {
            selected_entity: None,
            diagnostics: None,
            constraints: None,
            current_file: None,
            attached_contexts: None,
        };
        let fallback = PathBuf::from("/fallback");
        let result = resolve_workspace_dir(Some(&ctx), Some(Path::new("/init/foo.ri")), &fallback);
        assert_eq!(result, PathBuf::from("/init"));
    }

    #[test]
    fn both_none_returns_fallback_cwd() {
        let fallback = PathBuf::from("/my/cwd");
        let result = resolve_workspace_dir(None, None, &fallback);
        assert_eq!(result, fallback);
    }

    #[test]
    fn current_file_no_parent_falls_through_to_fallback() {
        // "main.ri" has no parent component
        let ctx = ctx_with_file("main.ri");
        let fallback = PathBuf::from("/fallback");
        let result = resolve_workspace_dir(Some(&ctx), None, &fallback);
        assert_eq!(result, fallback);
    }

    #[test]
    fn current_file_empty_string_falls_through_to_fallback() {
        let ctx = ctx_with_file("");
        let fallback = PathBuf::from("/fallback");
        let result = resolve_workspace_dir(Some(&ctx), None, &fallback);
        assert_eq!(result, fallback);
    }

    #[test]
    fn no_context_but_initial_file_uses_initial_parent() {
        let fallback = PathBuf::from("/fallback");
        let result = resolve_workspace_dir(None, Some(Path::new("/init/foo.ri")), &fallback);
        assert_eq!(result, PathBuf::from("/init"));
    }
}

// --- sidecar_env tests (task 3210 step-13) ---

mod sidecar_env {
    use crate::claude_bridge::compute_sidecar_env;
    use std::path::Path;

    #[test]
    fn workspace_and_landlock_exec_both_present() {
        let envs = compute_sidecar_env(Path::new("/ws"), Some(Path::new("/sb/le.py")));
        let reify_ws = envs.iter().find(|(k, _)| k == "REIFY_WORKSPACE");
        let reify_le = envs.iter().find(|(k, _)| k == "REIFY_LANDLOCK_EXEC");
        assert_eq!(
            reify_ws,
            Some(&("REIFY_WORKSPACE".to_string(), "/ws".to_string()))
        );
        assert_eq!(
            reify_le,
            Some(&("REIFY_LANDLOCK_EXEC".to_string(), "/sb/le.py".to_string()))
        );
    }

    #[test]
    fn landlock_exec_none_omits_key() {
        let envs = compute_sidecar_env(Path::new("/ws"), None);
        let reify_ws = envs.iter().find(|(k, _)| k == "REIFY_WORKSPACE");
        let reify_le = envs.iter().find(|(k, _)| k == "REIFY_LANDLOCK_EXEC");
        assert_eq!(
            reify_ws,
            Some(&("REIFY_WORKSPACE".to_string(), "/ws".to_string()))
        );
        assert!(
            reify_le.is_none(),
            "REIFY_LANDLOCK_EXEC should not appear when None"
        );
    }

    #[test]
    fn ordering_is_workspace_first_then_landlock_exec() {
        let envs = compute_sidecar_env(Path::new("/ws"), Some(Path::new("/sb/le.py")));
        assert_eq!(envs.len(), 2);
        assert_eq!(envs[0].0, "REIFY_WORKSPACE");
        assert_eq!(envs[1].0, "REIFY_LANDLOCK_EXEC");
    }

    #[test]
    fn only_workspace_when_no_landlock() {
        let envs = compute_sidecar_env(Path::new("/ws"), None);
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].0, "REIFY_WORKSPACE");
    }
}

// --- apply_sidecar_env + spawn_sidecar_impl signature tests (task 3210 step-15) ---

mod apply_sidecar_env_tests {
    use crate::claude_bridge::apply_sidecar_env;
    use std::path::Path;

    #[test]
    fn sets_workspace_only_when_no_landlock_exec() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(&mut cmd, Path::new("/ws"), None);
        let envs: Vec<_> = cmd
            .as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let has_ws = envs
            .iter()
            .any(|(k, v)| k == "REIFY_WORKSPACE" && v.as_deref() == Some("/ws"));
        let has_le = envs.iter().any(|(k, _)| k == "REIFY_LANDLOCK_EXEC");
        assert!(has_ws, "REIFY_WORKSPACE=/ws should be set: {:?}", envs);
        assert!(!has_le, "REIFY_LANDLOCK_EXEC should not be set: {:?}", envs);
    }

    #[test]
    fn sets_both_when_landlock_exec_some() {
        let mut cmd = tokio::process::Command::new("/bin/true");
        apply_sidecar_env(&mut cmd, Path::new("/ws"), Some(Path::new("/sb/le.py")));
        let envs: Vec<_> = cmd
            .as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let has_ws = envs
            .iter()
            .any(|(k, v)| k == "REIFY_WORKSPACE" && v.as_deref() == Some("/ws"));
        let has_le = envs
            .iter()
            .any(|(k, v)| k == "REIFY_LANDLOCK_EXEC" && v.as_deref() == Some("/sb/le.py"));
        assert!(has_ws, "REIFY_WORKSPACE=/ws should be set: {:?}", envs);
        assert!(
            has_le,
            "REIFY_LANDLOCK_EXEC=/sb/le.py should be set: {:?}",
            envs
        );
    }
}

// spawn_sidecar_impl signature test (step-15c)
#[tokio::test]
async fn spawn_sidecar_impl_with_workspace_and_no_landlock_returns_error_for_missing_binary() {
    use crate::engine::EngineSession;
    use reify_constraints::SimpleConstraintChecker;
    use reify_test_support::MockGeometryKernel;
    use std::path::Path;
    use std::sync::Arc;

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    let engine = Arc::new(std::sync::Mutex::new(session));
    let selection = Arc::new(std::sync::RwLock::new(reify_mcp::SelectionInfo::default()));

    let result = spawn_sidecar_impl(
        Path::new("/tmp/no-such-binary"),
        engine,
        |_name: String, _payload: serde_json::Value| {},
        selection,
        Path::new("/tmp/ws"),
        None,
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

// --- on_sidecar_exit direct unit tests (task 3301) ---

#[tokio::test]
async fn on_sidecar_exit_emits_crashed_event_when_state_was_ready() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, Notify};

    // Set up state = Ready
    let state = Arc::new(Mutex::new(SidecarState::Ready));

    // Set up notify — capture the Notified future BEFORE calling on_sidecar_exit
    // so it stores the current epoch. When notify_waiters() fires inside the helper,
    // polling the future will see the epoch change and return Ready immediately.
    let notify = Arc::new(Notify::new());
    let notified = notify.notified();

    // Set up event sink
    let events: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>> =
        Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);
    let emitter = Arc::new(move |name: String, payload: serde_json::Value| {
        events_clone.lock().unwrap().push((name, payload));
    });

    // Call the helper directly — RED until on_sidecar_exit is defined
    on_sidecar_exit(state.clone(), Arc::clone(&notify), Some(emitter)).await;

    // (a) State must be Crashed after the await
    assert!(
        matches!(*state.lock().await, SidecarState::Crashed(_)),
        "Expected SidecarState::Crashed after on_sidecar_exit with Ready input"
    );

    // (b) Event sink must have exactly one claude-sidecar-crashed entry with non-empty reason
    // Block-scope the std::sync::Mutex guard so it cannot live across the await below
    // (clippy::await_holding_lock does not honour explicit `drop`).
    {
        let emitted = events.lock().unwrap();
        let crashed: Vec<_> = emitted
            .iter()
            .filter(|(name, _)| name == "claude-sidecar-crashed")
            .collect();
        assert_eq!(
            crashed.len(),
            1,
            "Expected exactly one claude-sidecar-crashed event, got: {:?}",
            emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
        let payload = &crashed[0].1;
        assert!(
            payload["reason"].is_string() && !payload["reason"].as_str().unwrap().is_empty(),
            "Expected non-empty string 'reason' in payload, got: {:?}",
            payload
        );
    }

    // (c) notify.notified() must resolve within 50 ms (notify_waiters was called inside helper)
    tokio::time::timeout(Duration::from_millis(50), notified)
        .await
        .expect("notify_waiters should have been called inside on_sidecar_exit");
}

#[tokio::test]
async fn on_sidecar_exit_does_not_emit_when_state_was_not_started() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, Notify};

    // Set up state = NotStarted (the "killed" case — sidecar was never running)
    let state = Arc::new(Mutex::new(SidecarState::NotStarted));

    // Capture the Notified future BEFORE calling on_sidecar_exit so it sees
    // the epoch increment from notify_waiters() when polled.
    let notify = Arc::new(Notify::new());
    let notified = notify.notified();

    // Set up event sink
    let events: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>> =
        Arc::new(std::sync::Mutex::new(vec![]));
    let events_clone = Arc::clone(&events);
    let emitter = Arc::new(move |name: String, payload: serde_json::Value| {
        events_clone.lock().unwrap().push((name, payload));
    });

    on_sidecar_exit(state.clone(), Arc::clone(&notify), Some(emitter)).await;

    // (a) State must remain NotStarted — the kill-suppression branch does NOT
    // overwrite state when the input was NotStarted.
    assert!(
        matches!(*state.lock().await, SidecarState::NotStarted),
        "Expected SidecarState::NotStarted to be preserved after on_sidecar_exit with NotStarted input"
    );

    // (b) Event sink must be empty — no claude-sidecar-crashed emitted for a killed sidecar.
    // Block-scope so the std::sync::Mutex guard cannot live across the await below.
    {
        let emitted = events.lock().unwrap();
        assert!(
            emitted.is_empty(),
            "Expected no events emitted for NotStarted input, got: {:?}",
            emitted.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }

    // (c) notify_waiters must still fire so wait_ready callers wake up even on kill.
    tokio::time::timeout(Duration::from_millis(50), notified)
        .await
        .expect("notify_waiters should fire even when state was NotStarted (kill path)");
}

#[tokio::test]
async fn on_sidecar_exit_handles_missing_emitter_gracefully() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Mutex, Notify};

    // Set up state = Ready (so the crash branch is taken and state transitions)
    let state = Arc::new(Mutex::new(SidecarState::Ready));

    // Capture Notified before calling so it sees notify_waiters() epoch change.
    let notify = Arc::new(Notify::new());
    let notified = notify.notified();

    // Pass None as the emitter — exercises the tracing::debug! fallback path.
    // Use fn(String, Value) as the concrete type for the turbofish; bare fn pointers
    // satisfy F: Fn(String, Value) + Send + Sync + 'static.
    on_sidecar_exit::<fn(String, Value)>(state.clone(), Arc::clone(&notify), None).await;

    // (a) Must not panic — if we reached this point the helper handled None gracefully.

    // (b) State must still transition to Crashed even without an emitter — the should_emit
    // flag drives only the emitter branch, not the state mutation.
    assert!(
        matches!(*state.lock().await, SidecarState::Crashed(_)),
        "Expected SidecarState::Crashed even when emitter is None"
    );

    // (c) notify_waiters must fire (so wait_ready wakes up regardless of emitter presence).
    tokio::time::timeout(Duration::from_millis(50), notified)
        .await
        .expect("notify_waiters should fire even when emitter is None");
}
