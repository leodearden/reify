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

/// Deserialize an LSP params payload into `T`, attaching `label` to any
/// resulting error. Used by every deserializing arm of
/// [`InProcessLsp::handle_request`] to keep the per-arm code one line.
///
/// On failure, returns `Err(format!("{label}: {e}"))`, where `label` is used
/// **verbatim** as the error prefix. Callers pass an [`error_prefix`] constant
/// directly, making that constant the literal source of truth for the runtime
/// error message. Editing a constant is the only change needed to rotate its
/// prefix; there is no parallel string in the implementation that could drift.
fn parse_params<T: serde::de::DeserializeOwned>(
    params: serde_json::Value,
    label: &str,
) -> Result<T, String> {
    serde_json::from_value(params).map_err(|e| format!("{label}: {e}"))
}

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
    ///   `textDocument/definition`, `textDocument/documentSymbol`,
    ///   `textDocument/documentHighlight`, `textDocument/prepareRename`,
    ///   `textDocument/rename`.
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
                let p = parse_params::<InitializeParams>(params, error_prefix::INITIALIZE_PARAMS)?;
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
                let p =
                    parse_params::<InitializedParams>(params, error_prefix::INITIALIZED_PARAMS)?;
                server.initialized(p).await;
                Ok(Value::Null)
            }
            "textDocument/didOpen" => {
                let p = parse_params::<DidOpenTextDocumentParams>(
                    params,
                    error_prefix::DID_OPEN_PARAMS,
                )?;
                server.did_open(p).await;
                Ok(Value::Null)
            }
            "textDocument/didChange" => {
                let p = parse_params::<DidChangeTextDocumentParams>(
                    params,
                    error_prefix::DID_CHANGE_PARAMS,
                )?;
                server.did_change(p).await;
                Ok(Value::Null)
            }
            "textDocument/didClose" => {
                let p = parse_params::<DidCloseTextDocumentParams>(
                    params,
                    error_prefix::DID_CLOSE_PARAMS,
                )?;
                server.did_close(p).await;
                Ok(Value::Null)
            }
            "textDocument/completion" => {
                let p = parse_params::<CompletionParams>(params, error_prefix::COMPLETION_PARAMS)?;
                let result = server
                    .completion(p)
                    .await
                    .map_err(|e| format!("completion error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/hover" => {
                let p = parse_params::<HoverParams>(params, error_prefix::HOVER_PARAMS)?;
                let result = server
                    .hover(p)
                    .await
                    .map_err(|e| format!("hover error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/definition" => {
                let p =
                    parse_params::<GotoDefinitionParams>(params, error_prefix::DEFINITION_PARAMS)?;
                let result = server
                    .goto_definition(p)
                    .await
                    .map_err(|e| format!("definition error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/documentSymbol" => {
                let p = parse_params::<DocumentSymbolParams>(
                    params,
                    error_prefix::DOCUMENT_SYMBOL_PARAMS,
                )?;
                let result = server
                    .document_symbol(p)
                    .await
                    .map_err(|e| format!("documentSymbol error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/documentHighlight" => {
                let p = parse_params::<DocumentHighlightParams>(
                    params,
                    error_prefix::DOCUMENT_HIGHLIGHT_PARAMS,
                )?;
                let result = server
                    .document_highlight(p)
                    .await
                    .map_err(|e| format!("documentHighlight error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/prepareRename" => {
                let p = parse_params::<TextDocumentPositionParams>(
                    params,
                    error_prefix::PREPARE_RENAME_PARAMS,
                )?;
                let result = server
                    .prepare_rename(p)
                    .await
                    .map_err(|e| format!("prepareRename error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/rename" => {
                let p = parse_params::<RenameParams>(params, error_prefix::RENAME_PARAMS)?;
                let result = server
                    .rename(p)
                    .await
                    .map_err(|e| format!("rename error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "textDocument/references" => {
                let p = parse_params::<ReferenceParams>(params, error_prefix::REFERENCES_PARAMS)?;
                let result = server
                    .references(p)
                    .await
                    .map_err(|e| format!("references error: {e}"))?;
                serde_json::to_value(result).map_err(|e| format!("serialize error: {e}"))
            }
            "shutdown" => {
                server
                    .shutdown()
                    .await
                    .map_err(|e| format!("shutdown error: {e}"))?;
                Ok(Value::Null)
            }
            other => Err(format!("{} {other}", error_prefix::UNSUPPORTED_METHOD)),
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
/// These constants are threaded directly into `parse_params` as the
/// verbatim `label` argument, making each constant the **literal source of
/// truth** for the runtime error prefix. There is no parallel string in the
/// implementation — mismatches between the constant and the runtime error
/// cannot exist because the constant *is* the runtime error prefix.
///
/// Updating a constant here is the only change needed to rotate the
/// corresponding runtime error string; the test file's `use … error_prefix::*`
/// import reflects the change automatically, turning any stale assertion into
/// a compile error or immediate test failure.
///
/// Every deserializing arm of `handle_request` threads its error prefix
/// through a constant defined here. There are no remaining hardcoded strings
/// in the implementation.
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

    /// Prefix for deserialization failures on `textDocument/completion` params.
    pub const COMPLETION_PARAMS: &str = "completion params error";

    /// Prefix for deserialization failures on `textDocument/hover` params.
    pub const HOVER_PARAMS: &str = "hover params error";

    /// Prefix for deserialization failures on `textDocument/definition` params.
    pub const DEFINITION_PARAMS: &str = "definition params error";

    /// Prefix for deserialization failures on `textDocument/documentSymbol` params.
    pub const DOCUMENT_SYMBOL_PARAMS: &str = "documentSymbol params error";

    /// Prefix for deserialization failures on `textDocument/documentHighlight` params.
    pub const DOCUMENT_HIGHLIGHT_PARAMS: &str = "documentHighlight params error";

    /// Prefix for deserialization failures on `textDocument/prepareRename` params.
    pub const PREPARE_RENAME_PARAMS: &str = "prepareRename params error";

    /// Prefix for deserialization failures on `textDocument/rename` params.
    pub const RENAME_PARAMS: &str = "rename params error";

    /// Prefix for deserialization failures on `textDocument/references` params.
    pub const REFERENCES_PARAMS: &str = "references params error";

    /// Prefix used when an unrecognised LSP method name is requested.
    ///
    /// The full error message is `"{UNSUPPORTED_METHOD} {method_name}"`.
    pub const UNSUPPORTED_METHOD: &str = "unsupported LSP method:";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `parse_params` must include the caller-supplied label as a prefix in its
    /// error message so that callers can identify which arm failed.
    ///
    /// Uses `error_prefix::INITIALIZE_PARAMS` (not a literal) to validate the
    /// constant-based pattern: the constant is the source of truth, not a copy.
    #[test]
    fn parse_params_includes_label_in_error() {
        let result = parse_params::<InitializeParams>(json!(42), error_prefix::INITIALIZE_PARAMS);
        assert!(result.is_err(), "expected Err for malformed params, got Ok");
        let err = result.unwrap_err();
        assert!(
            err.contains(error_prefix::INITIALIZE_PARAMS),
            "error should contain '{}', got: {err}",
            error_prefix::INITIALIZE_PARAMS
        );
    }

    // --- task 4207 η: SIGNAL TEST — documentSymbol through the bridge ---

    /// Recursively assert every symbol's `selection_range` lies within its
    /// `range` (LSP requires `selection_range ⊆ range`).
    fn assert_selection_within_range(syms: &[DocumentSymbol]) {
        for s in syms {
            let r = &s.range;
            let sr = &s.selection_range;
            assert!(
                (sr.start.line, sr.start.character) >= (r.start.line, r.start.character)
                    && (sr.end.line, sr.end.character) <= (r.end.line, r.end.character),
                "selection_range {sr:?} not within range {r:?} for symbol {}",
                s.name
            );
            if let Some(children) = &s.children {
                assert_selection_within_range(children);
            }
        }
    }

    /// End-to-end signal for task 4207 η: the GUI `lsp_request` Tauri command
    /// dispatches through `InProcessLsp::handle_request`, so the consumer
    /// θ/4208 (command-palette symbol-jump) can only reach the provider if the
    /// bridge has a `textDocument/documentSymbol` arm. This drives a
    /// comprehensive fixture through that exact path and asserts the full
    /// hierarchical symbol tree.
    #[tokio::test]
    async fn handle_request_document_symbol_returns_hierarchical_symbols() {
        let lsp = InProcessLsp::new();
        let uri = "file:///signal.ri";
        let source = "\
import std.math
structure Bracket {
    param width : Length = 80mm
    let footprint = width * width
}
occurrence def Joint {
    param diameter : Length = 10mm
}
trait Rigid {
    param mass : Length = 5mm
}
enum Shape {
    Point,
    Circle
}
fn area(w : Length) -> Length { w }
";

        lsp.handle_request(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "reify",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await
        .expect("didOpen should succeed");

        let value = lsp
            .handle_request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
            )
            .await
            .expect("documentSymbol request should reach the provider via the bridge");

        let response: DocumentSymbolResponse = serde_json::from_value(value)
            .expect("response should deserialize as DocumentSymbolResponse");
        let symbols = match response {
            DocumentSymbolResponse::Nested(s) => s,
            DocumentSymbolResponse::Flat(_) => panic!("expected Nested document symbols"),
        };

        // `import std.math` is excluded; the 5 navigable top-level declarations
        // remain in source order with their mapped kinds.
        let top: Vec<(&str, SymbolKind)> =
            symbols.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(
            top,
            vec![
                ("Bracket", SymbolKind::STRUCT),
                ("Joint", SymbolKind::CLASS),
                ("Rigid", SymbolKind::INTERFACE),
                ("Shape", SymbolKind::ENUM),
                ("area", SymbolKind::FUNCTION),
            ],
            "top-level symbols (import excluded) in source order"
        );

        // Structure children: param → FIELD, let → VARIABLE.
        let bracket_children = symbols[0]
            .children
            .as_ref()
            .expect("Bracket should have member children");
        let bc: Vec<(&str, SymbolKind)> = bracket_children
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect();
        assert_eq!(
            bc,
            vec![
                ("width", SymbolKind::FIELD),
                ("footprint", SymbolKind::VARIABLE),
            ],
            "Bracket children: width FIELD then footprint VARIABLE"
        );

        // Enum children: the two variants as ENUM_MEMBER in source order.
        let shape_children = symbols[3]
            .children
            .as_ref()
            .expect("Shape should have variant children");
        let sc: Vec<(&str, SymbolKind)> = shape_children
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect();
        assert_eq!(
            sc,
            vec![
                ("Point", SymbolKind::ENUM_MEMBER),
                ("Circle", SymbolKind::ENUM_MEMBER),
            ],
            "Shape variants: Point then Circle as ENUM_MEMBER"
        );

        // LSP invariant across the whole tree: selection_range ⊆ range.
        assert_selection_within_range(&symbols);
    }

    // --- task 4203 γ: SIGNAL TESTS — prepareRename/rename through the bridge ---
    //
    // The GUI reaches the rename surface only through the Tauri `lsp_request`
    // command -> `InProcessLsp::handle_request`, which dispatches strictly by
    // method name. Without the "textDocument/prepareRename" and
    // "textDocument/rename" arms the GUI would get Err("unsupported LSP
    // method: …"); these drive the canonical bracket fixture through that exact
    // path. Positions match the server-handler tests (width use at line 7
    // col 17; `structure` keyword at line 0 col 0).

    async fn open_bracket(lsp: &InProcessLsp, uri: &str) {
        lsp.handle_request(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "reify",
                    "version": 1,
                    "text": reify_test_support::bracket_source()
                }
            }),
        )
        .await
        .expect("didOpen should succeed");
    }

    #[tokio::test]
    async fn handle_request_prepare_rename_returns_target_for_width() {
        let lsp = InProcessLsp::new();
        let uri = "file:///rename.ri";
        open_bracket(&lsp, uri).await;

        let value = lsp
            .handle_request(
                "textDocument/prepareRename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 17 }
                }),
            )
            .await
            .expect("prepareRename request should reach the provider via the bridge");

        let response: PrepareRenameResponse = serde_json::from_value(value)
            .expect("response should deserialize as PrepareRenameResponse");
        match response {
            PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } => {
                assert_eq!(placeholder, "width", "placeholder is the current name");
            }
            other => panic!("expected RangeWithPlaceholder for a width use, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_request_prepare_rename_refuses_keyword_returns_null() {
        let lsp = InProcessLsp::new();
        let uri = "file:///rename.ri";
        open_bracket(&lsp, uri).await;

        // 'structure' keyword at line 0, col 0 — Invariant 4 refusal. The
        // handler's Ok(None) serializes to JSON null so the editor refuses.
        let value = lsp
            .handle_request(
                "textDocument/prepareRename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 0, "character": 0 }
                }),
            )
            .await
            .expect("prepareRename on a keyword should succeed with a null payload");

        assert_eq!(
            value,
            Value::Null,
            "a non-renameable position serializes to null (editor refuses)"
        );
    }

    #[tokio::test]
    async fn handle_request_rename_returns_workspace_edit_with_four_changes() {
        let lsp = InProcessLsp::new();
        let uri = "file:///rename.ri";
        open_bracket(&lsp, uri).await;

        let value = lsp
            .handle_request(
                "textDocument/rename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 17 },
                    "newName": "girth"
                }),
            )
            .await
            .expect("rename request should reach the provider via the bridge");

        let edit: WorkspaceEdit =
            serde_json::from_value(value).expect("response should deserialize as WorkspaceEdit");
        let changes = edit.changes.expect("rename edit should carry changes");
        let parsed_uri = Url::parse(uri).unwrap();
        let edits = changes
            .get(&parsed_uri)
            .expect("edits keyed by the document uri");
        assert_eq!(edits.len(), 4, "bracket fixture: 1 decl + 3 uses of width");
        assert!(
            edits.iter().all(|e| e.new_text == "girth"),
            "every edit writes the new name"
        );
    }

    // --- task 4204 δ: SIGNAL TEST — documentHighlight through the bridge ---
    //
    // The GUI reaches the occurrence-highlight provider ONLY through the Tauri
    // `lsp_request` command -> `InProcessLsp::handle_request`, dispatched strictly
    // by method name. Without the "textDocument/documentHighlight" arm the GUI
    // would get Err("unsupported LSP method: …"); this drives the canonical
    // bracket fixture through that exact path. Positions match the server-handler
    // tests (width use at line 7 col 17; `structure` keyword at line 0 col 0).

    #[tokio::test]
    async fn handle_request_document_highlight_returns_text_highlights() {
        let lsp = InProcessLsp::new();
        let uri = "file:///highlight.ri";
        open_bracket(&lsp, uri).await;

        let value = lsp
            .handle_request(
                "textDocument/documentHighlight",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 17 }
                }),
            )
            .await
            .expect("documentHighlight request should reach the provider via the bridge");

        let highlights: Vec<DocumentHighlight> = serde_json::from_value(value)
            .expect("response should deserialize as Vec<DocumentHighlight>");
        assert_eq!(
            highlights.len(),
            4,
            "bracket fixture: 1 decl + 3 uses of width"
        );
        assert!(
            highlights
                .iter()
                .all(|h| h.kind == Some(DocumentHighlightKind::TEXT)),
            "every occurrence highlight is kind TEXT, got {highlights:?}"
        );

        // A non-resolvable position (the `structure` keyword at line 0 col 0)
        // yields Ok(None), which serializes to JSON null (no occurrences).
        let null_value = lsp
            .handle_request(
                "textDocument/documentHighlight",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 0, "character": 0 }
                }),
            )
            .await
            .expect("documentHighlight on a keyword should succeed with a null payload");
        assert_eq!(
            null_value,
            Value::Null,
            "a non-resolvable position serializes to null (no occurrences)"
        );
    }

    // --- task 4202 β: SIGNAL TEST — references through the bridge ---

    /// End-to-end signal for task 4202 β: the GUI `lsp_request` Tauri command
    /// dispatches through `InProcessLsp::handle_request`, so the Find-uses panel
    /// can only reach the references provider if the bridge has a
    /// `textDocument/references` arm. Drive a fixture with a member used twice
    /// through that exact path and assert the returned Location set.
    #[tokio::test]
    async fn handle_request_references_returns_locations() {
        let lsp = InProcessLsp::new();
        let uri = "file:///signal.ri";
        let source = "\
structure Bracket {
    param width : Length = 80mm
    let footprint = width * width
}
";

        lsp.handle_request(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "reify",
                    "version": 1,
                    "text": source
                }
            }),
        )
        .await
        .expect("didOpen should succeed");

        // Cursor on the `width` declaration token (line 1, char 10).
        let value = lsp
            .handle_request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 1, "character": 10 },
                    "context": { "includeDeclaration": true }
                }),
            )
            .await
            .expect("references request should reach the provider via the bridge");

        let locations: Vec<Location> =
            serde_json::from_value(value).expect("response should deserialize as Vec<Location>");
        // declaration ∪ 2 uses of width = 3 Locations, all in the same document.
        assert_eq!(
            locations.len(),
            3,
            "width: declaration + 2 uses = 3 Locations"
        );
        let expected_uri = Url::parse(uri).unwrap();
        assert!(
            locations.iter().all(|l| l.uri == expected_uri),
            "every Location must carry the opened document uri"
        );
    }
}
