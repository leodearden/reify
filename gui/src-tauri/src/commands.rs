// Tauri command handlers — thin wrappers around EngineSession methods.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use reify_eval::CancellationHandle;
use reify_mcp::{SelectionInfo, SourceLocationInfo};

use crate::claude_bridge::SidecarHandle;
use crate::engine::EngineSession;
use crate::types::{
    DefInfo, EntityIdentity, EntityTreeNode, FileData, GuiState, MechanismDescriptor,
    PersistentViewState,
};
use crate::watcher::FileWatcher;

/// Application state shared across all Tauri commands.
pub struct AppState {
    pub engine: Arc<Mutex<EngineSession>>,
    /// Last emitted state for computing minimal diffs.
    pub last_state: Mutex<Option<GuiState>>,
    /// File watcher for the currently loaded .ri file (re-targeted on open_file_engine).
    pub watcher: Mutex<Option<FileWatcher>>,
    /// Claude Code SDK sidecar handle (lazily spawned on first claude_send_message).
    /// Uses tokio::sync::Mutex because sidecar operations span await points.
    pub sidecar: tokio::sync::Mutex<Option<SidecarHandle>>,
    /// Shared selection state updated by the frontend, read by MCP tools.
    pub selection: Arc<RwLock<SelectionInfo>>,
    /// Initial file passed on the CLI at startup (used for workspace resolution).
    pub initial_file: Mutex<Option<std::path::PathBuf>>,
    /// In-flight FEA solve cancellation handle.
    ///
    /// Published by the engine-side dispatch wiring (follow-on task) when a
    /// `solve_elastic_static` dispatch starts; cleared on completion.  The
    /// `cancel_solve` Tauri command reads this slot, calls `.cancel()` if
    /// present, and clears the slot.  A `None` value means no solve is
    /// currently running.
    ///
    /// Design: the engine mutex is held for the duration of a solve, so we
    /// cannot reach the `CancellationHandle` through `AppState::engine` without
    /// deadlocking.  Publishing a clone here (the same pattern used by
    /// `cancellation_compute_dispatch.rs:124-127`) sidesteps the lock order
    /// issue.  PRD §11 Q2 / compute-node-contract §2 SLA.
    ///
    /// # Stale-handle invariant
    ///
    /// The slot must be `None` before the engine-side producer publishes a new
    /// handle.  If the producer fails to clear the slot on solve completion
    /// (early-return, panic, or oversight), a stale handle lingers: the next
    /// `cancel_solve` call will fire `.cancel()` on a completed run (no
    /// effect), and the following solve will inherit the stale cancelled flag.
    ///
    /// The engine-side publisher **must** `debug_assert!(slot.lock().unwrap().is_none())`
    /// before inserting a new handle.  `cancel_solve_impl` always clears the
    /// slot via `.take()`, so a successful cancel also cleans up.  No run-id
    /// matching is performed; the assumption is that at most one FEA solve runs
    /// at a time (enforced by the engine mutex).
    pub pending_solve_cancel: Arc<Mutex<Option<CancellationHandle>>>,
}

// --- Helper functions for testability ---
// These take &Mutex<EngineSession> directly, avoiding the need for a Tauri runtime in tests.

/// Get the current GUI state.
pub fn get_initial_state_impl(engine: &Mutex<EngineSession>) -> Result<GuiState, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.build_gui_state())
        .and_then(std::convert::identity)
}

/// Set a parameter value and return updated state.
pub fn set_parameter_impl(
    engine: &Mutex<EngineSession>,
    cell_id: &str,
    value: &str,
) -> Result<GuiState, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.set_parameter(cell_id, value))
        .and_then(std::convert::identity)
}

/// Update source code and return updated state.
pub fn update_source_impl(
    engine: &Mutex<EngineSession>,
    path: &str,
    content: &str,
) -> Result<GuiState, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.update_source(path, content))
        .and_then(std::convert::identity)
}

