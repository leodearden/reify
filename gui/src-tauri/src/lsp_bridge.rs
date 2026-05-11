//! Tauri-side LSP bridge wrapping the in-process LSP server.
//!
//! [`LspBridge`] owns an [`InProcessLsp`] and provides helper functions
//! that can be used by Tauri command handlers without requiring the Tauri
//! runtime (for testability).

use std::sync::Arc;

use reify_lsp::bridge::InProcessLsp;
use reify_lsp::server::NotificationSink;

/// Tauri-side wrapper around the in-process LSP server.
///
/// Holds the [`InProcessLsp`] instance and provides an interface
/// suitable for Tauri command dispatch.
pub struct LspBridge {
    lsp: InProcessLsp,
}

impl LspBridge {
    /// Create a new LSP bridge with a fresh in-process LSP server.
    pub fn new() -> Self {
        Self {
            lsp: InProcessLsp::new(),
        }
    }

    /// Create a new LSP bridge with a custom notification sink.
    pub fn with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        Self {
            lsp: InProcessLsp::with_sink(sink),
        }
    }

    /// Retrieve the last published diagnostics for a given URI.
    ///
    /// Returns a `Vec<serde_json::Value>` suitable for serialization
    /// as a Tauri event payload.
    pub async fn get_diagnostics(&self, uri: &str) -> Vec<serde_json::Value> {
        self.lsp.get_diagnostics(uri).await
    }
}

impl Default for LspBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Implementation of the `lsp_request` Tauri command, separated for testability.
///
/// Dispatches the given LSP method with JSON params through the bridge
/// and returns the JSON-serialized response.
pub async fn lsp_request_impl(
    bridge: &LspBridge,
    method: &str,
    params: String,
) -> Result<String, String> {
    let params_value: serde_json::Value =
        serde_json::from_str(&params).map_err(|e| format!("invalid JSON params: {e}"))?;

    let result = bridge.lsp.handle_request(method, params_value).await?;

    serde_json::to_string(&result).map_err(|e| format!("serialize error: {e}"))
}
