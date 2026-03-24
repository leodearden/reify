// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
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
/// State uses `std::sync::Mutex` (not tokio) because state transitions
/// are instantaneous assignments — no await points inside the lock.
/// `stdin` uses `tokio::sync::Mutex` because writes span await points.
pub struct SidecarHandle {
    stdin: SharedStdin,
    reader_handle: JoinHandle<()>,
    state: Arc<std::sync::Mutex<SidecarState>>,
    /// Notified when the sidecar sends the "ready" message.
    ready_notify: Arc<Notify>,
    /// The OS child process, if started via `set_child`.
    child: Option<tokio::process::Child>,
}

impl SidecarHandle {
    /// Construct a SidecarHandle from pre-existing I/O parts.
    /// The reader task handles Ready state transitions and crash detection.
    /// Use [`from_parts_with_mcp`] to also wire up event emission and MCP interception.
    pub fn from_parts<W, R>(writer: W, reader: R, state: Arc<std::sync::Mutex<SidecarState>>) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
    {
        let stdin: SharedStdin = Arc::new(Mutex::new(Box::new(writer)));
        Self::new_inner::<R, fn(String, Value) -> Result<Value, String>, fn(&str, Value)>(
            stdin, reader, state, None,
        )
    }

    /// Construct a SidecarHandle with full event and MCP wiring.
    ///
    /// The reader task will:
    /// - Transition state to Ready on ready message
    /// - Emit all outbound messages to `event_sink` via [`outbound_to_event`]
    /// - For `tool_call` messages with a `reify_` prefix, call `tool_dispatch` synchronously
    ///   and write the result back to the sidecar as a `tool_result` inbound message.
    ///   `tool_dispatch` receives the tool name and input; the engine dependency lives
    ///   at the call site (e.g. `main.rs`), not inside this module.
    pub fn from_parts_with_mcp<W, R, D, F>(
        writer: W,
        reader: R,
        state: Arc<std::sync::Mutex<SidecarState>>,
        tool_dispatch: D,
        event_sink: F,
    ) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
        D: Fn(String, Value) -> Result<Value, String> + Send + Sync + 'static,
        F: Fn(&str, Value) + Send + Sync + 'static,
    {
        let stdin: SharedStdin = Arc::new(Mutex::new(Box::new(writer)));
        Self::new_inner(stdin, reader, state, Some((tool_dispatch, event_sink)))
    }

