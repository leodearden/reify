// Tauri application entry point for Reify GUI.
//
// Constructs EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker)); OCCT
// registration is automatic via the cfg(has_occt)-gated inventory::submit! in
// reify-kernel-occt::register. The kernel_status::current_kernel_status() call surfaces the
// build-time OCCT_AVAILABLE constant for the startup banner. Wraps in AppState and starts the
// Tauri application with all command handlers. Engine-touching commands submit to the
// evaluation queue, which publishes each evaluation's delta and the evaluation status through
// TauriEvalObserver.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use tracing::warn;

use tauri::{Emitter, Manager};

use reify_constraints::SimpleConstraintChecker;
use reify_eval::SolverProgressSink;
use reify_gui::commands::AppState;
use reify_gui::diff::{StateDelta, delta_to_events};
use reify_gui::engine::{
    AutoResolveEmitter, EngineSession, FeaCaseEmitter, FeaConvergenceEmitter,
    FeaDiagnosticsEmitter, ModeShapeFrameEmitter, WarmPoolEventEmitter,
};
use reify_gui::eval_queue::{EditOrder, EvalActivity, EvalObserver, EvalQueue, EvalRequest};
use reify_gui::event_bus::emit_typed;
use reify_gui::lsp_bridge::LspBridge;
use reify_gui::types::EvaluationStatus;
use reify_gui::watcher::{FileEvent, FileWatcher};
use reify_lsp::server::{LogLine, NotificationSink};
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

/// Tells the frontend what the evaluation queue did: its activity as
/// `evaluation-status`, and each published delta as targeted events.
struct TauriEvalObserver {
    app: tauri::AppHandle,
}

impl EvalObserver for TauriEvalObserver {
    fn activity(&self, activity: EvalActivity) {
        let phase = match activity {
            EvalActivity::Evaluating => "evaluating",
            EvalActivity::Idle => "idle",
        };
        emit_status(&self.app, phase);
    }

    fn delta(&self, delta: &StateDelta) {
        emit_delta(&self.app, delta);
    }
}

