// Claude Code SDK sidecar bridge.
//
// Manages the lifecycle of the sidecar process that communicates with the
// Claude Code SDK, handles JSON-line IPC, and bridges sidecar events to
// Tauri frontend events.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_contexts: Option<Vec<String>>,
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
        /// The Claude CLI tool_use_id echoed from the corresponding ToolCall outbound.
        /// Enables id-based correlation in the sidecar (avoids FIFO-by-tool_name fallback).
        tool_use_id: String,
    },
    /// User's decision on a pending permission-prompt request.
    /// Sent in response to an outbound `PermissionRequest` identified by `request_id`.
    PermissionDecision {
        request_id: String,
        /// `"allow"` or `"deny"`.
        behavior: String,
        /// Optional human-readable explanation (shown in the CLI's audit log when set).
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        /// Optional override of the tool's input (PreToolUse-style patching).
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
        /// When true, the sidecar will remember this tool as always-allowed for
        /// the lifetime of the current permission server instance.
        #[serde(skip_serializing_if = "Option::is_none")]
        remember: Option<bool>,
    },
}

/// Arguments for the `claude_permission_decision` Tauri command.
/// Fields map 1-to-1 to `InboundMessage::PermissionDecision`, using snake_case
/// so they round-trip correctly through Rust serde without a camelCase bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDecisionArgs {
    pub request_id: String,
    pub behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember: Option<bool>,
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
        /// The Claude CLI tool_use_id enabling id-based correlation in the sidecar.
        /// Defaults to empty string when absent (stale sidecar without this field),
        /// so a missing field degrades gracefully (FIFO-by-tool_name fallback).
        #[serde(default)]
        tool_use_id: String,
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
    Notice {
        id: String,
        code: String,
        message: String,
    },
    Ready,
    /// Permission-prompt request emitted when Claude CLI wants to use a tool
    /// that requires user approval. The sidecar emits this; the host forwards
    /// it to the frontend via the `claude-permission-request` Tauri event.
    PermissionRequest {
        /// The in-flight send_message id (for turn correlation).
        id: String,
        /// Unique correlator for the permission round-trip (sidecar-generated UUID).
        request_id: String,
        /// Name of the tool Claude wants to invoke.
        tool_name: String,
        /// Arguments Claude intends to pass to the tool.
        tool_input: Value,
    },
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

/// Write an InboundMessage as a JSON line, returning the raw `io::Error`.
///
/// This is the low-level primitive. Callers that need `ErrorKind` inspection (e.g.
/// `SidecarHandle::abort` checking for `BrokenPipe`) use this directly; callers that
/// want a `String` error use `write_to_sidecar`.
async fn try_write_inbound<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &InboundMessage,
) -> Result<(), std::io::Error> {
    let line = format_inbound(msg);
    writer.write_all(line.as_bytes()).await
}

/// Write an InboundMessage as a JSON line to the sidecar stdin.
pub async fn write_to_sidecar<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &InboundMessage,
) -> Result<(), String> {
    try_write_inbound(writer, msg)
        .await
        .map_err(|e| format!("write_to_sidecar: {}", e))
}

/// Emit a WARN when a ToolCall arrives without a `tool_use_id` field.
///
/// This is the dev-mode version-skew case: a stale sidecar (pre-dating task #2766)
/// omits `tool_use_id` from ToolCall events.  The message is still delivered to
/// `on_message`; id-correlation falls back to FIFO-by-tool_name in the sidecar
/// TypeScript.  All other `OutboundMessage` variants are silently ignored.
pub(crate) fn warn_if_stale_tool_call(msg: &OutboundMessage) {
    if let OutboundMessage::ToolCall {
        tool_use_id,
        id,
        tool_name,
        ..
    } = msg
        && tool_use_id.is_empty()
    {
        tracing::warn!(
            message_id = %id,
            tool_name = %tool_name,
            "sidecar tool_call missing tool_use_id; \
             likely dev-mode version skew between sidecar and Rust binary \
             — id-correlation will fall back to FIFO-by-tool_name"
        );
    }
}

