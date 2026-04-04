// Tauri command handlers — thin wrappers around EngineSession methods.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use reify_mcp::{SelectionInfo, SourceLocationInfo};

use crate::claude_bridge::SidecarHandle;
use crate::engine::EngineSession;
use crate::types::{FileData, GuiState};
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
}

// --- Helper functions for testability ---
// These take &Mutex<EngineSession> directly, avoiding the need for a Tauri runtime in tests.

/// Get the current GUI state.
pub fn get_initial_state_impl(engine: &Mutex<EngineSession>) -> Result<GuiState, String> {
    let mut session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session.build_gui_state()
}

/// Set a parameter value and return updated state.
pub fn set_parameter_impl(
    engine: &Mutex<EngineSession>,
    cell_id: &str,
    value: &str,
) -> Result<GuiState, String> {
    let mut session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session.set_parameter(cell_id, value)
}

/// Update source code and return updated state.
pub fn update_source_impl(
    engine: &Mutex<EngineSession>,
    path: &str,
    content: &str,
) -> Result<GuiState, String> {
    let mut session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session.update_source(path, content)
}

/// Export geometry to a file.
pub fn export_impl(engine: &Mutex<EngineSession>, format: &str, path: &str) -> Result<(), String> {
    let export_format = match format {
        "step" | "stp" => reify_types::ExportFormat::Step,
        "stl" => reify_types::ExportFormat::Stl,
        _ => return Err(format!("Unknown export format: {}", format)),
    };
    let mut session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session.export(export_format, Path::new(path))
}

/// Get source location for an entity path.
pub fn get_source_location_impl(
    engine: &Mutex<EngineSession>,
    entity_path: &str,
) -> Result<SourceLocationInfo, String> {
    let session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session
        .get_source_location(entity_path)
        .ok_or_else(|| format!("No source location found for '{}'", entity_path))
}

/// Open a file from disk (direct fs read, no engine involvement).
pub fn open_file_impl(path: &str) -> Result<FileData, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Error reading {}: {}", path, e))?;
    Ok(FileData {
        path: path.to_string(),
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
    let mut session = engine.lock().map_err(|e| format!("Lock error: {}", e))?;
    session.load_file(Path::new(path))
}
