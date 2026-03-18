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
        todo!()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for ReifyLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        todo!()
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
}
