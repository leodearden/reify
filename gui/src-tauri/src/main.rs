// Tauri application entry point for Reify GUI.
//
// Wires the EngineSession with SimpleConstraintChecker + DispatchPlanner + OcctKernelHandle,
// wraps it in AppState, and starts the Tauri application with all command handlers.
// After state-mutating commands, diffs old vs new state and emits targeted events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

use reify_constraints::SimpleConstraintChecker;
use reify_geometry::DispatchPlanner;
use reify_gui::commands::AppState;
use reify_gui::diff::{compute_delta, delta_to_events, StateDelta};
use reify_gui::engine::EngineSession;
use reify_gui::lsp_bridge::LspBridge;
use reify_gui::types::EvaluationStatus;
use reify_gui::watcher::FileWatcher;
use reify_kernel_occt::OcctKernelHandle;

// --- Event emission helpers ---

/// Emit targeted events for each changed/removed item in a StateDelta.
fn emit_delta(app: &tauri::AppHandle, delta: &StateDelta) {
    for (event_name, payload) in delta_to_events(delta) {
        app.emit(&event_name, payload).ok();
    }
}

/// Emit an evaluation-status event.
fn emit_status(app: &tauri::AppHandle, phase: &str) {
    app.emit(
        "evaluation-status",
        EvaluationStatus {
            phase: phase.to_string(),
            progress: None,
        },
    )
    .ok();
}

/// RAII guard that emits `evaluation-status: idle` when dropped.
///
/// Ensures the frontend never gets stuck in "evaluating" state, even if
/// a called function panics (provided the panic is caught or unwinds).
struct IdleGuard(tauri::AppHandle);

impl Drop for IdleGuard {
    fn drop(&mut self) {
        emit_status(&self.0, "idle");
    }
}

/// Create a FileWatcher for the given file, wired to update the engine and emit events.
fn create_watcher(app_handle: &tauri::AppHandle, file_path: &std::path::Path) -> Option<FileWatcher> {
    let parent = file_path.parent()?;
    let target = Some(PathBuf::from(file_path.file_name()?));
    let handle = app_handle.clone();

    match FileWatcher::new(parent, target, move |changed_path| {
        if let Ok(content) = std::fs::read_to_string(&changed_path) {
            let state: tauri::State<'_, AppState> = handle.state();
            let path_str = changed_path.to_string_lossy().to_string();

            emit_status(&handle, "evaluating");
            {
                let _idle = IdleGuard(handle.clone());
                if let Ok(gui_state) =
                    reify_gui::commands::update_source_impl(&state.engine, &path_str, &content)
                {
                    let delta = compute_delta(&state.last_state, &gui_state);
                    emit_delta(&handle, &delta);
                }
            }

            handle
                .emit(
                    "file-changed",
                    reify_gui::types::FileData {
                        path: changed_path.to_string_lossy().to_string(),
                        content,
                    },
                )
                .ok();
        }
    }) {
        Ok(watcher) => {
            eprintln!("Watching {} for changes", file_path.display());
            Some(watcher)
        }
        Err(e) => {
            eprintln!("Warning: failed to start file watcher: {}", e);
            None
        }
    }
}

// --- Tauri command wrappers ---
// These thin wrappers delegate to the _impl functions in commands.rs,
// extracting the engine from Tauri's managed state.
// State-mutating commands emit evaluation-status and targeted events.

