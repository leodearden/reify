// Debug server — MCP Streamable HTTP + REST fallback on localhost:3939.
//
// The MCP endpoint (`POST /mcp`) implements a stateless Streamable HTTP transport:
// no session tracking, any `Mcp-Session-Id` header is accepted. This follows the
// fused-memory pattern so Claude Code sessions survive GUI restarts without
// requiring `/mcp` reconnection.
//
// The REST endpoint (`POST /debug/{command}`) provides the same tools via plain JSON
// for manual `curl` testing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::debug::DebugBridge;
use crate::engine::EngineSession;
use reify_mcp::SelectionInfo;

// --- Tool definitions ---

struct ToolDef {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "health",
            description: "Liveness check — returns ok:true when the debug server is running",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "engine_state",
            description: "Full engine state: meshes (entity paths + vertex/face counts, no raw arrays), values, constraints, eval status",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "mesh_stats",
            description: "Per-entity mesh statistics: vertex count, face count, bounding box",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "viewport_state",
            description: "Three.js viewport state: camera position/target/fov, mesh count, scene bounding box, selected entity",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "screenshot",
            description: "Take a screenshot of the 3D viewport. Returns a PNG image.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "screenshot_window",
            description: "Take a full-window screenshot including panels, overlays, and probe popups (DOM + WebGL composite via html-to-image). Returns a PNG image.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "store_state",
            description: "Snapshot of all Solid.js stores: engine (mesh keys, values, constraints, eval status), editor (open files, active file, cursor), selection, claude (session status, message count)",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "editor_content",
            description: "Get the active editor file content, cursor position, open files list, and dirty state",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "dom_query",
            description: "Query a DOM element by data-testid. Returns existence, visibility, text content, and bounding rect.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "testId": {
                        "type": "string",
                        "description": "The data-testid attribute value to query"
                    }
                },
                "required": ["testId"]
            }),
        },
        ToolDef {
            name: "list_elements",
            description: "List all DOM elements with data-testid attributes. Returns testId, tagName, visibility, and bounds for each.",
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "click_element",
            description: "Click a DOM element by data-testid. Dispatches a synthetic click event.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "testId": {
                        "type": "string",
                        "description": "The data-testid attribute value of the element to click"
                    }
                },
                "required": ["testId"]
            }),
        },
        ToolDef {
            name: "type_in_editor",
            description: "Set the editor content. Replaces the full document via CodeMirror dispatch, triggering evaluation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The new editor content"
                    }
                },
                "required": ["content"]
            }),
        },
        ToolDef {
            name: "keyboard",
            description: "Dispatch a keyboard event. Triggers keyboard shortcuts (e.g. F5 for re-evaluate, Ctrl+O for open).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "The key value (e.g. 'F5', 'o', 'Escape')"
                    },
                    "ctrl": {"type": "boolean", "description": "Ctrl modifier"},
                    "shift": {"type": "boolean", "description": "Shift modifier"},
                    "alt": {"type": "boolean", "description": "Alt modifier"},
                    "meta": {"type": "boolean", "description": "Meta/Cmd modifier"}
                },
                "required": ["key"]
            }),
        },
        ToolDef {
            name: "select_entity",
            description: "Select an entity in the viewport by its entity path.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "entityPath": {
                        "type": "string",
                        "description": "The entity path to select (or null to clear selection)"
                    }
                }
            }),
        },
        ToolDef {
            name: "fit_to_view",
            description: "Reset the camera to fit all geometry in the viewport.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "set_camera",
            description: "Set the viewport camera to an explicit pose. Used by the visual regression harness for deterministic framing — same input → same camera frame → same pixels.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    },
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Camera world-space position [x, y, z]"
                    },
                    "target": {
                        "type": "array",
                        "items": {"type": "number"},
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Look-at target [x, y, z]"
                    },
                    "up": {
                        "type": "array",
                        "items": {"type": "number"},
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Optional up vector [x, y, z]"
                    },
                    "zoom": {
                        "type": "number",
                        "description": "Optional positive zoom factor"
                    }
                },
                "required": ["position", "target"]
            }),
        },
        ToolDef {
            name: "open_file",
            description: "Open a .ri file from disk into the editor and engine. Reads the file, loads it into the engine for evaluation, and opens it in the frontend editor.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the .ri file to open"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "set_test_mode",
            description: "Freeze CSS animations and transitions for pixel-stable DOM screenshots. Does NOT pause JS-driven animations or the Three.js render loop. Returns { ok: true, test_mode: bool }.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "true to freeze animations, false to resume"
                    }
                },
                "required": ["enabled"]
            }),
        },
        ToolDef {
            name: "morph_stats",
            description: "Mesh-morph runtime stats: morph_count, remesh_count, last_rejection_reason. \
                          Surfaces reify-mesh-morph::stats::snapshot(). Per GR-016 / \
                          docs/prds/v0_3/gui-event-channel-inventory.md §2.3.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "body_id": {
                        "type": "string",
                        "description": "Optional body identifier (currently ignored; \
                                        returns global stats — per-body filtering deferred \
                                        to mesh-morphing PRD #2947+ engine wiring)."
                    }
                }
            }),
        },
        ToolDef {
            name: "mesh_morph_stats",
            description: "Session diagnostic counter snapshot from the mesh-morph engine \
                          (morphed, remeshed_quality_hard_fail, remeshed_quality_soft_fail, \
                          ineligible_structural_change, ineligible_bijection_failure, \
                          ineligible_naming_error, panicked) plus a session_start_unix_ms \
                          timestamp. Pass reset:true to atomically zero all counters and \
                          restart the session clock before returning the (post-reset) snapshot \
                          — useful for benchmark sequences. Per mesh-morphing PRD #12 / \
                          docs/prds/v0_3/mesh-morphing.md.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reset": {
                        "type": "boolean",
                        "description": "When true, atomically zero all diagnostic counters \
                                        and restart the session clock before returning the \
                                        post-reset snapshot. Defaults to false."
                    }
                }
            }),
        },
        ToolDef {
            name: "wait_for_idle",
            description: "Block until the engine is idle (no in-flight evaluation) and one frame has rendered. Returns {ok: true, idle_after_ms: N} or {error: 'timeout'}. Used by the visual-regression harness to replace engine_state polling.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum wait in milliseconds; default 30000."
                    }
                }
            }),
        },
    ]
}

