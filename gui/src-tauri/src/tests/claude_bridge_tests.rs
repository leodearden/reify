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