/// Notification sink that emits server-initiated notifications as Tauri
/// events.
///
/// Created during Tauri `setup()` where the [`tauri::AppHandle`] is available,
/// then passed into the [`LspBridge`] so the language server can push
/// diagnostics and server log lines directly to the frontend without manual
/// polling.
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

    fn log_message(&self, line: LogLine) {
        // Same shape as the `diagnostics` arm above: one event named for the
        // channel, carrying the LSP payload's own field names (`type` /
        // `message` — `window/logMessage`'s `LogMessageParams`) so the
        // frontend reads the protocol's vocabulary, not a GUI-local
        // re-spelling. `MessageType` serializes as its LSP integer.
        self.app
            .emit(
                "lsp-log",
                serde_json::json!({
                    "type": line.typ,
                    "message": line.message,
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

/// Emits `fea-diagnostics-changed` events to the frontend on every commit (task #4884).
///
/// Payload is a full-list snapshot of `Vec<FeaDiagnosticInfo>` — fires including the
/// empty list so a param edit that fixes the FEA problem clears the stale overlay.
/// Installed during `setup()` alongside [`TauriFeaCaseEmitter`] and other emitters.
struct TauriFeaDiagnosticsEmitter {
    app: tauri::AppHandle,
}

impl FeaDiagnosticsEmitter for TauriFeaDiagnosticsEmitter {
    fn changed(&self, payload: Vec<reify_gui::types::FeaDiagnosticInfo>) {
        if let Err(e) = emit_typed(&self.app, "fea-diagnostics-changed", &payload) {
            warn!("fea-diagnostics-changed emit failed: {}", e);
        }
    }
}

/// Emits `fea-convergence-changed` events to the frontend on every commit (task #5032).
///
/// Payload is a full-value snapshot of `Option<FeaConvergenceInfo>` — fires including
/// `None` so a param edit that clears the FEA problem clears the stale convergence
/// indicator. Installed during `setup()` alongside [`TauriFeaDiagnosticsEmitter`] and
/// other emitters.
struct TauriFeaConvergenceEmitter {
    app: tauri::AppHandle,
}

impl FeaConvergenceEmitter for TauriFeaConvergenceEmitter {
    fn changed(&self, payload: Option<reify_gui::types::FeaConvergenceInfo>) {
        if let Err(e) = emit_typed(&self.app, "fea-convergence-changed", &payload) {
            warn!("fea-convergence-changed emit failed: {}", e);
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

/// Create a FileWatcher for the given file: a change queues a reload of it and
/// hands the editor the new content; a removal tells the editor.
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
                    let evals: tauri::State<'_, Arc<EvalQueue>> = handle.state();
                    // Not awaited: the queue publishes the reload, and this
                    // callback must never wait on an evaluation, because
                    // `FileWatcher::drop` joins the thread it runs on.
                    drop(evals.submit(reify_gui::commands::disk_reload_edit(
                        Arc::clone(&state.engine),
                        changed_path.clone(),
                    )));

                    // Sent even for this process's own write coming back, which
                    // the reload skips: it is how the editor buffer learns what
                    // a parameter write put on disk.
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

/// Point the file watcher at `file`, replacing the previous one.
fn watch_file(app: &tauri::AppHandle, state: &AppState, file: &Path) {
    let watcher = create_watcher(app, file);
    if let Ok(mut watcher_guard) = state.watcher.lock() {
        *watcher_guard = watcher;
    }
}

// --- Tauri command wrappers ---
// Engine-touching commands are async: each submits a request to the evaluation
// queue and awaits its reply, so no thread — the GTK main thread least of all —
// blocks on engine work, and the queue publishes deltas and evaluation status.
// The remaining commands never touch the engine, so they stay sync.

/// Run `call` against the engine as an ordered request that publishes nothing.
async fn engine_call<T: Send + 'static>(
    state: &AppState,
    evals: &Arc<EvalQueue>,
    call: impl FnOnce(&Mutex<EngineSession>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(EvalRequest::engine_call(move || call(&engine)))
        .await
}

#[tauri::command]
async fn get_initial_state(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
) -> Result<reify_gui::types::GuiState, String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(reify_gui::commands::initial_state_evaluation(engine))
        .await
}

/// The DURABLE parameter write (INV-GUI-3, task 5099 η) — one per user gesture.
#[tauri::command]
async fn set_parameter(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    cell_id: String,
    value: String,
    order: EditOrder,
) -> Result<(), String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(reify_gui::commands::commit_parameter_edit(
            engine, cell_id, value, order,
        ))
        .await
}

/// The TRANSIENT parameter preview — one per slider-drag frame. Identical
/// plumbing to `set_parameter` above; only the engine cadence differs.
#[tauri::command]
async fn preview_parameter(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    cell_id: String,
    value: String,
    order: EditOrder,
) -> Result<(), String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(reify_gui::commands::preview_parameter_edit(
            engine, cell_id, value, order,
        ))
        .await
}

/// Register the GUI's PASSIVE observed-demand sources (selective-demand
/// precondition, task 4532). OBSERVATIONAL ONLY — never perturbs evaluation, so
/// it publishes nothing.
#[tauri::command]
async fn sync_observed_demand(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    visible_realizations: Vec<String>,
    displayed_cells: Vec<String>,
    panel_constraints: Vec<String>,
) -> Result<(), String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::sync_observed_demand_impl(
            engine,
            &visible_realizations,
            &displayed_cells,
            &panel_constraints,
        )
    })
    .await
}

/// Register the GUI's viewport-visible realizations as the PRODUCTION selective
/// demand (ENFORCEMENT, task 4737 α). Drives the registry `compute_eval_set`
/// reads, so a HIDDEN body's exclusive cells are pruned from the next warm
/// `edit_param`; the effect reaches the frontend with that edit's published
/// state, so this publishes nothing.
#[tauri::command]
async fn sync_demand(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    visible_realizations: Vec<String>,
) -> Result<(), String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::sync_demand_impl(engine, &visible_realizations)
    })
    .await
}

