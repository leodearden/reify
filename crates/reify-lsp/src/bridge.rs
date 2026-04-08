//! In-process LSP bridge for embedding the language server without I/O streams.
//!
//! [`InProcessLsp`] wraps the [`ReifyLanguageServer`] and provides a
//! `handle_request(method, params_json)` API that dispatches to the
//! appropriate [`LanguageServer`] trait methods directly, avoiding
//! JSON-RPC serialization overhead.

use std::sync::Arc;

use serde_json::Value;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use crate::server::{NoOpSink, NotificationSink, ReifyLanguageServer};

/// An in-process LSP server that can be called directly without I/O streams.
///
/// The `server` field holds a cloned `ReifyLanguageServer` (all `Arc`-wrapped
/// internals) which is `Send + Sync`. The `LspService` and `ClientSocket` are
/// kept alive in `_keepalive` behind a `std::sync::Mutex` to satisfy `Sync`,
/// since `LspService` itself is only `Send`.
pub struct InProcessLsp {
    server: ReifyLanguageServer,
    /// Keep `LspService` and `ClientSocket` alive so the `Client` sender
    /// remains connected, but never access them after construction.
    _keepalive: std::sync::Mutex<(LspService<ReifyLanguageServer>, tower_lsp::ClientSocket)>,
}

impl InProcessLsp {
    /// Create a new in-process LSP server with a [`NoOpSink`].
    pub fn new() -> Self {
        Self::with_sink(Arc::new(NoOpSink))
    }

