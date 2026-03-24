// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, Notify};
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
    /// Tool result sent back to the sidecar after in-process MCP tool execution.
    ToolResult {
        id: String,
        tool_name: String,
        result: Value,
    },
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

/// Shared stdin writer type — Arc<Mutex<>> so the reader task can write back tool results.
type SharedStdin = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;

/// Handle to a running sidecar process.
///
/// Uses `tokio::sync::Mutex` because operations (send, abort) are async
/// and the lock must be held across await points.
pub struct SidecarHandle {
    stdin: SharedStdin,
    reader_handle: JoinHandle<()>,
    state: Arc<Mutex<SidecarState>>,
    /// Notified when the sidecar sends the "ready" message.
    ready_notify: Arc<Notify>,
    /// The OS child process, if started via `set_child`.
    child: Option<tokio::process::Child>,
}

impl SidecarHandle {
    /// Construct a SidecarHandle from pre-existing I/O parts.
    /// The reader task handles Ready state transitions and crash detection.
    /// Use [`from_parts_with_mcp`] to also wire up event emission and MCP interception.
    pub fn from_parts<W, R>(writer: W, reader: R, state: Arc<Mutex<SidecarState>>) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
    {
        let stdin: SharedStdin = Arc::new(Mutex::new(Box::new(writer)));
        Self::new_inner::<R, fn(String, Value)>(stdin, reader, state, None)
    }

    /// Construct a SidecarHandle with full event and MCP wiring.
    ///
    /// The reader task will:
    /// - Transition state to Ready on ready message
    /// - Emit all outbound messages to `event_sink` via [`outbound_to_event`]
    /// - For `tool_call` messages with a `reify_` prefix, call [`crate::mcp_context::mcp_tool_call_impl`]
    ///   and write the result back to the sidecar as a `tool_result` inbound message
    pub fn from_parts_with_mcp<W, R, F>(
        writer: W,
        reader: R,
        state: Arc<Mutex<SidecarState>>,
        engine: Arc<std::sync::Mutex<crate::engine::EngineSession>>,
        event_sink: F,
    ) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        let stdin: SharedStdin = Arc::new(Mutex::new(Box::new(writer)));
        Self::new_inner(stdin, reader, state, Some((engine, event_sink)))
    }

    /// Internal constructor shared by `from_parts` and `from_parts_with_mcp`.
    fn new_inner<R, F>(
        stdin: SharedStdin,
        reader: R,
        state: Arc<Mutex<SidecarState>>,
        mcp_config: Option<(Arc<std::sync::Mutex<crate::engine::EngineSession>>, F)>,
    ) -> Self
    where
        R: AsyncBufRead + Unpin + Send + 'static,
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        let ready_notify = Arc::new(Notify::new());
        let state_for_ready = Arc::clone(&state);
        let state_for_crash = Arc::clone(&state);
        let notify_for_crash = Arc::clone(&ready_notify);
        let stdin_for_reader = Arc::clone(&stdin);
        let notify_for_reader = Arc::clone(&ready_notify);

        let reader_handle = tokio::spawn(async move {
            read_sidecar_output(
                reader,
                move |msg| {
                    // 1. State transition: Ready message
                    if let OutboundMessage::Ready = &msg {
                        let state_inner = Arc::clone(&state_for_ready);
                        let notify_inner = Arc::clone(&notify_for_reader);
                        tokio::spawn(async move {
                            *state_inner.lock().await = SidecarState::Ready;
                            notify_inner.notify_waiters();
                        });
                    }

                    // 2. Event emission and MCP interception
                    if let Some((ref engine, ref sink)) = mcp_config {
                        let (event_name, payload) = outbound_to_event(&msg);
                        sink(event_name, payload);

                        // 3. MCP tool interception for reify_ prefixed tool calls
                        if let OutboundMessage::ToolCall { id, tool_name, tool_input } = &msg
                            && tool_name.starts_with("reify_") {
                                let id = id.clone();
                                let tool_name = tool_name.clone();
                                let tool_input = tool_input.clone();
                                let engine_clone = Arc::clone(engine);
                                let stdin_clone = Arc::clone(&stdin_for_reader);
                                tokio::spawn(async move {
                                    let ctx = crate::mcp_context::TauriToolContext::new(engine_clone);
                                    let result = crate::mcp_context::mcp_tool_call_impl(
                                        &tool_name,
                                        tool_input,
                                        &ctx,
                                    );
                                    let result_val = match result {
                                        Ok(v) => v,
                                        Err(e) => serde_json::json!({ "error": e }),
                                    };
                                    let response = InboundMessage::ToolResult {
                                        id,
                                        tool_name,
                                        result: result_val,
                                    };
                                    let mut writer = stdin_clone.lock().await;
                                    write_to_sidecar(&mut *writer, &response).await.ok();
                                });
                            }
                    }
                },
                move || {
                    // on_exit: set state to Crashed unless we're already NotStarted (killed).
                    // Also notify waiters so anyone blocked in wait_ready wakes immediately
                    // instead of hanging for the full timeout.
                    let state_inner = state_for_crash;
                    let notify_inner = notify_for_crash;
                    tokio::spawn(async move {
                        let mut s = state_inner.lock().await;
                        if !matches!(*s, SidecarState::NotStarted) {
                            *s = SidecarState::Crashed("sidecar exited unexpectedly".to_string());
                        }
                        notify_inner.notify_waiters();
                    });
                },
            )
            .await;
        });

        SidecarHandle { stdin, reader_handle, state, ready_notify, child: None }
    }

    /// Get a reference to the state mutex.
    pub fn state(&self) -> &Arc<Mutex<SidecarState>> {
        &self.state
    }

    /// Get a reference to the ready notify so callers can await it without
    /// holding the outer sidecar lock.
    pub fn ready_notify(&self) -> &Arc<Notify> {
        &self.ready_notify
    }

    /// Wait until the sidecar transitions to the Ready state or the timeout expires.
    ///
    /// Fast path: if state is already Ready, returns immediately.
    /// Slow path: awaits `ready_notify` with the given timeout.
    pub async fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        // Fast path: already Ready
        if matches!(*self.state.lock().await, SidecarState::Ready) {
            return Ok(());
        }

        // Slow path: subscribe before checking again to avoid the race between
        // checking state and the notification being fired.
        let notified = self.ready_notify.notified();
        // Re-check under the subscription to avoid missing a notification that
        // arrived between the fast-path check and the subscribe.
        if matches!(*self.state.lock().await, SidecarState::Ready) {
            return Ok(());
        }

        tokio::time::timeout(timeout, notified)
            .await
            .map_err(|_| format!("Timeout waiting for sidecar ready after {}ms", timeout.as_millis()))?;

        // Re-check state: notification may have been triggered by a crash, not Ready.
        let state = self.state.lock().await;
        match &*state {
            SidecarState::Ready => Ok(()),
            SidecarState::Crashed(msg) => Err(format!("sidecar crashed: {}", msg)),
            other => Err(format!("sidecar not ready after notification: {:?}", other)),
        }
    }

    /// Store the OS child process handle so it can be properly terminated on kill.
    pub fn set_child(&mut self, child: tokio::process::Child) {
        self.child = Some(child);
    }

    /// Returns true if a child process has been stored via `set_child`.
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Kill the sidecar: terminate the OS process (if any), abort the reader task,
    /// and reset state to NotStarted.
    ///
    /// The child process is SIGKILLed and then reaped (waited) to prevent zombies.
    pub async fn kill(&mut self) {
        // 1. Terminate the OS child process first to release OS resources.
        if let Some(mut child) = self.child.take() {
            // Ignore errors: process may have already exited.
            child.kill().await.ok();
            child.wait().await.ok();
        }
        // 2. Abort the reader task.
        self.reader_handle.abort();
        // 3. Mark as not started.
        *self.state.lock().await = SidecarState::NotStarted;
    }

    /// Send an abort signal to the sidecar (cancels the current message).
    pub async fn abort(&mut self) -> Result<(), String> {
        let mut writer = self.stdin.lock().await;
        write_to_sidecar(&mut *writer, &InboundMessage::Abort).await
    }

    /// Send a clear_session signal to the sidecar (resets conversation history).
    pub async fn clear_session(&mut self) -> Result<(), String> {
        let mut writer = self.stdin.lock().await;
        write_to_sidecar(&mut *writer, &InboundMessage::ClearSession).await
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
        let mut writer = self.stdin.lock().await;
        write_to_sidecar(&mut *writer, &msg).await?;
        Ok(id)
    }
}