/// Export geometry to a file.
pub fn export_impl(engine: &Mutex<EngineSession>, format: &str, path: &str) -> Result<(), String> {
    let export_format = match format {
        "step" | "stp" => reify_ir::ExportFormat::Step,
        "stl" => reify_ir::ExportFormat::Stl,
        _ => return Err(format!("Unknown export format: {}", format)),
    };
    crate::engine_lock::with_engine_lock(engine, |s| s.export(export_format, Path::new(path)))
        .and_then(std::convert::identity)
}

/// Get source location for an entity path.
pub fn get_source_location_impl(
    engine: &Mutex<EngineSession>,
    entity_path: &str,
) -> Result<SourceLocationInfo, String> {
    crate::engine_lock::with_engine_lock(engine, |s| {
        s.get_source_location(entity_path)
            .ok_or_else(|| format!("No source location found for '{}'", entity_path))
    })
    .and_then(std::convert::identity)
}

/// Open a file from disk (direct fs read, no engine involvement).
///
/// The returned [`FileData::path`] is the canonical absolute realpath of the
/// file (via [`crate::path_key::canonicalize_document_key`]).  This ensures
/// the frontend can use it as a stable document identity key regardless of
/// whether the caller supplied a relative or absolute path.
pub fn open_file_impl(path: &str) -> Result<FileData, String> {
    let canonical = crate::path_key::canonicalize_document_key(path);
    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Error reading {}: {}", canonical, e))?;
    Ok(FileData {
        path: canonical,
        content,
    })
}

/// Save content to a file (direct fs write, no engine involvement).
pub fn save_file_impl(path: &str, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("Error writing {}: {}", path, e))
}

/// Load a file into the engine and return the initial state.
///
/// The input `path` is canonicalised to an absolute realpath via
/// [`crate::path_key::canonicalize_document_key`] before being passed to
/// `EngineSession::load_file`.  This propagates the canonical key into the
/// engine's `file_path` field (used later by `update_source` for import
/// resolution) and ensures the returned [`GuiState::files`] contains
/// absolute paths rather than bare module-key filenames.
///
/// Note: `engine.source_map()` stores entries under `module_key(name)` =
/// `"{name}.ri"` (a stem-only key).  After loading, this function rewrites
/// each `FileData.path` in the returned `GuiState` by resolving it against the
/// canonical entry path's parent directory, so the frontend receives a stable
/// absolute identity key regardless of how the caller spelled the input path.
pub fn open_file_engine_impl(
    engine: &Mutex<EngineSession>,
    path: &str,
) -> Result<GuiState, String> {
    let canonical = crate::path_key::canonicalize_document_key(path);
    let mut state =
        crate::engine_lock::with_engine_lock(engine, |s| s.load_file(Path::new(&canonical)))
            .and_then(std::convert::identity)?;

    // source_map keys are "{name}.ri" (stem-only). Resolve each against the
    // canonical entry directory so the frontend receives absolute paths.
    if let Some(entry_dir) = Path::new(&canonical).parent() {
        for f in &mut state.files {
            let resolved = entry_dir.join(&f.path);
            if let Ok(c) = std::fs::canonicalize(&resolved) {
                f.path = c.to_string_lossy().into_owned();
            }
        }
    }

    Ok(state)
}