    /// Create a new in-process LSP server with a custom notification sink.
    pub fn with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        let (service, socket) =
            LspService::new(|client| ReifyLanguageServer::with_sink(client, sink.clone()));
        let server = service.inner().clone();
        Self {
            server,
            _keepalive: std::sync::Mutex::new((service, socket)),
        }
    }

    /// Retrieve the last published diagnostics for a given URI.
    ///
    /// Returns a JSON array of LSP Diagnostic objects. The diagnostics
    /// are captured by the server after each didOpen/didChange.
    ///
    /// This method is async because the server state is guarded by a
    /// `tokio::sync::RwLock`. Using `.read().await` ensures we properly
    /// wait for any concurrent write lock to release, rather than
    /// silently returning empty diagnostics via `try_read()`.
    pub async fn get_diagnostics(&self, uri: &str) -> Vec<Value> {
        let state = self.server.state();

        let guard = state.read().await;

        let url = match Url::parse(uri) {
            Ok(u) => u,
            Err(_) => return vec![],
        };

        match guard.last_diagnostics_for(&url) {
            Some(diags) => diags
                .iter()
                .filter_map(|d| serde_json::to_value(d).ok())
                .collect(),
            None => vec![],
        }
    }

    /// Handle an LSP request or notification by method name.
    ///
    /// # Return value contract
    ///
    /// - **`Ok(Value)`** — A JSON-serialized response payload for successful *requests*:
    ///   `initialize`, `textDocument/completion`, `textDocument/hover`,
    ///   `textDocument/definition`.
    /// - **`Ok(Value::Null)`** — For successfully processed *notifications* and `shutdown`:
    ///   `initialized`, `textDocument/didOpen`, `textDocument/didChange`,
    ///   `textDocument/didClose`, `shutdown`.
    /// - **`Err(String)`** — In any of the following cases:
    ///   - **Unsupported method**: the `method` argument names a method not handled
    ///     by this bridge.
    ///   - **Deserialization failure**: `params` cannot be parsed into the expected
    ///     LSP parameter type — this applies to *both* requests and notifications.
    ///     Notifications do not silently succeed on bad params; the error is
    ///     propagated to the caller.
    ///   - **Server error**: the underlying `LanguageServer` implementation returns
    ///     an error (only possible for request methods, not notifications).
    ///   - **Serialization failure**: the server response cannot be serialized to
    ///     `serde_json::Value` (should not occur in practice with well-formed
    ///     LSP types).
    ///
    /// # Breaking change
    ///
    /// The `initialize` method previously tolerated malformed `InitializeParams`
    /// by falling back to a default value (`unwrap_or_default`). It now performs
    /// **strict deserialization** and returns `Err` if `params` cannot be
    /// deserialized into [`tower_lsp::lsp_types::InitializeParams`]. Callers
    /// that previously relied on the silent default must now supply valid params.
    pub async fn handle_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let server = &self.server;

        match method {
            "initialize" => {
                let p: InitializeParams = serde_json::from_value(params)
                    .map_err(|e| format!("initialize params error: {e}"))?;
                let result = server
                    .initialize(p)
                    .await
                    .map_err(|e| format!("initialize error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "initialized" => {
                // Some LSP clients (e.g. Neovim, certain VS Code extensions) send
                // `params: null` for notifications with no meaningful payload.
                // `InitializedParams` is an empty struct (`{}` in the LSP spec),
                // so null is semantically equivalent to an empty object.
                let params = if params.is_null() {
                    serde_json::json!({})
                } else {
                    params
                };
                let p: InitializedParams = serde_json::from_value(params)
                    .map_err(|e| format!("initialized params error: {e}"))?;
                server.initialized(p).await;
                Ok(Value::Null)
            }
            "textDocument/didOpen" => {
                let p: DidOpenTextDocumentParams = serde_json::from_value(params)
                    .map_err(|e| format!("didOpen params error: {e}"))?;
                server.did_open(p).await;
                Ok(Value::Null)
            }
            "textDocument/didChange" => {
                let p: DidChangeTextDocumentParams = serde_json::from_value(params)
                    .map_err(|e| format!("didChange params error: {e}"))?;
                server.did_change(p).await;
                Ok(Value::Null)
            }
            "textDocument/didClose" => {
                let p: DidCloseTextDocumentParams = serde_json::from_value(params)
                    .map_err(|e| format!("didClose params error: {e}"))?;
                server.did_close(p).await;
                Ok(Value::Null)
            }
            "textDocument/completion" => {
                let p: CompletionParams = serde_json::from_value(params)
                    .map_err(|e| format!("completion params error: {e}"))?;
                let result = server
                    .completion(p)
                    .await
                    .map_err(|e| format!("completion error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/hover" => {
                let p: HoverParams = serde_json::from_value(params)
                    .map_err(|e| format!("hover params error: {e}"))?;
                let result = server
                    .hover(p)
                    .await
                    .map_err(|e| format!("hover error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/definition" => {
                let p: GotoDefinitionParams = serde_json::from_value(params)
                    .map_err(|e| format!("definition params error: {e}"))?;
                let result = server
                    .goto_definition(p)
                    .await
                    .map_err(|e| format!("definition error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "shutdown" => {
                server
                    .shutdown()
                    .await
                    .map_err(|e| format!("shutdown error: {e}"))?;
                Ok(Value::Null)
            }
            other => Err(format!("unsupported LSP method: {other}")),
        }
    }
}

impl Default for InProcessLsp {
    fn default() -> Self {
        Self::new()
    }
}

/// Error-message prefix constants for [`InProcessLsp::handle_request`].
///
/// These constants are the leading substrings that appear in `Err(String)`
/// values returned by `handle_request` when a method receives malformed
/// params.  Exposing them as named constants lets tests import and assert
/// against the real values rather than duplicating hardcoded string literals.
///
/// If a prefix changes in the implementation, updating the constant here
/// causes the test file's `use … error_prefix::*` import to reflect the
/// change automatically — mismatches become compile errors or immediate
/// test failures rather than silent runtime drift.
///
/// Only the six prefixes asserted by existing tests are exported.  Additional
/// constants can be added here when new tests need them.
pub mod error_prefix {
    /// Prefix for deserialization failures on `initialize` params.
    pub const INITIALIZE_PARAMS: &str = "initialize params error";

    /// Prefix for deserialization failures on `initialized` params.
    pub const INITIALIZED_PARAMS: &str = "initialized params error";

    /// Prefix for deserialization failures on `textDocument/didOpen` params.
    pub const DID_OPEN_PARAMS: &str = "didOpen params error";

    /// Prefix for deserialization failures on `textDocument/didChange` params.
    pub const DID_CHANGE_PARAMS: &str = "didChange params error";

    /// Prefix for deserialization failures on `textDocument/didClose` params.
    pub const DID_CLOSE_PARAMS: &str = "didClose params error";

    /// Prefix used when an unrecognised LSP method name is requested.
    ///
    /// The full error message is `"{UNSUPPORTED_METHOD} {method_name}"`.
    pub const UNSUPPORTED_METHOD: &str = "unsupported LSP method:";
}
