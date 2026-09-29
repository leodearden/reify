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

/// [`lsp_request_impl`], dispatched on a persistent LARGE-STACK LSP lane instead
/// of on the awaiting tokio worker (task 5772), and ROUTED by method to one of
/// two such lanes (task 6517) — see "Which lane" below.
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
/// # Which lane, and what the routing does NOT cover
///
/// Since task 6517 there are TWO destinations, chosen by [`lane_for_method`]:
///
/// * ORDERED lane ([`crate::large_stack::LSP_LANE`], one consumer) — the six
///   state-mutating and lifecycle methods, plus any method `handle_request`
///   does not recognise.
/// * QUERY pool ([`crate::large_stack::LSP_POOL`],
///   [`crate::large_stack::LSP_POOL_SIZE`] consumers) — the eight read-only
///   queries.
///
/// The membership of each set is deliberately NOT restated here.
/// [`lane_for_method`]'s `matches!` arm is the single authoritative list, and
/// the only copy under test — `lsp_bridge_tests`' (j) drives every entry of its
/// `ORDERED_METHODS` / `QUERY_METHODS` constants through the real dispatcher, so
/// a method renamed or added in `reify-lsp` reds there. Prose copies are
/// unguarded by construction; only the COUNTS are repeated, because they are
/// load-bearing (six + eight = the fourteen arms `handle_request` accepts).
///
/// What the LARGE STACK covers is a different cut and is unchanged by that
/// split. Four of the pooled arms — `definition`, `prepareRename`, `rename`,
/// `references` — hop to `spawn_blocking`, so their compiler work executes on
/// tokio's BLOCKING POOL, whose threads take the std ~2 MiB default (nothing
/// under `gui/src-tauri` sets `thread_stack_size`). Putting `handle_request` on
/// a 256 MiB thread gives the big stack only to that thread's OWN frames, so
/// those four are unaffected by any lane. The arms that DO get the big stack are
/// the other ten — `initialize`, `initialized`, `didOpen`, `didChange`,
/// `didClose`, `completion`, `hover`, `documentSymbol`, `documentHighlight`,
/// `shutdown` — which are precisely the keystroke/cursor-frequency ones. Closing
/// the four needs a change in `crates/reify-lsp/src/server.rs`, which would also
/// regress the stdio `reify lsp` CLI server (it relies on `spawn_blocking` to
/// keep its 2-worker runtime responsive); tracked as task #6195 rather than
/// overclaimed here.
///
/// # What this COSTS: an abandoned request still runs
///
/// Stated alongside the coverage limit above because it is a behaviour change
/// this routing introduced, and one that is only structurally repaired. A
/// request already picked up by a consumer runs to completion, and so does every
/// abandoned request routed to the ordered lane; only a query abandoned while
/// still QUEUED on the pool is dropped unrun. Not a correctness bug in either
/// direction — the work is idempotent request-handling against the bridge's own
/// state, and every arm still RESOLVES — but a wasted-work and latency cost,
/// whose SIZE is what task 6517 changed: an abandoned in-flight query now
/// occupies one of [`crate::large_stack::LSP_POOL_SIZE`] consumers rather than
/// the only LSP consumer in the process, so it no longer stalls every subsequent
/// keystroke behind it.
///
/// The mechanism, the measurement that `tauri` 2.11.2 never drops an abandoned
/// command future anyway, and why the ordered lane must NOT discard are all in
/// [`crate::large_stack::dispatch_async`]'s "Drop-cancellation" section and on
/// [`crate::large_stack::OnAbandon`]. They are not restated here.
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
    lsp_request_on_lane(lane_for_method(&method), bridge, method, params).await
}

