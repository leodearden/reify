use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::document::DocumentStore;

/// Internal state shared across handler calls.
pub struct ServerState {
    pub documents: DocumentStore,
}

/// The Reify language server.
pub struct ReifyLanguageServer {
    client: Client,
    state: Arc<RwLock<ServerState>>,
}

impl ReifyLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(ServerState {
                documents: DocumentStore::new(),
            })),
        }
    }

    /// Access server state (for testing).
    #[cfg(test)]
    pub(crate) fn state(&self) -> &Arc<RwLock<ServerState>> {
        &self.state
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
}
