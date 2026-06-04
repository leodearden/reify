// DebugBridge — manages request/response round-trip between HTTP server and JS frontend.
//
// The JS debug bridge listens for `debug-request` Tauri events, processes the command,
// and sends results back via the `debug_response` Tauri command. This module manages
// the pending oneshot channels that connect the two sides.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime, Wry};

pub struct DebugBridge<R: Runtime = Wry> {
    pending: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<String>>>,
    next_id: AtomicU64,
    app: AppHandle<R>,
}

impl<R: Runtime> DebugBridge<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();

        self.pending
            .lock()
            .map_err(|e| format!("pending lock poisoned: {e}"))?
            .insert(id, tx);

        self.app
            .emit(
                "debug-request",
                serde_json::json!({
                    "id": id,
                    "command": command,
                    "params": params,
                }),
            )
            .map_err(|e| format!("failed to emit debug-request: {e}"))?;

        let result = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| {
                // Clean up the pending entry on timeout
                self.pending.lock().ok().map(|mut m| m.remove(&id));
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
        let tx = self
            .pending
            .lock()
            .map_err(|e| format!("pending lock poisoned: {e}"))?
            .remove(&id)
            .ok_or_else(|| format!("no pending request with id {id}"))?;

        tx.send(result)
            .map_err(|_| format!("receiver for id {id} already dropped"))
    }
}
