// See `reify-types::value::SampledField` for the rationale behind this allow:
// `Value::SampledField` carries an `AtomicBool` (excluded from
// `PartialEq`/`Ord`/`Hash`/`content_hash`) that nonetheless triggers
// `mutable_key_type` on every `BTreeMap<Value, _>` site.
#![allow(clippy::mutable_key_type)]

pub mod analysis;
pub mod blocking_work;
pub mod bridge;
pub mod completion;
pub mod convert;
pub mod diagnostics;
pub mod document;
pub mod goto_def;
pub mod hover;
pub mod references;
pub mod server;

/// Re-export test support types for cross-crate test use.
#[cfg(any(test, feature = "test-support"))]
pub use server::test_support;

use std::sync::Arc;

use tower_lsp::{Client, LspService, Server};

use blocking_work::BlockingWorkPlacement;
use server::{ClientSink, ReifyLanguageServer};

/// Start the Reify LSP server on stdin/stdout.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(stdio_language_server);
    Server::new(stdin, stdout, socket).serve(service).await;
}

/// The language server [`run_server`] serves. Its handlers' blocking work
/// goes to tokio's blocking pool, keeping the async workers it shares across
/// requests free while handlers parse and compile.
fn stdio_language_server(client: Client) -> ReifyLanguageServer {
    let sink = Arc::new(ClientSink::new(client.clone()));
    ReifyLanguageServer::with_sink(client, sink)
        .with_blocking_work_placement(BlockingWorkPlacement::BlockingPool)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use tower_lsp::lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};
    use tower_lsp::{LanguageServer, LspService, jsonrpc};

    use super::stdio_language_server;
    use crate::blocking_work::test_support::{
        IDLE_POOL_WOULD_HAVE_RUN_IT_BY, SaturatedBlockingPool, blocking_work_requests, poll_once,
    };

    const URI: &str = "file:///stdio.ri";

    type Resolution<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

    /// Reduce a handler's answer to whether it resolved, so the four
    /// differently-typed handlers can be driven side by side.
    fn resolution<'a, T: 'a>(
        answer: impl Future<Output = jsonrpc::Result<Option<T>>> + 'a,
    ) -> Resolution<'a> {
        Box::pin(async move {
            match answer.await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err("answered None at a position the bracket fixture resolves".into()),
                Err(error) => Err(error.to_string()),
            }
        })
    }

    fn params<P: serde::de::DeserializeOwned>(request: serde_json::Value) -> P {
        serde_json::from_value(request).expect("the request's params deserialize")
    }

    #[test]
    fn stdio_server_queues_its_blocking_work_behind_the_pool() {
        let pool = SaturatedBlockingPool::new();
        pool.block_on(async {
            let (service, _socket) = LspService::new(stdio_language_server);
            let server = service.inner();
            server
                .initialize(
                    serde_json::from_value(reify_test_support::minimal_init_params())
                        .expect("minimal init params deserialize"),
                )
                .await
                .expect("initialize should succeed");
            server
                .did_open(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem::new(
                        Url::parse(URI).unwrap(),
                        "reify".to_string(),
                        1,
                        reify_test_support::bracket_source().to_string(),
                    ),
                })
                .await;

            let [definition, prepare_rename, rename, references] = blocking_work_requests(URI);
            let mut answers = [
                (
                    definition.0,
                    resolution(server.goto_definition(params(definition.1))),
                ),
                (
                    prepare_rename.0,
                    resolution(server.prepare_rename(params(prepare_rename.1))),
                ),
                (rename.0, resolution(server.rename(params(rename.1)))),
                (
                    references.0,
                    resolution(server.references(params(references.1))),
                ),
            ];

            for (method, answer) in &mut answers {
                assert!(
                    poll_once(answer).await.is_pending(),
                    "{method} answered on the calling thread; the stdio server must hand \
                     its blocking work to the pool"
                );
            }
            tokio::time::sleep(IDLE_POOL_WOULD_HAVE_RUN_IT_BY).await;
            for (method, answer) in &mut answers {
                assert!(
                    poll_once(answer).await.is_pending(),
                    "{method} answered while the pool's only thread was occupied, so its \
                     blocking work never waited on the pool"
                );
            }
            pool.release();
            for (method, answer) in answers {
                answer
                    .await
                    .unwrap_or_else(|failure| panic!("{method}: {failure}"));
            }
        });
    }
}
