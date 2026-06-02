use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::diagnostics::EvalState;
use crate::document::DocumentStore;

/// Trait for emitting server-initiated notifications to the frontend.
///
/// Replaces direct use of `tower_lsp::Client` for notifications, so the
/// same `ReifyLanguageServer` can work with:
/// - `NoOpSink` (tests and backward compatibility)
/// - `ClientSink` (stdio/TCP mode via tower-lsp)
/// - `TauriNotificationSink` (in-process Tauri mode)
pub trait NotificationSink: Send + Sync {
    /// Publish diagnostics for the given document.
    fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>, version: Option<i32>);
}

/// A no-op sink that discards all notifications.
pub struct NoOpSink;

impl NotificationSink for NoOpSink {
    fn publish_diagnostics(&self, _uri: Url, _diagnostics: Vec<Diagnostic>, _version: Option<i32>) {
    }
}

/// A sink that wraps the tower-lsp [`Client`] for stdio/TCP mode.
///
/// Since [`NotificationSink`] methods are synchronous but `Client.publish_diagnostics()`
/// is async, this implementation spawns a fire-and-forget tokio task for each call.
pub struct ClientSink {
    client: Client,
}

impl ClientSink {
    /// Create a new `ClientSink` wrapping the given tower-lsp `Client`.
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl NotificationSink for ClientSink {
    fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>, version: Option<i32>) {
        let client = self.client.clone();
        tokio::spawn(async move {
            client.publish_diagnostics(uri, diagnostics, version).await;
        });
    }
}

/// Internal state shared across handler calls.
///
/// Contains document storage and captured diagnostics. The RwLock guards
/// only these lightweight fields — eval_state lives separately on
/// ReifyLanguageServer to avoid holding the RwLock during expensive
/// evaluation.
pub struct ServerState {
    pub documents: DocumentStore,
    /// Diagnostics last published for each URI (for test verification).
    last_published_diagnostics: HashMap<Url, Vec<Diagnostic>>,
    /// Workspace root path, populated from `InitializeParams.root_uri`.
    pub workspace_root: Option<PathBuf>,
    /// Explicit stdlib path from `initializationOptions.stdlibPath`.
    /// When `None`, goto_definition falls back to the dev-mode heuristic.
    pub stdlib_path: Option<PathBuf>,
}

impl ServerState {
    /// Retrieve the last published diagnostics for a given URI.
    pub fn last_diagnostics_for(&self, uri: &Url) -> Option<&Vec<Diagnostic>> {
        self.last_published_diagnostics.get(uri)
    }
}

/// The Reify language server.
#[derive(Clone)]
pub struct ReifyLanguageServer {
    /// Retained for tower-lsp infrastructure; notifications now go through `sink`.
    #[allow(dead_code)]
    client: Client,
    state: Arc<RwLock<ServerState>>,
    /// Evaluation state lives outside the RwLock so eval can run without
    /// blocking concurrent LSP requests that only need document state.
    /// Wrapped in Mutex because Engine internals (OpaqueState) are Send but not Sync.
    eval_state: Arc<Mutex<EvalState>>,
    /// Notification sink for server-initiated messages (diagnostics, etc.).
    sink: Arc<dyn NotificationSink>,
}

impl ReifyLanguageServer {
    /// Create a new server with a [`NoOpSink`] (backward compatibility).
    pub fn new(client: Client) -> Self {
        Self::with_sink(client, Arc::new(NoOpSink))
    }

