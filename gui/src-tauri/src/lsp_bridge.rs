//! Tauri-side LSP bridge wrapping the in-process LSP server.
//!
//! [`LspBridge`] owns an [`InProcessLsp`] and provides helper functions
//! that can be used by Tauri command handlers without requiring the Tauri
//! runtime (for testability).

use std::sync::Arc;

use reify_lsp::bridge::InProcessLsp;
use reify_lsp::server::NotificationSink;

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
pub struct LspBridge {
    lsp: InProcessLsp,
}

impl LspBridge {
    /// Create a new LSP bridge with a fresh in-process LSP server.
    pub fn new() -> Self {
        Self {
            lsp: InProcessLsp::new(),
        }
    }

    /// Create a new LSP bridge with a custom notification sink.
    pub fn with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        Self {
            lsp: InProcessLsp::with_sink(sink),
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

/// [`lsp_request_impl`], dispatched on the persistent LARGE-STACK LSP lane
/// instead of on the awaiting tokio worker (task 5772).
///
/// `lsp_request` fires on effectively every keystroke and cursor move, and the
/// work it reaches is compiler-adjacent: `reify-syntax`'s CST-to-AST walk (which
/// has neither a `stacker` guard nor a depth cap) and `reify-compiler`'s
/// recursive compile. A tokio worker gives that the default ~2 MiB stack; the
/// lane gives it [`crate::large_stack::COMPILE_STACK_SIZE`] (256 MiB), amortised
/// over one thread for the process lifetime rather than a fresh 256 MiB mapping
/// per keystroke.
///
/// # What this hands the lane, and what the lane does with it
///
/// A FUTURE, not a closure. The lane thread has no ambient runtime, so the
/// future does need a driver — [`tokio::runtime::Handle::block_on`], because
/// four of `InProcessLsp::handle_request`'s arms call
/// [`tokio::task::spawn_blocking`], whose first statement is `Handle::current()`
/// — but choosing that driver is [`crate::large_stack::dispatch_async`]'s job,
/// not this function's. Pre-baking the `block_on` here would break the lane's
/// degraded arms, which run in the submitting async frame where `block_on`
/// panics "Cannot start a runtime from within a runtime"; see
/// [`crate::large_stack::dispatch_async`]'s degradation policy.
///
/// # What this does NOT cover
///
/// Those same four arms hop to `spawn_blocking`, so their compiler work executes
/// on tokio's BLOCKING POOL, whose threads take the std ~2 MiB default (nothing
/// under `gui/src-tauri` sets `thread_stack_size`). Putting `handle_request` on
/// a 256 MiB thread gives the big stack only to that thread's OWN frames, so
/// those four are unaffected by this routing. The arms it does cover are the
/// other ten — `initialize`, `initialized`, `didOpen`, `didChange`, `didClose`,
/// `completion`, `hover`, `documentSymbol`, `documentHighlight`, `shutdown` —
/// which are precisely the keystroke/cursor-frequency ones. Closing the four
/// needs a change in `crates/reify-lsp/src/server.rs`, which would also regress
/// the stdio `reify lsp` CLI server (it relies on `spawn_blocking` to keep its
/// 2-worker runtime responsive); tracked as task #6195 rather than overclaimed
/// here.
///
/// # What this COSTS: the request is no longer DROP-CANCELLABLE
///
/// Stated alongside the coverage limit above because it is a behaviour change
/// this routing INTRODUCES, not merely one it fails to fix.
///
/// Before task 5772 the body ran inside the Tauri command's own future, so a
/// frontend `invoke` that was abandoned — window closed, pane navigated away,
/// a keystroke's request superseded by the next one — dropped that future and
/// the LSP work stopped at its next `.await` point. Now the future is MOVED
/// into a lane job and driven by [`tokio::runtime::Handle::block_on`] on a
/// thread that has no cancellation point at all. Dropping the awaiting side
/// only drops the `oneshot` receiver; `reply_tx.send` then fails silently
/// (`let _ = ...`) while the work runs to completion regardless.
///
/// That compounds with the single-consumer serialization documented on
/// [`crate::large_stack::Lane`]: an abandoned request still occupies the lane
/// for its full duration, so it delays the LIVE requests queued behind it.
/// Bounding it means threading a cancellation token into the job and checking
/// it at the lane before driving the future, so an abandoned request is dropped
/// from the queue instead of executed — same lane, same shape, but a different
/// job contract than this task specified. Tracked with the serialization it
/// compounds, on task #6517.
///
/// Not a correctness bug in either direction: the work is idempotent
/// request-handling against the bridge's own state, and every arm still
/// RESOLVES. It is a wasted-work and latency cost, and the honest statement of
/// it is this paragraph rather than silence.
///
/// # Why this composition lives here, not inline in `main.rs`
///
/// `main.rs` is the `--features gui` bin and has no test module, so a wrapper
/// written there would be untestable. Keeping it in the lib is what lets
/// `lsp_bridge_tests.rs` prove result parity against a direct
/// [`lsp_request_impl`] call.
pub async fn lsp_request_on_worker(
    bridge: Arc<LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    crate::large_stack::run_on_lsp_worker(lsp_request_future(bridge, method, params)).await
}

/// The ONE future both LSP entry points submit: `lsp_request_impl`, owned and
/// `'static` so a lane can take it.
///
/// Factored out so [`lsp_request_on_worker`] (production) and
/// `lsp_request_on_lane` (the lane-parameterised test seam) submit the SAME
/// body rather than two independently-written `async move` blocks. Two spellings
/// of the composition is precisely the divergence hazard the seam exists to
/// avoid: the tested one could keep resolving while the production one acquired
/// a defect. With one body, the only thing the seam varies is which lane the
/// work travels — which is the variable the tests actually mean to control.
/// Every argument is OWNED, so the returned future is `Send + 'static` — the
/// bound a lane requires — without spelling either out (clippy rejects the
/// explicit `-> impl Future` form here as `manual_async_fn`).
async fn lsp_request_future(
    bridge: Arc<LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    lsp_request_impl(&bridge, &method, params).await
}

/// [`lsp_request_on_worker`] with its "is there a lane?" question turned into a
/// PARAMETER — the one body both the lane path and the degraded path run.
///
/// The lane a request travels is a parameter for the same reason
/// [`crate::large_stack::dispatch_async`]'s is: it makes the DEGRADED arm
/// reachable from a test. Provoking a real `pthread_create` failure from a unit
/// test is not possible, so passing `None` here tests the seam instead of the
/// OS — and it tests it through the REAL composition. A test that rebuilt the
/// `dispatch_async(None, async { lsp_request_impl(..) })` composition itself
/// would only prove that its own copy resolves; the production body could
/// diverge and stay green. That is exactly how the earlier generic guard went
/// vacuous: its closure contained no `block_on`, so it could not see that the
/// real one panicked.
///
/// # Relationship to [`lsp_request_on_worker`]
///
/// `lsp_request_on_lane(LSP_LANE.sender(), ..)` IS `lsp_request_on_worker`, by
/// construction rather than by resemblance: both submit
/// [`lsp_request_future`]'s single body, and
/// [`crate::large_stack::run_on_lsp_worker`] — which production goes through —
/// is defined as `dispatch_async(LSP_LANE.sender(), fut)`. So a lane-path test
/// written against this seam exercises the production path, and the only
/// difference either side can develop is the lane argument itself.
///
/// `#[cfg(test)] pub(crate)` — production reaches the lane through
/// [`lsp_request_on_worker`], so this seam exists only to vary the lane
/// argument from a test. Gating it to test builds keeps that honest and adds no
/// public API surface; `main.rs` is unaffected.
#[cfg(test)]
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
}
