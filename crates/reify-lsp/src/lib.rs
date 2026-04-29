// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

pub mod analysis;
pub mod bridge;
pub mod completion;
pub mod convert;
pub mod diagnostics;
pub mod document;
pub mod goto_def;
pub mod hover;
pub mod server;

/// Re-export test support types for cross-crate test use.
#[cfg(any(test, feature = "test-support"))]
pub use server::test_support;

use std::sync::Arc;

use tower_lsp::{LspService, Server};

use server::ClientSink;

/// Start the Reify LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        let sink = Arc::new(ClientSink::new(client.clone()));
        server::ReifyLanguageServer::with_sink(client, sink)
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