    /// Create a new server with a custom notification sink.
    pub fn with_sink(client: Client, sink: Arc<dyn NotificationSink>) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState {
                documents: DocumentStore::new(),
                last_published_diagnostics: HashMap::new(),
                workspace_root: None,
                stdlib_path: None,
            })),
            eval_state: Arc::new(Mutex::new(EvalState::new())),
            sink,
        }
    }

    /// Access server state (for testing and embedding).
    pub fn state(&self) -> &Arc<RwLock<ServerState>> {
        &self.state
    }

    /// Access eval_state (for testing, e.g. poison recovery tests).
    #[cfg(test)]
    pub(crate) fn eval_state(&self) -> &Arc<Mutex<EvalState>> {
        &self.eval_state
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ReifyLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store workspace root from root_uri (preferred) or root_path (legacy).
        let workspace_root = params
            .root_uri
            .as_ref()
            .and_then(|uri| uri.to_file_path().ok());
        // Parse optional stdlibPath from initializationOptions.
        let stdlib_path = params
            .initialization_options
            .as_ref()
            .and_then(|opts| opts.get("stdlibPath"))
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        {
            let mut state = self.state.write().await;
            state.workspace_root = workspace_root;
            state.stdlib_path = stdlib_path;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions::default()),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {}

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        // Brief write lock: store the document
        {
            let mut state = self.state.write().await;
            state.documents.open(uri.clone(), text.clone(), version);
        }

        // Eval runs outside the RwLock, using only the eval_state Mutex.
        // Recovers from poisoned lock (e.g., prior panic during eval).
        let diagnostics = {
            let mut eval_state = self.eval_state.lock().unwrap_or_else(|e| {
                eprintln!("eval_state lock poisoned, recovering");
                e.into_inner()
            });
            let result =
                crate::diagnostics::compute_diagnostics_with_state(&mut eval_state, &text, &uri);
            result.diagnostics
        };

        // Brief write lock: capture diagnostics
        {
            let mut state = self.state.write().await;
            state
                .last_published_diagnostics
                .insert(uri.clone(), diagnostics.clone());
        }

        self.sink
            .publish_diagnostics(uri, diagnostics, Some(version));
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        // Full sync: take the last content change (there should be exactly one)
        let text = match params.content_changes.into_iter().last() {
            Some(change) => change.text,
            None => return,
        };

        // Brief write lock: update the document
        {
            let mut state = self.state.write().await;
            if !state.documents.update(&uri, text.clone(), version) {
                eprintln!("[reify-lsp] didChange for unknown URI: {}", uri);
            }
        }

        // Eval runs outside the RwLock, using only the eval_state Mutex.
        // Recovers from poisoned lock (e.g., prior panic during eval).
        let diagnostics = {
            let mut eval_state = self.eval_state.lock().unwrap_or_else(|e| {
                eprintln!("eval_state lock poisoned, recovering");
                e.into_inner()
            });
            let result =
                crate::diagnostics::compute_diagnostics_with_state(&mut eval_state, &text, &uri);
            result.diagnostics
        };

        // Brief write lock: capture diagnostics
        {
            let mut state = self.state.write().await;
            state
                .last_published_diagnostics
                .insert(uri.clone(), diagnostics.clone());
        }

        self.sink
            .publish_diagnostics(uri, diagnostics, Some(version));
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        // Remove from store and clear captured diagnostics
        {
            let mut state = self.state.write().await;
            state.documents.close(&uri);
            state.last_published_diagnostics.remove(&uri);
        }

        // Clear diagnostics for the closed file
        self.sink.publish_diagnostics(uri, vec![], None);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.read().await;
        let text = match state.documents.get(&uri) {
            Some(doc) => doc.text.clone(),
            None => return Ok(None),
        };
        drop(state);

        Ok(crate::hover::compute_hover(&text, &uri, position))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.read().await;
        let text = match state.documents.get(&uri) {
            Some(doc) => doc.text.clone(),
            None => return Ok(None),
        };
        let workspace_root = state.workspace_root.clone();
        let stdlib_path = state.stdlib_path.clone();
        // Snapshot all open documents so the blocking closure can check editor
        // buffers before falling back to disk. This avoids unnecessary I/O and
        // ensures unsaved changes are reflected in goto-def results.
        let open_docs = state.documents.snapshot_as_path_map();
        drop(state);

        // Move all CPU-bound parsing and blocking filesystem I/O
        // (ModuleResolver::resolve_import_path calls .exists(), and
        // std::fs::read_to_string) to Tokio's blocking thread pool so the
        // async worker thread stays free for other LSP requests.
        let location = match tokio::task::spawn_blocking(move || {
            if let Some(root) = workspace_root {
                // Build a resolver closure using ModuleResolver for cross-file navigation.
                // Use explicit stdlib_path if configured; fall back to the dev-mode path
                // relative to workspace root.
                let stdlib_root =
                    stdlib_path.unwrap_or_else(|| root.join("crates/reify-compiler/stdlib"));
                let resolver = reify_compiler::module_dag::ModuleResolver::new(root, stdlib_root);
                let resolve_import = |import_path: &str| -> Option<(Url, String)> {
                    let path = resolver.resolve_import_path(import_path).ok()?;
                    // Prefer editor buffer content over disk for open documents,
                    // so unsaved changes are reflected immediately.
                    let source = open_docs
                        .get(&path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&path).ok())?;
                    let target_uri = Url::from_file_path(&path).ok()?;
                    Some((target_uri, source))
                };
                crate::goto_def::compute_goto_definition_cross_file(
                    &text,
                    &uri,
                    position,
                    &resolve_import,
                )
            } else {
                // No workspace root — fall back to single-file resolution.
                crate::goto_def::compute_goto_definition(&text, &uri, position)
            }
        })
        .await
        {
            Ok(loc) => loc,
            Err(e) => {
                // Log panics from the blocking task rather than silently dropping them.
                // The client still gets Ok(None) ("definition not found") for graceful
                // degradation, but the panic is visible in server logs for debugging.
                tracing::error!("goto_definition blocking task failed: {e}");
                None
            }
        };
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let state = self.state.read().await;
        let text = match state.documents.get(&uri) {
            Some(doc) => doc.text.clone(),
            None => return Ok(None),
        };
        drop(state);

        let items = crate::completion::compute_completions(&text, &uri, position);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        // Brief read lock: snapshot the document text, then release before the
        // (pure, CPU-only) symbol walk — mirrors the hover/completion handlers.
        let state = self.state.read().await;
        let text = match state.documents.get(&uri) {
            Some(doc) => doc.text.clone(),
            None => return Ok(None),
        };
        drop(state);

        let symbols = crate::analysis::compute_document_symbols(&text, &uri);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Test support types exported for cross-crate test use.