    /// Internal constructor shared by `from_parts` and `from_parts_with_mcp`.
    fn new_inner<R, D, F>(
        stdin: SharedStdin,
        reader: R,
        state: Arc<std::sync::Mutex<SidecarState>>,
        mcp_config: Option<(D, F)>,
    ) -> Self
    where
        R: AsyncBufRead + Unpin + Send + 'static,
        D: Fn(String, Value) -> Result<Value, String> + Send + Sync + 'static,
        F: Fn(&str, Value) + Send + Sync + 'static,
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
                        *state_for_ready.lock().unwrap() = SidecarState::Ready;
                        notify_for_reader.notify_waiters();
                    }

                    // 2. Event emission and MCP interception
                    if let Some((ref dispatch, ref sink)) = mcp_config {
                        let (event_name, payload) = outbound_to_event(&msg);
                        sink(event_name, payload);

                        // 3. MCP tool interception for reify_ prefixed tool calls
                        if let OutboundMessage::ToolCall { id, tool_name, tool_input } = &msg
                            && tool_name.starts_with("reify_") {
                                let id = id.clone();
                                let tool_name = tool_name.clone();
                                let tool_input = tool_input.clone();
                                // tool_dispatch is synchronous — call it directly here,
                                // then spawn only for the async stdin write.
                                let result_val = match dispatch(tool_name.clone(), tool_input) {
                                    Ok(v) => v,
                                    Err(e) => serde_json::json!({ "error": e }),
                                };
                                let response = InboundMessage::ToolResult {
                                    id,
                                    tool_name,
                                    result: result_val,
                                };
                                let stdin_clone = Arc::clone(&stdin_for_reader);
                                tokio::spawn(async move {
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
                    {
                        let mut s = state_for_crash.lock().unwrap();
                        if !matches!(*s, SidecarState::NotStarted) {
                            *s = SidecarState::Crashed("sidecar exited unexpectedly".to_string());
                        }
                    } // guard dropped before notify_waiters
                    notify_for_crash.notify_waiters();
                },
            )
            .await;
        });

        SidecarHandle { stdin, reader_handle, state, ready_notify, child: None }
    }

    /// Get a reference to the state mutex.
    pub fn state(&self) -> &Arc<std::sync::Mutex<SidecarState>> {
        &self.state
    }

    /// Subscribe to the ready notification.
    ///
    /// Returns an owned `'static` future that resolves when the sidecar sends
    /// the "ready" message (i.e. when [`Notify::notify_waiters`] is called).
    /// The future clones the `Arc<Notify>` internally, so it is safe to hold
    /// across lock boundaries without keeping a reference to the handle.
    pub fn subscribe_ready(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let notify = Arc::clone(&self.ready_notify);
        async move { notify.notified().await }
    }

    /// Wait until the sidecar transitions to the Ready state or the timeout expires.
    ///
    /// Fast path: if state is already Ready, returns immediately.
    /// Slow path: awaits `ready_notify` with the given timeout.
    pub async fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        // Fast path: already Ready
        if matches!(*self.state.lock().unwrap(), SidecarState::Ready) {
            return Ok(());
        }

        // Slow path: subscribe before checking again to avoid the race between
        // checking state and the notification being fired.
        let notified = self.ready_notify.notified();
        // Re-check under the subscription to avoid missing a notification that
        // arrived between the fast-path check and the subscribe.
        if matches!(*self.state.lock().unwrap(), SidecarState::Ready) {
            return Ok(());
        }

        tokio::time::timeout(timeout, notified)
            .await
            .map_err(|_| format!("Timeout waiting for sidecar ready after {}ms", timeout.as_millis()))?;

        // Re-check state: notification may have been triggered by a crash, not Ready.
        let state = self.state.lock().unwrap();
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
        *self.state.lock().unwrap() = SidecarState::NotStarted;
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
        let n = MSG_COUNTER.fetch_add(1, Ordering::Relaxed);
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

/// Lock the sidecar mutex, check that a handle is present, and run `f` on it.
///
/// Returns `Err("sidecar not started")` if the slot is `None`.
/// Otherwise awaits `f(handle)` and returns its result.
///
/// The closure must return a `Pin<Box<dyn Future + Send + 'a>>` where `'a` is
/// the lifetime of the `&mut SidecarHandle` borrow. Use `Box::pin(...)` at
/// the call site:
/// ```rust,ignore
/// with_handle(&sidecar, |h| Box::pin(h.abort())).await
/// ```
async fn with_handle<T, F>(sidecar: &Mutex<Option<SidecarHandle>>, f: F) -> Result<T, String>
where
    F: for<'a> FnOnce(
        &'a mut SidecarHandle,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + 'a>>,
{
    let mut guard = sidecar.lock().await;
    match guard.as_mut() {
        None => Err("sidecar not started".to_string()),
        Some(handle) => f(handle).await,
    }
}

/// Send a message to the sidecar. Returns the generated message ID.
///
/// Returns an error if the sidecar is not started or not in the Ready state.
pub async fn claude_send_message_impl(
    sidecar: &Mutex<Option<SidecarHandle>>,
    text: &str,
    context: Option<MessageContext>,
) -> Result<String, String> {
    let text = text.to_string();
    with_handle(sidecar, move |handle| {
        Box::pin(async move {
            let state = handle.state().lock().unwrap().clone();
            match state {
                SidecarState::Ready => handle.send_message(&text, context).await,
                SidecarState::Crashed(msg) => Err(format!("sidecar crashed: {}", msg)),
                SidecarState::NotStarted => Err("sidecar not started".to_string()),
                SidecarState::Starting => Err("sidecar not ready (still starting)".to_string()),
            }
        })
    })
    .await
}

