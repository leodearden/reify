// Debug server — MCP Streamable HTTP + REST fallback on localhost:3939.
//
// The MCP endpoint (`POST /mcp`) implements a stateless Streamable HTTP transport:
// no session tracking, any `Mcp-Session-Id` header is accepted. This follows the
// fused-memory pattern so Claude Code sessions survive GUI restarts without
// requiring `/mcp` reconnection.
//
// The REST endpoint (`POST /debug/{command}`) provides the same tools via plain JSON
// for manual `curl` testing.

use std::sync::{Arc, Mutex, RwLock};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
            input_schema: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "screenshot",
            description: "Take a screenshot of the 3D viewport. Returns a PNG image.",
            input_schema: json!({"type": "object", "properties": {}}),
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
            input_schema: json!({"type": "object", "properties": {}}),
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
            description: "Freeze time-driven UI (CSS animations, transitions, pulses) so visual-regression screenshots are pixel-stable. Frontend-mediated. Returns { ok: true, test_mode: bool }.",
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

// --- Tool dispatch ---

async fn dispatch_tool(
    state: &DebugServerState,
    name: &str,
    params: Value,
) -> Result<Value, String> {
    match name {
        "health" => Ok(json!({"ok": true})),
        "engine_state" => handle_engine_state(state).await,
        "mesh_stats" => handle_mesh_stats(state).await,
        "open_file" => handle_open_file(state, params).await,
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
        let result = engine
            .lock()
            .map_err(|e| format!("engine lock poisoned: {e}"))
            .and_then(|mut session| f(&mut session));
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

async fn handle_open_file(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let path = params["path"]
        .as_str()
        .ok_or_else(|| "path is required".to_string())?
        .to_owned();

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
    let gui_state_json = serde_json::to_value(&gui_state)
        .map_err(|e| format!("serialize gui_state failed: {e}"))?;

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
                    // Check if this is a screenshot (contains base64 image data)
                    if tool_name == "screenshot"
                        && let Some(data) = result.get("data").and_then(|d| d.as_str())
                    {
                        // Strip data URL prefix if present
                        let base64 = data
                            .strip_prefix("data:image/png;base64,")
                            .unwrap_or(data);
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        )
            .into_response(),
    }
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

    #[test]
    fn tool_defs_contains_set_test_mode() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "set_test_mode")
            .expect("set_test_mode must be present in tool_defs()");

        // Description must be non-empty
        assert!(
            !entry.description.is_empty(),
            "set_test_mode description must not be empty"
        );

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
}