#[tauri::command]
fn get_initial_state(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<reify_gui::types::GuiState, String> {
    let result = reify_gui::commands::get_initial_state_impl(&state.engine);
    if let Ok(ref gui_state) = result {
        // Store as last_state so subsequent commands produce correct diffs
        let delta = compute_delta(&state.last_state, gui_state);
        emit_delta(&app, &delta);
        emit_status(&app, "idle");
    }
    result
}

#[tauri::command]
fn set_parameter(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    cell_id: String,
    value: String,
) -> Result<reify_gui::types::GuiState, String> {
    emit_status(&app, "evaluating");
    let _idle = IdleGuard(app.clone());
    let result = reify_gui::commands::set_parameter_impl(&state.engine, &cell_id, &value);
    if let Ok(ref gui_state) = result {
        let delta = compute_delta(&state.last_state, gui_state);
        emit_delta(&app, &delta);
    }
    result
}

#[tauri::command]
fn update_source(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    content: String,
) -> Result<reify_gui::types::GuiState, String> {
    emit_status(&app, "evaluating");
    let _idle = IdleGuard(app.clone());
    let result = reify_gui::commands::update_source_impl(&state.engine, &path, &content);
    if let Ok(ref gui_state) = result {
        let delta = compute_delta(&state.last_state, gui_state);
        emit_delta(&app, &delta);
    }
    result
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<reify_gui::types::GuiState, String> {
    emit_status(&app, "evaluating");
    let _idle = IdleGuard(app.clone());
    let result = reify_gui::commands::open_file_engine_impl(&state.engine, &path);
    if let Ok(ref gui_state) = result {
        let delta = compute_delta(&state.last_state, gui_state);
        emit_delta(&app, &delta);

        // Re-target the file watcher to the newly opened file
        let new_watcher = create_watcher(&app, std::path::Path::new(&path));
        if let Ok(mut watcher_guard) = state.watcher.lock() {
            *watcher_guard = new_watcher;
        }
    }
    result
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
async fn lsp_request(
    app: tauri::AppHandle,
    bridge: tauri::State<'_, LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    // Extract URI before dispatch (for diagnostics emission after mutations)
    let uri = if method == "textDocument/didOpen" || method == "textDocument/didChange" {
        extract_document_uri(&params)
    } else {
        None
    };

    let result = reify_gui::lsp_bridge::lsp_request_impl(&*bridge, &method, params).await?;

    // After document mutations, emit diagnostics as a Tauri event
    if let Some(uri) = uri {
        let diags = bridge.get_diagnostics(&uri).await;
        app.emit("diagnostics", serde_json::json!({
            "uri": uri,
            "diagnostics": diags,
        }))
        .ok();
    }

    Ok(result)
}

/// Extract the document URI from LSP params JSON.
fn extract_document_uri(params: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(params).ok()?;
    v["textDocument"]["uri"]
        .as_str()
        .map(|s| s.to_string())
}

fn main() {
    // Set up the geometry kernel with OCCT
    let checker = SimpleConstraintChecker;
    let mut planner = DispatchPlanner::new();
    planner.register_kernel(Box::new(OcctKernelHandle::spawn()));

    let session = EngineSession::new(Box::new(checker), Some(Box::new(planner)));

    // Check for initial file from command-line args or environment
    let mut session = session;
    let mut initial_file: Option<std::path::PathBuf> = None;
    if let Some(path_str) = std::env::args().nth(1) {
        let path = std::path::PathBuf::from(&path_str);
        if path.exists() && path.extension().is_some_and(|ext| ext == "ri") {
            if let Err(e) = session.load_file(&path) {
                eprintln!("Warning: failed to load initial file {}: {}", path.display(), e);
            } else {
                initial_file = Some(path);
            }
        }
    }

    let app_state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: std::sync::Mutex::new(None),
        watcher: Mutex::new(None),
    };

    let lsp_bridge = LspBridge::new();

    tauri::Builder::default()
        .manage(app_state)
        .manage(lsp_bridge)
        .setup(move |app| {
            // If an initial file was loaded, start watching its parent directory
            if let Some(ref file_path) = initial_file {
                let watcher = create_watcher(app.handle(), file_path);
                let state: tauri::State<'_, AppState> = app.state();
                if let Ok(mut watcher_guard) = state.watcher.lock() {
                    *watcher_guard = watcher;
                }
            }
            Ok(())
        })
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
