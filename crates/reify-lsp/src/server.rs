use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::diagnostics::EvalState;
use crate::document::DocumentStore;

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
pub struct ReifyLanguageServer {
    client: Client,
    state: Arc<RwLock<ServerState>>,
    /// Evaluation state lives outside the RwLock so eval can run without
    /// blocking concurrent LSP requests that only need document state.
    /// Wrapped in Mutex because Engine internals (OpaqueState) are Send but not Sync.
    eval_state: Arc<Mutex<EvalState>>,
}

impl ReifyLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState {
                documents: DocumentStore::new(),
                last_published_diagnostics: HashMap::new(),
            })),
            eval_state: Arc::new(Mutex::new(EvalState::new())),
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

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
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
            state.documents.update(&uri, text.clone(), version);
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

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
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
        self.client
            .publish_diagnostics(uri, vec![], None)
            .await;
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
