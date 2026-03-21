use std::collections::HashMap;
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
    fn publish_diagnostics(&self, _uri: Url, _diagnostics: Vec<Diagnostic>, _version: Option<i32>) {}
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
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
            let mut eval_state = self
                .eval_state
                .lock()
                .unwrap_or_else(|e| {
                    eprintln!("eval_state lock poisoned, recovering");
                    e.into_inner()
                });
            let result = crate::diagnostics::compute_diagnostics_with_state(
                &mut eval_state,
                &text,
                &uri,
            );
            result.diagnostics
        };

        // Brief write lock: capture diagnostics
        {
            let mut state = self.state.write().await;
            state
                .last_published_diagnostics
                .insert(uri.clone(), diagnostics.clone());
        }

        self.sink.publish_diagnostics(uri, diagnostics, Some(version));
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
            let mut eval_state = self
                .eval_state
                .lock()
                .unwrap_or_else(|e| {
                    eprintln!("eval_state lock poisoned, recovering");
                    e.into_inner()
                });
            let result = crate::diagnostics::compute_diagnostics_with_state(
                &mut eval_state,
                &text,
                &uri,
            );
            result.diagnostics
        };

        // Brief write lock: capture diagnostics
        {
            let mut state = self.state.write().await;
            state
                .last_published_diagnostics
                .insert(uri.clone(), diagnostics.clone());
        }

        self.sink.publish_diagnostics(uri, diagnostics, Some(version));
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
        drop(state);

        let location = crate::goto_def::compute_goto_definition(&text, &uri, position);
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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::LspService;

    fn test_uri() -> Url {
        Url::parse("file:///test.ri").unwrap()
    }

    /// A recording sink that captures all publish_diagnostics calls.
    #[derive(Default)]
    struct RecordingSink {
        calls: Mutex<Vec<(Url, Vec<Diagnostic>, Option<i32>)>>,
    }

    impl NotificationSink for RecordingSink {
        fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>, version: Option<i32>) {
            self.calls.lock().unwrap().push((uri, diagnostics, version));
        }
    }

    impl RecordingSink {
        fn take_calls(&self) -> Vec<(Url, Vec<Diagnostic>, Option<i32>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[test]
    fn noop_sink_implements_notification_sink() {
        let sink: Arc<dyn NotificationSink> = Arc::new(NoOpSink);
        // Should not panic
        sink.publish_diagnostics(
            Url::parse("file:///test.ri").unwrap(),
            vec![],
            None,
        );
    }

    #[tokio::test]
    async fn sink_receives_diagnostics_on_did_open() {
        let sink = Arc::new(RecordingSink::default());
        let (service, _socket) = LspService::new(|client| {
            ReifyLanguageServer::with_sink(client, sink.clone())
        });
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
        assert_eq!(calls.len(), 1, "sink should receive exactly one publish_diagnostics call");
        assert_eq!(calls[0].0, uri, "sink should receive the correct URI");
        assert_eq!(calls[0].2, Some(1), "sink should receive version 1");
    }

    #[tokio::test]
    async fn sink_receives_diagnostics_on_did_change() {
        let sink = Arc::new(RecordingSink::default());
        let (service, _socket) = LspService::new(|client| {
            ReifyLanguageServer::with_sink(client, sink.clone())
        });
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
        assert_eq!(calls.len(), 2, "sink should receive 2 calls (did_open + did_change)");

        // Second call (did_change with broken source) should contain error diagnostics
        let (_, ref diags, version) = calls[1];
        assert_eq!(version, Some(2));
        let has_error = diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR));
        assert!(has_error, "did_change with broken source should produce error diagnostics");
    }

    #[tokio::test]
    async fn server_with_sink_initializes() {
        let (service, _socket) = LspService::new(|client| {
            ReifyLanguageServer::with_sink(client, Arc::new(NoOpSink))
        });
        let server = service.inner();
        let result = server.initialize(InitializeParams::default()).await.unwrap();

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
    async fn initialize_returns_full_sync_capability() {
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);

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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
        let server = service.inner();
        let init_result = server.initialize(InitializeParams::default()).await.unwrap();

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
    async fn did_open_stores_document_and_runs_pipeline() {
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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

        assert!(
            comp_result.is_some(),
            "completion should return items"
        );
        match comp_result.unwrap() {
            CompletionResponse::Array(items) => {
                assert!(!items.is_empty(), "completion should return non-empty items");
            }
            CompletionResponse::List(list) => {
                assert!(
                    !list.items.is_empty(),
                    "completion should return non-empty items"
                );
            }
        }
    }

    #[tokio::test]
    async fn server_captures_published_diagnostics() {
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
        let (service, _socket) = LspService::new(ReifyLanguageServer::new);
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
}