/// Resolve the CLI argv path to a canonical [`PathBuf`] suitable for
/// passing to `EngineSession::load_file`.
///
/// Rules (mirrors and extends the inline argv-parsing block previously in `main.rs`):
///
/// 1. Returns `None` for an empty `path_str`.
/// 2. Builds a [`PathBuf`] from `path_str`.
/// 3. Returns `None` if `extension()` is not `"ri"` (preserves the existing
///    main.rs filter for non-Reify files).
/// 4. Calls [`crate::path_key::canonicalize_document_key`] to obtain the
///    canonical absolute form.  Falls back to the original string when
///    canonicalize errors (e.g. file not yet on disk), so the caller can
///    still attempt `load_file` and surface the actionable IO error.
/// 5. Returns `Some(PathBuf::from(canonical))`.
///
/// Note: `path.exists()` is intentionally NOT checked here.  The old main.rs
/// code silently ignored non-existent argv files; this helper lets the caller
/// attempt `load_file` and receive a proper IO error instead.
pub fn resolve_initial_file_path(path_str: &str) -> Option<PathBuf> {
    if path_str.is_empty() {
        return None;
    }
    let path = PathBuf::from(path_str);
    if path.extension().is_none_or(|ext| ext != "ri") {
        return None;
    }
    let canonical = crate::path_key::canonicalize_document_key(path_str);
    Some(PathBuf::from(canonical))
}

/// Return the hierarchical entity tree for the currently loaded module.
///
/// Returns an empty vec when no module is loaded.
pub fn get_entity_tree_impl(engine: &Mutex<EngineSession>) -> Result<Vec<EntityTreeNode>, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_entity_tree())
}

/// Return the entity identity map (entity_path → EntityIdentity) for the loaded module.
///
/// Returns an empty map when no module is loaded.
pub fn get_entity_identity_map_impl(
    engine: &Mutex<EngineSession>,
) -> Result<HashMap<String, EntityIdentity>, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_entity_identity_map())
}

/// Return a preview GuiState for a single named definition evaluated with its defaults.
///
/// Returns `Err` when no module is loaded or the definition name is not found.
pub fn get_def_preview_impl(
    engine: &Mutex<EngineSession>,
    def_name: &str,
) -> Result<GuiState, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_def_preview(def_name))
        .and_then(std::convert::identity)
}

/// Read the view sidecar file for `ri_path`.
///
/// The sidecar lives at `{ri_path}.views.json` (literal suffix append, NOT
/// `Path::with_extension` which would replace the `.ri` extension).
///
/// Returns:
/// - `Ok(None)` when the sidecar file does not exist.
/// - `Ok(Some(state))` when the file exists and parses successfully.
/// - `Err(message)` when the file exists but contains malformed JSON.
pub fn read_view_sidecar_impl(ri_path: &str) -> Result<Option<PersistentViewState>, String> {
    let sidecar_path = format!("{}.views.json", ri_path);
    match std::fs::read_to_string(&sidecar_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Error reading {}: {}", sidecar_path, e)),
        Ok(content) => {
            let state: PersistentViewState = serde_json::from_str(&content)
                .map_err(|e| format!("Error parsing {}: {}", sidecar_path, e))?;
            Ok(Some(state))
        }
    }
}

/// Write `state` as pretty-printed JSON to the view sidecar file for `ri_path`.
///
/// The sidecar lives at `{ri_path}.views.json` (literal suffix append).
///
/// The write is atomic: the payload is first written to `{sidecar}.tmp` and
/// then renamed over the final path.  `std::fs::rename` is atomic on POSIX
/// (same-filesystem) and uses MoveFileEx with MOVEFILE_REPLACE_EXISTING on
/// Windows.  A crash or power loss mid-write therefore cannot leave the
/// sidecar truncated or partially written — either the old content survives,
/// or the new content replaces it.  The `.tmp` file is removed on
/// serialisation or write errors to avoid leaving debris.
pub fn write_view_sidecar_impl(ri_path: &str, state: &PersistentViewState) -> Result<(), String> {
    let sidecar_path = format!("{}.views.json", ri_path);
    let tmp_path = format!("{}.tmp", sidecar_path);
    let json =
        serde_json::to_string_pretty(state).map_err(|e| format!("Error serialising: {}", e))?;
    std::fs::write(&tmp_path, json).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Error writing {}: {}", tmp_path, e)
    })?;
    std::fs::rename(&tmp_path, &sidecar_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("Error renaming {} -> {}: {}", tmp_path, sidecar_path, e)
    })
}

