// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

// --- IPC types mirroring gui/sidecar/src/types.ts ---

/// Context attached to a user message (optional fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_entity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Vec<String>>,
}

/// Inbound messages sent from the GUI to the sidecar (over sidecar stdin).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundMessage {
    SendMessage {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<MessageContext>,
    },
    Abort,
    ClearSession,
}

/// Outbound messages sent from the sidecar to the GUI (over sidecar stdout).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundMessage {
    TextDelta {
        id: String,
        content: String,
    },
    ThinkingDelta {
        id: String,
        content: String,
    },
    ToolCall {
        id: String,
        tool_name: String,
        tool_input: Value,
    },
    ToolResult {
        id: String,
        tool_name: String,
        result: Value,
    },
    Done {
        id: String,
    },
    #[serde(rename = "error")]
    ErrorMessage {
        id: String,
        message: String,
    },
    Ready,
}

// --- Pure IPC functions ---

/// Serialize an InboundMessage to a JSON line (with trailing newline).
pub fn format_inbound(msg: &InboundMessage) -> String {
    let mut s = serde_json::to_string(msg).expect("InboundMessage serialization cannot fail");
    s.push('\n');
    s
}

/// Parse a JSON line from the sidecar into an OutboundMessage.
pub fn parse_outbound(line: &str) -> Result<OutboundMessage, String> {
    serde_json::from_str(line.trim()).map_err(|e| format!("parse_outbound: {}", e))
}

/// Read lines from sidecar stdout, parse each as OutboundMessage, and call callbacks.
/// Skips lines that fail to parse. Calls on_exit when the stream ends (EOF).
pub async fn read_sidecar_output<R: AsyncBufRead + Unpin>(
    reader: R,
    on_message: impl Fn(OutboundMessage),
    on_exit: impl FnOnce(),
) {
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Ok(msg) = parse_outbound(&line) {
                    on_message(msg);
                }
            }
            Ok(None) => break, // EOF
            Err(_) => break,   // I/O error treated as EOF
        }
    }
    on_exit();
}

/// Map an OutboundMessage to a Tauri event name and JSON payload.
pub fn outbound_to_event(msg: &OutboundMessage) -> (String, Value) {
    match msg {
        OutboundMessage::TextDelta { id, content } => (
            "claude-text-delta".to_string(),
            serde_json::json!({ "id": id, "content": content }),
        ),
        OutboundMessage::ThinkingDelta { id, content } => (
            "claude-thinking-delta".to_string(),
            serde_json::json!({ "id": id, "content": content }),
        ),
        OutboundMessage::ToolCall { id, tool_name, tool_input } => (
            "claude-tool-call".to_string(),
            serde_json::json!({ "id": id, "tool_name": tool_name, "tool_input": tool_input }),
        ),
        OutboundMessage::ToolResult { id, tool_name, result } => (
            "claude-tool-result".to_string(),
            serde_json::json!({ "id": id, "tool_name": tool_name, "result": result }),
        ),
        OutboundMessage::Done { id } => (
            "claude-done".to_string(),
            serde_json::json!({ "id": id }),
        ),
        OutboundMessage::ErrorMessage { id, message } => (
            "claude-error".to_string(),
            serde_json::json!({ "id": id, "message": message }),
        ),
        OutboundMessage::Ready => (
            "claude-ready".to_string(),
            serde_json::json!({}),
        ),
    }
}
