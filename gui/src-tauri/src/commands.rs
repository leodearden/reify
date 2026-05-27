// Tauri command handlers — thin wrappers around EngineSession methods.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

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
        "step" | "stp" => reify_types::ExportFormat::Step,
        "stl" => reify_types::ExportFormat::Stl,
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
pub fn open_file_engine_impl(
    engine: &Mutex<EngineSession>,
    path: &str,
) -> Result<GuiState, String> {
    crate::engine_lock::with_engine_lock(engine, |s| s.load_file(Path::new(path)))
        .and_then(std::convert::identity)
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