/// Return the innermost definition (structure/occurrence) containing the given
/// 1-based (line, col) source position.
///
/// Returns `Ok(None)` when no module is loaded or the position is outside any definition.
/// Returns `Err` when the engine mutex is poisoned.
pub fn get_containing_definition_impl(
    engine: &Mutex<EngineSession>,
    line: u32,
    col: u32,
) -> Result<Option<DefInfo>, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_containing_definition(line, col))
}

/// Return the entity (and optionally member) at the given 1-based `(line, col)` source position.
///
/// Returns `Ok(Some("Entity.member"))` when the cursor is inside a value cell span,
/// `Ok(Some("Entity"))` when inside a template body but no cell matches,
/// `Ok(None)` when no module is loaded, zero line/col, or outside every template span.
/// Returns `Err` when the engine mutex is poisoned.
pub fn get_entity_at_source_location_impl(
    engine: &Mutex<EngineSession>,
    line: u32,
    col: u32,
) -> Result<Option<String>, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_entity_at_source_location(line, col))
}

/// Return mechanism descriptors for all non-errored mechanisms in the loaded module.
///
/// Each descriptor contains the mechanism's cell id, entity path, name, body count,
/// and a list of joint descriptors with kind, dimension, range bounds, axis, and
/// the resolved driving param cell id (when a `bind(joint, param_ref)` is found
/// via AST traversal).
///
/// Returns an empty vec when no module is loaded or no mechanisms are present.
/// Returns `Err` when the engine mutex is poisoned.
pub fn get_mechanism_descriptors_impl(
    engine: &Mutex<EngineSession>,
) -> Result<Vec<MechanismDescriptor>, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.get_mechanism_descriptors())
}

/// Production [`crate::engine::SolveCancellationSink`] that writes the
/// in-flight handle into `AppState::pending_solve_cancel` (task γ/4086).
///
/// Constructed in `main.rs::setup()` with the same `Arc<Mutex<...>>` that is
/// stored in `AppState.pending_solve_cancel`, so `cancel_solve_impl` (the
/// consumer) reads the exact slot the engine-side producer writes.
///
/// **Invariant:** `solve_started` fires a `debug_assert!(slot.is_none())`
/// before inserting — holds because publishing is serialized under the
/// session mutex (`with_engine_lock`).  `cancel_solve_impl` always clears
/// via `.take()`, so a successful cancel also resets the slot.
pub struct PendingSolveCancelSink {
    slot: Arc<Mutex<Option<CancellationHandle>>>,
}

impl PendingSolveCancelSink {
    pub fn new(slot: Arc<Mutex<Option<CancellationHandle>>>) -> Self {
        Self { slot }
    }
}

impl crate::engine::SolveCancellationSink for PendingSolveCancelSink {
    fn solve_started(&self, handle: CancellationHandle) {
        let mut guard = self.slot.lock().expect("pending_solve_cancel mutex poisoned");
        debug_assert!(
            guard.is_none(),
            "solve_started called while a handle is already in the slot — stale handle invariant violated"
        );
        *guard = Some(handle);
    }

    fn solve_finished(&self) {
        self.slot
            .lock()
            .expect("pending_solve_cancel mutex poisoned")
            .take();
    }
}

/// Cancel an in-flight FEA solve (GR-016 ζ).
///
/// Reads `state.pending_solve_cancel`, calls `.cancel()` on the
/// `CancellationHandle` if one is present, and clears the slot.
/// Returns `Ok(())` in both the "cancelled" and "no-op" cases — there is
/// nothing to cancel when the slot is empty, and that is a valid outcome.
///
/// PRD §11 Q2 / compute-node-contract §2 SLA.
pub fn cancel_solve_impl(state: &AppState) -> Result<(), String> {
    let handle = state
        .pending_solve_cancel
        .lock()
        .map_err(|e| format!("pending_solve_cancel mutex poisoned: {e}"))?
        .take();
    if let Some(h) = handle {
        h.cancel();
    }
    Ok(())
}