///
/// Contains [`RecordingSink`], a [`NotificationSink`] implementation that
/// captures all `publish_diagnostics` calls for assertion in tests.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use std::sync::Mutex;

    use tower_lsp::lsp_types::{Diagnostic, Url};

    use super::NotificationSink;

    /// A recording sink that captures all `publish_diagnostics` calls.
    ///
    /// Use `take_calls()` to inspect what was recorded.
    #[derive(Default)]
    pub struct RecordingSink {
        #[allow(clippy::type_complexity)]
        calls: Mutex<Vec<(Url, Vec<Diagnostic>, Option<i32>)>>,
    }

    impl NotificationSink for RecordingSink {
        fn publish_diagnostics(
            &self,
            uri: Url,
            diagnostics: Vec<Diagnostic>,
            version: Option<i32>,
        ) {
            self.calls.lock().unwrap().push((uri, diagnostics, version));
        }
    }

    impl RecordingSink {
        /// Return a clone of all recorded calls.
        pub fn take_calls(&self) -> Vec<(Url, Vec<Diagnostic>, Option<i32>)> {
            self.calls.lock().unwrap().clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::RecordingSink;
    use super::*;
    use tower_lsp::LspService;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// Create a test LspService with NoOpSink (reduces boilerplate across tests).
    fn test_service() -> (LspService<ReifyLanguageServer>, tower_lsp::ClientSocket) {
        LspService::new(|client| ReifyLanguageServer::with_sink(client, Arc::new(NoOpSink)))
    }

    #[test]
    fn noop_sink_implements_notification_sink() {
        let sink: Arc<dyn NotificationSink> = Arc::new(NoOpSink);
        // Should not panic
        sink.publish_diagnostics(Url::parse("file:///test.ri").unwrap(), vec![], None);
    }

    #[tokio::test]
    async fn sink_receives_diagnostics_on_did_open() {
        let sink = Arc::new(RecordingSink::default());
        let (service, _socket) =
            LspService::new(|client| ReifyLanguageServer::with_sink(client, sink.clone()));
        let server = service.inner();
        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        let calls = sink.take_calls();
        assert_eq!(
            calls.len(),
            1,
            "sink should receive exactly one publish_diagnostics call"
        );
        assert_eq!(calls[0].0, uri, "sink should receive the correct URI");
        assert_eq!(calls[0].2, Some(1), "sink should receive version 1");
    }

    #[tokio::test]
    async fn sink_receives_diagnostics_on_did_change() {
        let sink = Arc::new(RecordingSink::default());
        let (service, _socket) =
            LspService::new(|client| ReifyLanguageServer::with_sink(client, sink.clone()));
        let server = service.inner();
        let uri = test_uri();

        // Open with valid source
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: reify_test_support::bracket_source().to_string(),
                },
            })
            .await;

        // Change to broken source
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "structure {".to_string(),
                }],
            })
            .await;

        let calls = sink.take_calls();
        assert_eq!(
            calls.len(),
            2,
            "sink should receive 2 calls (did_open + did_change)"
        );

        // Second call (did_change with broken source) should contain error diagnostics
        let (_, ref diags, version) = calls[1];
        assert_eq!(version, Some(2));
        let has_error = diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR));
        assert!(
            has_error,
            "did_change with broken source should produce error diagnostics"
        );
    }

    #[tokio::test]
    async fn sink_receives_clear_on_did_close() {
        let sink = Arc::new(RecordingSink::default());
        let (service, _socket) =
            LspService::new(|client| ReifyLanguageServer::with_sink(client, sink.clone()));
        let server = service.inner();
        let uri = test_uri();

        // Open a document
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: "structure Foo {}".to_string(),
                },
            })
            .await;

        // Close it
        server
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .await;

        let calls = sink.take_calls();
        assert_eq!(
            calls.len(),
            2,
            "sink should receive 2 calls (did_open + did_close)"
        );

        // Last call should be the clear (empty diagnostics, no version)
        let (ref close_uri, ref close_diags, close_version) = calls[1];
        assert_eq!(close_uri, &uri, "close should clear the same URI");
        assert!(
            close_diags.is_empty(),
            "close should send empty diagnostics"
        );
        assert_eq!(close_version, None, "close should send version=None");
    }

    #[tokio::test]
    async fn in_process_lsp_with_sink_receives_diagnostics() {
        use crate::bridge::InProcessLsp;

        let sink = Arc::new(RecordingSink::default());
        let lsp = InProcessLsp::with_sink(sink.clone());

        let source = reify_test_support::bracket_source();
        let params = serde_json::json!({
            "textDocument": {
                "uri": "file:///test.ri",
                "languageId": "reify",
                "version": 1,
                "text": source
            }
        });

        lsp.handle_request("textDocument/didOpen", params)
            .await
            .expect("didOpen should succeed");

        let calls = sink.take_calls();
        assert_eq!(
            calls.len(),
            1,
            "sink should receive diagnostics from InProcessLsp"
        );
        assert_eq!(
            calls[0].0,
            Url::parse("file:///test.ri").unwrap(),
            "should receive the correct URI"
        );
    }

    #[tokio::test]
    async fn server_with_sink_initializes() {
        let (service, _socket) =
            LspService::new(|client| ReifyLanguageServer::with_sink(client, Arc::new(NoOpSink)));
        let server = service.inner();
        let result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        // Verify same capabilities as the default constructor
        match result.capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => {
                assert_eq!(kind, TextDocumentSyncKind::FULL);
            }
            other => panic!("Expected TextDocumentSyncKind::FULL, got {other:?}"),
        }
        assert!(result.capabilities.hover_provider.is_some());
        assert!(result.capabilities.definition_provider.is_some());
        assert!(result.capabilities.completion_provider.is_some());
    }

    #[tokio::test]
    async fn initialize_stores_workspace_root_from_root_uri() {
        let (service, _socket) = test_service();
        let server = service.inner();

        let root_uri = Url::parse("file:///home/user/project").unwrap();
        let params = InitializeParams {
            root_uri: Some(root_uri),
            ..Default::default()
        };
        server.initialize(params).await.unwrap();

        let state = server.state().read().await;
        let ws_root = state
            .workspace_root
            .as_ref()
            .expect("workspace_root should be set after initialize with root_uri");
        assert_eq!(ws_root, &std::path::PathBuf::from("/home/user/project"));
    }

    #[tokio::test]
    async fn initialize_without_root_uri_leaves_workspace_root_none() {
        let (service, _socket) = test_service();
        let server = service.inner();

        server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        let state = server.state().read().await;
        assert!(
            state.workspace_root.is_none(),
            "workspace_root should be None when no root_uri provided"
        );
    }

    #[tokio::test]
    async fn initialize_returns_full_sync_capability() {
        let (service, _socket) = test_service();

        // Get the inner LanguageServer to call initialize directly
        let server = service.inner();
        let params = InitializeParams::default();
        let init_result = server.initialize(params).await.unwrap();

        // Check text document sync is FULL
        match init_result.capabilities.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => {
                assert_eq!(kind, TextDocumentSyncKind::FULL);
            }
            other => panic!("Expected TextDocumentSyncKind::FULL, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn initialize_advertises_hover_definition_completion() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let init_result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        let caps = &init_result.capabilities;
        assert!(
            caps.hover_provider.is_some(),
            "should advertise hover_provider"
        );
        assert!(
            caps.definition_provider.is_some(),
            "should advertise definition_provider"
        );
        assert!(
            caps.completion_provider.is_some(),
            "should advertise completion_provider"
        );
    }

    #[tokio::test]
    async fn initialize_advertises_document_symbol_provider() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let init_result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        assert!(
            init_result.capabilities.document_symbol_provider.is_some(),
            "should advertise document_symbol_provider (task 4207 η)"
        );
    }

    #[tokio::test]
    async fn did_open_stores_document_and_runs_pipeline() {
        let (service, _socket) = test_service();
        let server = service.inner();

        let source = reify_test_support::bracket_source();
        let uri = test_uri();

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "reify".to_string(),
                version: 1,
                text: source.to_string(),
            },
        };

        server.did_open(params).await;

        // Verify document was stored
        let state = server.state().read().await;
        let doc = state
            .documents
            .get(&uri)
            .expect("document should be stored after did_open");
        assert_eq!(doc.text, source);
        assert_eq!(doc.version, 1);
    }

    #[tokio::test]
    async fn did_change_updates_document_text() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = test_uri();

        // Open with valid source
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: reify_test_support::bracket_source().to_string(),
                },
            })
            .await;

        // Change to broken source
        let broken_source = "structure {";
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: broken_source.to_string(),
                }],
            })
            .await;

        // Verify document text was updated
        let state = server.state().read().await;
        let doc = state
            .documents
            .get(&uri)
            .expect("document should exist after change");
        assert_eq!(doc.text, broken_source);
        assert_eq!(doc.version, 2);
    }

    // --- step-13: integration tests for hover/goto-def/completion handlers ---

    async fn open_bracket_source(server: &ReifyLanguageServer) -> Url {
        let source = reify_test_support::bracket_source();
        let uri = test_uri();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;
        uri
    }

    #[tokio::test]
    async fn hover_handler_returns_info_for_width() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let hover_result = server
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(1, 10), // on 'width'
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap();

        assert!(
            hover_result.is_some(),
            "hover should return info for 'width'"
        );
    }

    #[tokio::test]
    async fn goto_definition_handler_returns_location_for_thickness() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let goto_result = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(9, 15), // on 'thickness' in constraint
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        assert!(
            goto_result.is_some(),
            "goto-def should return location for 'thickness'"
        );
    }

    #[tokio::test]
    async fn completion_handler_returns_items() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let comp_result = server
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(1, 0),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .unwrap();

        assert!(comp_result.is_some(), "completion should return items");
        match comp_result.unwrap() {
            CompletionResponse::Array(items) => {
                assert!(
                    !items.is_empty(),
                    "completion should return non-empty items"
                );
            }
            CompletionResponse::List(list) => {
                assert!(
                    !list.items.is_empty(),
                    "completion should return non-empty items"
                );
            }
        }
    }

    // --- task 4207 η: document_symbol handler tests ---

    #[tokio::test]
    async fn document_symbol_handler_returns_nested_symbols() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let result = server
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        match result {
            Some(DocumentSymbolResponse::Nested(syms)) => {
                assert_eq!(syms.len(), 1, "bracket_source has one top-level symbol");
                assert_eq!(syms[0].name, "Bracket");
            }
            other => panic!("expected Some(Nested(..)), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn document_symbol_unknown_uri_returns_none() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Never opened — the document is not in the store.
        let result = server
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///never_opened.ri").unwrap(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        assert!(
            result.is_none(),
            "document_symbol for an unknown URI should return Ok(None), got {result:?}"
        );
    }

    #[tokio::test]
    async fn server_captures_published_diagnostics() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = test_uri();
        let source = reify_test_support::bracket_source();

        // Open with valid bracket source
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        // Read captured diagnostics from server state
        let state = server.state().read().await;
        let captured = state
            .last_diagnostics_for(&uri)
            .expect("diagnostics should be captured after did_open");

        // Valid bracket source should have no ERROR-severity diagnostics
        let errors: Vec<_> = captured
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "valid bracket source should have no errors in captured diagnostics, got: {errors:?}"
        );
    }

    #[tokio::test]
    async fn server_recovers_from_eval_state_lock_poisoning() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = test_uri();

        // Get the eval_state Arc and poison the Mutex by panicking while holding the lock
        let eval_state_arc = server.eval_state().clone();
        let handle = std::thread::spawn(move || {
            let _guard = eval_state_arc.lock().unwrap();
            panic!("intentional panic to poison the mutex");
        });
        // Wait for the thread to finish (it panicked)
        let _ = handle.join();

        // Confirm the lock is poisoned
        assert!(
            server.eval_state().lock().is_err(),
            "lock should be poisoned after panic"
        );

        // did_open should recover from the poisoned lock, not panic
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: reify_test_support::bracket_source().to_string(),
                },
            })
            .await;

        // Verify diagnostics were captured (server recovered successfully)
        let state = server.state().read().await;
        let captured = state
            .last_diagnostics_for(&uri)
            .expect("diagnostics should be captured even after poison recovery");
        // Valid bracket source should have no ERROR-severity diagnostics
        let errors: Vec<_> = captured
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            errors.is_empty(),
            "valid source should have no errors after poison recovery, got: {errors:?}"
        );
    }

    #[tokio::test]
    async fn did_close_removes_document_from_store() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = test_uri();

        // Open a document
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: "structure Foo {}".to_string(),
                },
            })
            .await;

        // Close it
        server
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .await;

        // Verify removed
        let state = server.state().read().await;
        assert!(
            state.documents.get(&uri).is_none(),
            "document should be removed after did_close"
        );
    }

    #[tokio::test]
    async fn initialize_stores_stdlib_path_from_initialization_options() {
        let (service, _socket) = test_service();
        let server = service.inner();

        server
            .initialize(InitializeParams {
                initialization_options: Some(serde_json::json!({"stdlibPath": "/custom/stdlib"})),
                ..Default::default()
            })
            .await
            .unwrap();

        let state = server.state().read().await;
        assert_eq!(
            state.stdlib_path,
            Some(PathBuf::from("/custom/stdlib")),
            "stdlib_path should be parsed from initialization_options"
        );
    }

    #[tokio::test]
    async fn initialize_without_options_has_no_stdlib_path() {
        let (service, _socket) = test_service();
        let server = service.inner();

        server
            .initialize(InitializeParams {
                ..Default::default()
            })
            .await
            .unwrap();

        let state = server.state().read().await;
        assert!(
            state.stdlib_path.is_none(),
            "stdlib_path should be None when initialization_options are absent"
        );
    }

    #[tokio::test]
    async fn goto_definition_uses_custom_stdlib_path() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Create a temporary workspace with a custom stdlib directory
        let tmp_dir = std::env::temp_dir().join(format!("reify-lsp-stdlib-{}", std::process::id()));
        let custom_stdlib = tmp_dir.join("custom-stdlib");
        std::fs::create_dir_all(&custom_stdlib).unwrap();

        // Write a module in the custom stdlib
        std::fs::write(
            custom_stdlib.join("mymod.ri"),
            "structure Widget {\n    param size: Scalar = 5mm\n}",
        )
        .unwrap();

        // Initialize with stdlibPath pointing to the custom stdlib
        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                initialization_options: Some(serde_json::json!({
                    "stdlibPath": custom_stdlib.to_str().unwrap()
                })),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open main.ri that imports from std.mymod
        let main_source = "import std.mymod.Widget\nstructure S {\n    sub w = Widget\n}";
        let main_uri = Url::from_file_path(tmp_dir.join("main.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: main_source.to_string(),
                },
            })
            .await;

        // Goto definition on 'Widget' in 'sub w = Widget' (line 2, col 12)
        let goto_result = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(2, 12),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Should resolve to custom-stdlib/mymod.ri
        let response = goto_result.expect("goto-def should resolve Widget from custom stdlib");
        match response {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("mymod.ri"),
                    "should point to mymod.ri, got {}",
                    loc.uri
                );
                assert_eq!(
                    loc.range.start.line, 0,
                    "should point to structure Widget on line 0"
                );
            }
            other => panic!("expected Scalar location, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn goto_definition_resolves_imported_symbol_across_files() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Create a temporary workspace with two .ri files
        let tmp_dir = std::env::temp_dir().join(format!("reify-lsp-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        // Write the target file: parts.ri
        let parts_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        std::fs::write(tmp_dir.join("parts.ri"), parts_source).unwrap();

        // Initialize with workspace root
        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open main.ri with an import
        let main_source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole\n}";
        let main_uri = Url::from_file_path(tmp_dir.join("main.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: main_source.to_string(),
                },
            })
            .await;

        // Goto definition on 'Hole' in 'sub hole = Hole' (line 2, col 16)
        let goto_result = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(2, 16),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        // Clean up temp directory
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Verify the result points to parts.ri
        let response = goto_result.expect("goto-def should return a result for imported symbol");
        match response {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("parts.ri"),
                    "should point to parts.ri, got {}",
                    loc.uri
                );
                assert_eq!(
                    loc.range.start.line, 0,
                    "should point to structure Hole on line 0"
                );
            }
            other => panic!("expected Scalar location, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn concurrent_goto_definition_completes_without_stalling() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Create a temporary workspace with multiple .ri files
        let tmp_dir =
            std::env::temp_dir().join(format!("reify-lsp-concurrent-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        // Write three target files
        std::fs::write(
            tmp_dir.join("parts.ri"),
            "structure Hole {\n    param diameter: Scalar = 10mm\n}",
        )
        .unwrap();
        std::fs::write(
            tmp_dir.join("fasteners.ri"),
            "structure Bolt {\n    param length: Scalar = 20mm\n}",
        )
        .unwrap();
        std::fs::write(
            tmp_dir.join("utils.ri"),
            "structure Helper {\n    param size: Scalar = 5mm\n}",
        )
        .unwrap();

        // Initialize with workspace root
        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open main.ri with imports from all three files
        // Line 0: import parts.Hole
        // Line 1: import fasteners.Bolt
        // Line 2: import utils.Helper
        // Line 3: structure Assembly {
        // Line 4:     sub h = Hole        ← 'Hole' at col 12
        // Line 5:     sub b = Bolt        ← 'Bolt' at col 12
        // Line 6:     sub helper = Helper  ← 'Helper' at col 17
        // Line 7: }
        let main_source = "\
import parts.Hole
import fasteners.Bolt
import utils.Helper
structure Assembly {
    sub h = Hole
    sub b = Bolt
    sub helper = Helper
}";
        let main_uri = Url::from_file_path(tmp_dir.join("main.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: main_source.to_string(),
                },
            })
            .await;

        // Fire 3 concurrent goto_definition requests.
        // With spawn_blocking, these offload to the blocking thread pool and
        // the single Tokio worker remains free to drive all futures concurrently.
        let (r1, r2, r3) = tokio::join!(
            server.goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(4, 12), // 'Hole'
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
            server.goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(5, 12), // 'Bolt'
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
            server.goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(6, 17), // 'Helper'
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
        );

        // Clean up temp directory
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Assert all 3 completed and returned correct locations
        let resp1 = r1.unwrap().expect("request 1 should return a result");
        let resp2 = r2.unwrap().expect("request 2 should return a result");
        let resp3 = r3.unwrap().expect("request 3 should return a result");

        match resp1 {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("parts.ri"),
                    "request 1 should point to parts.ri, got {}",
                    loc.uri
                );
            }
            other => panic!("expected Scalar for request 1, got {other:?}"),
        }
        match resp2 {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("fasteners.ri"),
                    "request 2 should point to fasteners.ri, got {}",
                    loc.uri
                );
            }
            other => panic!("expected Scalar for request 2, got {other:?}"),
        }
        match resp3 {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("utils.ri"),
                    "request 3 should point to utils.ri, got {}",
                    loc.uri
                );
            }
            other => panic!("expected Scalar for request 3, got {other:?}"),
        }
    }

    // --- step-18: silent_error_swallow regression tests ---

    #[tokio::test]
    async fn goto_definition_unresolvable_symbol_returns_none_gracefully() {
        // Regression test: goto_definition for an unknown symbol should return
        // Ok(None) rather than panicking or returning an error — verifies that
        // the spawn_blocking task handles failures gracefully.
        let (service, _socket) = test_service();
        let server = service.inner();

        // Initialize without workspace root (single-file mode)
        server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        // Open a document with an import but no target file
        let source = "import nonexistent.Foo\nstructure S {\n    sub f = Foo\n}";
        let uri = Url::parse("file:///test_unresolvable.ri").unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: source.to_string(),
                },
            })
            .await;

        // Goto definition on 'Foo' (line 2, col 12) — not locally defined
        let goto_result = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(2, 12),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;

        // Must return Ok(None), not panic or error
        let result = goto_result.expect("goto_definition should return Ok, not Err");
        assert!(
            result.is_none(),
            "unresolvable symbol should return None, got {result:?}"
        );
    }

    #[tokio::test]
    async fn spawn_blocking_join_error_is_err_not_silent() {
        // Verify that a JoinError from spawn_blocking is an Err that should be
        // explicitly handled (logged) rather than silently dropped via unwrap_or.
        // This test validates the error-handling contract: panics in blocking tasks
        // produce recoverable JoinErrors that carry diagnostic information.
        let result: std::result::Result<Option<String>, _> = tokio::task::spawn_blocking(|| {
            panic!("simulated panic in blocking task");
        })
        .await;

        // JoinError must be Err, not silently mapped to Ok(None)
        assert!(
            result.is_err(),
            "spawn_blocking panic should produce JoinError"
        );
        let err = result.unwrap_err();
        assert!(err.is_panic(), "JoinError should indicate a panic");
        // The error message should contain diagnostic info for logging
        let err_msg = format!("{err}");
        assert!(
            !err_msg.is_empty(),
            "JoinError should have a displayable message for logging"
        );
    }

    #[tokio::test]
    async fn goto_definition_prefers_document_store_over_disk() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Create a temporary workspace
        let tmp_dir =
            std::env::temp_dir().join(format!("reify-lsp-docstore-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        // Write parts.ri on disk with ONLY Hole (no Plate)
        let disk_source = "structure Hole {\n    param diameter: Scalar = 10mm\n}";
        std::fs::write(tmp_dir.join("parts.ri"), disk_source).unwrap();

        // Initialize with workspace root
        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open parts.ri in the editor with MODIFIED content that adds Plate on line 0.
        // The editor version differs from disk — Plate only exists in the editor buffer.
        let editor_source = "structure Plate {\n    param width: Scalar = 5mm\n}\nstructure Hole {\n    param diameter: Scalar = 10mm\n}";
        let parts_uri = Url::from_file_path(tmp_dir.join("parts.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: parts_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: editor_source.to_string(),
                },
            })
            .await;

        // Open main.ri that imports Plate from parts
        let main_source = "import parts.Plate\nstructure Assembly {\n    sub p = Plate\n}";
        let main_uri = Url::from_file_path(tmp_dir.join("main.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: main_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: main_source.to_string(),
                },
            })
            .await;

        // Goto definition on 'Plate' in 'sub p = Plate' (line 2, col 12)
        let goto_result = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: main_uri.clone(),
                    },
                    position: Position::new(2, 12),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap();

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Should resolve to parts.ri line 0 (from editor content, not disk).
        // Disk version doesn't have Plate, so this proves DocumentStore is used.
        let response = goto_result
            .expect("goto-def should resolve Plate from DocumentStore content, not disk");
        match response {
            GotoDefinitionResponse::Scalar(loc) => {
                assert!(
                    loc.uri.path().ends_with("parts.ri"),
                    "should point to parts.ri, got {}",
                    loc.uri
                );
                assert_eq!(
                    loc.range.start.line, 0,
                    "should point to structure Plate on line 0 (editor content)"
                );
            }
            other => panic!("expected Scalar location, got {other:?}"),
        }
    }
}