// --- JSON-RPC types ---

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

// --- Server state ---

#[derive(Clone)]
struct DebugServerState {
    engine: Arc<Mutex<EngineSession>>,
    #[allow(dead_code)]
    selection: Arc<RwLock<SelectionInfo>>,
    debug_bridge: Arc<DebugBridge>,
}

fn is_image_tool(name: &str) -> bool {
    matches!(name, "screenshot" | "screenshot_window")
}

// --- Tool dispatch ---

// Handles state-free tool arms so they can be tested without a DebugServerState.
// Returns Some(result) when the name matches a stateless arm, None otherwise.
async fn dispatch_stateless_tool(name: &str, params: &Value) -> Option<Result<Value, String>> {
    match name {
        "health" => Some(Ok(json!({"ok": true}))),
        "morph_stats" => Some(handle_morph_stats(params.clone()).await),
        "mesh_morph_stats" => Some(handle_mesh_morph_stats(params.clone()).await),
        _ => None,
    }
}

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    if let Some(result) = dispatch_stateless_tool(name, &params).await {
        return result;
    }
    match name {
        "engine_state" => handle_engine_state(state).await,
        "mesh_stats" => handle_mesh_stats(state).await,
        "open_file" => handle_open_file(state, params).await,
        "wait_for_idle" => handle_wait_for_idle(state, params).await,
        _ => {
            // Frontend-mediated: delegate to DebugBridge
            state.debug_bridge.query_frontend(name, params).await
        }
    }
}

/// Run a closure that needs engine access on a real OS thread (not tokio).
/// EngineSession uses OcctKernelHandle::execute() with blocking_send which
/// panics inside any tokio runtime context.
async fn run_on_engine<F, T>(engine: &Arc<Mutex<EngineSession>>, f: F) -> Result<T, String>
where
    F: FnOnce(&mut EngineSession) -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    let engine = Arc::clone(engine);
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        // with_engine_lock catches panics and recovers from mutex poisoning,
        // so f() panicking will not leave the mutex poisoned for future callers.
        // The user closure f returns Result<T, String>, so with_engine_lock
        // wraps it in another Result layer — flatten with and_then(identity).
        let result =
            crate::engine_lock::with_engine_lock(&engine, f).and_then(std::convert::identity);
        let _ = tx.send(result);
    });
    rx.await.map_err(|_| "engine thread died".to_string())?
}

