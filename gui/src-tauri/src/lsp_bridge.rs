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
