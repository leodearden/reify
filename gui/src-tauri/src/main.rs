// Tauri application entry point for Reify GUI.
//
// Constructs EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker)); OCCT
// registration is automatic via the cfg(has_occt)-gated inventory::submit! in
// reify-kernel-occt::register. The kernel_status::current_kernel_status() call surfaces the
// build-time OCCT_AVAILABLE constant for the startup banner. Wraps in AppState and starts the
// Tauri application with all command handlers. After state-mutating commands, diffs old vs new
// state and emits targeted events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tracing::warn;

use tauri::{Emitter, Manager};

use reify_constraints::SimpleConstraintChecker;
use reify_gui::commands::AppState;
use reify_gui::diff::{StateDelta, compute_delta, delta_to_events};
use reify_gui::engine::{AutoResolveEmitter, EngineSession, FeaCaseEmitter, ModeShapeFrameEmitter, WarmPoolEventEmitter};
use reify_eval::SolverProgressSink;
use reify_gui::event_bus::emit_typed;
use reify_gui::lsp_bridge::LspBridge;
use reify_gui::types::EvaluationStatus;
use reify_gui::watcher::{FileEvent, FileWatcher};
use reify_lsp::server::NotificationSink;
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

/// Emits auto-resolve lifecycle events to the frontend via Tauri.
struct TauriAutoResolveEmitter {
    app: tauri::AppHandle,
}

impl AutoResolveEmitter for TauriAutoResolveEmitter {
    fn start(&self) {
        if let Err(e) = emit_typed(&self.app, "auto-resolve-start", &()) {
            warn!("auto-resolve emit 'auto-resolve-start' failed: {}", e);
        }
    }

    fn iteration(&self, iter: reify_gui::types::AutoResolveIteration) {
        if let Err(e) = emit_typed(&self.app, "auto-resolve-iteration", &iter) {
            warn!("auto-resolve emit 'auto-resolve-iteration' failed: {}", e);
        }
    }

    fn complete(&self) {
        if let Err(e) = emit_typed(&self.app, "auto-resolve-complete", &()) {
            warn!("auto-resolve emit 'auto-resolve-complete' failed: {}", e);
        }
    }
}

/// Emits warm-pool lifecycle events (evictions and donations) to the frontend via Tauri.
///
/// Installed during `setup()` alongside [`TauriAutoResolveEmitter`]. The backend emits
/// unconditionally; the frontend panel only subscribes when `REIFY_DEBUG=1` (PRD §11 Q6).
struct TauriWarmPoolEventEmitter {
    app: tauri::AppHandle,
}

impl WarmPoolEventEmitter for TauriWarmPoolEventEmitter {
    fn emit(&self, event: reify_gui::types::WarmPoolEvent) {
        if let Err(e) = emit_typed(&self.app, "warm-pool-event", &event) {
            warn!("warm-pool-event emit failed: {}", e);
        }
    }
}

/// Emits `fea-case-changed` events to the frontend when a MultiCaseResult-shaped value
/// is observed in `CheckResult.values` at commit time.
///
/// Installed during `setup()` alongside [`TauriAutoResolveEmitter`] and
/// [`TauriWarmPoolEventEmitter`]. Per PRD §2.2 task η — fires unconditionally on every
/// check that detects a multi-case value (no engine-side dedup, mirroring auto-resolve).
struct TauriFeaCaseEmitter {
    app: tauri::AppHandle,
}

impl FeaCaseEmitter for TauriFeaCaseEmitter {
    fn changed(&self, payload: reify_gui::types::FeaCaseChanged) {
        if let Err(e) = emit_typed(&self.app, "fea-case-changed", &payload) {
            warn!("fea-case-changed emit failed: {}", e);
        }
    }
}

/// Emits `mode-shape-frame` events to the frontend whenever a BucklingResult-shaped
/// value is observed at commit time (task ι/3458).
///
/// Installed during `setup()` alongside other emitters. Fires one undeformed base frame
/// (phase=0.0) and one peak frame per mode (phase=1.0).
struct TauriModeShapeFrameEmitter {
    app: tauri::AppHandle,
}

impl ModeShapeFrameEmitter for TauriModeShapeFrameEmitter {
    fn frame(&self, payload: reify_gui::types::ModeShapeFrame) {
        if let Err(e) = emit_typed(&self.app, "mode-shape-frame", &payload) {
            warn!("mode-shape-frame emit failed: {}", e);
        }
    }
}