async fn handle_engine_state(state: &DebugServerState) -> Result<Value, String> {
    run_on_engine(&state.engine, |session| {
        let gui_state = session
            .build_gui_state()
            .map_err(|e| format!("build_gui_state failed: {e}"))?;

        let meshes: Vec<Value> = gui_state
            .meshes
            .iter()
            .map(|m| {
                json!({
                    "entity_path": m.entity_path,
                    "vertex_count": m.vertices.len() / 3,
                    "face_count": m.indices.len() / 3,
                    "has_normals": m.normals.is_some(),
                })
            })
            .collect();

        Ok(json!({
            "meshes": meshes,
            "values": gui_state.values,
            "constraints": gui_state.constraints,
            "files": gui_state.files,
        }))
    })
    .await
}

async fn handle_mesh_stats(state: &DebugServerState) -> Result<Value, String> {
    run_on_engine(&state.engine, |session| {
        let gui_state = session
            .build_gui_state()
            .map_err(|e| format!("build_gui_state failed: {e}"))?;

        let stats: Vec<Value> = gui_state
            .meshes
            .iter()
            .map(|m| {
                let vertex_count = m.vertices.len() / 3;
                let face_count = m.indices.len() / 3;

                let mut min = [f32::INFINITY; 3];
                let mut max = [f32::NEG_INFINITY; 3];
                for chunk in m.vertices.chunks_exact(3) {
                    for i in 0..3 {
                        min[i] = min[i].min(chunk[i]);
                        max[i] = max[i].max(chunk[i]);
                    }
                }

                json!({
                    "entity_path": m.entity_path,
                    "vertex_count": vertex_count,
                    "face_count": face_count,
                    "bounding_box": if vertex_count > 0 {
                        json!({"min": min, "max": max})
                    } else {
                        json!(null)
                    }
                })
            })
            .collect();

        Ok(json!({"meshes": stats}))
    })
    .await
}

/// `morph_stats` debug-MCP RPC handler. Surfaces the process-global
/// `reify_mesh_morph::stats::snapshot()`. State-free: mesh-morph stats are
/// not engine-bound, so no DebugServerState / engine lock is needed.
///
/// `_params` may carry an optional `body_id` (per PRD §2.3 request shape) but
/// it is intentionally ignored — per-body filtering is deferred to the
/// mesh-morphing engine wiring (PRD tasks #2947-#2949). Both the `()` and
/// `{body_id: ...}` request forms return the same global snapshot. See
/// docs/prds/v0_3/gui-event-channel-inventory.md §2.3.
async fn handle_morph_stats(_params: Value) -> Result<Value, String> {
    let stats = reify_mesh_morph::stats::snapshot();
    serde_json::to_value(&stats).map_err(|e| format!("failed to serialize MorphStats: {e}"))
}

// --- Session-start timestamp (process-global) ---

/// Unix-epoch-milliseconds at the time of first access. Zero before first call.
/// Never rolls back — reset_session_start() stores a new "now" to restart the clock.
static SESSION_START_UNIX_MS: AtomicU64 = AtomicU64::new(0);

/// Returns the current session-start timestamp, initializing it lazily on the
/// first call via compare-exchange (0 → now). Subsequent calls return the same
/// value until reset_session_start() is called.
fn session_start_unix_ms() -> u64 {
    let current = SESSION_START_UNIX_MS.load(Ordering::Relaxed);
    if current != 0 {
        return current;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    // CAS: only set if still 0 (first caller wins; harmless race).
    match SESSION_START_UNIX_MS.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed) {
        Ok(_) => now,
        Err(actual) => actual, // another thread raced and won
    }
}

/// Restart the session clock to "now". Called by handle_mesh_morph_stats when
/// reset:true is requested, so the timestamp reflects the post-reset window start.
fn reset_session_start() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    SESSION_START_UNIX_MS.store(now, Ordering::Relaxed);
}

