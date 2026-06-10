use std::collections::{HashMap, HashSet};
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
                document_symbol_provider: Some(OneOf::Left(true)),
                // Occurrence highlight (task 4204 δ): the editor requests
                // textDocument/documentHighlight on cursor-idle.
                document_highlight_provider: Some(OneOf::Left(true)),
                // Advertise rename with prepareProvider so the editor issues
                // prepareRename (the Invariant-4 refusal gate) before rename.
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                references_provider: Some(OneOf::Left(true)),
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
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        drop(state);

        // Reuse the per-document cached parse (one parse per edit) instead of
        // re-parsing inside AnalysisContext::new; compile+check stay per-request.
        // The parse uses the document's own module path (no per-call argument).
        let text = doc.text.clone();
        let ctx = crate::analysis::AnalysisContext::from_parsed(doc.parsed_module());
        Ok(crate::hover::compute_hover_in_context(&ctx, &text, position))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let state = self.state.read().await;
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
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
        // async worker thread stays free for other LSP requests. The primary
        // document's parse comes from its per-document cache (one parse per
        // edit): the Arc<DocumentState> moves into the blocking task and the
        // parse — cache-filling on the first request after an edit — runs there.
        let location = match tokio::task::spawn_blocking(move || {
            let parsed = doc.parsed_module();
            let primary_source = doc.text.as_str();
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
                crate::goto_def::compute_goto_definition_cross_file_with_parsed(
                    &parsed,
                    primary_source,
                    &uri,
                    position,
                    &resolve_import,
                )
            } else {
                // No workspace root — fall back to single-file resolution.
                crate::goto_def::compute_goto_definition_with_parsed(
                    &parsed,
                    primary_source,
                    &uri,
                    position,
                )
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
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        drop(state);

        // Reuse the per-document cached parse (one parse per edit).
        let text = doc.text.clone();
        let ctx = crate::analysis::AnalysisContext::from_parsed(doc.parsed_module());
        let items = crate::completion::compute_completions_in_context(&ctx, &text, position);
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
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        drop(state);

        // Reuse the per-document cached parse (one parse per edit).
        let text = doc.text.clone();
        let parsed = doc.parsed_module();
        let symbols = crate::analysis::compute_document_symbols_from_parsed(&parsed, &text);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Brief read lock: snapshot the document text, then drop it before the
        // (CPU-only) parse + scope walk — mirrors document_symbol/prepare_rename.
        let state = self.state.read().await;
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        drop(state);

        // Reuse the per-document cached parse (one parse per edit) — prelude-aware
        // for AST-shape consistency with goto_def/rename.
        let text = doc.text.clone();
        let parsed = doc.parsed_module();

        // The δ producer returns None for non-resolvable positions (keywords/
        // literals/types/declaration names), which the editor renders as "no
        // occurrences". Its spans are inherently in-document (boundary row 7),
        // so no active-doc filtering is needed.
        Ok(crate::references::compute_document_highlights(
            &text, &parsed, position,
        ))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;

        // κ (task 4210): prepare_rename now resolves cross-file structure homes via
        // the same workspace rig as goto_definition. The single-file α producer
        // keeps its exact refusal surface (Invariant 4 — keywords/literals/builtins/
        // types/declaration names); the cross-file producer lifts the refusal ONLY
        // for structures reachable through the import graph. None passes through so
        // the editor refuses the rename.
        let state = self.state.read().await;
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        let workspace_root = state.workspace_root.clone();
        let stdlib_path = state.stdlib_path.clone();
        let open_docs = state.documents.snapshot_as_path_map();
        drop(state);

        let target = match tokio::task::spawn_blocking(move || {
            let parsed = doc.parsed_module();
            let primary_source = doc.text.as_str();
            if let Some(root) = workspace_root {
                let stdlib_root =
                    stdlib_path.unwrap_or_else(|| root.join("crates/reify-compiler/stdlib"));
                let resolver = reify_compiler::module_dag::ModuleResolver::new(root, stdlib_root);
                let resolve_import = |import_path: &str| -> Option<(Url, String)> {
                    let path = resolver.resolve_import_path(import_path).ok()?;
                    let source = open_docs
                        .get(&path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&path).ok())?;
                    let target_uri = Url::from_file_path(&path).ok()?;
                    Some((target_uri, source))
                };
                crate::references::prepare_rename_cross_file(
                    primary_source,
                    &parsed,
                    &uri,
                    position,
                    &resolve_import,
                )
            } else {
                crate::references::prepare_rename(primary_source, &parsed, position)
            }
        })
        .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("prepare_rename blocking task failed: {e}");
                None
            }
        };

        Ok(target.map(|target| {
            PrepareRenameResponse::RangeWithPlaceholder {
                range: target.range,
                placeholder: target.placeholder,
            }
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        // κ (task 4210): rename now produces a cross-file WorkspaceEdit via the same
        // workspace rig as goto_definition. new_name validation + the re-parse-clean
        // guarantee (Invariant 5) live in the producers, so the handler stays a thin
        // forwarder; Ok(None) is preserved for unknown uri / non-renameable cursor /
        // invalid new_name.
        let state = self.state.read().await;
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        let workspace_root = state.workspace_root.clone();
        let stdlib_path = state.stdlib_path.clone();
        // Snapshot open documents: a path-keyed map for resolve_import's
        // editor-buffer-over-disk fallback (and as the open-text override for
        // build_workspace_docs so the in-memory version wins over the disk copy).
        // The workspace_docs (Url, String) list is built inside spawn_blocking
        // because it may need to read closed-importer files from disk.
        let open_docs = state.documents.snapshot_as_path_map();
        drop(state);

        let edit = match tokio::task::spawn_blocking(move || {
            let parsed = doc.parsed_module();
            let primary_source = doc.text.as_str();
            if let Some(root) = workspace_root {
                let stdlib_root =
                    stdlib_path.unwrap_or_else(|| root.join("crates/reify-compiler/stdlib"));
                let resolver = reify_compiler::module_dag::ModuleResolver::new(root.clone(), stdlib_root);
                // Build workspace_docs here (blocking context) so the disk walk
                // for closed importers is off the async worker thread.  Open-buffer
                // text overrides the on-disk copy for already-open files.
                let workspace_docs = build_workspace_docs(&root, &open_docs);
                let resolve_import = |import_path: &str| -> Option<(Url, String)> {
                    let path = resolver.resolve_import_path(import_path).ok()?;
                    let source = open_docs
                        .get(&path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&path).ok())?;
                    let target_uri = Url::from_file_path(&path).ok()?;
                    Some((target_uri, source))
                };
                crate::references::compute_rename_cross_file(
                    primary_source,
                    &parsed,
                    &uri,
                    position,
                    &new_name,
                    &workspace_docs,
                    &resolve_import,
                )
            } else {
                crate::references::compute_rename(primary_source, &parsed, &uri, position, &new_name)
            }
        })
        .await
        {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("rename blocking task failed: {e}");
                None
            }
        };
        Ok(edit)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        // κ (task 4210): references now follow the import graph. Assemble the same
        // cross-file rig as goto_definition — workspace_root + ModuleResolver +
        // resolve_import (editor buffer over disk) + the open-document snapshot —
        // and move the parsing + blocking filesystem I/O off the async worker.
        let state = self.state.read().await;
        let doc = match state.documents.get(&uri) {
            Some(doc) => doc,
            None => return Ok(None),
        };
        let workspace_root = state.workspace_root.clone();
        let stdlib_path = state.stdlib_path.clone();
        // Snapshot open documents: a path-keyed map for resolve_import's
        // editor-buffer-over-disk fallback (and as the open-text override for
        // build_workspace_docs so the in-memory version wins over the disk copy).
        // The workspace_docs (Url, String) list is built inside spawn_blocking
        // because it may need to read closed-importer files from disk.
        let open_docs = state.documents.snapshot_as_path_map();
        drop(state);

        let locations = match tokio::task::spawn_blocking(move || {
            let parsed = doc.parsed_module();
            let primary_source = doc.text.as_str();
            if let Some(root) = workspace_root {
                let stdlib_root =
                    stdlib_path.unwrap_or_else(|| root.join("crates/reify-compiler/stdlib"));
                let resolver = reify_compiler::module_dag::ModuleResolver::new(root.clone(), stdlib_root);
                // Build workspace_docs here (blocking context) so the disk walk
                // for closed importers is off the async worker thread.  Open-buffer
                // text overrides the on-disk copy for already-open files.
                let workspace_docs = build_workspace_docs(&root, &open_docs);
                let resolve_import = |import_path: &str| -> Option<(Url, String)> {
                    let path = resolver.resolve_import_path(import_path).ok()?;
                    let source = open_docs
                        .get(&path)
                        .cloned()
                        .or_else(|| std::fs::read_to_string(&path).ok())?;
                    let target_uri = Url::from_file_path(&path).ok()?;
                    Some((target_uri, source))
                };
                crate::references::compute_references_cross_file(
                    primary_source,
                    &parsed,
                    &uri,
                    position,
                    include_declaration,
                    &workspace_docs,
                    &resolve_import,
                )
            } else {
                // No workspace root — single-file references (cross-module symbols
                // remain refused, as before κ).
                crate::references::compute_references(
                    primary_source,
                    &parsed,
                    &uri,
                    position,
                    include_declaration,
                )
            }
        })
        .await
        {
            Ok(locs) => locs,
            Err(e) => {
                tracing::error!("references blocking task failed: {e}");
                None
            }
        };
        Ok(locations)
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

// ─────────────────────────────────────────────────────────────────────────────
// Workspace-docs builder (task 4466, part 2)
// ─────────────────────────────────────────────────────────────────────────────

/// Walk `root` recursively for `*.ri` files and merge with the open-buffer
/// snapshot so every importer — whether currently open or only on disk — feeds
/// the cross-file collectors.
///
/// Resolution order per file:
/// 1. If the file's `PathBuf` is a key in `open`, the **in-memory text** wins.
/// 2. Otherwise the file is **read from disk**; unreadable files are skipped.
///
/// Directory pruning: `target`, `.git`, `node_modules`, and any name beginning
/// with `.` are not descended.  Symlinks are never followed.
///
/// Files in `open` but **not** under `root` (e.g. a buffer from a different
/// project that happens to be open) are appended at the end so the cross-file
/// collectors still see them.
fn build_workspace_docs(root: &std::path::Path, open: &HashMap<PathBuf, String>) -> Vec<(Url, String)> {
    let mut docs: Vec<(Url, String)> = Vec::new();
    // Track canonical paths we emitted to avoid double-including open files.
    let mut covered: HashSet<PathBuf> = HashSet::new();
    collect_ri_files(root, &mut docs, &mut covered, open);
    // Append open buffers that are not under root (cross-root imports).
    for (path, text) in open {
        if !covered.contains(path.as_path()) {
            if let Ok(url) = Url::from_file_path(path) {
                docs.push((url, text.clone()));
            }
        }
    }
    docs
}

/// Recursively collect `*.ri` files from `dir` into `docs`.
///
/// See [`build_workspace_docs`] for the full contract.
fn collect_ri_files(
    dir: &std::path::Path,
    docs: &mut Vec<(Url, String)>,
    covered: &mut HashSet<PathBuf>,
    open: &HashMap<PathBuf, String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // unreadable dir — silently skip
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Use entry.file_type() (NOT metadata) so symlinks are detected without
        // following them — the check below skips them unconditionally.
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        // Never follow symlinks.
        if file_type.is_symlink() {
            continue;
        }
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Skip hidden entries (names starting with '.').
        if file_name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            // Skip well-known build / VCS / package-manager directories.
            if matches!(file_name.as_str(), "target" | "node_modules") {
                continue;
            }
            collect_ri_files(&path, docs, covered, open);
        } else if file_type.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("ri")
        {
            // Open override wins over disk; unreadable disk files are skipped.
            let text = if let Some(t) = open.get(&path) {
                t.clone()
            } else {
                match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };
            if let Ok(url) = Url::from_file_path(&path) {
                covered.insert(path);
                docs.push((url, text));
            }
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
    async fn initialize_advertises_rename_provider_with_prepare() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let init_result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        // rename_provider must be advertised as RenameOptions with
        // prepareProvider: true so the editor issues prepareRename before rename
        // (task 4203 γ; PRD §Contract invariants 4-5).
        match init_result.capabilities.rename_provider {
            Some(OneOf::Right(RenameOptions {
                prepare_provider, ..
            })) => {
                assert_eq!(
                    prepare_provider,
                    Some(true),
                    "rename_provider should advertise prepareProvider: true"
                );
            }
            other => panic!(
                "expected rename_provider = Some(OneOf::Right(RenameOptions {{ prepare_provider: Some(true), .. }})), got {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn initialize_advertises_references_provider() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let init_result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        assert!(
            init_result.capabilities.references_provider.is_some(),
            "should advertise references_provider (task 4202 β)"
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

    // --- task 4250 step-13: single parse per edit, shared & invalidated ---

    /// A single parse is cached per document version, shared across the store's
    /// `get()` path, and structurally invalidated by an edit (did_change).
    ///
    /// Cache reuse is proven with `Arc::ptr_eq` (same allocation = no re-parse);
    /// invalidation is proven by a fresh allocation reflecting the new text after
    /// a version bump. The test also guards that every primary-document provider
    /// (hover/goto-def/completion/document_symbol) still returns correct results
    /// once the per-document cache is the parse source — output-equivalence
    /// across the cache-wiring refactor.
    #[tokio::test]
    async fn single_parse_per_edit_shared_and_invalidated() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await; // v1 = bracket_source

        // v1: the cached parse is stable across repeated store lookups — two
        // get()+parsed_module() calls return the SAME Arc allocation (a cache
        // hit, i.e. a single parse per version).
        let a = {
            let state = server.state().read().await;
            let doc = state.documents.get(&uri).expect("doc present at v1");
            doc.parsed_module()
        };
        let b = {
            let state = server.state().read().await;
            let doc = state.documents.get(&uri).expect("doc present at v1");
            doc.parsed_module()
        };
        assert!(
            Arc::ptr_eq(&a, &b),
            "same document version must reuse one cached parse (no re-parse)"
        );
        assert_eq!(
            a.declarations.len(),
            1,
            "bracket_source has a single top-level declaration"
        );

        // Providers must return correct results with the cache as the parse source.
        assert!(
            server
                .hover(HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: uri.clone() },
                        position: Position::new(1, 10), // 'width'
                    },
                    work_done_progress_params: Default::default(),
                })
                .await
                .unwrap()
                .is_some(),
            "hover should resolve 'width' after did_open"
        );
        assert!(
            server
                .goto_definition(GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: uri.clone() },
                        position: Position::new(9, 15), // 'thickness' in constraint
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .unwrap()
                .is_some(),
            "goto-def should resolve 'thickness' after did_open"
        );
        let comp = server
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 0),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .unwrap();
        assert!(comp.is_some(), "completion should return items after did_open");
        match server
            .document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
        {
            Some(DocumentSymbolResponse::Nested(syms)) => {
                assert_eq!(syms.len(), 1, "bracket_source has one top-level symbol");
                assert_eq!(syms[0].name, "Bracket");
            }
            other => panic!("expected Some(Nested(..)), got {other:?}"),
        }

        // Edit (did_change) to a different valid source bumps the version and
        // structurally invalidates the cache: the new document owns a fresh,
        // empty cache, so its parse is a DIFFERENT Arc reflecting the new text.
        let v2 = "structure A {\n    param x: Length = 1mm\n}\nstructure B {\n    param y: Length = 2mm\n}";
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: v2.to_string(),
                }],
            })
            .await;

        let c = {
            let state = server.state().read().await;
            let doc = state.documents.get(&uri).expect("doc present at v2");
            doc.parsed_module()
        };
        assert!(
            !Arc::ptr_eq(&a, &c),
            "an edit must invalidate the cache (a fresh parse allocation)"
        );
        assert_eq!(
            c.declarations.len(),
            2,
            "the post-edit cached parse must reflect the new text (two declarations)"
        );
    }

    /// A provider handler must CONSUME the per-document parse cache — i.e. fill
    /// it as a side effect — rather than re-parsing the document internally.
    ///
    /// The big integration test above proves the cache mechanism and that the
    /// providers return correct results, but those are independent: a provider
    /// that ignored the cache and re-parsed via `AnalysisContext::new` would
    /// still pass every assertion there. This test ties the wiring to the cache:
    /// on a freshly-opened document whose cache is cold, running hover must leave
    /// the cache populated. A regression that reverted hover to internal parsing
    /// would leave the cache cold here and fail — guarding the refactor's core
    /// performance goal (one parse per edit shared across providers).
    #[tokio::test]
    async fn hover_handler_consumes_per_document_parse_cache() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        // After did_open the per-document cache is cold: diagnostics parse via a
        // separate path and the cache fills lazily on the first provider request.
        // `doc` is an Arc clone of the same DocumentState the handler will fetch,
        // so it observes the interior-mutable cache the handler fills.
        let doc = {
            let state = server.state().read().await;
            state
                .documents
                .get(&uri)
                .expect("doc present after did_open")
        };
        assert!(
            doc.peek_cached_parse().is_none(),
            "cache must be cold before any provider runs"
        );

        // Exercise a provider through the real handler.
        server
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 10), // 'width'
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap();

        // The handler must have populated the cache (consumed it) — not re-parsed
        // internally and discarded the result.
        let filled = doc.peek_cached_parse();
        assert!(
            filled.is_some(),
            "the hover handler must populate the per-document parse cache, not re-parse internally"
        );

        // And the parse the handler cached is exactly what later reads reuse —
        // a single shared allocation per edit.
        assert!(
            Arc::ptr_eq(filled.as_ref().unwrap(), &doc.parsed_module()),
            "subsequent parsed_module() must reuse the handler-populated cache (one parse per edit)"
        );
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

    // --- task 4203 γ: prepare_rename handler tests ---
    //
    // Positions reference the canonical bracket fixture (0-based):
    //   line 0: `structure Bracket {`
    //   line 1: `    param width: Length = 80mm`   (Scalar type at col 17)
    //   line 7: `    let volume = width * height * thickness`  (width use at col 17)

    #[tokio::test]
    async fn prepare_rename_returns_target_for_width_use() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let result = server
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(7, 17), // 'width' use in `let volume = width * ...`
            })
            .await
            .unwrap();

        match result {
            Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, range }) => {
                assert_eq!(placeholder, "width", "placeholder is the current name");
                // The range covers the 5-char identifier token on line 7.
                assert_eq!(range.start, Position::new(7, 17));
                assert_eq!(range.end, Position::new(7, 22));
            }
            other => panic!("expected RangeWithPlaceholder for a width use, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn prepare_rename_refuses_type_name() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        // 'Scalar' (a type) on line 1 — Invariant 4 refusal.
        let result = server
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(1, 17),
            })
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "prepare_rename must refuse a type name (Invariant 4), got {result:?}"
        );
    }

    #[tokio::test]
    async fn prepare_rename_refuses_keyword() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        // 'structure' keyword at line 0, col 0 — Invariant 4 refusal.
        let result = server
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 0),
            })
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "prepare_rename must refuse a keyword, got {result:?}"
        );
    }

    #[tokio::test]
    async fn prepare_rename_unknown_uri_returns_none() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Never opened — not in the document store.
        let result = server
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse("file:///never_opened.ri").unwrap(),
                },
                position: Position::new(1, 10),
            })
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "prepare_rename for an unknown URI should return Ok(None)"
        );
    }

    // --- task 4203 γ: rename handler tests ---

    fn rename_params(uri: Url, line: u32, character: u32, new_name: &str) -> RenameParams {
        RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(line, character),
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn rename_width_edits_decl_and_all_uses() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let edit = server
            .rename(rename_params(uri.clone(), 7, 17, "girth"))
            .await
            .unwrap()
            .expect("renaming a width use returns a WorkspaceEdit");

        let edits = edit
            .changes
            .expect("changes present")
            .get(&uri)
            .expect("edits keyed by uri")
            .clone();
        assert_eq!(edits.len(), 4, "bracket fixture: 1 decl + 3 uses of width");
        assert!(
            edits.iter().all(|e| e.new_text == "girth"),
            "every edit writes the new name"
        );

        // Invariant 5: applying the edits yields a buffer that re-parses clean.
        // Apply descending by start offset so earlier offsets are not shifted.
        let mut buffer = reify_test_support::bracket_source().to_string();
        let mut ordered = edits.clone();
        ordered.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        for e in ordered.iter().rev() {
            let start = crate::convert::position_to_offset(&buffer, e.range.start);
            let end = crate::convert::position_to_offset(&buffer, e.range.end);
            buffer.replace_range(start..end, &e.new_text);
        }
        let reparsed = reify_syntax::parse(&buffer, reify_core::ModulePath::single("test"));
        assert!(
            reparsed.errors.is_empty(),
            "renamed buffer must re-parse clean (Invariant 5): {:?}\n{buffer}",
            reparsed.errors
        );
    }

    #[tokio::test]
    async fn rename_refuses_non_renameable_position() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;
        // 'Scalar' type position — not renameable.
        let result = server.rename(rename_params(uri, 1, 17, "girth")).await.unwrap();
        assert!(
            result.is_none(),
            "rename must refuse a non-renameable position, got {result:?}"
        );
    }

    #[tokio::test]
    async fn rename_refuses_invalid_new_name() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;
        // Renameable position (width use), but invalid identifiers must be refused.
        for bad in ["let", "2x"] {
            let result = server
                .rename(rename_params(uri.clone(), 7, 17, bad))
                .await
                .unwrap();
            assert!(
                result.is_none(),
                "rename must refuse invalid new_name {bad:?}, got {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn rename_unknown_uri_returns_none() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let result = server
            .rename(rename_params(
                Url::parse("file:///never_opened.ri").unwrap(),
                7,
                17,
                "girth",
            ))
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "rename for an unknown URI should return Ok(None)"
        );
    }

    // --- task 4204 δ: document_highlight handler tests ---
    //
    // Positions reference the canonical bracket fixture (0-based); line 7 is
    // `    let volume = width * height * thickness` with the `width` use at col 17.

    fn document_highlight_params(uri: Url, line: u32, character: u32) -> DocumentHighlightParams {
        DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(line, character),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn document_highlight_handler_returns_text_highlights_for_width() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        let result = server
            .document_highlight(document_highlight_params(uri, 7, 17))
            .await
            .unwrap();

        let highlights = result.expect("a width use should produce document highlights");
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
    }

    #[tokio::test]
    async fn document_highlight_on_keyword_returns_none() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        // 'structure' keyword at line 0, col 0 — not a resolvable value symbol.
        let result = server
            .document_highlight(document_highlight_params(uri, 0, 0))
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "document_highlight on a keyword should return Ok(None), got {result:?}"
        );
    }

    #[tokio::test]
    async fn document_highlight_unknown_uri_returns_none() {
        let (service, _socket) = test_service();
        let server = service.inner();

        // Never opened — not in the document store.
        let result = server
            .document_highlight(document_highlight_params(
                Url::parse("file:///never_opened.ri").unwrap(),
                7,
                17,
            ))
            .await
            .unwrap();
        assert!(
            result.is_none(),
            "document_highlight for an unknown URI should return Ok(None)"
        );
    }

    #[tokio::test]
    async fn initialize_advertises_document_highlight_provider() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let init_result = server
            .initialize(InitializeParams::default())
            .await
            .unwrap();

        assert!(
            init_result
                .capabilities
                .document_highlight_provider
                .is_some(),
            "should advertise document_highlight_provider (task 4204 δ)"
        );
    }

    // --- task 4202 β: references handler tests ---

    /// Build a ReferenceParams at `pos` in `uri` with the given declaration flag.
    fn ref_params(uri: Url, pos: Position, include_declaration: bool) -> ReferenceParams {
        ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: pos,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration,
            },
        }
    }

    #[tokio::test]
    async fn references_handler_returns_locations_for_member() {
        let (service, _socket) = test_service();
        let server = service.inner();
        let uri = open_bracket_source(server).await;

        // `width` in bracket_source is used 3× (declaration + 3 uses = 4 spans).
        // Cursor on the `width` token at line 1, char 10 — the same position the
        // hover handler test uses for 'width'.
        let with_decl = server
            .references(ref_params(uri.clone(), Position::new(1, 10), true))
            .await
            .unwrap()
            .expect("references should resolve for the width member");
        assert_eq!(
            with_decl.len(),
            4,
            "declaration ∪ 3 uses of width = 4 Locations"
        );
        assert!(
            with_decl.iter().all(|l| l.uri == uri),
            "every Location must carry the document uri"
        );

        // include_declaration=false drops the declaration token → the 3 use spans.
        let without_decl = server
            .references(ref_params(uri.clone(), Position::new(1, 10), false))
            .await
            .unwrap()
            .expect("references resolve without the declaration");
        assert_eq!(
            without_decl.len(),
            3,
            "include_declaration=false yields the 3 use spans"
        );

        // A position off any identifier (column 0 = indentation whitespace) → None.
        let off_ident = server
            .references(ref_params(uri.clone(), Position::new(1, 0), true))
            .await
            .unwrap();
        assert!(
            off_ident.is_none(),
            "a position off any identifier must return Ok(None)"
        );

        // An unknown URI → Ok(None) (mirrors document_symbol_unknown_uri_returns_none).
        let unknown = server
            .references(ref_params(
                Url::parse("file:///never_opened.ri").unwrap(),
                Position::new(1, 10),
                true,
            ))
            .await
            .unwrap();
        assert!(
            unknown.is_none(),
            "references for an unknown URI must return Ok(None)"
        );
    }

    #[tokio::test]
    async fn references_handler_follows_imports_across_files() {
        // κ (task 4210): the `references` handler now follows the import graph.
        // Mirror goto_definition_resolves_imported_symbol_across_files: parts.ri on
        // disk declares `structure Hole`; main.ri (open) imports + constructs it.
        // Find-references on the main.ri `Hole` use must span BOTH files — proving
        // the handler assembles the workspace rig (workspace_root + ModuleResolver +
        // resolve_import + workspace_docs) and calls the cross-file collector rather
        // than the single-file producer (which refuses cross-module symbols).
        let (service, _socket) = test_service();
        let server = service.inner();

        let tmp_dir =
            std::env::temp_dir().join(format!("reify-lsp-refs-xfile-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(tmp_dir.join("parts.ri"), parts_source).unwrap();

        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // main.ri uses the parenthesized constructor `Hole()` so the construction
        // site lowers to a SubDecl carrying structure_name="Hole" (the bare
        // `sub hole = Hole` form is a syntax error and never lowers to a SubDecl).
        let main_source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n}";
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

        // Cursor on the main.ri `Hole` use in `sub hole = Hole()` (line 2, col 15).
        let locations = server
            .references(ref_params(main_uri.clone(), Position::new(2, 15), true))
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&tmp_dir);

        let locations =
            locations.expect("cross-file references should resolve for the imported Hole use");
        assert_eq!(
            locations.len(),
            3,
            "home decl + import entity token + sub use = 3 cross-file Locations, got {locations:?}"
        );
        assert!(
            locations.iter().any(|l| l.uri.path().ends_with("parts.ri")),
            "references must include a parts.ri Location (the structure decl), got {locations:?}"
        );
        assert!(
            locations.iter().any(|l| l.uri.path().ends_with("main.ri")),
            "references must include main.ri Locations (import entity + sub use), got {locations:?}"
        );
        // The parts.ri Location points at `structure Hole` on line 0.
        let parts_loc = locations
            .iter()
            .find(|l| l.uri.path().ends_with("parts.ri"))
            .unwrap();
        assert_eq!(
            parts_loc.range.start.line, 0,
            "parts.ri Location is the structure Hole decl on line 0"
        );
    }

    #[tokio::test]
    async fn prepare_rename_and_rename_follow_imports_across_files() {
        // κ (task 4210): prepare_rename + rename now follow the import graph. The
        // formerly-refused cross-module symbol `Hole` becomes renameable, and the
        // rename emits a multi-file WorkspaceEdit that re-parses clean (Invariant 5).
        let (service, _socket) = test_service();
        let server = service.inner();

        let tmp_dir =
            std::env::temp_dir().join(format!("reify-lsp-rename-xfile-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(tmp_dir.join("parts.ri"), parts_source).unwrap();

        let root_uri = Url::from_file_path(&tmp_dir).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        let main_source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n}";
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

        // (a) prepare_rename on the main.ri `Hole` use (line 2, col 15) — the
        // single-file cross-module refusal is lifted.
        let prepared = server
            .prepare_rename(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position: Position::new(2, 15),
            })
            .await
            .unwrap();

        // (b) rename Hole→Bore from the same cursor.
        let edit = server
            .rename(rename_params(main_uri.clone(), 2, 15, "Bore"))
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&tmp_dir);

        // (a) assertions: a cross-module structure use is now a rename target.
        match prepared {
            Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) => {
                assert_eq!(
                    placeholder, "Hole",
                    "placeholder is the current name of the cross-module symbol"
                );
            }
            other => panic!(
                "prepare_rename must lift the cross-module refusal and return a target, got {other:?}"
            ),
        }

        // (b) assertions: a multi-file WorkspaceEdit keyed by BOTH files.
        let changes = edit
            .expect("cross-file rename returns a WorkspaceEdit")
            .changes
            .expect("changes present");
        let parts_key = changes
            .keys()
            .find(|u| u.path().ends_with("parts.ri"))
            .expect("changes keyed by parts.ri (the home declaration)")
            .clone();
        let main_key = changes
            .keys()
            .find(|u| u.path().ends_with("main.ri"))
            .expect("changes keyed by main.ri (import entity + sub use)")
            .clone();
        assert!(
            changes.values().flatten().all(|e| e.new_text == "Bore"),
            "every edit writes the new name Bore"
        );
        assert_eq!(
            changes.get(&parts_key).unwrap().len(),
            1,
            "parts.ri: 1 edit (the structure Hole decl token)"
        );
        assert_eq!(
            changes.get(&main_key).unwrap().len(),
            2,
            "main.ri: 2 edits (import entity token + sub use)"
        );

        // Invariant 5: applying each file's edits yields a buffer that re-parses
        // clean (no new ERROR/recovery nodes) — `structure Bore`, `import
        // parts.Bore`, and `sub hole = Bore()` are all valid.
        for (key, original) in [(&parts_key, parts_source), (&main_key, main_source)] {
            let mut buffer = original.to_string();
            let mut edits = changes.get(key).unwrap().clone();
            edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
            for e in edits.iter().rev() {
                let start = crate::convert::position_to_offset(&buffer, e.range.start);
                let end = crate::convert::position_to_offset(&buffer, e.range.end);
                buffer.replace_range(start..end, &e.new_text);
            }
            let reparsed = reify_syntax::parse(&buffer, reify_core::ModulePath::single("test"));
            assert!(
                reparsed.errors.is_empty(),
                "renamed buffer for {key} must re-parse clean (Invariant 5): {:?}\n{buffer}",
                reparsed.errors
            );
        }
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
            "structure Widget {\n    param size: Length = 5mm\n}",
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
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
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
            "structure Hole {\n    param diameter: Length = 10mm\n}",
        )
        .unwrap();
        std::fs::write(
            tmp_dir.join("fasteners.ri"),
            "structure Bolt {\n    param length: Length = 20mm\n}",
        )
        .unwrap();
        std::fs::write(
            tmp_dir.join("utils.ri"),
            "structure Helper {\n    param size: Length = 5mm\n}",
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
        let disk_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
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
        let editor_source = "structure Plate {\n    param width: Length = 5mm\n}\nstructure Hole {\n    param diameter: Length = 10mm\n}";
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

    // -------------------------------------------------------------------------
    // build_workspace_docs — unit tests (task 4466 step-9, RED)
    // -------------------------------------------------------------------------

    /// Verify that `build_workspace_docs` walks a project root recursively,
    /// collects only *.ri files, skips ignored directories, and that a path
    /// present in the `open` override map supplies the in-memory text instead
    /// of the on-disk content.
    #[test]
    fn build_workspace_docs_walks_ri_files_and_applies_open_override() {
        use std::path::Path;

        let tmp = std::env::temp_dir()
            .join(format!("reify-lsp-bwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp); // clean slate
        std::fs::create_dir_all(&tmp).unwrap();

        // Two .ri files at root level.
        std::fs::write(tmp.join("a.ri"), "file_a_on_disk").unwrap();
        std::fs::write(tmp.join("b.ri"), "file_b_on_disk").unwrap();
        // One .ri file in a nested subdir — must be discovered recursively.
        let sub = tmp.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("c.ri"), "file_c_on_disk").unwrap();
        // A non-.ri file — must be excluded.
        std::fs::write(tmp.join("d.txt"), "not_ri").unwrap();
        // A *.ri file inside an ignored dir (target/) — must be excluded.
        let ignored = tmp.join("target");
        std::fs::create_dir_all(&ignored).unwrap();
        std::fs::write(ignored.join("e.ri"), "should_be_ignored").unwrap();

        // `open` override: a.ri uses in-memory text (simulates an open editor buffer).
        let a_path = tmp.join("a.ri");
        let mut open: HashMap<PathBuf, String> = HashMap::new();
        open.insert(a_path.clone(), "file_a_open".to_string());

        let docs = build_workspace_docs(Path::new(&tmp), &open);

        // Clean up before assertions so a panic doesn't leave the dir behind.
        let _ = std::fs::remove_dir_all(&tmp);

        // Collect the (path-suffix, content) pairs for easy assertions.
        let collected: Vec<(String, String)> = docs
            .iter()
            .map(|(url, content)| (url.path().to_string(), content.clone()))
            .collect();

        // Every .ri file (outside ignored dirs) must be present.
        let has = |suffix: &str| collected.iter().any(|(p, _)| p.ends_with(suffix));
        assert!(has("a.ri"),   "a.ri must be in docs, got {collected:?}");
        assert!(has("b.ri"),   "b.ri must be in docs, got {collected:?}");
        assert!(has("c.ri"),   "sub/c.ri must be in docs, got {collected:?}");

        // Non-.ri file excluded.
        assert!(!has("d.txt"), "d.txt must NOT be in docs, got {collected:?}");
        // .ri inside target/ excluded.
        assert!(!has("e.ri"),  "target/e.ri must NOT be in docs, got {collected:?}");

        // Exactly three docs expected.
        assert_eq!(
            docs.len(), 3,
            "expected 3 docs (a.ri, b.ri, sub/c.ri), got {} docs: {collected:?}",
            docs.len()
        );

        // Open override: a.ri must serve the in-memory text, NOT the disk content.
        let a_content = collected
            .iter()
            .find(|(p, _)| p.ends_with("a.ri"))
            .map(|(_, c)| c.as_str())
            .unwrap_or("NOT FOUND");
        assert_eq!(
            a_content, "file_a_open",
            "open override for a.ri must be returned instead of disk content"
        );

        // b.ri must serve the on-disk text (not in the open map).
        let b_content = collected
            .iter()
            .find(|(p, _)| p.ends_with("b.ri"))
            .map(|(_, c)| c.as_str())
            .unwrap_or("NOT FOUND");
        assert_eq!(
            b_content, "file_b_on_disk",
            "b.ri (not in open map) must serve on-disk content"
        );
    }

    // -------------------------------------------------------------------------
    // Closed-importer disk indexing — integration tests (task 4466 step-11, RED)
    // -------------------------------------------------------------------------

    /// Mirror `references_handler_follows_imports_across_files` but with the
    /// importer CLOSED on disk.
    ///
    /// Setup: `parts.ri` declares `structure Hole` and is **opened** in the LSP
    /// (did_open).  `other.ri` imports and constructs `Hole` but is **never
    /// opened** — it only exists on disk.
    ///
    /// Cursor: on the `Hole` **declaration** in `parts.ri` (line 0, col 10 —
    /// "structure Hole", `Hole` starts at char 10).
    ///
    /// Expected: the `references` response includes at least one Location in
    /// `other.ri`, proving the handler picked up the closed importer via the
    /// disk walk (step-12 wires `build_workspace_docs` into the handler).
    ///
    /// This test FAILS on pre-step-12 code because only open docs are scanned.
    #[tokio::test]
    async fn references_handler_finds_closed_disk_importer() {
        let (service, _socket) = test_service();
        let server = service.inner();

        let tmp = std::env::temp_dir()
            .join(format!("reify-lsp-refs-closed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // parts.ri — the HOME file; will be opened in LSP.
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(tmp.join("parts.ri"), parts_source).unwrap();

        // other.ri — CLOSED importer; exists on disk only.
        let other_source = "import parts.Hole\nstructure A {\n    sub h = Hole()\n}";
        std::fs::write(tmp.join("other.ri"), other_source).unwrap();

        // Initialize with workspace root so the disk walk has a root.
        let root_uri = Url::from_file_path(&tmp).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open ONLY parts.ri — other.ri is intentionally left closed.
        let parts_uri = Url::from_file_path(tmp.join("parts.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: parts_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: parts_source.to_string(),
                },
            })
            .await;

        // Cursor on the `Hole` declaration in parts.ri:
        // "structure Hole ..." — "structure " is 10 chars → col 10.
        let locations = server
            .references(ref_params(parts_uri.clone(), Position::new(0, 10), true))
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&tmp);

        let locations = locations.expect(
            "references on Hole declaration should return Some(locations), not None",
        );
        // Must include at least one Location in the closed other.ri.
        assert!(
            locations.iter().any(|l| l.uri.path().ends_with("other.ri")),
            "references must include a Location in the CLOSED other.ri \
             (disk-walk importer discovery), got {locations:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Closed-importer disk indexing — rename integration test (task 4466 step-13, RED)
    // -------------------------------------------------------------------------

    /// Mirror `prepare_rename_and_rename_follow_imports_across_files` but with
    /// the importer CLOSED on disk.
    ///
    /// Setup: `parts.ri` declares `structure Hole` and is **opened** in the LSP
    /// (did_open).  `other.ri` imports and constructs `Hole` but is **never
    /// opened** — it only exists on disk.
    ///
    /// Action: rename `Hole` → `Bore` on the declaration cursor in `parts.ri`
    /// (line 0, col 10).
    ///
    /// Expected: the returned `WorkspaceEdit.changes` contains an entry keyed
    /// by `other.ri`'s URI with at least one edit replacing `Hole` with `Bore`,
    /// proving the rename handler picked up the closed importer via the disk walk
    /// (step-14 wires `build_workspace_docs` into the handler).
    ///
    /// This test FAILS on pre-step-14 code because only open docs are included
    /// in `workspace_docs`, so the closed `other.ri` is invisible to the rename
    /// collector.
    #[tokio::test]
    async fn rename_handler_includes_closed_disk_importer_edits() {
        let (service, _socket) = test_service();
        let server = service.inner();

        let tmp = std::env::temp_dir()
            .join(format!("reify-lsp-rename-closed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // parts.ri — the HOME file; will be opened in LSP.
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(tmp.join("parts.ri"), parts_source).unwrap();

        // other.ri — CLOSED importer; exists on disk only.
        let other_source = "import parts.Hole\nstructure A {\n    sub h = Hole()\n}";
        std::fs::write(tmp.join("other.ri"), other_source).unwrap();

        // Initialize with workspace root so the disk walk has a root.
        let root_uri = Url::from_file_path(&tmp).unwrap();
        server
            .initialize(InitializeParams {
                root_uri: Some(root_uri),
                ..Default::default()
            })
            .await
            .unwrap();

        // Open ONLY parts.ri — other.ri is intentionally left closed.
        let parts_uri = Url::from_file_path(tmp.join("parts.ri")).unwrap();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: parts_uri.clone(),
                    language_id: "reify".to_string(),
                    version: 1,
                    text: parts_source.to_string(),
                },
            })
            .await;

        // Rename `Hole` → `Bore` from the declaration in parts.ri:
        // "structure Hole ..." — "structure " is 10 chars → col 10.
        let edit = server
            .rename(rename_params(parts_uri.clone(), 0, 10, "Bore"))
            .await
            .unwrap();

        let _ = std::fs::remove_dir_all(&tmp);

        let changes = edit
            .expect("rename returns a WorkspaceEdit")
            .changes
            .expect("changes map present");

        // Must include an entry for the CLOSED other.ri.
        let other_key = changes
            .keys()
            .find(|u| u.path().ends_with("other.ri"))
            .expect(
                "WorkspaceEdit.changes must include an entry for the CLOSED other.ri \
                 (disk-walk importer discovery)",
            )
            .clone();

        // Every edit in other.ri must write the new name.
        let other_edits = changes.get(&other_key).unwrap();
        assert!(
            !other_edits.is_empty(),
            "other.ri edits must be non-empty"
        );
        assert!(
            other_edits.iter().all(|e| e.new_text == "Bore"),
            "all other.ri edits must write the new name 'Bore', got {other_edits:?}"
        );

        // Invariant 5: applying the other.ri edits yields a buffer that
        // re-parses clean.
        let mut buffer = other_source.to_string();
        let mut edits_sorted = other_edits.clone();
        edits_sorted.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        for e in edits_sorted.iter().rev() {
            let start = crate::convert::position_to_offset(&buffer, e.range.start);
            let end = crate::convert::position_to_offset(&buffer, e.range.end);
            buffer.replace_range(start..end, &e.new_text);
        }
        let reparsed = reify_syntax::parse(&buffer, reify_core::ModulePath::single("test"));
        assert!(
            reparsed.errors.is_empty(),
            "renamed other.ri buffer must re-parse clean (Invariant 5): {:?}\n{buffer}",
            reparsed.errors
        );
    }
}