/// Read lines from sidecar stdout, parse each as OutboundMessage, and call callbacks.
/// Skips lines that fail to parse (and warns operators). Calls on_exit when the stream ends (EOF).
pub async fn read_sidecar_output<R: AsyncBufRead + Unpin>(
    reader: R,
    on_message: impl Fn(OutboundMessage),
    on_exit: impl FnOnce(),
) {
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                match parse_outbound(&line) {
                    Ok(msg) => {
                        warn_if_stale_tool_call(&msg);
                        on_message(msg);
                    }
                    Err(e) => {
                        // Cap the payload snippet to avoid log spam from unexpectedly large lines.
                        // Use chars().take(200) to truncate on character boundaries rather than raw
                        // byte index 200, which would panic if a multi-byte UTF-8 codepoint straddles
                        // that boundary.
                        let snippet: String = line.chars().take(200).collect();
                        tracing::warn!(
                            error = %e,
                            payload_snippet = %snippet,
                            "failed to parse sidecar message; dropping line"
                        );
                    }
                }
            }
            Ok(None) => break, // EOF
            Err(_) => break,   // I/O error treated as EOF
        }
    }
    on_exit();
}

/// Handle the sidecar process exit event: update state, notify waiters, and
/// optionally emit a `claude-sidecar-crashed` Tauri event via the supplied emitter.
///
/// This is the extracted body of the `tokio::spawn(async move { ... })` block that
/// previously lived inline inside `SidecarHandle::new_inner`'s `on_exit` closure.
/// Extracting it here allows direct unit testing without the EngineSession /
/// SelectionInfo ballast required by `from_parts_with_mcp`.
///
/// # Lock-release-before-emit invariant
///
/// The state guard is acquired, the state is updated, and the guard is dropped
/// **before** `notify_waiters()` and the emitter call. Holding the lock across
/// `emit()` would stall every other task awaiting `state.lock()` (notably
/// `wait_ready` and `kill`) if the emitter ever blocks or panics.
pub(crate) async fn on_sidecar_exit<F>(
    state: Arc<Mutex<SidecarState>>,
    notify: Arc<Notify>,
    emitter: Option<Arc<F>>,
) where
    F: Fn(String, Value) + Send + Sync + 'static,
{
    let reason = "sidecar exited unexpectedly".to_string();
    let should_emit = {
        let mut s = state.lock().await;
        let crashed = !matches!(*s, SidecarState::NotStarted);
        if crashed {
            *s = SidecarState::Crashed(reason.clone());
        }
        crashed
    }; // lock dropped here
    notify.notify_waiters();
    if should_emit {
        if let Some(ref emit) = emitter {
            emit(
                "claude-sidecar-crashed".to_string(),
                serde_json::json!({ "reason": reason }),
            );
        } else {
            tracing::debug!(
                "sidecar crashed but no event emitter is wired (from_parts path); \
                 frontend will not receive claude-sidecar-crashed"
            );
        }
    }
}

/// Named configuration for MCP tool interception in the sidecar reader task.
///
/// Replaces the anonymous 3-tuple `(Arc<Mutex<EngineSession>>, F, Arc<RwLock<SelectionInfo>>)`
/// that previously required `#[allow(clippy::type_complexity)]`.
pub struct McpConfig<F> {
    pub(crate) engine: Arc<std::sync::Mutex<crate::engine::EngineSession>>,
    pub(crate) event_emitter: Arc<F>,
    pub(crate) selection: Arc<std::sync::RwLock<reify_mcp::SelectionInfo>>,
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
    /// **Test-only** constructor — gated behind `#[cfg(test)]` because it does not
    /// wire up an event emitter (see `# Note: no claude-sidecar-crashed event` below).
    /// Production code paths should use [`from_parts_with_mcp`] via [`spawn_sidecar_impl`].
    ///
    /// The reader task handles Ready state transitions and crash detection.
    ///
    /// # Note: no `claude-sidecar-crashed` event
    ///
    /// This constructor does **not** wire up a Tauri event emitter. If the sidecar
    /// crashes, state transitions to `Crashed` as usual, but no `claude-sidecar-crashed`
    /// event is emitted to the frontend. Use [`from_parts_with_mcp`] for production
    /// code paths where the frontend must be notified of unexpected sidecar exits.
    #[cfg(test)]
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
    /// - Emit all outbound messages to `event_emitter` via [`outbound_to_event`]
    /// - For `tool_call` messages with a `reify_` prefix, call [`crate::mcp_context::mcp_tool_call_impl`]
    ///   and write the result back to the sidecar as a `tool_result` inbound message
    pub fn from_parts_with_mcp<W, R, F>(
        writer: W,
        reader: R,
        state: Arc<Mutex<SidecarState>>,
        engine: Arc<std::sync::Mutex<crate::engine::EngineSession>>,
        event_emitter: F,
        selection: Arc<std::sync::RwLock<reify_mcp::SelectionInfo>>,
    ) -> Self
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncBufRead + Unpin + Send + 'static,
        F: Fn(String, Value) + Send + Sync + 'static,
    {
        let stdin: SharedStdin = Arc::new(Mutex::new(Box::new(writer)));
        let mcp_config = McpConfig {
            engine,
            event_emitter: Arc::new(event_emitter),
            selection,
        };
        Self::new_inner(stdin, reader, state, Some(mcp_config))
    }