/// `mesh_morph_stats` debug-MCP RPC handler.
///
/// Returns a flat JSON object containing all seven diagnostic counters from
/// `reify_mesh_morph::diagnostics::snapshot()` (morphed,
/// remeshed_quality_hard_fail, remeshed_quality_soft_fail,
/// ineligible_structural_change, ineligible_bijection_failure,
/// ineligible_naming_error, panicked) plus a `session_start_unix_ms` field.
///
/// When `params["reset"]` is `true`, atomically zeros all counters via
/// `diagnostics::reset()` and refreshes the session clock before taking the
/// snapshot — the response reflects the post-reset zeros and the new clock.
///
/// State-free: both the counters and the session timestamp are process-globals,
/// so no DebugServerState / engine lock is needed.
async fn handle_mesh_morph_stats(params: Value) -> Result<Value, String> {
    let reset = params.get("reset").and_then(Value::as_bool).unwrap_or(false);
    if reset {
        reify_mesh_morph::diagnostics::reset();
        reset_session_start();
    }
    let ts = session_start_unix_ms();
    let snapshot = reify_mesh_morph::diagnostics::snapshot();
    let mut obj = serde_json::to_value(&snapshot)
        .map_err(|e| format!("failed to serialize DiagnosticSnapshot: {e}"))?;
    if let Some(map) = obj.as_object_mut() {
        map.insert("session_start_unix_ms".to_string(), serde_json::Value::Number(ts.into()));
    }
    Ok(obj)
}

async fn handle_open_file(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let raw_path = params["path"]
        .as_str()
        .ok_or_else(|| "path is required".to_string())?;

    // Canonicalise the path before reading so the frontend receives the same
    // absolute key regardless of whether the caller supplied a relative or
    // absolute spelling (fixes bug #3892: duplicate tabs via debug bridge).
    let path = crate::path_key::canonicalize_debug_open_path(raw_path);

    // Read file from disk
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read {path}: {e}"))?;

    // Load into engine and build GUI state (on OS thread — OCCT panics in tokio)
    let path_clone = path.clone();
    let content_clone = content.clone();
    let gui_state = run_on_engine(&state.engine, move |session| {
        session
            .update_source(&path_clone, &content_clone)
            .map_err(|e| format!("update_source failed: {e}"))?;
        session
            .build_gui_state()
            .map_err(|e| format!("build_gui_state failed: {e}"))
    })
    .await?;

    // Serialize GUI state for the frontend
    let gui_state_json =
        serde_json::to_value(&gui_state).map_err(|e| format!("serialize gui_state failed: {e}"))?;

    // Tell frontend to open file and init engine state
    let file_data = json!({
        "path": path,
        "content": content,
        "guiState": gui_state_json,
    });
    state
        .debug_bridge
        .query_frontend("open_file", file_data)
        .await
}

// --- MCP Streamable HTTP handler ---

async fn handle_mcp(
    State(state): State<DebugServerState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let id = req.id.unwrap_or(Value::Null);

    if req.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::err(
            id,
            -32600,
            "expected jsonrpc: \"2.0\"".to_string(),
        ));
    }

    let response = match req.method.as_str() {
        "initialize" => JsonRpcResponse::ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "reify-debug",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),

        "notifications/initialized" => {
            // Acknowledgement, no response needed — but JSON-RPC requires one if id is present
            JsonRpcResponse::ok(id, json!({}))
        }

        "tools/list" => {
            let tools: Vec<Value> = tool_defs()
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                    })
                })
                .collect();
            JsonRpcResponse::ok(id, json!({"tools": tools}))
        }

        "tools/call" => {
            let tool_name = req.params["name"].as_str().unwrap_or("");
            let tool_args = req.params.get("arguments").cloned().unwrap_or(json!({}));

            match dispatch_tool(&state, tool_name, tool_args).await {
                Ok(result) => {
                    // Check if this is an image tool (contains base64 image data)
                    if is_image_tool(tool_name)
                        && let Some(data) = result.get("data").and_then(|d| d.as_str())
                    {
                        // Strip data URL prefix if present
                        let base64 = data.strip_prefix("data:image/png;base64,").unwrap_or(data);
                        return Json(JsonRpcResponse::ok(
                            id,
                            json!({
                                "content": [{
                                    "type": "image",
                                    "data": base64,
                                    "mimeType": "image/png"
                                }]
                            }),
                        ));
                    }

                    // Standard text content block
                    let text = serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string());
                    JsonRpcResponse::ok(
                        id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": text
                            }]
                        }),
                    )
                }
                Err(e) => JsonRpcResponse::ok(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {e}")
                        }],
                        "isError": true
                    }),
                ),
            }
        }

        _ => JsonRpcResponse::err(id, -32601, format!("method not found: {}", req.method)),
    };

    Json(response)
}

