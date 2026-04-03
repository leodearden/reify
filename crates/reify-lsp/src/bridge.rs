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
    /// For requests (initialize, completion, hover, definition, shutdown),
    /// returns the JSON-serialized response. For notifications (didOpen,
    /// didChange, didClose, initialized), returns `Ok(Value::Null)`.
    pub async fn handle_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let server = &self.server;

        match method {
            "initialize" => {
                let p: InitializeParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("malformed InitializeParams, using defaults: {e}");
                        InitializeParams::default()
                    }
                };
                let result = server
                    .initialize(p)
                    .await
                    .map_err(|e| format!("initialize error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "initialized" => {
                server.initialized(InitializedParams {}).await;
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