/// Which large-stack lane a given LSP method travels: the size-1 ORDERED lane,
/// or the [`crate::large_stack::LSP_POOL_SIZE`]-consumer QUERY POOL (task 6517).
///
/// # This `matches!` arm is the AUTHORITATIVE classification
///
/// Every other site that describes the split — [`lsp_request_on_worker`]'s
/// "Which lane", [`crate::large_stack::LSP_LANE`] and
/// [`crate::large_stack::LSP_POOL`], the `large_stack` module docs,
/// `main.rs::lsp_request`, both test-file section headers — carries the COUNTS
/// and points here for the membership. That is deliberate: prose copies of a
/// list are unguarded, so reclassifying one method (or `reify-lsp` adding an
/// arm) used to leave six stale lists reading as authoritative. The only other
/// copies are `lsp_bridge_tests`' `ORDERED_METHODS` / `QUERY_METHODS`
/// constants, and those are guarded — (j) relates them to `HANDLE_REQUEST_ARMS`
/// and drives every entry through the real dispatcher.
///
/// # The classification key is LSP PROTOCOL semantics
///
/// The question this answers is "does this method mutate server-side document or
/// session state?", and it is answered from the LSP specification's own notion
/// of notifications-versus-requests — NOT from `reify-lsp`'s internal choice of
/// which arms hop to [`tokio::task::spawn_blocking`].
///
/// That distinction is the whole reason this function exists rather than a
/// method-keyed bypass of the lane. Keying on `spawn_blocking` would be the
/// narrower change — those four arms are exactly the ones whose lane occupancy
/// hurts most, since they hold a consumer for a workspace-wide walk while their
/// deep frames run on the blocking pool's ~2 MiB threads — but it would couple
/// `gui/src-tauri` to an implementation detail of another crate that this crate
/// can neither observe nor test, and it would rot silently the day `reify-lsp`
/// moved an arm. A pool subsumes it WITHOUT the coupling: work that gains
/// nothing from the big stack merely occupies one of N consumers instead of the
/// only one, and no list of `reify-lsp` internals is needed to say so.
///
/// # Why the fallthrough is the ORDERED lane
///
/// The `matches!` below lists the CONCURRENCY-SAFE set and defaults everything
/// else — including every method `InProcessLsp::handle_request` does not
/// recognise — to the ordered lane. Spelled the other way round (list the
/// ordered set, default to the pool) it would look identical in review and would
/// hand concurrency to every method added to `reify-lsp` in future, including a
/// state-mutating one. Making the safe direction STRUCTURAL rather than a
/// comment is what stops that; `lsp_bridge_tests`' (h) pins it with unrecognised
/// methods, and (j) makes adding an arm a decision here rather than a silent
/// default.
///
/// # What each lane costs the other
///
/// Order among NOTIFICATIONS is preserved exactly — they share one FIFO
/// consumer, which is what `didChange` correctness rests on. The one ordering
/// property given up is query-versus-notification: a query may now read text
/// older than a concurrently-processing `didChange`. Server-side that is
/// staleness, never corruption (`reify-lsp`'s own `RwLock`/`Mutex` serialise
/// the accesses for safety), and it is precisely the pre-task-5772 behaviour on
/// the multi-threaded tauri runtime. What the shipped frontend then does with a
/// stale answer is on [`crate::large_stack::Lane`].
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
        // Everything else: the state-mutating and lifecycle methods, and —
        // conservatively — anything `InProcessLsp::handle_request` does not
        // recognise. Defined by EXCLUSION from the arm above rather than
        // enumerated, which is the structural half of "Why the fallthrough is
        // the ORDERED lane".
        crate::large_stack::LSP_LANE.sender()
    }
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
/// Since task 6517 this is the ONE production body, and the lane is genuinely
/// its only variable: [`lsp_request_on_worker`] IS
/// `lsp_request_on_lane(lane_for_method(&method), ..)`, by construction rather
/// than by resemblance. Both spellings submit [`lsp_request_future`]'s single
/// body, so a test written against this seam exercises the production path and
/// the only difference either side can develop is the lane argument itself.
///
/// It is therefore no longer `#[cfg(test)]`. The gate was honest while
/// production reached the lane through
/// [`crate::large_stack::run_on_lsp_worker`] and this existed only to vary the
/// lane from a test; now that production routes over TWO lanes, the lane must be
/// a parameter of the real path, not a test-only one. `pub(crate)` still adds no
/// public API surface, and `main.rs` is unaffected.
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
