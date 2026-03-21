pub mod analysis;
pub mod bridge;
pub mod completion;
pub mod convert;
pub mod diagnostics;
pub mod document;
pub mod goto_def;
pub mod hover;
pub mod server;

use tower_lsp::{LspService, Server};

/// Start the Reify LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::ReifyLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