/// Tauri implementation of `SolverProgressSink` (task 4079).
///
/// Maps `SolverProgressUpdate` → `types::SolverProgress` and emits it on the
/// `"solver-progress"` IPC channel.  `eta_ms` is left `None` — ETA estimation
/// is deferred to a follow-up task.
///
/// Installed during `setup()` alongside other emitters.  The production path:
/// 1. `with_solve_slot` installs a fresh handle on the engine.
/// 2. `run_compute_dispatch` reads the sink + cancel from the thread-local.
/// 3. The elastic_static trampoline emits one update per CG iteration.
struct TauriSolverProgressEmitter {
    app: tauri::AppHandle,
}

impl SolverProgressSink for TauriSolverProgressEmitter {
    fn on_iteration(&self, update: &reify_eval::SolverProgressUpdate) {
        let payload = reify_gui::types::SolverProgress {
            solver_kind: update.solver_kind.to_string(),
            iter: update.iter,
            residual: update.residual,
            eta_ms: None,
        };
        if let Err(e) = emit_typed(&self.app, "solver-progress", &payload) {
            warn!("solver-progress emit failed: {}", e);
        }
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

    match FileWatcher::new(parent, target, move |file_event| {
        match file_event {
            FileEvent::Changed(changed_path) => {
                if let Ok(content) = std::fs::read_to_string(&changed_path) {
                    let state: tauri::State<'_, AppState> = handle.state();
                    let path_str = changed_path.to_string_lossy().to_string();

                    emit_status(&handle, "evaluating");
                    {
                        let _idle = IdleGuard(handle.clone());
                        // reload_for_watch_impl always returns Ok(GuiState): success
                        // returns the fresh state; failure returns the last-good state
                        // carrying the reload-error diagnostic in compile_diagnostics.
                        // The failure path therefore surfaces a compile-diagnostics Tauri
                        // event to the frontend instead of being silently dropped (the
                        // former behaviour with update_source_impl's Err branch).
                        if let Ok(gui_state) = reify_gui::commands::reload_for_watch_impl(
                            &state.engine,
                            &path_str,
                            &content,
                        ) {
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
            }
            FileEvent::Removed(removed_path) => {
                handle
                    .emit(
                        "file-removed",
                        serde_json::json!({
                            "path": removed_path.to_string_lossy().as_ref()
                        }),
                    )
                    .ok();
            }
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

/// Register the GUI's PASSIVE observed-demand sources (selective-demand
/// precondition, task 4532). OBSERVATIONAL ONLY — never perturbs evaluation, so
/// it emits no status/delta: the recorded would-prune measurement rides back on
/// the next `set_parameter` response's `GuiState.demand_prune_measurement`.
#[tauri::command]
fn sync_observed_demand(
    state: tauri::State<'_, AppState>,
    visible_realizations: Vec<String>,
    displayed_cells: Vec<String>,
    panel_constraints: Vec<String>,
) -> Result<(), String> {
    reify_gui::commands::sync_observed_demand_impl(
        &state.engine,
        &visible_realizations,
        &displayed_cells,
        &panel_constraints,
    )
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
    let result = reify_gui::commands::reload_for_watch_impl(&state.engine, &path, &content);
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
fn get_entity_tree(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<reify_gui::types::EntityTreeNode>, String> {
    reify_gui::commands::get_entity_tree_impl(&state.engine)
}

#[tauri::command]
fn get_entity_identity_map(
    state: tauri::State<'_, AppState>,
) -> Result<std::collections::HashMap<String, reify_gui::types::EntityIdentity>, String> {
    reify_gui::commands::get_entity_identity_map_impl(&state.engine)
}

#[tauri::command]
fn get_mechanism_descriptors(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<reify_gui::types::MechanismDescriptor>, String> {
    reify_gui::commands::get_mechanism_descriptors_impl(&state.engine)
}

#[tauri::command]
fn get_def_preview(
    state: tauri::State<'_, AppState>,
    def_name: String,
) -> Result<reify_gui::types::GuiState, String> {
    reify_gui::commands::get_def_preview_impl(&state.engine, &def_name)
}

#[tauri::command]
fn get_containing_definition(
    state: tauri::State<'_, AppState>,
    line: u32,
    col: u32,
) -> Result<Option<reify_gui::types::DefInfo>, String> {
    reify_gui::commands::get_containing_definition_impl(&state.engine, line, col)
}

#[tauri::command]
fn get_entity_at_source_location(
    state: tauri::State<'_, AppState>,
    line: u32,
    col: u32,
) -> Result<Option<String>, String> {
    reify_gui::commands::get_entity_at_source_location_impl(&state.engine, line, col)
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
    selected_entities: Option<Vec<String>>,
) -> Result<(), String> {
    let mut sel = state
        .selection
        .write()
        .map_err(|e| format!("Selection lock poisoned: {}", e))?;
    sel.selected_entity = selected_entity;
    sel.hovered_entity = hovered_entity;
    sel.selected_entities = selected_entities.unwrap_or_default();
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

// --- Debug commands ---

/// Wrapper for REIFY_DEBUG=1 state, managed by Tauri.
struct DebugEnabled(bool);

#[tauri::command]
fn is_debug_enabled(state: tauri::State<'_, DebugEnabled>) -> bool {
    state.0
}

#[tauri::command]
fn debug_response(
    bridge: tauri::State<'_, Arc<reify_gui::debug::DebugBridge>>,
    id: u64,
    result: String,
) -> Result<(), String> {
    bridge.resolve(id, result)
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

    // Resolve the sidecar binary path. Tauri's externalBin (declared in
    // tauri.conf.json) copies the sidecar binary to `<resource_dir>/<basename>`
    // in both dev (`target/<profile>/reify-sidecar`) and bundled builds —
    // it does NOT place it in a `sidecar/` subdirectory of resource_dir,
    // despite the source layout being `gui/src-tauri/sidecar/...`.
    let sidecar_path = app
        .path()
        .resource_dir()
        .map(|p| p.join("reify-sidecar"))
        .unwrap_or_else(|_| std::path::PathBuf::from("reify-sidecar"));

    // Resolve the writable workspace directory for the landlock sandbox.
    let initial_file_opt: Option<std::path::PathBuf> =
        state.initial_file.lock().ok().and_then(|g| g.clone());
    let fallback_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let workspace = reify_gui::claude_bridge::resolve_workspace_dir(
        context.as_ref(),
        initial_file_opt.as_deref(),
        &fallback_cwd,
    );

    // Resolve the landlock helper path from the bundle resource dir.
    // Only set when the file actually exists (dev + bundled builds have it; CI/test may not).
    let landlock_exec_path: Option<std::path::PathBuf> = app
        .path()
        .resource_dir()
        .ok()
        .map(|p| p.join("sandbox/landlock_exec.py"))
        .filter(|p| p.exists());

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
            let ws = workspace;
            let le = landlock_exec_path;
            async move {
                reify_gui::claude_bridge::spawn_sidecar_impl(
                    &path,
                    eng,
                    move |name, payload| {
                        app_c.emit(&name, payload).ok();
                    },
                    sel,
                    &ws,
                    le.as_deref(),
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

/// Resolve a pending permission-prompt request from the Claude CLI.
///
/// Routes the user's Allow/Deny/Always decision back to the sidecar, which
/// forwards it to the in-process MCP permission server to unblock the pending
/// `approve_tool` call.
#[tauri::command]
async fn claude_permission_decision(
    state: tauri::State<'_, AppState>,
    decision: reify_gui::claude_bridge::PermissionDecisionArgs,
) -> Result<(), String> {
    reify_gui::claude_bridge::claude_permission_decision_impl(&state.sidecar, decision).await
}

/// Return the current kernel availability status.
#[tauri::command]
fn read_view_sidecar(
    ri_path: String,
) -> Result<Option<reify_gui::types::PersistentViewState>, String> {
    reify_gui::commands::read_view_sidecar_impl(&ri_path)
}

#[tauri::command]
fn write_view_sidecar(
    ri_path: String,
    state: reify_gui::types::PersistentViewState,
) -> Result<(), String> {
    reify_gui::commands::write_view_sidecar_impl(&ri_path, &state)
}

#[tauri::command]
fn get_kernel_status() -> reify_gui::kernel_status::KernelStatus {
    reify_gui::kernel_status::current_kernel_status()
}

/// Cancel an in-flight FEA solve (GR-016 ζ, PRD §11 Q2).
///
/// Reads `AppState::pending_solve_cancel`, calls `.cancel()` on the handle if
/// present, and clears the slot.  Returns `Ok(())` in both the "cancelled" and
/// "no-op" cases.  The engine-side wiring that publishes the handle is a
/// follow-on task.
#[tauri::command]
fn cancel_solve(state: tauri::State<'_, AppState>) -> Result<(), String> {
    reify_gui::commands::cancel_solve_impl(&*state)
}

/// Return the currently active FEA case name (task 3026 case-picker).
///
/// `None` means the active case has never been set (engine defaults to
/// lex-first). Returns `Some(name)` after a `set_active_fea_case` call.
#[tauri::command]
fn get_active_fea_case(
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    reify_gui::commands::get_active_fea_case_impl(&state.engine)
}

/// Switch to the named FEA case and return a rebuilt GuiState (task 3026 case-picker).
///
/// Stores the case name in the engine session, re-applies FEA scalar channels
/// from the cached tessellation snapshot (no re-evaluation, no re-tessellation),
/// and returns the updated GuiState so the frontend can apply the re-sourced
/// contour.  Unknown case names fall back to the lex-first default.
#[tauri::command]
fn set_active_fea_case(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    case: String,
) -> Result<reify_gui::types::GuiState, String> {
    let result = reify_gui::commands::set_active_fea_case_impl(&state.engine, &case);
    if let Ok(ref gui_state) = result {
        let delta = compute_delta(&state.last_state, gui_state);
        emit_delta(&app, &delta);
    }
    result
}

fn main() {
    // Sweep stale tempfiles and orphan directories from the persistent cache
    // before any engine work. Best-effort: resolver errors are logged at
    // tracing::debug! level and the sweep is skipped; IO errors inside the
    // sweep are never fatal per the wrapper's contract. Wired here (task 3698)
    // so the cleanup runs on every GUI launch without per-feature wiring.
    reify_gui::engine::bootstrap_persistent_cache_sweep();

    // Boot the engine via the inventory-based kernel registry. OCCT is registered automatically
    // via the cfg(has_occt)-gated inventory::submit! in reify-kernel-occt::register.
    let checker = SimpleConstraintChecker;
    let kernel_status = reify_gui::kernel_status::current_kernel_status();
    let session = EngineSession::with_registered_kernel(Box::new(checker));

    // Check for initial file from command-line args or environment.
    // `resolve_initial_file_path` canonicalises the argv path to an absolute
    // realpath before loading so the engine's `file_path` field (used by
    // `update_source` for import resolution) is always an absolute canonical
    // path, regardless of how the user spelled the CLI argument.
    let mut session = session;
    let mut initial_file: Option<std::path::PathBuf> = None;
    if let Some(path_str) = std::env::args().nth(1) {
        if let Some(canonical_path) =
            reify_gui::commands::resolve_initial_file_path(&path_str)
        {
            if let Err(e) = session.load_file(&canonical_path) {
                eprintln!(
                    "Warning: failed to load initial file {}: {}",
                    canonical_path.display(),
                    e
                );
            } else {
                initial_file = Some(canonical_path);
            }
        }
    }

    let debug_enabled = std::env::var("REIFY_DEBUG").is_ok_and(|v| v == "1");

    let engine_arc = Arc::new(Mutex::new(session));
    let selection_arc = Arc::new(RwLock::new(reify_mcp::SelectionInfo::default()));

    // Shared slot for in-flight FEA solve handle (task γ/4086).
    // PendingSolveCancelSink (installed below in setup()) writes this slot;
    // cancel_solve_impl reads it via AppState.pending_solve_cancel.
    // Both hold an Arc clone — same underlying Mutex.
    let solve_cancel_slot: Arc<Mutex<Option<reify_eval::CancellationHandle>>> =
        Arc::new(Mutex::new(None));

    let app_state = AppState {
        engine: Arc::clone(&engine_arc),
        last_state: std::sync::Mutex::new(None),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::clone(&selection_arc),
        initial_file: Mutex::new(initial_file.clone()),
        pending_solve_cancel: Arc::clone(&solve_cancel_slot),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .manage(DebugEnabled(debug_enabled))
        .setup(move |app| {
            // Create LspBridge with TauriNotificationSink now that AppHandle is available
            let sink = Arc::new(TauriNotificationSink {
                app: app.handle().clone(),
            });
            let lsp_bridge = LspBridge::with_sink(sink);
            app.manage(lsp_bridge);

            // Install the auto-resolve emitter so the frontend receives lifecycle events
            // whenever the constraint solver resolves auto parameters.
            let emitter = Arc::new(TauriAutoResolveEmitter {
                app: app.handle().clone(),
            });
            if let Ok(mut session) = engine_arc.lock() {
                session.set_auto_resolve_emitter(emitter);
            }

            // Install the warm-pool emitter so the frontend receives eviction/donation events.
            // The backend emits unconditionally; the WarmPoolDebugPanel only subscribes under
            // REIFY_DEBUG=1 (PRD §11 Q6 resolution).
            let warm_pool_emitter = Arc::new(TauriWarmPoolEventEmitter {
                app: app.handle().clone(),
            });
            if let Ok(mut session) = engine_arc.lock() {
                session.set_warm_pool_event_emitter(warm_pool_emitter);
            }

            // Install the fea-case-changed emitter so the frontend FeaCasePickerDropdown
            // receives the active case set whenever a MultiCaseResult is observed at commit
            // time. The emitter is a no-op until task 3026 lands solve_load_cases.
            let fea_case_emitter = Arc::new(TauriFeaCaseEmitter {
                app: app.handle().clone(),
            });
            if let Ok(mut session) = engine_arc.lock() {
                session.set_fea_case_emitter(fea_case_emitter);
            }

            // Install the mode-shape-frame emitter so the frontend BucklingPanel
            // receives reference frames (one undeformed base + one peak per mode)
            // whenever a BucklingResult is observed at commit time (task ι/3458).
            let mode_shape_frame_emitter = Arc::new(TauriModeShapeFrameEmitter {
                app: app.handle().clone(),
            });
            if let Ok(mut session) = engine_arc.lock() {
                session.set_mode_shape_frame_emitter(mode_shape_frame_emitter);
            }

            // Install the solve-cancellation sink so cancel_solve_impl can reach
            // the in-flight FEA handle (task γ/4086).  The sink holds the same
            // Arc as AppState.pending_solve_cancel — writes are visible to reads.
            let solve_cancel_sink = Arc::new(
                reify_gui::commands::PendingSolveCancelSink::new(Arc::clone(&solve_cancel_slot)),
            );
            if let Ok(mut session) = engine_arc.lock() {
                session.set_solve_cancel_sink(solve_cancel_sink);
            }

            // Install the solver-progress sink so the frontend receives per-CG-iteration
            // progress events on the "solver-progress" IPC channel (task 4079).
            let solver_progress_emitter = Arc::new(TauriSolverProgressEmitter {
                app: app.handle().clone(),
            });
            if let Ok(mut session) = engine_arc.lock() {
                session.set_solver_progress_sink(solver_progress_emitter);
            }

            // Always create DebugBridge (inert when debug disabled — no JS listener, no HTTP server)
            let debug_bridge = Arc::new(reify_gui::debug::DebugBridge::new(app.handle().clone()));
            app.manage(debug_bridge.clone());

            // Spawn the debug HTTP/MCP server when REIFY_DEBUG=1
            if debug_enabled {
                let engine_for_debug = Arc::clone(&engine_arc);
                let selection_for_debug = Arc::clone(&selection_arc);
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = reify_gui::debug_server::spawn_debug_server(
                        engine_for_debug,
                        selection_for_debug,
                        debug_bridge,
                    )
                    .await
                    {
                        eprintln!("Debug server failed: {e}");
                    }
                });
                eprintln!("REIFY_DEBUG=1: debug server starting on {}", reify_gui::debug_server::debug_endpoint_url(reify_gui::debug_server::resolve_debug_port()));
            }

            // Notify the frontend of the kernel availability at startup.
            app.handle().emit("kernel-status", &kernel_status).ok();

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
            sync_observed_demand,
            update_source,
            save_file,
            open_file,
            open_file_engine,
            export,
            get_source_location,
            get_entity_tree,
            get_entity_identity_map,
            get_mechanism_descriptors,
            get_def_preview,
            get_containing_definition,
            get_entity_at_source_location,
            focus_entity,
            update_selection,
            mcp_tool_call,
            lsp_request,
            claude_send_message,
            claude_abort,
            claude_clear_session,
            claude_permission_decision,
            is_debug_enabled,
            debug_response,
            get_kernel_status,
            read_view_sidecar,
            write_view_sidecar,
            cancel_solve,
            get_active_fea_case,
            set_active_fea_case,
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