// --- High-level command implementations ---

/// Send a message to the sidecar. Returns the generated message ID.
///
/// Returns an error if the sidecar is not started or not in the Ready state.
pub async fn claude_send_message_impl(
    sidecar: &Mutex<Option<SidecarHandle>>,
    text: &str,
    context: Option<MessageContext>,
) -> Result<String, String> {
    let mut guard = sidecar.lock().await;
    match guard.as_mut() {
        None => Err("sidecar not started".to_string()),
        Some(handle) => {
            let state = handle.state().lock().await.clone();
            match state {
                SidecarState::Ready => handle.send_message(text, context).await,
                SidecarState::Crashed(msg) => Err(format!("sidecar crashed: {}", msg)),
                SidecarState::NotStarted => Err("sidecar not started".to_string()),
                SidecarState::Starting => Err("sidecar not ready (still starting)".to_string()),
            }
        }
    }
}

/// Send an abort signal to the sidecar.
///
/// Returns an error if the sidecar is not started.
pub async fn claude_abort_impl(sidecar: &Mutex<Option<SidecarHandle>>) -> Result<(), String> {
    let mut guard = sidecar.lock().await;
    match guard.as_mut() {
        None => Err("sidecar not started".to_string()),
        Some(handle) => handle.abort().await,
    }
}

/// Clear the conversation session in the sidecar.
///
/// Returns an error if the sidecar is not started.
pub async fn claude_clear_session_impl(
    sidecar: &Mutex<Option<SidecarHandle>>,
) -> Result<(), String> {
    let mut guard = sidecar.lock().await;
    match guard.as_mut() {
        None => Err("sidecar not started".to_string()),
        Some(handle) => handle.clear_session().await,
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
