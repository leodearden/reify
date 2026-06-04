// DebugBridge — manages request/response round-trip between HTTP server and JS frontend.
//
// The JS debug bridge listens for `debug-request` Tauri events, processes the command,
// and sends results back via the `debug_response` Tauri command. This module manages
// the pending oneshot channels that connect the two sides.
//
// Structure:
//   DebugTransport — pure Rust, no Tauri dependency; holds the id-keyed oneshot pending
//                    map.  Available in all compilation modes including test-only builds.
//   DebugBridge    — thin wrapper that adds the Tauri AppHandle and event emission.
//                    Compiled only when the `gui` feature is active.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

/// Core transport state: id-keyed oneshot channels used to match a
/// `debug-request` emission with its `debug_response` reply.
///
/// No Tauri dependency — available in all compilation modes so unit tests can
/// drive the request/resolve round-trip without a live or mock Tauri runtime.
/// [`DebugBridge`] wraps this and adds the AppHandle emit step; it is only
/// compiled under `feature = "gui"`.
pub struct DebugTransport {
    pending: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<String>>>,
    next_id: AtomicU64,
}

impl DebugTransport {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Allocate a new request id and insert the sender into `pending`.
    /// Returns `(id, receiver)` — the caller awaits the receiver to get the
    /// response, and the id is what the resolver passes back via resolve().
    pub(crate) fn create_request(
        &self,
    ) -> Result<(u64, tokio::sync::oneshot::Receiver<String>), String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending
            .lock()
            .map_err(|e| format!("pending lock poisoned: {e}"))?
            .insert(id, tx);
        Ok((id, rx))
    }

    /// Route a response from the JS bridge to the waiting receiver.
    pub fn resolve(&self, id: u64, result: String) -> Result<(), String> {
        let tx = self
            .pending
            .lock()
            .map_err(|e| format!("pending lock poisoned: {e}"))?
            .remove(&id)
            .ok_or_else(|| format!("no pending request with id {id}"))?;

        tx.send(result)
            .map_err(|_| format!("receiver for id {id} already dropped"))
    }

    /// Remove a pending entry on request timeout or emit failure (best-effort;
    /// ignores lock poisoning).
    pub(crate) fn remove_pending(&self, id: u64) {
        self.pending.lock().ok().map(|mut m| m.remove(&id));
    }
}

// ── Tauri-dependent wrapper ─────────────────────────────────────────────────
// Compiled only when the `gui` feature is active (which activates the
// optional `tauri` dependency).

#[cfg(feature = "gui")]
use std::time::Duration;
#[cfg(feature = "gui")]
use tauri::{AppHandle, Emitter, Runtime, Wry};

/// Tauri-backed wrapper around [`DebugTransport`].
///
/// Emits `debug-request` events via the Tauri AppHandle and waits on a oneshot
/// channel for the JS bridge to respond via the `debug_response` Tauri command.
/// Generic over `R: Runtime` (default `Wry`) so unit tests can use
/// `MockRuntime` if needed — though tests are expected to drive
/// [`DebugTransport`] directly to avoid depending on the tauri `test` feature.
#[cfg(feature = "gui")]
pub struct DebugBridge<R: Runtime = Wry> {
    transport: DebugTransport,
    app: AppHandle<R>,
}

#[cfg(feature = "gui")]
impl<R: Runtime> DebugBridge<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            transport: DebugTransport::new(),
            app,
        }
    }

    /// Send a command to the JS debug bridge and wait for the response,
    /// with a caller-specified timeout.
    ///
    /// Emits a `debug-request` event with `{ id, command, params }`, then waits
    /// on a oneshot channel for up to `timeout` for the JS bridge to respond via
    /// the `debug_response` Tauri command.
    pub async fn query_frontend_with_timeout(
        &self,
        command: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let (id, rx) = self.transport.create_request()?;

        self.app
            .emit(
                "debug-request",
                serde_json::json!({
                    "id": id,
                    "command": command,
                    "params": params,
                }),
            )
            .map_err(|e| {
                self.transport.remove_pending(id);
                format!("failed to emit debug-request: {e}")
            })?;

        let result = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| {
                // Clean up the pending entry on timeout
                self.transport.remove_pending(id);
                format!(
                    "debug-request '{command}' timed out after {}ms",
                    timeout.as_millis()
                )
            })?
            .map_err(|_| format!("debug-request '{command}' channel dropped"))?;

        serde_json::from_str(&result).map_err(|e| format!("invalid JSON from JS bridge: {e}"))
    }

    /// Send a command to the JS debug bridge and wait for the response.
    ///
    /// Emits a `debug-request` event with `{ id, command, params }`, then waits
    /// on a oneshot channel for up to 5 seconds for the JS bridge to respond via
    /// the `debug_response` Tauri command.
    pub async fn query_frontend(&self, command: &str, params: Value) -> Result<Value, String> {
        self.query_frontend_with_timeout(command, params, Duration::from_secs(5))
            .await
    }

    /// Route a response from the JS debug bridge to the waiting oneshot channel.
    pub fn resolve(&self, id: u64, result: String) -> Result<(), String> {
        self.transport.resolve(id, result)
    }
}