/// Send an abort signal to the sidecar.
///
/// Returns an error if the sidecar is not started.
pub async fn claude_abort_impl(sidecar: &Mutex<Option<SidecarHandle>>) -> Result<(), String> {
    with_handle(sidecar, |h| Box::pin(h.abort())).await
}

/// Clear the conversation session in the sidecar.
///
/// Returns an error if the sidecar is not started.
pub async fn claude_clear_session_impl(
    sidecar: &Mutex<Option<SidecarHandle>>,
) -> Result<(), String> {
    with_handle(sidecar, |h| Box::pin(h.clear_session())).await
}

/// Spawn the Claude sidecar process and return a ready-to-use [`SidecarHandle`].
///
/// Extracts stdin/stdout from the child, wraps stdout in a [`BufReader`], and
/// wires up event emission and MCP interception via [`SidecarHandle::from_parts_with_mcp`].
/// The caller is responsible for calling [`SidecarHandle::wait_ready`] and storing
/// the returned handle in the shared sidecar slot.
///
/// Returns `Err` if the process cannot be spawned or if stdin/stdout are unavailable.
pub async fn spawn_sidecar_impl<D, F>(
    path: &Path,
    tool_dispatch: D,
    event_sink: F,
) -> Result<SidecarHandle, String>
where
    D: Fn(String, Value) -> Result<Value, String> + Send + Sync + 'static,
    F: Fn(&str, Value) + Send + Sync + 'static,
{
    let mut proc = tokio::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to spawn sidecar {:?}: {}", path, e))?;

    let stdin = match proc.stdin.take() {
        Some(s) => s,
        None => {
            proc.kill().await.ok();
            return Err("sidecar has no stdin".to_string());
        }
    };
    let stdout = match proc.stdout.take() {
        Some(s) => s,
        None => {
            proc.kill().await.ok();
            return Err("sidecar has no stdout".to_string());
        }
    };

    let reader = BufReader::new(stdout);
    let sidecar_state = Arc::new(std::sync::Mutex::new(SidecarState::Starting));
    let mut handle = SidecarHandle::from_parts_with_mcp(stdin, reader, sidecar_state, tool_dispatch, event_sink);
    handle.set_child(proc);
    Ok(handle)
}

/// Shut down the sidecar: kill it if running and clear the slot.
///
/// Locks the mutex, kills the handle if `Some`, then sets the slot to `None`.
pub async fn shutdown_sidecar(sidecar: &Mutex<Option<SidecarHandle>>) {
    let mut guard = sidecar.lock().await;
    if let Some(handle) = guard.as_mut() {
        handle.kill().await;
    }
    *guard = None;
}

/// Map an OutboundMessage to a Tauri event name and JSON payload.
pub fn outbound_to_event(msg: &OutboundMessage) -> (&'static str, Value) {
    match msg {
        OutboundMessage::TextDelta { id, content } => (
            "claude-text-delta",
            serde_json::json!({ "id": id, "content": content }),
        ),
        OutboundMessage::ThinkingDelta { id, content } => (
            "claude-thinking-delta",
            serde_json::json!({ "id": id, "content": content }),
        ),
        OutboundMessage::ToolCall { id, tool_name, tool_input } => (
            "claude-tool-call",
            serde_json::json!({ "id": id, "tool_name": tool_name, "tool_input": tool_input }),
        ),
        OutboundMessage::ToolResult { id, tool_name, result } => (
            "claude-tool-result",
            serde_json::json!({ "id": id, "tool_name": tool_name, "result": result }),
        ),
        OutboundMessage::Done { id } => (
            "claude-done",
            serde_json::json!({ "id": id }),
        ),
        OutboundMessage::ErrorMessage { id, message } => (
            "claude-error",
            serde_json::json!({ "id": id, "message": message }),
        ),
        OutboundMessage::Ready => (
            "claude-ready",
            serde_json::json!({}),
        ),
    }
}
