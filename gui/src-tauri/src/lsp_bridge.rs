//! Tauri-side LSP bridge wrapping the in-process LSP server.
//!
//! [`LspBridge`] owns an [`InProcessLsp`] and provides helper functions
//! that can be used by Tauri command handlers without requiring the Tauri
//! runtime (for testability).

use std::sync::Arc;

use reify_lsp::blocking_work::BlockingWorkPlacement;
use reify_lsp::bridge::InProcessLsp;
use reify_lsp::server::{NoOpSink, NotificationSink};

/// Tauri-side wrapper around the in-process LSP server.
///
/// Holds the [`InProcessLsp`] instance and provides an interface
/// suitable for Tauri command dispatch.
///
/// # Cross-file references & rename (the workspace-document substrate)
///
/// `textDocument/references`, `textDocument/prepareRename`, and
/// `textDocument/rename` follow the import graph (task 4210 κ) **only when the
/// in-process LSP holds a workspace root**. That root is seeded by an
/// `initialize` request carrying `rootUri`: [`lsp_request_impl`] forwards the
/// `initialize` params verbatim to the server, so a frontend that calls
/// `initialize` with `rootUri` (see `lspClient.initialize`) activates the
/// multi-document workspace view — the open-document set scanned for importers
/// plus on-disk resolution of imported targets. Without a `rootUri`, the server
/// has no `workspace_root` and these handlers fall back to single-file behavior
/// (cross-module symbols remain refused). No per-method dispatch arm is required
/// for cross-file: the substrate rides entirely on the forwarded `rootUri`.
///
/// # Where blocking work runs
///
/// Every `lsp_request` is dispatched on a large-stack lane
/// ([`lsp_request_on_worker`]), so the thread polling `handle_request` IS the
/// stack the parser and compiler need. The bridge therefore builds its server
/// with [`BlockingWorkPlacement::CallingThread`], giving `definition`,
/// `prepareRename`, `rename` and `references` the lane's stack too, instead of
/// a ~2 MiB blocking-pool thread. The one cost: in the degraded no-lane arm
/// (`dispatch_async(None, ..)` awaits in place), those four now run inline on a
/// Tauri tokio worker and occupy it for the request — as `didOpen` /
/// `didChange`'s compile always has there.
pub struct LspBridge {
    lsp: InProcessLsp,
}

impl LspBridge {
    /// Create a new LSP bridge with a fresh in-process LSP server.
    pub fn new() -> Self {
        Self::with_sink(Arc::new(NoOpSink))
    }

    /// Create a new LSP bridge with a custom notification sink.
    pub fn with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        Self {
            lsp: InProcessLsp::with_sink_and_placement(sink, BlockingWorkPlacement::CallingThread),
        }
    }

    /// Retrieve the last published diagnostics for a given URI.
    ///
    /// Returns a `Vec<serde_json::Value>` suitable for serialization
    /// as a Tauri event payload.
    pub async fn get_diagnostics(&self, uri: &str) -> Vec<serde_json::Value> {
        self.lsp.get_diagnostics(uri).await
    }
}

impl Default for LspBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Implementation of the `lsp_request` Tauri command, separated for testability.
///
/// Dispatches the given LSP method with JSON params through the bridge
/// and returns the JSON-serialized response.
pub async fn lsp_request_impl(
    bridge: &LspBridge,
    method: &str,
    params: String,
) -> Result<String, String> {
    let params_value: serde_json::Value =
        serde_json::from_str(&params).map_err(|e| format!("invalid JSON params: {e}"))?;

    let result = bridge.lsp.handle_request(method, params_value).await?;

    serde_json::to_string(&result).map_err(|e| format!("serialize error: {e}"))
}

