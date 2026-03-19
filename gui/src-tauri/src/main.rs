// Tauri application entry point for Reify GUI.
//
// Wires the EngineSession with SimpleConstraintChecker + DispatchPlanner + OcctKernelHandle,
// wraps it in AppState, and starts the Tauri application with all command handlers.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_geometry::DispatchPlanner;
use reify_gui::commands::AppState;
use reify_gui::engine::EngineSession;
use reify_kernel_occt::OcctKernelHandle;

// --- Tauri command wrappers ---
// These thin wrappers delegate to the _impl functions in commands.rs,
// extracting the engine from Tauri's managed state.

#[tauri::command]
fn get_initial_state(
    state: tauri::State<'_, AppState>,
) -> Result<reify_gui::types::GuiState, String> {
    reify_gui::commands::get_initial_state_impl(&state.engine)
}

#[tauri::command]
fn set_parameter(
    state: tauri::State<'_, AppState>,
    cell_id: String,
    value: String,
) -> Result<reify_gui::types::GuiState, String> {
    reify_gui::commands::set_parameter_impl(&state.engine, &cell_id, &value)
}

#[tauri::command]
fn update_source(
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
) -> Result<reify_gui::types::GuiState, String> {
    reify_gui::commands::update_source_impl(&state.engine, &path, &content)
}

#[tauri::command]
fn save_file(path: String, content: String) -> Result<(), String> {
    reify_gui::commands::save_file_impl(&path, &content)
}

#[tauri::command]
fn open_file(path: String) -> Result<reify_gui::types::FileData, String> {
    reify_gui::commands::open_file_impl(&path)
}

#[tauri::command]
fn open_file_engine(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<reify_gui::types::GuiState, String> {
    reify_gui::commands::open_file_engine_impl(&state.engine, &path)
}

#[tauri::command]
fn export(
    state: tauri::State<'_, AppState>,
    format: String,
    path: String,
) -> Result<(), String> {
    reify_gui::commands::export_impl(&state.engine, &format, &path)
}

#[tauri::command]
fn get_source_location(
    state: tauri::State<'_, AppState>,
    entity_path: String,
) -> Result<reify_gui::types::SourceLocation, String> {
    reify_gui::commands::get_source_location_impl(&state.engine, &entity_path)
}

#[tauri::command]
fn focus_entity(app: tauri::AppHandle, entity_path: String) -> Result<(), String> {
    // Emit an event to the frontend to focus on the given entity
    app.emit("focus-entity", entity_path)
        .map_err(|e| format!("Failed to emit event: {}", e))
}

#[tauri::command]
fn lsp_request(_method: String, _params: String) -> Result<String, String> {
    // Stub: LSP requests will be wired in a future milestone
    Err("LSP not yet available in GUI mode".to_string())
}

fn main() {
    // Set up the geometry kernel with OCCT
    let checker = SimpleConstraintChecker;
    let mut planner = DispatchPlanner::new();
    planner.register_kernel(Box::new(OcctKernelHandle::spawn()));

    let session = EngineSession::new(Box::new(checker), Some(Box::new(planner)));

    // Check for initial file from command-line args or environment
    let mut session = session;
    if let Some(path) = std::env::args().nth(1) {
        let path = std::path::Path::new(&path);
        if path.exists() && path.extension().is_some_and(|ext| ext == "ri") {
            if let Err(e) = session.load_file(path) {
                eprintln!("Warning: failed to load initial file {}: {}", path.display(), e);
            }
        }
    }

    let app_state = AppState {
        engine: Arc::new(Mutex::new(session)),
    };

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            set_parameter,
            update_source,
            save_file,
            open_file,
            open_file_engine,
            export,
            get_source_location,
            focus_entity,
            lsp_request,
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