#[tauri::command]
async fn update_source(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    path: String,
    content: String,
    order: EditOrder,
) -> Result<(), String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(reify_gui::commands::editor_source_edit(
            engine, path, content, order,
        ))
        .await
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
async fn open_file_engine(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    path: String,
) -> Result<reify_gui::types::GuiState, String> {
    let engine = Arc::clone(&state.engine);
    let opened = evals
        .submit(reify_gui::commands::open_file_evaluation(
            engine,
            path.clone(),
        ))
        .await?;
    watch_file(&app, &state, Path::new(&path));
    Ok(opened)
}

#[tauri::command]
async fn export(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    format: String,
    path: String,
) -> Result<(), String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::export_impl(engine, &format, &path)
    })
    .await
}

#[tauri::command]
async fn get_source_location(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    entity_path: String,
) -> Result<reify_mcp::SourceLocationInfo, String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::get_source_location_impl(engine, &entity_path)
    })
    .await
}

#[tauri::command]
async fn get_entity_tree(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
) -> Result<Vec<reify_gui::types::EntityTreeNode>, String> {
    engine_call(&state, &evals, reify_gui::commands::get_entity_tree_impl).await
}

#[tauri::command]
async fn get_entity_identity_map(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
) -> Result<std::collections::HashMap<String, reify_gui::types::EntityIdentity>, String> {
    engine_call(
        &state,
        &evals,
        reify_gui::commands::get_entity_identity_map_impl,
    )
    .await
}

#[tauri::command]
async fn get_mechanism_descriptors(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
) -> Result<Vec<reify_gui::types::MechanismDescriptor>, String> {
    engine_call(
        &state,
        &evals,
        reify_gui::commands::get_mechanism_descriptors_impl,
    )
    .await
}

#[tauri::command]
async fn get_def_preview(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    def_name: String,
) -> Result<reify_gui::types::GuiState, String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::get_def_preview_impl(engine, &def_name)
    })
    .await
}

#[tauri::command]
async fn get_containing_definition(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    line: u32,
    col: u32,
) -> Result<Option<reify_gui::types::DefInfo>, String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::get_containing_definition_impl(engine, line, col)
    })
    .await
}