    /// Internal constructor shared by `from_parts` and `from_parts_with_mcp`.
    fn new_inner<R, F>(
        stdin: SharedStdin,
        reader: R,
        state: Arc<Mutex<SidecarState>>,
        mcp_config: Option<McpConfig<F>>,
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
        // Capture event_emitter for the on_exit closure so we can emit
        // claude-sidecar-crashed when an unexpected exit is detected.
        let event_emitter_for_exit: Option<Arc<F>> =
            mcp_config.as_ref().map(|m| Arc::clone(&m.event_emitter));

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
                    if let Some(ref mcp) = mcp_config {
                        let (event_name, payload) = outbound_to_event(&msg);
                        (mcp.event_emitter)(event_name, payload);

                        // 3. MCP tool interception for reify_ prefixed tool calls
                        if let OutboundMessage::ToolCall {
                            id,
                            tool_name,
                            tool_input,
                            tool_use_id,
                        } = &msg
                            && tool_name.starts_with("reify_")
                        {
                            let id = id.clone();
                            let err_id = id.clone();
                            let tool_name = tool_name.clone();
                            let tool_input = tool_input.clone();
                            let tool_use_id = tool_use_id.clone();
                            let engine_clone = Arc::clone(&mcp.engine);
                            let selection_clone = Arc::clone(&mcp.selection);
                            let stdin_clone = Arc::clone(&stdin_for_reader);
                            let emitter_clone = Arc::clone(&mcp.event_emitter);
                            tokio::spawn(async move {
                                let ctx =
                                    crate::mcp_context::TauriToolContext::builder(engine_clone)
                                        .with_selection(selection_clone)
                                        .with_event_emitter({
                                            let e = Arc::clone(&emitter_clone);
                                            move |name: &str, payload: serde_json::Value| {
                                                e(name.to_string(), payload);
                                            }
                                        })
                                        .build();
                                let result = crate::mcp_context::mcp_tool_call_impl(
                                    &tool_name, tool_input, &ctx,
                                );
                                let result_val = match result {
                                    Ok(v) => v,
                                    Err(e) => serde_json::json!({ "error": e }),
                                };
                                let response = InboundMessage::ToolResult {
                                    id,
                                    tool_name,
                                    result: result_val,
                                    tool_use_id,
                                };
                                let mut writer = stdin_clone.lock().await;
                                if let Err(err) =
                                    write_to_sidecar(&mut *writer, &response).await
                                {
                                    tracing::error!(
                                        "failed to send tool result to sidecar: {err}"
                                    );
                                    emitter_clone(
                                        "claude-error".to_string(),
                                        serde_json::json!({
                                            "id": err_id,
                                            "message": format!("failed to send tool result to sidecar: {err}"),
                                        }),
                                    );
                                }
                            });
                        }
                    }
                },
                move || {
                    tokio::spawn(on_sidecar_exit(
                        state_for_crash,
                        notify_for_crash,
                        event_emitter_for_exit,
                    ));
                },
            )
            .await;
        });

        SidecarHandle {
            stdin,
            reader_handle,
            state,
            ready_notify,
            child: None,
        }
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
        //
        // IMPORTANT: `Notified::notified()` does NOT register the waiter until the
        // future is first polled. We must pin + enable() eagerly so that any
        // `notify_waiters()` call during the re-check await below is captured.
        // Without enable(), a multi-thread executor can interleave:
        //   1. notified() created — waiter not registered
        //   2. state lock acquired/released (re-check below)
        //   3. another task sets Ready + calls notify_waiters() — lost
        //   4. notified polled — registers waiter, but notification already fired
        let mut notified = std::pin::pin!(self.ready_notify.notified());
        notified.as_mut().enable();
        // Re-check under the subscription to avoid missing a notification that
        // arrived between the fast-path check and the subscribe.
        if matches!(*self.state.lock().await, SidecarState::Ready) {
            return Ok(());
        }

        tokio::time::timeout(timeout, notified).await.map_err(|_| {
            format!(
                "Timeout waiting for sidecar ready after {}ms",
                timeout.as_millis()
            )
        })?;

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
    ///
    /// This method is idempotent against a dead sidecar:
    /// - Returns `Ok(())` immediately if the sidecar is known not running
    ///   (`NotStarted` or `Crashed`), without touching stdin.
    /// - If the sidecar's stdin pipe happens to be closed (race window between
    ///   pipe closure and state transition to `Crashed`), the resulting
    ///   `BrokenPipe` error is also silently converted to `Ok(())` — the
    ///   user-visible action ("stop the request") is already complete.
    /// - When state is `Starting`, the call falls through to the stdin write —
    ///   stdin is already open during boot, so the write is benign.
    pub async fn abort(&mut self) -> Result<(), String> {
        // State pre-check: end the lock-guard temporary before taking the stdin lock
        // to avoid holding two locks simultaneously (matches lock-ordering hygiene in
        // claude_send_message_impl). A block drops the guard at `}` without a clone.
        let early_return = {
            let s = self.state.lock().await;
            matches!(*s, SidecarState::NotStarted | SidecarState::Crashed(_))
        };
        if early_return {
            return Ok(());
        }

        // Note: state may transition to Crashed between this check and the write below;
        // that race is handled by the BrokenPipe arm.
        let mut writer = self.stdin.lock().await;
        match try_write_inbound(&mut *writer, &InboundMessage::Abort).await {
            Ok(()) => Ok(()),
            // The sidecar already exited and its stdin pipe is closed.
            // The user-visible action ("stop the request") is trivially complete.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(format!("write_to_sidecar: {}", e)),
        }
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

    /// Forward a permission decision to the sidecar.
    ///
    /// The caller is responsible for ensuring the sidecar is in the Ready state.
    pub async fn permission_decision(
        &mut self,
        decision: PermissionDecisionArgs,
    ) -> Result<(), String> {
        let msg = InboundMessage::PermissionDecision {
            request_id: decision.request_id,
            behavior: decision.behavior,
            message: decision.message,
            updated_input: decision.updated_input,
            remember: decision.remember,
        };
        let mut writer = self.stdin.lock().await;
        write_to_sidecar(&mut *writer, &msg).await
    }
}

