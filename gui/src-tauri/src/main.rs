// Tauri application entry point for Reify GUI.
//
// Wires the EngineSession with SimpleConstraintChecker + DispatchPlanner + OcctKernelHandle,
// wraps it in AppState, and starts the Tauri application with all command handlers.
// After state-mutating commands, diffs old vs new state and emits targeted events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tauri::{Emitter, Manager};

use reify_constraints::SimpleConstraintChecker;
use reify_geometry::DispatchPlanner;
use reify_gui::commands::AppState;
use reify_gui::diff::{StateDelta, compute_delta, delta_to_events};
use reify_gui::engine::EngineSession;
use reify_gui::lsp_bridge::LspBridge;
use reify_gui::types::EvaluationStatus;
use reify_gui::watcher::FileWatcher;
use reify_kernel_occt::OcctKernelHandle;
use reify_lsp::server::NotificationSink;
use reify_mcp;
use tower_lsp::lsp_types::{Diagnostic, Url};

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

/// Notification sink that emits diagnostics as Tauri events.
///
/// Created during Tauri `setup()` where the [`tauri::AppHandle`] is available,
/// then passed into the [`LspBridge`] so the language server can push
/// diagnostics directly to the frontend without manual polling.
struct TauriNotificationSink {
    app: tauri::AppHandle,
}

impl NotificationSink for TauriNotificationSink {
    fn publish_diagnostics(&self, uri: Url, diagnostics: Vec<Diagnostic>, _version: Option<i32>) {
        let diags: Vec<serde_json::Value> = diagnostics
            .iter()
            .filter_map(|d| serde_json::to_value(d).ok())
            .collect();
        self.app
            .emit(
                "diagnostics",
                serde_json::json!({
                    "uri": uri.as_str(),
                    "diagnostics": diags,
                }),
            )
            .ok();
    }
}