#[tauri::command]
async fn get_entity_at_source_location(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    line: u32,
    col: u32,
) -> Result<Option<String>, String> {
    engine_call(&state, &evals, move |engine| {
        reify_gui::commands::get_entity_at_source_location_impl(engine, line, col)
    })
    .await
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
async fn mcp_tool_call(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    name: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let engine = Arc::clone(&state.engine);
    let ctx = reify_gui::mcp_context::TauriToolContext::builder(Arc::clone(&engine))
        .with_event_emitter(move |event_name, payload| {
            app.emit(event_name, payload).ok();
        })
        .with_selection(Arc::clone(&state.selection))
        .build();
    evals
        .submit(reify_gui::mcp_context::mcp_tool_call_evaluation(
            ctx, engine, name, params,
        ))
        .await
}

/// Task 5772: dispatched on the persistent large-stack LSP lane rather than on
/// the awaiting tokio worker, whose ~2 MiB stack this compiler-adjacent work
/// reaches at keystroke frequency.
///
/// Stays `async`. Converting it to a sync command would make Tauri run it as
/// `ExecutionContext::Blocking` on the IPC thread with NO ambient tokio runtime,
/// so `Handle::current()` inside `lsp_request_on_worker` would panic — and that
/// is also precisely the condition under which `handle_request`'s four
/// `spawn_blocking` arms panic.
///
/// The `Arc` is the `'static` price of a persistent lane, and follows the shape
/// `debug_response` already uses with `tauri::State<'_, Arc<DebugBridge>>`.
#[tauri::command]
async fn lsp_request(
    bridge: tauri::State<'_, Arc<LspBridge>>,
    method: String,
    params: String,
) -> Result<String, String> {
    // Diagnostics are emitted automatically by TauriNotificationSink
    // during didOpen/didChange/didClose processing — no manual polling needed.
    reify_gui::lsp_bridge::lsp_request_on_worker(Arc::clone(&bridge), method, params).await
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
/// Tool calls (including any `reify_` prefixed names) are forwarded to the
/// frontend as `claude-tool-call` events only; there is no in-process engine-
/// mutating interception here (deleted per gui-state-sync PRD L8). A future
/// properly-wired, synced re-introduction of engine-mutating sidecar tools is
/// owned by docs/prds/v0_6/ai-native-editing.md.
#[tauri::command]
async fn claude_send_message(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    context: Option<reify_gui::claude_bridge::MessageContext>,
) -> Result<String, String> {
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

    // Lazily spawn the sidecar (if not running) and wait for it to become ready.
    reify_gui::claude_bridge::ensure_sidecar_ready(
        &state.sidecar,
        move || {
            let path = sidecar_path;
            let app_c = app_for_events;
            let ws = workspace;
            let le = landlock_exec_path;
            async move {
                reify_gui::claude_bridge::spawn_sidecar_impl(
                    &path,
                    move |name, payload| {
                        app_c.emit(&name, payload).ok();
                    },
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

/// Return the per-dimension display-unit ladders backing the Parameters
/// panel's per-cell unit picker (task #5199). Pure data table — no engine
/// state involved, so this command takes no `AppState`.
#[tauri::command]
fn get_unit_ladders() -> Vec<reify_gui::display_units::DimensionLadder> {
    reify_gui::display_units::unit_ladders()
}

/// Cancel an in-flight FEA solve (GR-016 ζ, PRD §11 Q2).
///
/// Reads `AppState::pending_solve_cancel`, calls `.cancel()` on the handle if
/// present, and clears the slot.  Returns `Ok(())` in both the "cancelled" and
/// "no-op" cases.  The engine-side wiring that publishes the handle is a
/// follow-on task.
#[tauri::command]
fn cancel_solve(state: tauri::State<'_, AppState>) -> Result<(), String> {
    reify_gui::commands::cancel_solve_impl(&state)
}

/// Return the currently active FEA case name (task 3026 case-picker).
///
/// `None` means the active case has never been set (engine defaults to
/// lex-first). Returns `Some(name)` after a `set_active_fea_case` call.
#[tauri::command]
async fn get_active_fea_case(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
) -> Result<Option<String>, String> {
    engine_call(
        &state,
        &evals,
        reify_gui::commands::get_active_fea_case_impl,
    )
    .await
}

/// Switch to the named FEA case (task 3026 case-picker).
///
/// Stores the case name in the engine session and re-applies FEA scalar channels
/// from the cached tessellation snapshot (no re-evaluation, no re-tessellation);
/// the re-sourced contour reaches the frontend as a published delta. Unknown
/// case names fall back to the lex-first default.
#[tauri::command]
async fn set_active_fea_case(
    state: tauri::State<'_, AppState>,
    evals: tauri::State<'_, Arc<EvalQueue>>,
    case: String,
) -> Result<(), String> {
    let engine = Arc::clone(&state.engine);
    evals
        .submit(reify_gui::commands::active_fea_case_evaluation(
            engine, case,
        ))
        .await
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
    let engine_arc = Arc::new(Mutex::new(session));

    // Loaded in `setup()`, through the evaluation queue.
    let argv = std::env::args().nth(1).unwrap_or_default();

    let debug_enabled = std::env::var("REIFY_DEBUG").is_ok_and(|v| v == "1");
    let selection_arc = Arc::new(RwLock::new(reify_mcp::SelectionInfo::default()));

    // Shared slot for in-flight FEA solve handle (task γ/4086).
    // PendingSolveCancelSink (installed below in setup()) writes this slot;
    // cancel_solve_impl reads it via AppState.pending_solve_cancel.
    // Both hold an Arc clone — same underlying Mutex.
    let solve_cancel_slot: Arc<Mutex<Option<reify_eval::CancellationHandle>>> =
        Arc::new(Mutex::new(None));

    // Shared delta baseline — the SAME `Arc` is handed to the evaluation queue
    // and to `DebugServerState` below, so a debug-driven mutation and a queued
    // evaluation diff against the SAME baseline (INV-GUI-2, task 5035 L6).
    let last_state_arc: Arc<Mutex<Option<reify_gui::types::GuiState>>> =
        Arc::new(Mutex::new(None));

    let app_state = AppState {
        engine: Arc::clone(&engine_arc),
        watcher: Mutex::new(None),
        sidecar: tokio::sync::Mutex::new(None),
        selection: Arc::clone(&selection_arc),
        initial_file: Mutex::new(None),
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
            // Managed as an `Arc` (task 5772): `lsp_request` clones it into the
            // `'static` closure the persistent LSP lane requires. Same shape the
            // DebugBridge below already uses.
            let lsp_bridge = Arc::new(LspBridge::with_sink(sink));
            app.manage(lsp_bridge);

            // Install the auto-resolve emitter so the frontend receives lifecycle events
            // whenever the constraint solver resolves auto parameters.
            let emitter = Arc::new(TauriAutoResolveEmitter {
                app: app.handle().clone(),
            });

            // Install the warm-pool emitter so the frontend receives eviction/donation events.
            // The backend emits unconditionally; the WarmPoolDebugPanel only subscribes under
            // REIFY_DEBUG=1 (PRD §11 Q6 resolution).
            let warm_pool_emitter = Arc::new(TauriWarmPoolEventEmitter {
                app: app.handle().clone(),
            });

            // Install the fea-case-changed emitter so the frontend FeaCasePickerDropdown
            // receives the active case set whenever a MultiCaseResult is observed at commit
            // time. The emitter is a no-op until task 3026 lands solve_load_cases.
            let fea_case_emitter = Arc::new(TauriFeaCaseEmitter {
                app: app.handle().clone(),
            });

            // Install the fea-diagnostics-changed emitter so the frontend FEA diagnostic
            // overlay refreshes live on every commit (task #4884 — param-edit re-solve path).
            // Payload is a full-list snapshot including the empty list to clear a stale overlay.
            let fea_diagnostics_emitter = Arc::new(TauriFeaDiagnosticsEmitter {
                app: app.handle().clone(),
            });

            // Install the fea-convergence-changed emitter so the frontend FEA convergence
            // indicator refreshes live on every commit (task #5032 — param-edit re-solve
            // path). Payload is a full-value snapshot including None to clear a stale
            // indicator.
            let fea_convergence_emitter = Arc::new(TauriFeaConvergenceEmitter {
                app: app.handle().clone(),
            });

            // Install the mode-shape-frame emitter so the frontend BucklingPanel
            // receives reference frames (one undeformed base + one peak per mode)
            // whenever a BucklingResult is observed at commit time (task ι/3458).
            let mode_shape_frame_emitter = Arc::new(TauriModeShapeFrameEmitter {
                app: app.handle().clone(),
            });

            // Install the solve-cancellation sink so cancel_solve_impl can reach
            // the in-flight FEA handle (task γ/4086).  The sink holds the same
            // Arc as AppState.pending_solve_cancel — writes are visible to reads.
            let solve_cancel_sink = Arc::new(reify_gui::commands::PendingSolveCancelSink::new(
                Arc::clone(&solve_cancel_slot),
            ));

            // Install the solver-progress sink so the frontend receives per-CG-iteration
            // progress events on the "solver-progress" IPC channel (task 4079).
            let solver_progress_emitter = Arc::new(TauriSolverProgressEmitter {
                app: app.handle().clone(),
            });

            // Install all session emitters/sinks in a single lock acquisition instead of
            // one lock/unlock per emitter — they all target the same engine_arc mutex, so
            // there is no isolation benefit to separate critical sections (amendment,
            // task #5032 review).
            if let Ok(mut session) = engine_arc.lock() {
                session.set_auto_resolve_emitter(emitter);
                session.set_warm_pool_event_emitter(warm_pool_emitter);
                session.set_fea_case_emitter(fea_case_emitter);
                session.set_fea_diagnostics_emitter(fea_diagnostics_emitter);
                session.set_fea_convergence_emitter(fea_convergence_emitter);
                session.set_mode_shape_frame_emitter(mode_shape_frame_emitter);
                session.set_solve_cancel_sink(solve_cancel_sink);
                session.set_solver_progress_sink(solver_progress_emitter);
            } else {
                // A poisoned mutex here silently skips installing ALL emitters/sinks,
                // degrading the GUI to no live events with no other signal (amendment,
                // task #5032 review) — surface it so a poisoned lock at startup is
                // observable instead of a silent no-op.
                warn!(
                    "engine_arc lock poisoned during setup(): no session emitters/sinks \
                     installed — GUI will not receive live auto-resolve/warm-pool/fea-case/\
                     fea-diagnostics/fea-convergence/mode-shape/solver-progress events"
                );
            }

            // Only now, after the engine lock above: were an evaluation already
            // running, taking that lock would stall the main thread behind it.
            let evals = EvalQueue::on_engine_lane(
                Arc::clone(&last_state_arc),
                Arc::new(TauriEvalObserver {
                    app: app.handle().clone(),
                }),
            );
            app.manage(Arc::clone(&evals));

            // The main loop dispatches no invoke until setup() returns, so the
            // frontend's first get_initial_state queues behind this load, and
            // the window paints while it runs.
            if let Some((file, load)) =
                reify_gui::commands::begin_initial_file_load(&evals, Arc::clone(&engine_arc), &argv)
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match load.await {
                        Ok(_) => {
                            let state: tauri::State<'_, AppState> = handle.state();
                            watch_file(&handle, &state, &file);
                            if let Ok(mut initial_file) = state.initial_file.lock() {
                                *initial_file = Some(file);
                            }
                        }
                        Err(e) => eprintln!(
                            "Warning: failed to load initial file {}: {}",
                            file.display(),
                            e
                        ),
                    }
                });
            }

            // Always create DebugBridge (inert when debug disabled — no JS listener, no HTTP server)
            let debug_bridge = Arc::new(reify_gui::debug::DebugBridge::new(app.handle().clone()));
            app.manage(debug_bridge.clone());

            // Spawn the debug HTTP/MCP server when REIFY_DEBUG=1
            if debug_enabled {
                let engine_for_debug = Arc::clone(&engine_arc);
                let selection_for_debug = Arc::clone(&selection_arc);
                let last_state_for_debug = Arc::clone(&last_state_arc);
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = reify_gui::debug_server::spawn_debug_server(
                        engine_for_debug,
                        selection_for_debug,
                        debug_bridge,
                        last_state_for_debug,
                    )
                    .await
                    {
                        eprintln!("Debug server failed: {e}");
                    }
                });
                eprintln!(
                    "REIFY_DEBUG=1: debug server starting on {}",
                    reify_gui::debug_server::debug_endpoint_url(
                        reify_gui::debug_server::resolve_debug_port()
                    )
                );
            }

            // Notify the frontend of the kernel availability at startup.
            app.handle().emit("kernel-status", &kernel_status).ok();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            set_parameter,
            preview_parameter,
            sync_observed_demand,
            sync_demand,
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
            get_unit_ladders,
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
