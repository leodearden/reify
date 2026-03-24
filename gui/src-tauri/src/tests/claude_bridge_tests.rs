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