/// Create a FileWatcher for the given file, wired to update the engine and emit events.
fn create_watcher(
    app_handle: &tauri::AppHandle,
    file_path: &std::path::Path,
) -> Option<FileWatcher> {
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
fn export(state: tauri::State<'_, AppState>, format: String, path: String) -> Result<(), String> {
    reify_gui::commands::export_impl(&state.engine, &format, &path)
}

#[tauri::command]
fn get_source_location(
    state: tauri::State<'_, AppState>,
    entity_path: String,
) -> Result<reify_mcp::SourceLocationInfo, String> {
    reify_gui::commands::get_source_location_impl(&state.engine, &entity_path)
}

#[tauri::command]
fn focus_entity(app: tauri::AppHandle, entity_path: String) -> Result<(), String> {
    // Emit an event to the frontend to focus on the given entity
    app.emit("focus-entity", entity_path)
        .map_err(|e| format!("Failed to emit event: {}", e))
}

#[tauri::command]
fn update_selection(
    state: tauri::State<'_, AppState>,
    selected_entity: Option<String>,
    hovered_entity: Option<String>,
) -> Result<(), String> {
    let mut sel = state
        .selection
        .write()
        .map_err(|e| format!("Selection lock poisoned: {}", e))?;
    sel.selected_entity = selected_entity;
    sel.hovered_entity = hovered_entity;
    Ok(())
}

#[tauri::command]
fn mcp_tool_call(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Clone app before moving into the event_emitter closure
    let app_for_emitter = app.clone();
    let ctx = reify_gui::mcp_context::TauriToolContext::builder(state.engine.clone())
        .with_event_emitter(move |event_name, payload| {
            app_for_emitter.emit(event_name, payload).ok();
        })
        .with_selection(state.selection.clone())
        .build();

    // Bracket the MCP call with evaluation-status events
    emit_status(&app, "evaluating");
    let _idle = IdleGuard(app.clone());

    let result = reify_gui::mcp_context::mcp_tool_call_impl(&name, params, &ctx);

    // Sync state and emit delta events (conservative: runs even after read-only tools,
    // since build_gui_state is cheap for unchanged state and compute_delta produces
    // an empty delta when nothing changed)
    if let Ok(mut session) = state.engine.lock()
        && let Ok(gui_state) = session.build_gui_state()
    {
        let delta = compute_delta(&state.last_state, &gui_state);
        emit_delta(&app, &delta);
    }

    result
}

#[tauri::command]
async fn lsp_request(
    bridge: tauri::State<'_, LspBridge>,
    method: String,
    params: String,
) -> Result<String, String> {
    // Diagnostics are emitted automatically by TauriNotificationSink
    // during didOpen/didChange/didClose processing — no manual polling needed.
    let result = reify_gui::lsp_bridge::lsp_request_impl(&bridge, &method, params).await?;
    Ok(result)
}

/// Lazy-spawn the Claude sidecar (if not already running) and send a user message.
/// Returns the generated message ID for correlating response events.
///
/// All outbound messages from the sidecar are emitted as Tauri events:
/// - `claude-ready`, `claude-text-delta`, `claude-thinking-delta`
/// - `claude-tool-call`, `claude-tool-result`
/// - `claude-done`, `claude-error`
///
/// `reify_` prefixed tool calls are intercepted and executed in-process
/// via the MCP registry before a `tool_result` is written back to the sidecar.
#[tauri::command]
async fn claude_send_message(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    context: Option<reify_gui::claude_bridge::MessageContext>,
) -> Result<String, String> {
    use std::sync::Arc;

    // Resolve the sidecar binary path relative to the app bundle.
    // In development, the sidecar is in the adjacent sidecar/ directory.
    let sidecar_path = app
        .path()
        .resource_dir()
        .map(|p| p.join("sidecar").join("reify-sidecar"))
        .unwrap_or_else(|_| std::path::PathBuf::from("sidecar/reify-sidecar"));

    let app_for_events = app.clone();
    let engine = Arc::clone(&state.engine);
    let selection = Arc::clone(&state.selection);

    // Lazily spawn the sidecar (if not running) and wait for it to become ready.
    reify_gui::claude_bridge::ensure_sidecar_ready(
        &state.sidecar,
        move || {
            let path = sidecar_path;
            let app_c = app_for_events;
            let eng = engine;
            let sel = selection;
            async move {
                reify_gui::claude_bridge::spawn_sidecar_impl(
                    &path,
                    eng,
                    move |name, payload| {
                        app_c.emit(&name, payload).ok();
                    },
                    sel,
                )
                .await
            }
        },
        std::time::Duration::from_secs(10),
    )
    .await?;

    reify_gui::claude_bridge::claude_send_message_impl(&state.sidecar, &text, context).await
}

/// Send an abort signal to the sidecar (cancels the current in-flight message).
#[tauri::command]
async fn claude_abort(state: tauri::State<'_, AppState>) -> Result<(), String> {
    reify_gui::claude_bridge::claude_abort_impl(&state.sidecar).await
}

/// Clear the Claude conversation session (resets conversation history).
#[tauri::command]
async fn claude_clear_session(state: tauri::State<'_, AppState>) -> Result<(), String> {
    reify_gui::claude_bridge::claude_clear_session_impl(&state.sidecar).await
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
                eprintln!(
                    "Warning: failed to load initial file {}: {}",
                    path.display(),
                    e
                );
            } else {
                initial_file = Some(path);
            }
        }
    }

    let app_state = AppState {
        engine: Arc::new(Mutex::new(session)),
        last_state: std::sync::Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::new(RwLock::new(reify_mcp::SelectionInfo::default())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .setup(move |app| {
            // Create LspBridge with TauriNotificationSink now that AppHandle is available
            let sink = Arc::new(TauriNotificationSink {
                app: app.handle().clone(),
            });
            let lsp_bridge = LspBridge::with_sink(sink);
            app.manage(lsp_bridge);

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
            update_selection,
            mcp_tool_call,
            lsp_request,
            claude_send_message,
            claude_abort,
            claude_clear_session,
        ])
        .on_window_event(|window, event| {
            // Gracefully shut down the sidecar when the window closes.
            // CloseRequested fires while the runtime is still fully operational,
            // making the async kill more reliable than Destroyed (post-teardown).
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state: tauri::State<'_, AppState> = app.state();
                    reify_gui::claude_bridge::shutdown_sidecar(&state.sidecar).await;
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
