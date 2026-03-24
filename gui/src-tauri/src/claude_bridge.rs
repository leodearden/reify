// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