/// [`lsp_request_impl`], dispatched on a persistent LARGE-STACK lane chosen by
/// [`lane_for_method`] instead of on the awaiting tokio worker.
///
/// `lsp_request` fires on effectively every keystroke, and the work it reaches
/// is compiler-adjacent: `reify-syntax`'s CST-to-AST walk (no `stacker` guard,
/// no depth cap) and `reify-compiler`'s recursive compile. A tokio worker gives
/// that ~2 MiB of stack; a lane gives it
/// [`crate::large_stack::COMPILE_STACK_SIZE`], on threads that live for the
/// process. That covers every `handle_request` arm, the four blocking-work arms
/// included (see [`LspBridge`]'s "Where blocking work runs").
///
/// The lane is handed a FUTURE; choosing its driver is
/// [`crate::large_stack::dispatch_async`]'s job.
///
/// # An abandoned request still runs
///
/// Dropping this future does not cancel the work: it runs to completion on its
/// lane and its answer is discarded (see [`crate::large_stack::dispatch_async`]'s
/// "Abandonment"). An abandoned query occupies one of
/// [`crate::large_stack::LSP_POOL_SIZE`] consumers for its duration, not the only
/// LSP consumer in the process.
///
/// It lives in the lib rather than in `main.rs`, the `--features gui` bin with
/// no test module, so `lsp_bridge_tests.rs` can prove result parity against a
/// direct [`lsp_request_impl`] call.
pub async fn lsp_request_on_worker(
    bridge: Arc<LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    lsp_request_on_lane(lane_for_method(&method), bridge, method, params).await
}

/// Which large-stack lane an LSP method travels: the size-1 ORDERED lane
/// [`crate::large_stack::LSP_LANE`], or the QUERY POOL
/// [`crate::large_stack::LSP_POOL`].
///
/// This `matches!` arm is the AUTHORITATIVE classification; every other
/// description of the split carries only counts and points here. The test
/// copies, `lsp_lane_routing_tests`' `ORDERED_METHODS` / `QUERY_METHODS`, are
/// driven through the real dispatcher by its (j), so an arm renamed or removed
/// in `reify-lsp` reds there. An arm ADDED in `reify-lsp` is not caught: it takes
/// the ordered-lane default until someone classifies it here.
///
/// The key is LSP PROTOCOL semantics — does this method mutate server-side
/// document or session state? — not `reify-lsp`'s choice of which arms carry
/// blocking work. Those four arms compute on the thread that drives them, so
/// routing them off the lanes would hand their deep work back to a ~2 MiB
/// thread; the pool bounds their occupancy instead.
///
/// The arm lists the CONCURRENCY-SAFE set and defaults everything else,
/// including every unrecognised method, to the ordered lane. The opposite
/// spelling would hand concurrency to any method added later, state-mutating
/// ones included. `lsp_lane_routing_tests`' (h) pins the default with
/// unrecognised methods.
///
/// The ordering given up is query-versus-notification: a query may read text
/// older than a concurrently-processing `didChange` — staleness, never
/// corruption. What the shipped frontend does with a stale answer is on
/// [`crate::large_stack::Lane`].
pub(crate) fn lane_for_method(method: &str) -> Option<&'static crate::large_stack::JobSender> {
    let concurrency_safe = matches!(
        method,
        // Read-only queries: each takes `state.read().await`, clones what it
        // needs and drops the guard; none touches `eval_state` or mutates the
        // document set.
        "textDocument/completion"
            | "textDocument/hover"
            | "textDocument/definition"
            | "textDocument/documentSymbol"
            | "textDocument/documentHighlight"
            | "textDocument/prepareRename"
            // Its edit is version-stamped, and a stale one is refused client-side.
            | "textDocument/rename"
            | "textDocument/references"
    );

    if concurrency_safe {
        crate::large_stack::LSP_POOL.sender()
    } else {
        // The state-mutating and lifecycle methods, and anything
        // `InProcessLsp::handle_request` does not recognise.
        crate::large_stack::LSP_LANE.sender()
    }
}

/// The ONE future both LSP entry points submit: [`lsp_request_impl`] with owned
/// arguments, so it is `Send + 'static` as a lane requires.
///
/// [`lsp_request_on_worker`] and [`lsp_request_on_lane`] share this body, so a
/// test of the seam cannot pass while the production composition diverges.
/// (clippy rejects the explicit `-> impl Future` spelling as `manual_async_fn`.)
async fn lsp_request_future(
    bridge: Arc<LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    lsp_request_impl(&bridge, &method, params).await
}

