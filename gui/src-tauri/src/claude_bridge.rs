// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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

/// Write an InboundMessage as a JSON line to the sidecar stdin.
pub async fn write_to_sidecar<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &InboundMessage,
) -> Result<(), String> {
    let line = format_inbound(msg);
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("write_to_sidecar: {}", e))
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

// --- Sidecar lifecycle management ---

/// State of the sidecar subprocess.
#[derive(Debug, Clone)]
pub enum SidecarState {
    /// Not yet started.
    NotStarted,
    /// Process started, waiting for the "ready" message.
    Starting,
    /// Ready to accept messages.
    Ready,
    /// Process crashed or exited unexpectedly.
    Crashed(String),
}

/// Counter for generating unique message IDs per session.
static MSG_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Handle to a running sidecar process.
///
/// Uses `tokio::sync::Mutex` because operations (send, abort) are async
/// and the lock must be held across await points.
pub struct SidecarHandle {
    stdin: Box<dyn AsyncWrite + Unpin + Send>,
    reader_handle: JoinHandle<()>,
    state: Arc<Mutex<SidecarState>>,
}

impl SidecarHandle {
    /// Construct a SidecarHandle from pre-existing I/O parts.
    /// The reader task is spawned immediately; it processes outbound messages
    /// and transitions state to Ready when it receives the "ready" message.
    pub fn from_parts<W, R>(
        writer: W,
        reader: R,
        state: Arc<Mutex<SidecarState>>,
    ) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
    {
        let state_for_reader = Arc::clone(&state);
        let reader_handle = tokio::spawn(async move {
            read_sidecar_output(
                reader,
                move |msg| {
                    if let OutboundMessage::Ready = &msg {
                        let state_inner = Arc::clone(&state_for_reader);
                        tokio::spawn(async move {
                            *state_inner.lock().await = SidecarState::Ready;
                        });
                    }
                },
                || {
                    // on_exit: reader task ends
                },
            )
            .await;
        });

        SidecarHandle {
            stdin: Box::new(writer),
            reader_handle,
            state,
        }
    }

    /// Get a reference to the state mutex.
    pub fn state(&self) -> &Arc<Mutex<SidecarState>> {
        &self.state
    }

    /// Send a user message to the sidecar. Returns the generated message ID.
    ///
    /// The caller is responsible for ensuring the sidecar is in the Ready state.
    pub async fn send_message(
        &mut self,
        text: &str,
        context: Option<MessageContext>,
    ) -> Result<String, String> {
        let n = MSG_COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = format!("msg-{}", n);
        let msg = InboundMessage::SendMessage {
            id: id.clone(),
            text: text.to_string(),
            context,
        };
        write_to_sidecar(&mut self.stdin, &msg).await?;
        Ok(id)
    }
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