impl Drop for SidecarHandle {
    fn drop(&mut self) {
        // Abort the reader task so it doesn't continue running detached.
        // JoinHandle::abort() is sync and marks the task for cancellation
        // at its next .await point.
        self.reader_handle.abort();
        // Kill the OS child process if one was attached via set_child().
        // start_kill() is sync and sends SIGKILL without waiting for exit —
        // best-effort OS cleanup in a Drop context where async is unavailable.
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
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

/// Forward a user permission decision to the sidecar.
///
/// Returns an error if the sidecar is not started or not in the Ready state.
pub async fn claude_permission_decision_impl(
    sidecar: &Mutex<Option<SidecarHandle>>,
    decision: PermissionDecisionArgs,
) -> Result<(), String> {
    let mut guard = sidecar.lock().await;
    match guard.as_mut() {
        None => Err("sidecar not started".to_string()),
        Some(handle) => {
            let state = handle.state().lock().await.clone();
            match state {
                SidecarState::Ready => handle.permission_decision(decision).await,
                SidecarState::Crashed(msg) => Err(format!("sidecar crashed: {}", msg)),
                SidecarState::NotStarted => Err("sidecar not started".to_string()),
                SidecarState::Starting => Err("sidecar not ready (still starting)".to_string()),
            }
        }
    }
}

/// Resolve the writable workspace directory for the Claude sidecar sandbox.
///
/// Resolution chain (first non-empty parent wins):
/// 1. `message_context.current_file` parent — the dir of the currently-open editor file.
/// 2. `initial_file` parent — the dir of the file passed on the CLI at startup.
/// 3. `fallback_cwd` — typically `std::env::current_dir()`.
///
/// Paths without a parent component (e.g. bare filenames like `"main.ri"`) and
/// empty strings both fall through to the next option.
pub fn resolve_workspace_dir(
    message_context: Option<&MessageContext>,
    initial_file: Option<&std::path::Path>,
    fallback_cwd: &std::path::Path,
) -> std::path::PathBuf {
    // 1. current_file from message context
    if let Some(ctx) = message_context
        && let Some(ref cf) = ctx.current_file
        && !cf.is_empty()
    {
        let p = std::path::Path::new(cf);
        // parent is empty ("") for bare filenames — filter that out
        if let Some(parent) = p.parent()
            && parent != std::path::Path::new("")
        {
            return parent.to_path_buf();
        }
    }

    // 2. initial_file
    if let Some(init) = initial_file
        && let Some(parent) = init.parent()
        && parent != std::path::Path::new("")
    {
        return parent.to_path_buf();
    }

    // 3. fallback
    fallback_cwd.to_path_buf()
}

/// Build the environment variable list to inject into the spawned sidecar process.
///
/// Always includes `REIFY_WORKSPACE` (the landlock-writable workspace dir).
/// Includes `REIFY_LANDLOCK_EXEC` only when `landlock_exec` is `Some`.
/// Ordering is deterministic: workspace first, then landlock_exec if present.
pub fn compute_sidecar_env(
    workspace: &std::path::Path,
    landlock_exec: Option<&std::path::Path>,
) -> Vec<(String, String)> {
    let mut envs = vec![(
        "REIFY_WORKSPACE".to_string(),
        workspace.to_string_lossy().into_owned(),
    )];
    if let Some(le) = landlock_exec {
        envs.push((
            "REIFY_LANDLOCK_EXEC".to_string(),
            le.to_string_lossy().into_owned(),
        ));
    }
    envs
}

/// Apply workspace + landlock env vars to a [`tokio::process::Command`].
///
/// Calls [`compute_sidecar_env`] and sets each key-value pair on the command
/// via [`tokio::process::Command::env`].
pub fn apply_sidecar_env(
    cmd: &mut tokio::process::Command,
    workspace: &std::path::Path,
    landlock_exec: Option<&std::path::Path>,
) {
    for (k, v) in compute_sidecar_env(workspace, landlock_exec) {
        cmd.env(k, v);
    }
}

/// Spawn the Claude sidecar process and return a [`SidecarHandle`] in `Starting` state.
///
/// Extracts stdin/stdout from the child, wraps stdout in a [`BufReader`], and
/// wires up event emission and MCP interception via [`SidecarHandle::from_parts_with_mcp`].
/// The returned handle starts in [`SidecarState::Starting`]; the caller typically uses
/// [`ensure_sidecar_ready`] to store it in the shared sidecar slot and await readiness,
/// or can call [`SidecarHandle::wait_ready`] manually.
///
/// `workspace` is the landlock-writable directory injected as `REIFY_WORKSPACE`.
/// `landlock_exec` is the path to the vendored `landlock_exec.py` helper,
/// injected as `REIFY_LANDLOCK_EXEC` when `Some`.
///
/// Returns `Err` if the process cannot be spawned or if stdin/stdout are unavailable.
pub async fn spawn_sidecar_impl<F>(
    path: &Path,
    engine: Arc<std::sync::Mutex<crate::engine::EngineSession>>,
    event_emitter: F,
    selection: Arc<std::sync::RwLock<reify_mcp::SelectionInfo>>,
    workspace: &std::path::Path,
    landlock_exec: Option<&std::path::Path>,
) -> Result<SidecarHandle, String>
where
    F: Fn(String, Value) + Send + Sync + 'static,
{
    let mut command = tokio::process::Command::new(path);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());
    apply_sidecar_env(&mut command, workspace, landlock_exec);
    let mut proc = command
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
    let sidecar_state = Arc::new(Mutex::new(SidecarState::Starting));
    let mut handle = SidecarHandle::from_parts_with_mcp(
        stdin,
        reader,
        sidecar_state,
        engine,
        event_emitter,
        selection,
    );
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

/// Ensure the sidecar is spawned and ready to accept messages.
///
/// **Fast path**: if the slot contains a handle in `SidecarState::Ready`,
/// returns `Ok(())` immediately. If the handle exists but is **not** ready
/// (e.g. `Crashed` or `Starting`), the stale handle is killed and the sidecar
/// is re-spawned — enabling automatic recovery without an explicit
/// `shutdown_sidecar` call.
///
/// **Spawn path**: calls `spawn_fn` **outside** the sidecar lock so that
/// `shutdown_sidecar` can run concurrently during slow OS process creation
/// (the previous implementation held the lock for the entire spawn duration,
/// which blocked the `CloseRequested` shutdown handler). The `Notified` future
/// is subscribed immediately after spawn (before re-locking) so the reader
/// task cannot fire `notify_waiters()` between spawn and subscription.
///
/// **Concurrent-caller guard**: after re-locking to store the handle, if a
/// concurrent caller already stored a Ready handle, our redundant handle is
/// killed and `Ok(())` is returned immediately.
///
/// **Resource safety**: all eviction and error-cleanup paths call
/// `handle.kill().await` (not just `*guard = None`) so the OS child process
/// is terminated and the reader task is aborted on every code path.
///
/// **Error recovery**: on any error after the handle has been stored (timeout,
/// crash, or unexpected state), the handle is killed and the slot is cleared
/// so the next call can re-enter the spawn path.
///
/// Returns `Err` if:
/// - `spawn_fn` returns an error
/// - The ready notification does not fire within `ready_timeout`
/// - The sidecar crashes before becoming ready (notification fires due to crash)
pub async fn ensure_sidecar_ready<F, Fut>(
    sidecar: &Mutex<Option<SidecarHandle>>,
    spawn_fn: F,
    ready_timeout: Duration,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<SidecarHandle, String>>,
{
    // Phase 1: check existing handle; kill stale ones; release lock before spawning.
    // The lock is released here so `shutdown_sidecar` can proceed concurrently
    // while Phase 2 (spawn_fn) is in progress.
    {
        let mut guard = sidecar.lock().await;
        if let Some(handle) = guard.as_ref() {
            let state_val = handle.state().lock().await.clone();
            if matches!(state_val, SidecarState::Ready) {
                return Ok(());
            }
            // Not ready (Crashed, Starting, …) — kill the stale handle to release
            // OS resources and abort the reader task, then fall through to re-spawn.
            if let Some(mut h) = guard.take() {
                h.kill().await;
            }
        }
        // Lock released here.
    }

    // Phase 2: spawn OUTSIDE the lock so `shutdown_sidecar` can acquire the lock
    // concurrently during slow OS process creation.
    let mut handle = spawn_fn().await?;
    let notify_arc = Arc::clone(handle.ready_notify());
    let spawned_state = Arc::clone(handle.state());

    // Subscribe to the ready notification BEFORE re-locking.  On a multi-thread
    // executor the reader task can call `notify_waiters()` immediately after
    // spawn_fn returns and before Phase 3 re-acquires the lock.
    //
    // IMPORTANT: `Notified::notified()` does NOT register the waiter until the
    // future is first polled. We must pin + enable() eagerly so that any
    // `notify_waiters()` call during the Phase 3 await points (sidecar.lock,
    // state_arc.lock, h.kill) is captured. Without enable(), a multi-thread
    // executor can interleave:
    //   1. notified() created — waiter not registered
    //   2. Phase 3 await points execute (lock, state check, kill)
    //   3. reader task sets Ready + calls notify_waiters() — lost
    //   4. notified polled at Phase 4 — registers waiter, but notification already fired
    let mut notified = std::pin::pin!(notify_arc.notified());
    notified.as_mut().enable();

    // Phase 3: re-lock, double-check for concurrent callers, then store handle.
    {
        let mut guard = sidecar.lock().await;

        // Clone the state Arc from any existing handle to avoid holding a borrow
        // on `guard` across the await point below.
        let existing_state_arc = guard.as_ref().map(|h| Arc::clone(h.state()));
        if let Some(state_arc) = existing_state_arc {
            let existing_val = state_arc.lock().await.clone();
            if matches!(existing_val, SidecarState::Ready) {
                // A concurrent caller already spawned and is ready — our handle
                // is redundant.  Kill it to prevent an orphan process and return Ok.
                drop(guard); // Release lock before async kill.
                handle.kill().await;
                return Ok(());
            }
            // Concurrent non-ready handle: evict it (fall through to take+kill below).
        }

        // Evict any existing non-ready handle (from a concurrent caller or a
        // stale handle placed between Phase 1 and Phase 3).
        if let Some(mut h) = guard.take() {
            h.kill().await;
        }
        *guard = Some(handle);
        // Guard dropped here — lock released.
    }

    // Re-check: handles the *pre-creation* race window — Ready set during
    // spawn_fn's internal await points, BEFORE `notified()` is created at
    // the `spawn_fn().await` call in Phase 2.  In that window no `Notified`
    // future exists, so the `notify_waiters()` call is irretrievably lost.
    //
    // This is distinct from the *post-creation* window (notified() created
    // but waiter not yet registered), which is handled by the `pin! +
    // enable()` call below Phase 2 eagerly registering the waiter.
    //
    // Together, enable() + re-check provide defense-in-depth against both
    // windows.  This mirrors the `pin! + enable()` sequence in `wait_ready`.
    if matches!(*spawned_state.lock().await, SidecarState::Ready) {
        return Ok(());
    }

    // Phase 4: wait for the ready notification with timeout.
    let wait_result = tokio::time::timeout(ready_timeout, notified)
        .await
        .map_err(|_| {
            format!(
                "Sidecar did not become ready within {}ms",
                ready_timeout.as_millis()
            )
        });

    if let Err(e) = wait_result {
        // Timeout: compare-and-kill+clear — only kill/remove the handle if it
        // is still the one *this* call placed.  A concurrent call may have
        // already replaced it with a new, healthy handle; blindly clearing
        // would destroy that handle and orphan the old process.
        let mut guard = sidecar.lock().await;
        if guard
            .as_ref()
            .is_some_and(|h| Arc::ptr_eq(h.ready_notify(), &notify_arc))
            && let Some(mut h) = guard.take()
        {
            h.kill().await;
        }
        return Err(e);
    }

    // Phase 5: check state after notification — the notify may have been
    // triggered by a crash rather than the Ready message.
    let state_val = spawned_state.lock().await.clone();
    match state_val {
        SidecarState::Ready => Ok(()),
        SidecarState::Crashed(msg) => {
            // Crash: compare-and-kill+clear — only kill/remove if our handle
            // is still in the slot (see timeout branch for rationale).
            let mut guard = sidecar.lock().await;
            if guard
                .as_ref()
                .is_some_and(|h| Arc::ptr_eq(h.ready_notify(), &notify_arc))
                && let Some(mut h) = guard.take()
            {
                h.kill().await;
            }
            Err(format!("sidecar crashed: {}", msg))
        }
        other => {
            // Unexpected state: compare-and-kill+clear.
            let mut guard = sidecar.lock().await;
            if guard
                .as_ref()
                .is_some_and(|h| Arc::ptr_eq(h.ready_notify(), &notify_arc))
                && let Some(mut h) = guard.take()
            {
                h.kill().await;
            }
            Err(format!("sidecar not ready after notification: {:?}", other))
        }
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
        OutboundMessage::ToolCall {
            id,
            tool_name,
            tool_input,
            tool_use_id,
        } => (
            "claude-tool-call".to_string(),
            serde_json::json!({ "id": id, "tool_name": tool_name, "tool_input": tool_input, "tool_use_id": tool_use_id }),
        ),
        OutboundMessage::ToolResult {
            id,
            tool_name,
            result,
        } => (
            "claude-tool-result".to_string(),
            serde_json::json!({ "id": id, "tool_name": tool_name, "result": result }),
        ),
        OutboundMessage::Done { id } => {
            ("claude-done".to_string(), serde_json::json!({ "id": id }))
        }
        OutboundMessage::ErrorMessage { id, message } => (
            "claude-error".to_string(),
            serde_json::json!({ "id": id, "message": message }),
        ),
        OutboundMessage::Notice { id, code, message } => (
            "claude-notice".to_string(),
            serde_json::json!({ "id": id, "code": code, "message": message }),
        ),
        OutboundMessage::Ready => ("claude-ready".to_string(), serde_json::json!({})),
        OutboundMessage::PermissionRequest {
            id,
            request_id,
            tool_name,
            tool_input,
        } => (
            "claude-permission-request".to_string(),
            serde_json::json!({
                "id": id,
                "request_id": request_id,
                "tool_name": tool_name,
                "tool_input": tool_input,
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;
    use tokio::process::Command;

    /// Verify that dropping a SidecarHandle kills the OS child process and
    /// aborts the reader task. Without a custom Drop impl, tokio's
    /// Child::drop does NOT send any signal (kill_on_drop defaults to false),
    /// so the spawned `sleep` process would continue running.
    #[tokio::test]
    async fn test_drop_kills_child_process() {
        let mut child = Command::new("sleep")
            .arg("100")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("failed to spawn sleep");

        let pid = child.id().expect("child must have pid");

        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let reader = BufReader::new(stdout);

        let state = Arc::new(Mutex::new(SidecarState::NotStarted));
        let mut handle = SidecarHandle::from_parts(stdin, reader, state);
        handle.set_child(child);

        // Drop the handle — should kill the child process and abort reader.
        drop(handle);

        // Give the OS a moment to reap the process.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify the process is dead (no longer consuming resources).
        //
        // On Linux, kill -0 returns success for zombie processes (killed but not yet
        // reaped by waitpid). Under heavy parallel test load the tokio SIGCHLD reaper
        // may not drain within 100 ms. We therefore accept "zombie" as equivalent to
        // "killed": the process DID receive SIGKILL (it is not running), it just has
        // not been reaped from the process table yet.
        let probe = std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .expect("kill -0 probe failed");

        let process_is_dead_or_zombie = if !probe.success() {
            // Process table entry is gone — truly dead.
            true
        } else {
            // kill -0 succeeded → process still in process table.
            // Distinguish zombie (State: Z) from alive (State: S/R/...).
            std::fs::read_to_string(format!("/proc/{}/status", pid))
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("State:"))
                        .map(|l| l.contains('Z'))
                })
                .unwrap_or(true) // can't read status → process entry gone
        };

        assert!(
            process_is_dead_or_zombie,
            "sleep process (pid {}) should have been killed by Drop but is still running",
            pid
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn compute_sidecar_env_includes_debug_port_and_workspace() {
        let workspace = std::path::Path::new("/tmp/test-workspace");
        let env = compute_sidecar_env(workspace, None);

        // Regression guard: REIFY_WORKSPACE must still be present.
        assert!(
            env.iter().any(|(k, _)| k == "REIFY_WORKSPACE"),
            "REIFY_WORKSPACE must be present in compute_sidecar_env output"
        );

        // New: REIFY_DEBUG_PORT must be present and consistent with the resolver.
        let port_entry = env.iter().find(|(k, _)| k == "REIFY_DEBUG_PORT");
        assert!(
            port_entry.is_some(),
            "REIFY_DEBUG_PORT must be present in compute_sidecar_env output"
        );
        let expected = crate::debug_server::resolve_debug_port().to_string();
        assert_eq!(
            port_entry.unwrap().1,
            expected,
            "REIFY_DEBUG_PORT value must match resolve_debug_port()"
        );
    }
}