// --- REST handler ---

async fn handle_rest(
    Path(command): Path<String>,
    State(state): State<DebugServerState>,
    Json(params): Json<Value>,
) -> impl IntoResponse {
    match dispatch_tool(&state, &command, params).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_wait_for_idle(state: &DebugServerState, params: Value) -> Result<Value, String> {
    // Validate and canonicalize timeout_ms here so the Rust oneshot and the
    // frontend handler both use the same effective timeout with no drift
    // between two independent parsers.
    let timeout_ms: u64 = match params.get("timeout_ms") {
        None => 30_000,
        Some(v) => match v.as_u64().filter(|&n| n > 0) {
            Some(n) => n,
            None => return Ok(json!({"error": "timeout_ms must be a positive integer"})),
        },
    };

    // Fast Rust-side pre-check: if the engine session has never completed a
    // compile/check cycle, return immediately rather than delegating to the
    // frontend where `evalStatus` starts as `'idle'` by default and would
    // produce a false-positive ok response on a fresh (un-loaded) session.
    {
        let is_idle = crate::engine_lock::with_engine_lock(&state.engine, |s| s.is_idle())?;
        if !is_idle {
            return Ok(json!({"error": "engine_not_started"}));
        }
    }

    // Build a canonical params object so the frontend receives a validated value.
    let canonical_params = json!({ "timeout_ms": timeout_ms });
    // Add a 5-second buffer so the Rust-side oneshot fires *after* the frontend
    // has had a chance to return its own {error: "timeout"} response.
    let rust_timeout = Duration::from_millis(timeout_ms.saturating_add(5_000));
    state
        .debug_bridge
        .query_frontend_with_timeout("wait_for_idle", canonical_params, rust_timeout)
        .await
}

// --- Server spawn ---

pub async fn spawn_debug_server(
    engine: Arc<Mutex<EngineSession>>,
    selection: Arc<RwLock<SelectionInfo>>,
    debug_bridge: Arc<DebugBridge>,
) -> Result<(), String> {
    let state = DebugServerState {
        engine,
        selection,
        debug_bridge,
    };

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/debug/{command}", post(handle_rest))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3939")
        .await
        .map_err(|e| format!("failed to bind debug server on :3939: {e}"))?;

    tracing::info!("Debug server listening on http://127.0.0.1:3939");

    axum::serve(listener, app.into_make_service())
        .await
        .map_err(|e| format!("debug server error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Process-global lock for tests that touch the global diagnostics counters.
    // Acquire this before reset_for_test() + handler call so parallel test
    // threads do not race on the shared AtomicU64 counters.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn run_on_engine_does_not_poison_mutex_when_closure_panics() {
        let engine = crate::tests::make_test_engine();

        // First call: closure panics — run_on_engine must return Err, not propagate.
        let first = run_on_engine(&engine, |_s| -> Result<(), String> {
            panic!("from-closure")
        })
        .await;
        assert!(
            first.is_err(),
            "panicking closure must produce Err from run_on_engine"
        );

        // Second call: mutex must be usable (not poisoned after the first call).
        let second = run_on_engine(&engine, |s| Ok(s.is_idle())).await;
        assert_eq!(
            second,
            Ok(true),
            "engine must still be usable after a panicking closure (mutex must not be poisoned)"
        );
    }

    #[test]
    fn tool_defs_includes_set_camera() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "set_camera")
            .expect("set_camera must be present in tool_defs()");

        let schema = &entry.input_schema;

        // Tool exposes an object schema
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );

        // Only position and target are required; up and zoom are optional
        let required = schema["required"]
            .as_array()
            .expect("input_schema.required must be an array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("position")),
            "'position' must be in required"
        );
        assert!(
            required.iter().any(|v| v.as_str() == Some("target")),
            "'target' must be in required"
        );
        assert!(
            !required.iter().any(|v| v.as_str() == Some("up")),
            "'up' must NOT be in required"
        );
        assert!(
            !required.iter().any(|v| v.as_str() == Some("zoom")),
            "'zoom' must NOT be in required"
        );
    }

    #[test]
    fn tool_defs_contains_set_test_mode() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "set_test_mode")
            .expect("set_test_mode must be present in tool_defs()");

        // input_schema must declare an object with required boolean 'enabled'
        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        let enabled_prop = &schema["properties"]["enabled"];
        assert_eq!(
            enabled_prop["type"].as_str(),
            Some("boolean"),
            "properties.enabled.type must be 'boolean'"
        );
        let required = schema["required"]
            .as_array()
            .expect("input_schema.required must be an array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("enabled")),
            "'enabled' must be listed in required"
        );
    }

    #[test]
    fn tool_defs_includes_screenshot_window() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "screenshot_window")
            .expect("screenshot_window must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(
            schema["properties"].is_object(),
            "input_schema.properties must be an object"
        );
    }

    #[test]
    fn is_image_tool_recognizes_both_screenshot_variants() {
        assert!(is_image_tool("screenshot"));
        assert!(is_image_tool("screenshot_window"));
        assert!(!is_image_tool("health"));
        assert!(!is_image_tool(""));
    }

    #[test]
    fn tool_defs_registers_mesh_morph_stats() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "mesh_morph_stats")
            .expect("mesh_morph_stats must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(
            !entry.description.is_empty(),
            "mesh_morph_stats must have a non-empty description"
        );
        // `reset` is optional — required may be absent entirely; if present it
        // must not list `reset`.
        if let Some(required) = schema["required"].as_array() {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("reset")),
                "'reset' must NOT be listed in required (it is optional)"
            );
        }
    }

    #[test]
    fn tool_defs_registers_morph_stats() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "morph_stats")
            .expect("morph_stats must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(!entry.description.is_empty(), "morph_stats must have a non-empty description");
        // `body_id` is optional — the no-args `()` form must be valid per PRD §2.3.
        // `required` may be absent entirely; if present it must not list body_id.
        if let Some(required) = schema["required"].as_array() {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("body_id")),
                "'body_id' must NOT be listed in required (it is optional)"
            );
        }
    }

    #[test]
    fn tool_defs_includes_wait_for_idle() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "wait_for_idle")
            .expect("wait_for_idle must be present in tool_defs()");

        let schema = &entry.input_schema;

        // Must be an object schema
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );

        // timeout_ms must be an integer (or number) property
        let timeout_prop = &schema["properties"]["timeout_ms"];
        let timeout_type = timeout_prop["type"].as_str();
        assert!(
            timeout_type == Some("integer") || timeout_type == Some("number"),
            "properties.timeout_ms.type must be 'integer' or 'number', got {:?}",
            timeout_type
        );

        // timeout_ms must NOT be required (it is optional)
        if let Some(required) = schema["required"].as_array() {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("timeout_ms")),
                "'timeout_ms' must NOT be listed in required (it is optional)"
            );
        }
    }

    #[tokio::test]
    async fn handle_morph_stats_returns_morph_stats_shape() {
        // Precondition: pristine stats. reset_for_test() is exposed via the
        // `testing` feature on reify-mesh-morph (activated by [dev-dependencies]
        // features = ["testing"] in Cargo.toml). This keeps the test correct even
        // after engine wiring (PRD #2947-#2949) lands and production code paths
        // start recording morph activity — without this reset, parallel tests or
        // leaked state from other test runs could produce non-zero counts.
        reify_mesh_morph::stats::reset_for_test();

        // State-free handler — call directly (not through dispatch). Zero snapshot expected after reset.
        let result = super::handle_morph_stats(serde_json::json!({}))
            .await
            .expect("morph_stats handler must succeed");

        assert!(result.is_object(), "response must be a JSON object");
        assert_eq!(result["morph_count"].as_u64(), Some(0), "morph_count key present, default 0");
        assert_eq!(result["remesh_count"].as_u64(), Some(0), "remesh_count key present, default 0");
        // last_rejection_reason: skip_serializing_if Option::is_none on Rust ⇒
        // key absent (Value::Null on index) when no rejection recorded.
        assert!(
            result.get("last_rejection_reason").is_none()
                || result["last_rejection_reason"].is_null(),
            "last_rejection_reason absent/null by default; got: {:?}",
            result.get("last_rejection_reason")
        );

        // body_id is accepted but ignored — `{body_id}` form returns the
        // identical response as the `()` form (forward-compat, per design).
        let with_body = super::handle_morph_stats(serde_json::json!({"body_id": "Bracket.body"}))
            .await
            .expect("morph_stats with body_id must succeed");
        assert_eq!(with_body, result, "body_id must be ignored — identical response");
    }

    #[tokio::test]
    async fn dispatch_stateless_tool_handles_morph_stats_arm() {
        // Unique coverage: the exact "morph_stats" match-arm string in
        // dispatch_stateless_tool. A typo or deletion returns None, caught
        // by the unwrap. Shape assertions live in
        // handle_morph_stats_returns_morph_stats_shape; here we only verify
        // delegation fidelity.
        reify_mesh_morph::stats::reset_for_test();

        let direct = super::handle_morph_stats(serde_json::json!({}))
            .await
            .expect("handle_morph_stats must succeed");

        let via_dispatch = super::dispatch_stateless_tool("morph_stats", &serde_json::json!({}))
            .await
            .expect("dispatch_stateless_tool must return Some for 'morph_stats'")
            .expect("morph_stats handler must succeed");

        assert_eq!(via_dispatch, direct, "dispatch_stateless_tool must delegate to handle_morph_stats");
    }

    #[tokio::test]
    async fn dispatch_stateless_tool_handles_mesh_morph_stats_arm() {
        // Unique coverage: the exact "mesh_morph_stats" match-arm string in
        // dispatch_stateless_tool. A typo or deletion returns None, caught
        // by the unwrap. Shape assertions live in
        // handle_mesh_morph_stats_returns_counters_and_session_start.
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reify_mesh_morph::diagnostics::reset_for_test();

        let direct = super::handle_mesh_morph_stats(serde_json::json!({}))
            .await
            .expect("handle_mesh_morph_stats must succeed");

        let via_dispatch =
            super::dispatch_stateless_tool("mesh_morph_stats", &serde_json::json!({}))
                .await
                .expect("dispatch_stateless_tool must return Some for 'mesh_morph_stats'")
                .expect("mesh_morph_stats handler must succeed");

        assert_eq!(
            via_dispatch, direct,
            "dispatch_stateless_tool must delegate to handle_mesh_morph_stats"
        );
    }

    #[tokio::test]
    async fn handle_mesh_morph_stats_returns_counters_and_session_start() {
        // Acquire process-global lock so no concurrent test races on counters.
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset all 7 diagnostic counters to zero for a clean baseline.
        reify_mesh_morph::diagnostics::reset_for_test();

        let result = super::handle_mesh_morph_stats(serde_json::json!({}))
            .await
            .expect("handle_mesh_morph_stats must succeed");

        assert!(result.is_object(), "response must be a JSON object");

        // All seven named buckets must be present and zero after reset.
        let buckets = [
            "morphed",
            "remeshed_quality_hard_fail",
            "remeshed_quality_soft_fail",
            "ineligible_structural_change",
            "ineligible_bijection_failure",
            "ineligible_naming_error",
            "panicked",
        ];
        for bucket in buckets {
            assert_eq!(
                result[bucket].as_u64(),
                Some(0),
                "bucket '{bucket}' must be present and 0 after reset"
            );
        }

        // session_start_unix_ms must be present and non-zero
        // (epoch-millis in 2026 is ~1.7e12, so > 0 is a safe non-flaky assertion).
        let session_start = result["session_start_unix_ms"]
            .as_u64()
            .expect("session_start_unix_ms must be a non-null u64");
        assert!(
            session_start > 0,
            "session_start_unix_ms must be > 0; got {session_start}"
        );
    }

    // step-5 RED → GREEN: all five viewport-aware tools must expose an optional
    // viewportId property in their schemas. Consolidated into one table-driven test
    // so adding a sixth tool is a one-line change (amend: suggestion-4).
    #[test]
    fn viewport_aware_tools_expose_optional_viewport_id() {
        let defs = tool_defs();
        let tools = [
            "viewport_state",
            "screenshot",
            "screenshot_window",
            "fit_to_view",
            "set_camera",
        ];
        for tool_name in tools {
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["properties"]["viewportId"]["type"].as_str(),
                Some("string"),
                "{tool_name}: properties.viewportId.type must be 'string'"
            );
            if let Some(required) = schema["required"].as_array() {
                assert!(
                    !required.iter().any(|v| v.as_str() == Some("viewportId")),
                    "{tool_name}: 'viewportId' must NOT be listed in required"
                );
            }
        }
    }
}