/// [`lsp_request_on_worker`] with the lane as a PARAMETER, so a test can reach
/// the degraded `None` arm through the real composition rather than through a
/// copy of it.
///
/// This is the ONE production body: [`lsp_request_on_worker`] is
/// `lsp_request_on_lane(lane_for_method(&method), ..)`, so a test written
/// against this seam exercises the production path and can vary only the lane.
pub(crate) async fn lsp_request_on_lane(
    sender: Option<&crate::large_stack::JobSender>,
    bridge: Arc<LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    crate::large_stack::dispatch_async(sender, lsp_request_future(bridge, method, params)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// End-to-end κ (task 4210): a cross-file rename driven entirely through the
    /// Tauri command seam [`lsp_request_impl`] — the "wire the workspace document
    /// set through lsp_bridge.rs" gate.
    ///
    /// Proves the multi-document workspace substrate (workspace_root + the open-doc
    /// set) is held by the in-process LSP and reachable through the bridge: an
    /// `initialize` carrying `rootUri` activates cross-file resolution, and a
    /// subsequent `rename`/`references` on an imported symbol spans BOTH files.
    /// `lsp_request_impl` forwards `initialize` params verbatim, so no dispatch arm
    /// is needed — this test pins that the result flows through unbroken.
    #[tokio::test]
    async fn lsp_request_impl_cross_file_rename_spans_both_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(dir.path().join("parts.ri"), parts_source).expect("write parts.ri");

        let root_uri = tower_lsp::lsp_types::Url::from_file_path(dir.path())
            .expect("root uri")
            .to_string();
        let main_uri = tower_lsp::lsp_types::Url::from_file_path(dir.path().join("main.ri"))
            .expect("main uri")
            .to_string();

        let bridge = LspBridge::new();

        // initialize WITH rootUri — the cross-file substrate activation point.
        lsp_request_impl(
            &bridge,
            "initialize",
            json!({ "rootUri": root_uri, "capabilities": {} }).to_string(),
        )
        .await
        .expect("initialize");
        lsp_request_impl(&bridge, "initialized", "{}".to_string())
            .await
            .expect("initialized");

        // didOpen main.ri — imports + constructs the cross-file Hole. The
        // parenthesized constructor `Hole()` lowers to a SubDecl carrying
        // structure_name="Hole" (the bare form is a syntax error).
        let main_source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n}";
        lsp_request_impl(
            &bridge,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": main_uri.clone(),
                    "languageId": "reify",
                    "version": 1,
                    "text": main_source
                }
            })
            .to_string(),
        )
        .await
        .expect("didOpen");

        // rename Hole→Bore from the main.ri `sub hole = Hole()` use (line 2, col 15).
        let rename_resp = lsp_request_impl(
            &bridge,
            "textDocument/rename",
            json!({
                "textDocument": { "uri": main_uri.clone() },
                "position": { "line": 2, "character": 15 },
                "newName": "Bore"
            })
            .to_string(),
        )
        .await
        .expect("rename");

        let edit: serde_json::Value =
            serde_json::from_str(&rename_resp).expect("rename response is JSON");
        let changes = edit
            .get("changes")
            .and_then(|c| c.as_object())
            .expect("WorkspaceEdit.changes present and keyed by uri");
        assert!(
            changes.keys().any(|k| k.ends_with("parts.ri")),
            "changes must include parts.ri (the home declaration), got keys {:?}",
            changes.keys().collect::<Vec<_>>()
        );
        assert!(
            changes.keys().any(|k| k.ends_with("main.ri")),
            "changes must include main.ri (import entity + sub use), got keys {:?}",
            changes.keys().collect::<Vec<_>>()
        );
        for edits in changes.values() {
            for e in edits.as_array().expect("edits array") {
                assert_eq!(
                    e.get("newText").and_then(|t| t.as_str()),
                    Some("Bore"),
                    "every TextEdit writes the new name Bore"
                );
            }
        }

        // references on the same use also spans both files.
        let refs_resp = lsp_request_impl(
            &bridge,
            "textDocument/references",
            json!({
                "textDocument": { "uri": main_uri.clone() },
                "position": { "line": 2, "character": 15 },
                "context": { "includeDeclaration": true }
            })
            .to_string(),
        )
        .await
        .expect("references");
        let locations: serde_json::Value =
            serde_json::from_str(&refs_resp).expect("references response is JSON");
        let locs = locations.as_array().expect("references returns an array");
        assert_eq!(
            locs.len(),
            3,
            "home decl + import entity token + sub use = 3 cross-file Locations"
        );
        let uris: Vec<&str> = locs
            .iter()
            .filter_map(|l| l.get("uri").and_then(|u| u.as_str()))
            .collect();
        assert!(
            uris.iter().any(|u| u.ends_with("parts.ri")),
            "references must span parts.ri, got {uris:?}"
        );
        assert!(
            uris.iter().any(|u| u.ends_with("main.ri")),
            "references must span main.ri, got {uris:?}"
        );
    }

    /// Task 7118: the versioned wire shape, asserted on the JSON that actually
    /// crosses the bridge rather than on Rust types.
    ///
    /// A client declaring `workspace.workspaceEdit.documentChanges` gets a bare
    /// `documentChanges` ARRAY (`DocumentChanges` is `#[serde(untagged)]`) whose
    /// entries carry `textDocument.version` — a number for the open buffer, JSON
    /// `null` for the closed on-disk file — and NO `changes` key, so the client
    /// cannot silently read an unversioned copy of the same edit.
    #[tokio::test]
    async fn lsp_request_impl_versioned_rename_stamps_open_and_closed_versions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let parts_source = "structure Hole {\n    param diameter: Length = 10mm\n}";
        std::fs::write(dir.path().join("parts.ri"), parts_source).expect("write parts.ri");

        let root_uri = tower_lsp::lsp_types::Url::from_file_path(dir.path())
            .expect("root uri")
            .to_string();
        let main_uri = tower_lsp::lsp_types::Url::from_file_path(dir.path().join("main.ri"))
            .expect("main uri")
            .to_string();

        let bridge = LspBridge::new();

        lsp_request_impl(
            &bridge,
            "initialize",
            json!({
                "rootUri": root_uri,
                "capabilities": { "workspace": { "workspaceEdit": { "documentChanges": true } } }
            })
            .to_string(),
        )
        .await
        .expect("initialize");
        lsp_request_impl(&bridge, "initialized", "{}".to_string())
            .await
            .expect("initialized");

        // main.ri is OPEN at version 1; parts.ri stays CLOSED on disk.
        let main_source = "import parts.Hole\nstructure Assembly {\n    sub hole = Hole()\n}";
        lsp_request_impl(
            &bridge,
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": main_uri.clone(),
                    "languageId": "reify",
                    "version": 1,
                    "text": main_source
                }
            })
            .to_string(),
        )
        .await
        .expect("didOpen");

        let rename_resp = lsp_request_impl(
            &bridge,
            "textDocument/rename",
            json!({
                "textDocument": { "uri": main_uri.clone() },
                "position": { "line": 2, "character": 15 },
                "newName": "Bore"
            })
            .to_string(),
        )
        .await
        .expect("rename");

        let edit: serde_json::Value =
            serde_json::from_str(&rename_resp).expect("rename response is JSON");
        assert!(
            edit.get("changes").is_none_or(|c| c.is_null()),
            "a documentChanges-capable client must not also receive changes, got {edit}"
        );
        let doc_changes = edit
            .get("documentChanges")
            .and_then(|d| d.as_array())
            .expect("documentChanges is a bare JSON array (untagged enum)");

        let version_of = |suffix: &str| -> &serde_json::Value {
            doc_changes
                .iter()
                .find(|entry| {
                    entry
                        .pointer("/textDocument/uri")
                        .and_then(|u| u.as_str())
                        .is_some_and(|u| u.ends_with(suffix))
                })
                .unwrap_or_else(|| panic!("documentChanges must include {suffix}, got {edit}"))
                .pointer("/textDocument/version")
                .expect("each entry carries textDocument.version")
        };
        assert_eq!(
            version_of("main.ri").as_i64(),
            Some(1),
            "the OPEN main.ri carries its numeric server-side version"
        );
        assert!(
            version_of("parts.ri").is_null(),
            "the CLOSED parts.ri is null-versioned — content on disk is master"
        );

        for entry in doc_changes {
            for e in entry
                .get("edits")
                .and_then(|e| e.as_array())
                .expect("each entry carries an edits array")
            {
                assert_eq!(
                    e.get("newText").and_then(|t| t.as_str()),
                    Some("Bore"),
                    "every TextEdit writes the new name Bore"
                );
            }
        }
    }
}
