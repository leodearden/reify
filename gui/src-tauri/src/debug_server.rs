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
            description: "Full engine state: meshes (entity paths + vertex/face counts), values, constraints, files, compile_diagnostics, tessellation_diagnostics, stale (bool), reload_error (string or null). stale=true means the last hot-reload failed; reload_error contains the failure message.",
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
            name: "load_fixture",
            description: "Load a named test fixture .ri file into the editor and engine. The name must be one of the catalogue keys: all_severities, small_cube, empty, broken_syntax, large_assembly, overflow. Resolves to gui/test/fixtures/{name}.ri relative to the repository root (cwd when the debug server launches).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Fixture name (catalogue key). One of: all_severities, small_cube, empty, broken_syntax, large_assembly, overflow."
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "element_screenshot",
            description: "Crop a screenshot to the bounds of a DOM element identified by data-testid. Captures the full window via html-to-image, then extracts the element's bounding rect (CSS-logical px from the window origin) scaled by devicePixelRatio (τ0 DPR contract). Returns { data: \"data:image/png;base64,...\" }. Frontend-mediated (no Rust dispatch arm).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "testId": {
                        "type": "string",
                        "description": "Value of the data-testid attribute on the target element (e.g. \"diagnostics-dialog\")."
                    }
                },
                "required": ["testId"]
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
                          ineligible_naming_error, panicked) plus session_start_unix_ms \
                          (debug-server spawn time, or the last reset time). Pass reset:true \
                          to zero all counters and restart the measurement clock before \
                          returning the post-reset snapshot — useful for benchmark sequences. \
                          Concurrent recorders active during the reset window may produce \
                          non-zero post-reset counters. Per mesh-morphing PRD #12 / \
                          docs/prds/v0_3/mesh-morphing.md.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "reset": {
                        "type": "boolean",
                        "description": "When true, zero all diagnostic counters and restart \
                                        the measurement clock before returning the post-reset \
                                        snapshot. Defaults to false."
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
        // --- DOM/style/layout/window inspection tools (R1) ---
        ToolDef {
            name: "query_selector",
            description: "Query a single DOM element by raw CSS selector. Returns { exists, tagName, testId, text, bounds, visible } on match, { exists: false } when no element matches, { error } on invalid selector or missing param.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector string (e.g. '.cm-scroller', '[data-testid=\"editor\"]')"
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "query_selector_all",
            description: "Query all DOM elements matching a raw CSS selector. Returns { count, elements: [...], truncated } (capped at 200 results), { count: 0, elements: [], truncated: false } when none match, { error } on invalid selector or missing param.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector string"
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "get_layout_metrics",
            description: "Read scroll/client/bounds metrics for a DOM element. Returns { exists, bounds, scroll: { top, left, width, height }, client: { width, height }, overflow: { horizontal, vertical } } where overflow.horizontal is true when scrollWidth > clientWidth (clipped/overflowing text).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector string identifying the element to measure"
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "get_computed_style",
            description: "Read computed CSS style for a DOM element. Returns { exists, style: { display, visibility, opacity, color, backgroundColor, fontSize, fontFamily, fontWeight, overflow, position, width, height } } by default; pass properties:[...] to request a custom subset.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "CSS selector string"
                    },
                    "properties": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of CSS property names to return (camelCase). When omitted, returns the default curated subset."
                    }
                },
                "required": ["selector"]
            }),
        },
        ToolDef {
            name: "active_element",
            description: "Return the currently focused DOM element: { testId, role, tagName }. testId and role are null when the element has no data-testid/role attribute. Returns { tagName: 'body', testId: null, role: null } when nothing specific is focused.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDef {
            name: "get_window_state",
            description: "Return window geometry and focus state: { innerWidth, innerHeight, screenX, screenY, devicePixelRatio, focused }. All sizes are CSS/logical pixels. focused reflects document.hasFocus().",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // C1 app-chrome tools (frontend-mediated via debug_bridge.query_frontend catch-all)
        ToolDef {
            name: "open_menu",
            description: "Open a top-level app menu by name (file|edit|view|help) by driving the MenuBar trigger; returns {ok, open}.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Menu name to open: file, edit, view, or help."
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "menu_state",
            description: "Report the currently-open menu and the open menu's per-item enabled-state, read from the rendered DOM.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDef {
            name: "press_tab",
            description: "Advance keyboard focus to the next focusable element in document order and report where document.activeElement lands.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDef {
            name: "tab_order",
            description: "Report the forward-Tab focus traversal order (document-order focusable elements).",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // --- R2 inspection tools (task-4297) — frontend-mediated, dispatched via query_frontend ---
        ToolDef {
            name: "get_diagnostics",
            description: "Structured dump of the engine's compile and tessellation diagnostics (severity, message, code, source range) read from the real engineStore.compileDiagnostics + tessellationDiagnostics. Returns { compile: Diag[], tessellation: Diag[], compileCount, tessellationCount } where Diag = { severity, message, code, file_path, range: { line, column, end_line, end_column } }.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDef {
            name: "ui_outline",
            description: "DOM-derived semantic text snapshot of visible UI elements in document order. Returns { outline: Node[], count, truncated } where Node = { tagName, role, testId, text, enabled }. Captures buttons, inputs, [role], [data-testid], and other interactive elements visible in the render tree (computed display != none && visibility != hidden). This is a pragmatic DOM APPROXIMATION — NOT a true accessibility tree (true AX tree support deferred to tracker AX-1). Capped at 500 elements.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // --- R3 tools (task-4298) — console-error capture + wait_for/wait_for_selector ---
        ToolDef {
            name: "list_console_errors",
            description: "Return captured console.error/console.warn/window.onerror/unhandledrejection entries from the always-on ring buffer installed at app startup. Returns { errors: ConsoleErrorEntry[], count: number }. Optional { clear: true } drains the buffer after returning. Useful for detecting JS errors that occurred before or after the debug bridge initialized.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clear": { "type": "boolean" }
                }
            }),
        },
        ToolDef {
            name: "wait_for",
            description: "Poll until a predicate is satisfied or a timeout elapses. Returns { ok: true, waited_ms: number } on success or { error: 'timeout' } when the deadline expires. Predicate is a tagged union: { kind: 'selector', testId, state: 'visible'|'gone', text? } for DOM presence checks, or { kind: 'store', path, equals } for store dotted-path equality checks. Optional timeout_ms (default 5000, must be positive).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "predicate": {
                        "type": "object",
                        "description": "Tagged predicate: { kind: 'selector', testId, state?, text? } or { kind: 'store', path, equals }"
                    },
                    "timeout_ms": { "type": "integer" }
                }
            }),
        },
        ToolDef {
            name: "wait_for_selector",
            description: "Poll until a [data-testid] element reaches the requested state or a timeout elapses. Returns { ok: true, waited_ms: number } or { error: 'timeout' }. state: 'visible' (default) or 'gone'. Optional text asserts el.textContent.trim() matches when state='visible'. Optional timeout_ms (default 5000, must be positive).",
            input_schema: json!({
                "type": "object",
                "required": ["testId"],
                "properties": {
                    "testId": { "type": "string" },
                    "state": { "type": "string", "enum": ["visible", "gone"] },
                    "text": { "type": "string" },
                    "timeout_ms": { "type": "integer" }
                }
            }),
        },
        // --- C2 layout-control tools (task-4302) — frontend-mediated ---
        ToolDef {
            name: "resize_panes",
            description: "Resize one or more layout panes by setting their pixel dimensions. \
                          All five dimensions are optional; omit any to leave them unchanged. \
                          Accepts non-negative finite numbers. Returns { ok, layout } on success.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "editorWidth":       { "type": "number", "description": "Width of the editor pane in pixels." },
                    "sideWidth":         { "type": "number", "description": "Width of the side panel in pixels." },
                    "designTreeHeight":  { "type": "number", "description": "Height of the design tree panel in pixels." },
                    "propertyHeight":    { "type": "number", "description": "Height of the property panel in pixels." },
                    "constraintHeight":  { "type": "number", "description": "Height of the constraint panel in pixels." }
                }
            }),
        },
        ToolDef {
            name: "set_window_size",
            description: "Resize the application window to the specified logical-pixel dimensions. \
                          Both width and height must be positive finite numbers. \
                          Returns { ok, width, height } on success.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "width":  { "type": "number", "description": "Target window width in logical pixels (must be > 0)." },
                    "height": { "type": "number", "description": "Target window height in logical pixels (must be > 0)." }
                },
                "required": ["width", "height"]
            }),
        },
        ToolDef {
            name: "expand_tree_node",
            description: "Expand a node in the design tree or constraint panel by clicking its toggle control. \
                          Idempotent: if the node is already expanded, no click is dispatched. \
                          panel defaults to 'design'; pass panel:'constraint' for the constraint panel. \
                          Returns { ok, path, expanded } where expanded reflects the post-operation state.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path":  { "type": "string", "description": "Entity path or node id to expand." },
                    "panel": {
                        "type": "string",
                        "enum": ["design", "constraint"],
                        "description": "Which panel's tree to operate on (default: 'design')."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "collapse_tree_node",
            description: "Collapse a node in the design tree or constraint panel by clicking its toggle control. \
                          Idempotent: if the node is already collapsed, no click is dispatched. \
                          panel defaults to 'design'; pass panel:'constraint' for the constraint panel. \
                          Returns { ok, path, expanded } where expanded reflects the post-operation state.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path":  { "type": "string", "description": "Entity path or node id to collapse." },
                    "panel": {
                        "type": "string",
                        "enum": ["design", "constraint"],
                        "description": "Which panel's tree to operate on (default: 'design')."
                    }
                },
                "required": ["path"]
            }),
        },
        // --- F2 LSP probe tools (task-4304) — frontend-mediated ---
        ToolDef {
            name: "hover_at",
            description: "Request LSP hover information at a given 0-based (line, col) position in the active editor file. \
                          Drives the in-process LSP via the editor's lspClient and returns the structured result: \
                          { markdown, markdownLength, contents, range }. \
                          Returns { error } when no file is active or line/col are invalid.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "line": { "type": "integer", "description": "0-based line number." },
                    "col":  { "type": "integer", "description": "0-based column (character offset)." }
                },
                "required": ["line", "col"]
            }),
        },
        ToolDef {
            name: "completion_at",
            description: "Request LSP completion items at a given 0-based (line, col) position in the active editor file. \
                          Drives the in-process LSP via the editor's lspClient and returns the structured result: \
                          { items, itemCount }. \
                          Returns { error } when no file is active or line/col are invalid.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "line": { "type": "integer", "description": "0-based line number." },
                    "col":  { "type": "integer", "description": "0-based column (character offset)." }
                },
                "required": ["line", "col"]
            }),
        },
        ToolDef {
            name: "definition_at",
            description: "Request LSP go-to-definition at a given 0-based (line, col) position in the active editor file. \
                          Drives the in-process LSP via the editor's lspClient and returns the structured result: \
                          { uri, range, found }. found=false when the LSP returns no location. \
                          Returns { error } when no file is active or line/col are invalid.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "line": { "type": "integer", "description": "0-based line number." },
                    "col":  { "type": "integer", "description": "0-based column (character offset)." }
                },
                "required": ["line", "col"]
            }),
        },
        // --- I1: Synthetic pointer/scroll/focus tools (task-4299) ---
        // These are FRONTEND-MEDIATED: no Rust handler arm is added; dispatch_tool's
        // default arm routes unknown names to debug_bridge.query_frontend (:693-697).
        ToolDef {
            name: "click_at",
            description: "Simulate a pointer click at CSS-logical-pixel coordinates (x, y) measured from the window origin \
                          (same frame as getBoundingClientRect / clientX/clientY; see contract §3). \
                          Resolves the target element via document.elementFromPoint(x, y), then dispatches \
                          pointerdown → pointerup → click events with clientX=x, clientY=y. \
                          Fires JS click handlers (React onClick etc.); CSS :hover/:active is NOT applied \
                          (synthetic-event fidelity gap — contract §4).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "number",
                        "description": "CSS-logical-px from the window origin (same frame as getBoundingClientRect / clientX/clientY; see contract §3)"
                    },
                    "y": {
                        "type": "number",
                        "description": "CSS-logical-px from the window origin (same frame as getBoundingClientRect / clientX/clientY; see contract §3)"
                    }
                },
                "required": ["x", "y"]
            }),
        },
        ToolDef {
            name: "hover",
            description: "Simulate a pointer move (hover) at CSS-logical-pixel coordinates (x, y). \
                          Resolves the target element via document.elementFromPoint(x, y), then dispatches \
                          pointermove + mousemove events with clientX=x, clientY=y. \
                          Fires JS move handlers; CSS :hover pseudo-class is NOT applied \
                          (synthetic-event fidelity gap — contract §4).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "number",
                        "description": "CSS-logical-px from the window origin (same frame as getBoundingClientRect / clientX/clientY; see contract §3)"
                    },
                    "y": {
                        "type": "number",
                        "description": "CSS-logical-px from the window origin (same frame as getBoundingClientRect / clientX/clientY; see contract §3)"
                    }
                },
                "required": ["x", "y"]
            }),
        },
        ToolDef {
            name: "drag",
            description: "Simulate a synthetic pointer drag from one coordinate to another. \
                          Dispatches pointerdown+mousedown at 'from', pointermove at 'to', then \
                          pointerup+mouseup at 'to'. Fires JS pointer/mouse handlers; \
                          there is NO native HTML5 drag-and-drop (dragstart/drop are NOT fired) — \
                          this is a synthetic pointer-move drag (contract §4).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": {
                        "type": "object",
                        "description": "Start coordinate {x, y} in CSS-logical-px from the window origin",
                        "properties": {
                            "x": { "type": "number" },
                            "y": { "type": "number" }
                        }
                    },
                    "to": {
                        "type": "object",
                        "description": "End coordinate {x, y} in CSS-logical-px from the window origin",
                        "properties": {
                            "x": { "type": "number" },
                            "y": { "type": "number" }
                        }
                    }
                },
                "required": ["from", "to"]
            }),
        },
        ToolDef {
            name: "focus_element",
            description: "Focus a DOM element by its data-testid attribute. \
                          Calls el.focus() on the resolved element. \
                          Returns {ok: true} on success or {error} if not found or testId is missing.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "testId": {
                        "type": "string",
                        "description": "The data-testid attribute value of the element to focus"
                    }
                },
                "required": ["testId"]
            }),
        },
        ToolDef {
            name: "scroll",
            description: "Scroll a DOM element or the CodeMirror editor. \
                          DOM mode: pass testId to set scrollTop/scrollLeft on the resolved element. \
                          Editor mode: pass target:'editor' to scroll the CodeMirror scrollDOM. \
                          Returns {ok: true, scrollTop, scrollLeft} with the resulting scroll offsets \
                          (enabling a get_layout_metrics round-trip to confirm the applied offset).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "testId": {
                        "type": "string",
                        "description": "data-testid of the DOM element to scroll (DOM mode)"
                    },
                    "target": {
                        "type": "string",
                        "enum": ["editor"],
                        "description": "Pass 'editor' to scroll the CodeMirror scrollDOM (editor mode)"
                    },
                    "top": {
                        "type": "number",
                        "description": "scrollTop value to set (pixels)"
                    },
                    "left": {
                        "type": "number",
                        "description": "scrollLeft value to set (pixels)"
                    }
                }
            }),
        },
        // task-4300 I2: canvas interaction tools — frontend-mediated (no dispatch arm needed;
        // the default `_ =>` arm at :817-821 routes unknown names to DebugBridge::query_frontend).
        ToolDef {
            name: "pick_entity_at",
            description: "Raycast at canvas CSS-px coords and return the entity hit (if any). \
                          PURE QUERY — does NOT mutate selection (select_entity covers the mutate case). \
                          Coords are CSS-logical-px from window origin (clientX/clientY). \
                          Omitted x/y default to canvas center (NDC origin, ray through look-at target). \
                          Returns {hit:true, entityPath, point:{x,y,z}, distance} on hit; \
                          {hit:false} on miss; {error} for unknown viewport or non-finite coords.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "number",
                        "description": "CSS-px x coordinate from window origin (clientX). Omit for canvas center."
                    },
                    "y": {
                        "type": "number",
                        "description": "CSS-px y coordinate from window origin (clientY). Omit for canvas center."
                    },
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "orbit_camera",
            description: "Drive the viewport camera via OrbitControls' public rotateLeft/rotateUp API. \
                          Units are RADIANS of azimuth/elevation delta (reproducible, resolution-independent). \
                          Omitted deltas default to 0. \
                          Returns {ok, azimuth, polar, azimuthDelta, polarDelta, camera:{position}}.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dazimuth": {
                        "type": "number",
                        "description": "Azimuth (yaw) delta in radians. Positive = rotate left. Defaults to 0."
                    },
                    "delevation": {
                        "type": "number",
                        "description": "Elevation (pitch) delta in radians. Positive = rotate up. Defaults to 0."
                    },
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "pan_camera",
            description: "Pan the viewport camera via OrbitControls' public pan API. \
                          Units are pixels. Omitted deltas default to 0. \
                          Returns {ok, target:{x,y,z}, camera:{position}}.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dx": {
                        "type": "number",
                        "description": "Horizontal pan in pixels. Positive = pan right. Defaults to 0."
                    },
                    "dy": {
                        "type": "number",
                        "description": "Vertical pan in pixels. Positive = pan down. Defaults to 0."
                    },
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
                    }
                }
            }),
        },
        ToolDef {
            name: "zoom_camera",
            description: "Zoom the viewport camera via OrbitControls' public dollyIn API. \
                          scale is a multiplicative distance factor: scale>1 moves farther, scale<1 closer. \
                          (dollyIn(scale) multiplies the orbit radius by scale per OrbitControls internals.) \
                          Returns {ok, distance, distanceDelta, camera:{position}}.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "scale": {
                        "type": "number",
                        "description": "Multiplicative distance scale factor. Must be finite and >0. scale>1 = farther, scale<1 = closer."
                    },
                    "viewportId": {
                        "type": "string",
                        "description": "Optional viewport id (e.g. 'design-main', 'def-preview'). When omitted, the first populated viewport is targeted."
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
    matches!(name, "screenshot" | "screenshot_window" | "element_screenshot")
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
        "load_fixture" => handle_load_fixture(state, params).await,
        "wait_for_idle" => handle_wait_for_idle(state, params).await,
        "wait_for" => handle_wait_for(state, params).await,
        "wait_for_selector" => handle_wait_for_selector(state, params).await,
        _ => {
            // Frontend-mediated: delegate to DebugBridge.
            // list_console_errors falls through here — it returns instantly so
            // the default 5s query_frontend timeout is more than sufficient.
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
        crate::commands::engine_state_json(session)
    })
    .await
}

/// Histogram of a mesh's per-face `element_kind` bytes.
///
/// Returns an empty map when `element_kind` is `None` (tet-only / non-shell
/// meshes carry no per-face classification). `BTreeMap` keeps the byte keys in
/// deterministic ascending order so the serialized JSON object
/// (`{"1": <n>}`) is stable across runs — the PRD §9 θ observable signal.
pub(crate) fn element_kind_count(
    mesh: &crate::types::MeshData,
) -> std::collections::BTreeMap<u8, usize> {
    let mut counts = std::collections::BTreeMap::new();
    if let Some(element_kind) = &mesh.element_kind {
        for &kind in element_kind {
            *counts.entry(kind).or_insert(0) += 1;
        }
    }
    counts
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

                // Per-face element-kind histogram, byte→count as a JSON object
                // with string keys (e.g. {"1": <n>}) — the PRD §9 θ signal.
                let element_kind_hist: serde_json::Map<String, Value> =
                    element_kind_count(m)
                        .into_iter()
                        .map(|(kind, count)| (kind.to_string(), json!(count)))
                        .collect();

                json!({
                    "entity_path": m.entity_path,
                    "vertex_count": vertex_count,
                    "face_count": face_count,
                    "element_kind_count": element_kind_hist,
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

// --- Session-start / measurement-window timestamp (process-global) ---

/// Unix-epoch-milliseconds captured at debug-server spawn (see [`spawn_debug_server`]).
/// Zero before server spawn; falls back to lazy first-call init for direct handler
/// calls in tests. Rolls forward when [`reset_session_start`] is called (i.e. when a
/// caller passes `reset:true` to restart the measurement window).
static SESSION_START_UNIX_MS: AtomicU64 = AtomicU64::new(0);

/// Returns the measurement-window start timestamp.
///
/// Normally initialized at server spawn via [`spawn_debug_server`] so the value
/// reflects the true debug-server start time. Falls back to lazy compare-exchange
/// init (0 → now) so direct handler calls in tests work correctly without a running
/// server. Subsequent calls return the same value until [`reset_session_start`] is
/// called.
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
/// ineligible_naming_error, panicked) plus a `session_start_unix_ms` field
/// (debug-server spawn time, or the last reset time).
///
/// When `params["reset"]` is `true`, zeros all counters via
/// `diagnostics::reset()` and refreshes the measurement clock before taking the
/// snapshot — the response reflects the post-reset state. Note: the counter
/// stores are independent (not one atomic operation), so a concurrent recorder
/// active during the reset window may produce non-zero post-reset counters.
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

/// Catalogue of allowed fixture names → repo-relative paths.
/// Mirrors the TS FIXTURES keys in gui/test/visual/assertions.ts.
/// The e2e harness launches reify-gui with cwd=REPO_ROOT, so these
/// relative paths resolve correctly via canonicalize_debug_open_path.
fn fixture_relpath(name: &str) -> Option<String> {
    match name {
        "all_severities" => Some("gui/test/fixtures/all_severities.ri".to_string()),
        "small_cube"     => Some("gui/test/fixtures/small_cube.ri".to_string()),
        "empty"          => Some("gui/test/fixtures/empty.ri".to_string()),
        "broken_syntax"  => Some("gui/test/fixtures/broken_syntax.ri".to_string()),
        "large_assembly" => Some("gui/test/fixtures/large_assembly.ri".to_string()),
        "overflow"       => Some("gui/test/fixtures/overflow.ri".to_string()),
        _                => None,
    }
}

/// Shared file-open helper: canonicalise raw_path, read from disk, load into
/// the engine on an OS thread (OCCT panics inside tokio), build GUI state, and
/// tell the frontend to open the file.
async fn open_path_into_engine(state: &DebugServerState, raw_path: &str) -> Result<Value, String> {
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

async fn handle_open_file(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let raw_path = params["path"]
        .as_str()
        .ok_or_else(|| "path is required".to_string())?;
    open_path_into_engine(state, raw_path).await
}

async fn handle_load_fixture(state: &DebugServerState, params: Value) -> Result<Value, String> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| "name is required".to_string())?;
    let relpath = fixture_relpath(name)
        .ok_or_else(|| format!("unknown fixture: {name}"))?;
    open_path_into_engine(state, &relpath).await
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

async fn handle_wait_for(state: &DebugServerState, params: Value) -> Result<Value, String> {
    // Validate and canonicalize timeout_ms. Default 5000ms (matching the frontend default).
    let timeout_ms: u64 = match params.get("timeout_ms") {
        None => 5_000,
        Some(v) => match v.as_u64().filter(|&n| n > 0) {
            Some(n) => n,
            None => return Ok(json!({"error": "timeout_ms must be a positive integer"})),
        },
    };

    // Build canonical params: pass predicate through as-is so the frontend parses it.
    let canonical_params = json!({
        "predicate": params.get("predicate").cloned().unwrap_or(Value::Null),
        "timeout_ms": timeout_ms
    });
    // Add a 5-second buffer so the Rust oneshot fires *after* the frontend
    // has had a chance to return its own {error: "timeout"} response.
    let rust_timeout = Duration::from_millis(timeout_ms.saturating_add(5_000));
    state
        .debug_bridge
        .query_frontend_with_timeout("wait_for", canonical_params, rust_timeout)
        .await
}

async fn handle_wait_for_selector(
    state: &DebugServerState,
    params: Value,
) -> Result<Value, String> {
    // Validate and canonicalize timeout_ms. Default 5000ms.
    let timeout_ms: u64 = match params.get("timeout_ms") {
        None => 5_000,
        Some(v) => match v.as_u64().filter(|&n| n > 0) {
            Some(n) => n,
            None => return Ok(json!({"error": "timeout_ms must be a positive integer"})),
        },
    };

    // Build canonical params and pass through to the frontend.
    let mut canonical_params = serde_json::Map::new();
    canonical_params.insert("timeout_ms".to_string(), json!(timeout_ms));
    if let Some(v) = params.get("testId") {
        canonical_params.insert("testId".to_string(), v.clone());
    }
    if let Some(v) = params.get("state") {
        canonical_params.insert("state".to_string(), v.clone());
    }
    if let Some(v) = params.get("text") {
        canonical_params.insert("text".to_string(), v.clone());
    }
    let canonical_params = Value::Object(canonical_params);

    let rust_timeout = Duration::from_millis(timeout_ms.saturating_add(5_000));
    state
        .debug_bridge
        .query_frontend_with_timeout("wait_for_selector", canonical_params, rust_timeout)
        .await
}

// --- Port resolution ---

pub const DEFAULT_DEBUG_PORT: u16 = 3939;

/// Parse a raw env-var value into a valid port (1..=65535), falling back to
/// DEFAULT_DEBUG_PORT for unset, empty, non-numeric, zero, or out-of-range input.
/// Whitespace is NOT stripped; " 4500 " falls back to 3939.
pub fn parse_debug_port(raw: Option<&str>) -> u16 {
    raw.and_then(|s| s.parse::<u32>().ok())
        .and_then(|n| u16::try_from(n).ok())
        .filter(|&p| p >= 1)
        .unwrap_or(DEFAULT_DEBUG_PORT)
}

pub fn resolve_debug_port() -> u16 {
    parse_debug_port(std::env::var("REIFY_DEBUG_PORT").ok().as_deref())
}

pub fn debug_endpoint_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

// --- Server spawn ---

pub async fn spawn_debug_server(
    engine: Arc<Mutex<EngineSession>>,
    selection: Arc<RwLock<SelectionInfo>>,
    debug_bridge: Arc<DebugBridge>,
) -> Result<(), String> {
    // Initialize the measurement-window clock at server spawn so
    // session_start_unix_ms reports the true debug-server start time
    // rather than the first-query time.
    session_start_unix_ms();

    let state = DebugServerState {
        engine,
        selection,
        debug_bridge,
    };

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/debug/{command}", post(handle_rest))
        .with_state(state);

    let port = resolve_debug_port();
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|e| format!("failed to bind debug server on :{port}: {e}"))?;

    tracing::info!("Debug server listening on {}", debug_endpoint_url(port));

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
    async fn handle_mesh_morph_stats_reset_true_zeros_counters() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // --- (a) reset:true path: counter recorded, then handler zeros it ---
        reify_mesh_morph::diagnostics::reset_for_test();
        reify_mesh_morph::diagnostics::record_morphed();
        // Pre-condition: morphed == 1 before the call.
        assert_eq!(reify_mesh_morph::diagnostics::snapshot().morphed, 1);

        // Capture the session clock before reset to verify reset:true restarts it.
        // A plain (no-reset) call initializes the clock if not yet set.
        let ts_before = super::handle_mesh_morph_stats(serde_json::json!({}))
            .await
            .expect("baseline read before reset must succeed")["session_start_unix_ms"]
            .as_u64()
            .expect("session_start_unix_ms must be present in baseline read");

        let result = super::handle_mesh_morph_stats(serde_json::json!({"reset": true}))
            .await
            .expect("handle_mesh_morph_stats({reset:true}) must succeed");

        // Response must show post-reset zeros.
        assert_eq!(
            result["morphed"].as_u64(),
            Some(0),
            "reset:true — response.morphed must be 0 after reset"
        );
        // Process-global counter must actually be zeroed.
        assert_eq!(
            reify_mesh_morph::diagnostics::snapshot().morphed,
            0,
            "reset:true — process-global morphed counter must be 0 after reset"
        );
        // Session clock must have been restarted (>= allows same-millisecond granularity).
        let ts_after = result["session_start_unix_ms"]
            .as_u64()
            .expect("session_start_unix_ms must be present in reset:true response");
        assert!(
            ts_after >= ts_before,
            "reset:true must restart the measurement clock: \
             ts_after ({ts_after}) must be >= ts_before ({ts_before})"
        );

        // --- (b) control: no reset flag — counter must survive ---
        reify_mesh_morph::diagnostics::reset_for_test();
        reify_mesh_morph::diagnostics::record_morphed();

        let result_no_reset = super::handle_mesh_morph_stats(serde_json::json!({}))
            .await
            .expect("handle_mesh_morph_stats({}) must succeed");

        // Response must preserve the non-zero value.
        assert_eq!(
            result_no_reset["morphed"].as_u64(),
            Some(1),
            "omitted reset — response.morphed must be 1 (unchanged)"
        );
        // Process-global counter must not be zeroed.
        assert_eq!(
            reify_mesh_morph::diagnostics::snapshot().morphed,
            1,
            "omitted reset — process-global morphed counter must remain 1"
        );
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

    // step-1 RED → GREEN: all six new inspection tools must be registered in tool_defs()
    // with the correct schema shape (object type, required arrays, non-empty description).
    #[test]
    fn tool_defs_registers_inspection_tools() {
        let defs = tool_defs();

        // Tools that require a "selector" param
        let selector_tools = [
            "query_selector",
            "query_selector_all",
            "get_layout_metrics",
            "get_computed_style",
        ];
        for tool_name in selector_tools {
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name}: required must be an array"));
            assert!(
                required.iter().any(|v| v.as_str() == Some("selector")),
                "{tool_name}: 'selector' must be listed in required"
            );
        }

        // Tools that require nothing (no required array or empty)
        let no_param_tools = ["active_element", "get_window_state"];
        for tool_name in no_param_tools {
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            if let Some(required) = schema["required"].as_array() {
                assert!(
                    required.is_empty(),
                    "{tool_name}: required array must be empty; got {required:?}"
                );
            }
        }
    }

    // task-4297 step-5 RED → step-6 GREEN: R2 tools get_diagnostics and ui_outline
    // must be registered in tool_defs() with correct schema shape.
    // Note: the ui_outline DOM-approximation / not-an-AX-tree label lives in the
    // ToolDef source description (see debug_server.rs:402); it is not substring-pinned
    // here to avoid brittle wording-pin failures on harmless rewording (step-9).
    #[test]
    fn tool_defs_registers_r2_inspection_tools() {
        let defs = tool_defs();

        // Both R2 tools take no required params.
        let r2_tools = ["get_diagnostics", "ui_outline"];
        for tool_name in r2_tools {
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            if let Some(required) = schema["required"].as_array() {
                assert!(
                    required.is_empty(),
                    "{tool_name}: required array must be empty; got {required:?}"
                );
            }
        }
    }

    // task-4298 step-9 RED → step-10 GREEN: R3 tools list_console_errors, wait_for,
    // wait_for_selector must be registered in tool_defs() with correct schema shapes.
    // Schema-shape-only assertions — NO prose/substring pinning of descriptions (step-9 convention).
    #[test]
    fn tool_defs_includes_list_console_errors() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "list_console_errors")
            .expect("list_console_errors must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(
            !entry.description.is_empty(),
            "list_console_errors must have a non-empty description"
        );
        // 'clear' is optional — if a required array exists it must NOT force 'clear'
        if let Some(required) = schema["required"].as_array() {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("clear")),
                "'clear' must NOT be listed in required (it is optional)"
            );
        }
    }

    #[test]
    fn tool_defs_includes_wait_for() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "wait_for")
            .expect("wait_for must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(
            !entry.description.is_empty(),
            "wait_for must have a non-empty description"
        );
        // Must have a 'predicate' property
        assert!(
            schema["properties"]["predicate"].is_object(),
            "properties.predicate must be present and an object"
        );
        // timeout_ms is optional — if a required array exists it must NOT force timeout_ms
        if let Some(required) = schema["required"].as_array() {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("timeout_ms")),
                "'timeout_ms' must NOT be listed in required (it is optional)"
            );
        }
    }

    #[test]
    fn tool_defs_includes_wait_for_selector() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|t| t.name == "wait_for_selector")
            .expect("wait_for_selector must be present in tool_defs()");

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "input_schema.type must be 'object'"
        );
        assert!(
            !entry.description.is_empty(),
            "wait_for_selector must have a non-empty description"
        );
        // testId must be a string property
        assert_eq!(
            schema["properties"]["testId"]["type"].as_str(),
            Some("string"),
            "properties.testId.type must be 'string'"
        );
        // testId IS required
        let required = schema["required"]
            .as_array()
            .expect("input_schema.required must be an array for wait_for_selector");
        assert!(
            required.iter().any(|v| v.as_str() == Some("testId")),
            "'testId' must be listed in required"
        );
        // timeout_ms is optional — must NOT be required
        assert!(
            !required.iter().any(|v| v.as_str() == Some("timeout_ms")),
            "'timeout_ms' must NOT be listed in required (it is optional)"
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
            // task-4300 I2: canvas interaction tools
            "pick_entity_at",
            "orbit_camera",
            "pan_camera",
            "zoom_camera",
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

    // step-9 RED → step-10 GREEN: four C1 app-chrome tools registered in tool_defs().
    #[test]
    fn tool_defs_registers_chrome_tools() {
        let defs = tool_defs();

        struct Expectation {
            name: &'static str,
            required_name: bool,
        }
        let tools = [
            Expectation { name: "open_menu",  required_name: true  },
            Expectation { name: "menu_state", required_name: false },
            Expectation { name: "press_tab",  required_name: false },
            Expectation { name: "tab_order",  required_name: false },
        ];

        for t in &tools {
            let entry = defs
                .iter()
                .find(|d| d.name == t.name)
                .unwrap_or_else(|| panic!("{} must be present in tool_defs()", t.name));
            let schema = &entry.input_schema;
            assert!(
                !entry.description.is_empty(),
                "{}: description must be non-empty", t.name
            );
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{}: input_schema.type must be 'object'", t.name
            );
            if t.required_name {
                assert_eq!(
                    schema["properties"]["name"]["type"].as_str(),
                    Some("string"),
                    "{}: properties.name.type must be 'string'", t.name
                );
                let required = schema["required"].as_array()
                    .unwrap_or_else(|| panic!("{}: required must be an array", t.name));
                assert!(
                    required.iter().any(|v| v.as_str() == Some("name")),
                    "{}: 'name' must be listed in required", t.name
                );
            } else if let Some(required) = schema["required"].as_array() {
                assert!(
                    required.is_empty(),
                    "{}: required must be absent or empty; got {:?}", t.name, required
                );
            }
        }
    }

    // ── parse_debug_port ────────────────────────────────────────────────────
    #[test]
    fn parse_debug_port_known_good_values() {
        assert_eq!(parse_debug_port(Some("3939")), 3939u16);
        assert_eq!(parse_debug_port(Some("4500")), 4500u16);
        assert_eq!(parse_debug_port(Some("1")), 1u16);
        assert_eq!(parse_debug_port(Some("65535")), 65535u16);
    }

    #[test]
    fn parse_debug_port_fallback_to_default() {
        assert_eq!(parse_debug_port(None), 3939u16);
        assert_eq!(parse_debug_port(Some("")), 3939u16);
        assert_eq!(parse_debug_port(Some("abc")), 3939u16);
        assert_eq!(parse_debug_port(Some("0")), 3939u16);
        // 70000 is out of u16 range (> 65535) — must fall back
        assert_eq!(parse_debug_port(Some("70000")), 3939u16);
        // leading/trailing whitespace is NOT stripped — falls back
        assert_eq!(parse_debug_port(Some(" 4500 ")), 3939u16);
    }

    // ── debug_endpoint_url ──────────────────────────────────────────────────
    #[test]
    fn debug_endpoint_url_formats_correctly() {
        assert_eq!(debug_endpoint_url(3939), "http://127.0.0.1:3939/mcp");
        assert_eq!(debug_endpoint_url(51000), "http://127.0.0.1:51000/mcp");
    }

    // task-4302 step-7 RED → step-8 GREEN: C2 layout-control tools must be
    // registered in tool_defs() with correct schema shapes.
    #[test]
    fn tool_defs_registers_layout_control_tools() {
        let defs = tool_defs();

        // --- resize_panes ---
        let rp = defs
            .iter()
            .find(|t| t.name == "resize_panes")
            .expect("resize_panes must be present in tool_defs()");
        assert_eq!(rp.input_schema["type"].as_str(), Some("object"),
            "resize_panes: input_schema.type must be 'object'");
        assert!(!rp.description.is_empty(), "resize_panes: description must be non-empty");
        // All 5 pane properties must be present and typed as number
        for dim in &["editorWidth", "sideWidth", "designTreeHeight", "propertyHeight", "constraintHeight"] {
            assert_eq!(
                rp.input_schema["properties"][dim]["type"].as_str(),
                Some("number"),
                "resize_panes: properties.{dim} must be 'number'",
            );
        }
        // None of the pane properties should be required (all optional)
        if let Some(required) = rp.input_schema["required"].as_array() {
            let pane_dims = ["editorWidth", "sideWidth", "designTreeHeight", "propertyHeight", "constraintHeight"];
            for dim in pane_dims {
                assert!(
                    !required.iter().any(|v| v.as_str() == Some(dim)),
                    "resize_panes: '{dim}' must NOT be listed in required (all pane dims are optional)"
                );
            }
        }

        // --- set_window_size ---
        let sws = defs
            .iter()
            .find(|t| t.name == "set_window_size")
            .expect("set_window_size must be present in tool_defs()");
        assert_eq!(sws.input_schema["type"].as_str(), Some("object"),
            "set_window_size: input_schema.type must be 'object'");
        assert!(!sws.description.is_empty(), "set_window_size: description must be non-empty");
        assert_eq!(sws.input_schema["properties"]["width"]["type"].as_str(), Some("number"),
            "set_window_size: properties.width.type must be 'number'");
        assert_eq!(sws.input_schema["properties"]["height"]["type"].as_str(), Some("number"),
            "set_window_size: properties.height.type must be 'number'");
        let sws_required = sws.input_schema["required"]
            .as_array()
            .expect("set_window_size: required must be an array");
        assert!(sws_required.iter().any(|v| v.as_str() == Some("width")),
            "set_window_size: 'width' must be in required");
        assert!(sws_required.iter().any(|v| v.as_str() == Some("height")),
            "set_window_size: 'height' must be in required");

        // --- expand_tree_node and collapse_tree_node ---
        for tool_name in &["expand_tree_node", "collapse_tree_node"] {
            let entry = defs
                .iter()
                .find(|t| t.name == *tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(schema["type"].as_str(), Some("object"),
                "{tool_name}: input_schema.type must be 'object'");
            assert!(!entry.description.is_empty(),
                "{tool_name}: description must be non-empty");
            // path is required
            assert_eq!(schema["properties"]["path"]["type"].as_str(), Some("string"),
                "{tool_name}: properties.path.type must be 'string'");
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name}: required must be an array"));
            assert!(required.iter().any(|v| v.as_str() == Some("path")),
                "{tool_name}: 'path' must be listed in required");
            // panel is optional
            assert!(
                !required.iter().any(|v| v.as_str() == Some("panel")),
                "{tool_name}: 'panel' must NOT be listed in required (it is optional)"
            );
        }
    }

    // task-4302 step-9 RED → step-10 GREEN: capabilities/default.json must
    // grant core:window:allow-set-size for set_window_size to work at runtime.
    #[test]
    fn capabilities_default_grants_window_set_size() {
        const CAPS: &str = include_str!("../capabilities/default.json");
        let v: serde_json::Value = serde_json::from_str(CAPS)
            .expect("capabilities/default.json must be valid JSON");
        let permissions = v["permissions"]
            .as_array()
            .expect("capabilities/default.json must have a 'permissions' array");
        let perm_strings: Vec<&str> = permissions
            .iter()
            .filter_map(|p| p.as_str())
            .collect();

        assert!(
            perm_strings.contains(&"core:window:allow-set-size"),
            "capabilities/default.json must grant 'core:window:allow-set-size' \
             so that set_window_size's getCurrentWindow().setSize() call is \
             authorized at runtime. Add it to the permissions array."
        );
        // Regression guard: the pre-existing core:window:default must still be present.
        assert!(
            perm_strings.contains(&"core:window:default"),
            "capabilities/default.json must still contain 'core:window:default' \
             (regression: do not replace it with core:window:allow-set-size)."
        );
    }

    // task-4299 step-1 RED → step-2 GREEN: five synthetic-interaction tools must be
    // registered in tool_defs() with the correct schema shapes.
    // Schema-shape-only — NO description-prose pinning (convention at :1668-1670).
    #[test]
    fn tool_defs_registers_synthetic_interaction_tools() {
        let defs = tool_defs();

        // click_at and hover both require exactly ["x", "y"]
        for tool_name in ["click_at", "hover"] {
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name}: required must be an array"));
            assert!(
                required.iter().any(|v| v.as_str() == Some("x")),
                "{tool_name}: 'x' must be listed in required"
            );
            assert!(
                required.iter().any(|v| v.as_str() == Some("y")),
                "{tool_name}: 'y' must be listed in required"
            );
        }

        // drag requires ["from", "to"]
        {
            let tool_name = "drag";
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name}: required must be an array"));
            assert!(
                required.iter().any(|v| v.as_str() == Some("from")),
                "{tool_name}: 'from' must be listed in required"
            );
            assert!(
                required.iter().any(|v| v.as_str() == Some("to")),
                "{tool_name}: 'to' must be listed in required"
            );
        }

        // focus_element requires ["testId"]
        {
            let tool_name = "focus_element";
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name}: required must be an array"));
            assert!(
                required.iter().any(|v| v.as_str() == Some("testId")),
                "{tool_name}: 'testId' must be listed in required"
            );
        }

        // scroll has no required array or an empty one
        {
            let tool_name = "scroll";
            let entry = defs
                .iter()
                .find(|t| t.name == tool_name)
                .unwrap_or_else(|| panic!("{tool_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{tool_name}: input_schema.type must be 'object'"
            );
            assert!(
                !entry.description.is_empty(),
                "{tool_name}: description must be non-empty"
            );
            if let Some(required) = schema["required"].as_array() {
                assert!(
                    required.is_empty(),
                    "{tool_name}: required array must be empty; got {required:?}"
                );
            }
        }
    }

    // F2 step-5 RED → step-6 GREEN: hover_at / completion_at / definition_at must be
    // registered in tool_defs() with an object schema that requires integer line + col.
    // Mirroring viewport_aware_tools_expose_optional_viewport_id (table-driven so adding
    // a fourth probe is a one-line change).
    #[test]
    fn lsp_probe_tools_expose_required_integer_line_col() {
        let defs = tool_defs();
        let probes = ["hover_at", "completion_at", "definition_at"];
        for probe_name in probes {
            let entry = defs
                .iter()
                .find(|t| t.name == probe_name)
                .unwrap_or_else(|| panic!("{probe_name} must be present in tool_defs()"));
            let schema = &entry.input_schema;

            // input_schema.type must be "object"
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{probe_name}: input_schema.type must be 'object'"
            );

            // must have a non-empty description
            assert!(
                !entry.description.is_empty(),
                "{probe_name}: description must be non-empty"
            );

            // properties.line.type and properties.col.type must be "integer"
            assert_eq!(
                schema["properties"]["line"]["type"].as_str(),
                Some("integer"),
                "{probe_name}: properties.line.type must be 'integer'"
            );
            assert_eq!(
                schema["properties"]["col"]["type"].as_str(),
                Some("integer"),
                "{probe_name}: properties.col.type must be 'integer'"
            );

            // both line and col must appear in required
            let required = schema["required"]
                .as_array()
                .unwrap_or_else(|| panic!("{probe_name}: input_schema.required must be an array"));
            assert!(
                required.iter().any(|v| v.as_str() == Some("line")),
                "{probe_name}: 'line' must be listed in required"
            );
            assert!(
                required.iter().any(|v| v.as_str() == Some("col")),
                "{probe_name}: 'col' must be listed in required"
            );
        }
    }

    // task-4300 step-1 RED → step-2 GREEN: four I2 canvas-interaction tools must be
    // registered in tool_defs() with correct schema shapes.
    #[test]
    fn tool_defs_registers_canvas_interaction_tools() {
        let defs = tool_defs();

        struct Expectation {
            name: &'static str,
            numeric_props: &'static [&'static str],
        }
        let tools = [
            Expectation { name: "pick_entity_at", numeric_props: &["x", "y"] },
            Expectation { name: "orbit_camera",   numeric_props: &["dazimuth", "delevation"] },
            Expectation { name: "pan_camera",     numeric_props: &["dx", "dy"] },
            Expectation { name: "zoom_camera",    numeric_props: &["scale"] },
        ];

        for t in &tools {
            let entry = defs
                .iter()
                .find(|d| d.name == t.name)
                .unwrap_or_else(|| panic!("{} must be present in tool_defs()", t.name));
            let schema = &entry.input_schema;

            // Non-empty description
            assert!(
                !entry.description.is_empty(),
                "{}: description must be non-empty", t.name
            );

            // type == "object"
            assert_eq!(
                schema["type"].as_str(),
                Some("object"),
                "{}: input_schema.type must be 'object'", t.name
            );

            // All params are optional — required must be absent or empty
            if let Some(required) = schema["required"].as_array() {
                assert!(
                    required.is_empty(),
                    "{}: required must be absent or empty; got {:?}", t.name, required
                );
            }

            // Tool-specific numeric properties
            for prop in t.numeric_props {
                assert_eq!(
                    schema["properties"][prop]["type"].as_str(),
                    Some("number"),
                    "{}: properties.{}.type must be 'number'", t.name, prop
                );
            }
        }
    }

    // --- F1: load_fixture ---

    #[test]
    fn tool_defs_registers_load_fixture() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|d| d.name == "load_fixture")
            .expect("load_fixture must be present in tool_defs()");
        let schema = &entry.input_schema;

        // Non-empty description
        assert!(
            !entry.description.is_empty(),
            "load_fixture: description must be non-empty"
        );

        // type == "object"
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "load_fixture: input_schema.type must be 'object'"
        );

        // "name" must be in required
        let required = schema["required"]
            .as_array()
            .expect("load_fixture: input_schema.required must be an array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("name")),
            "load_fixture: 'name' must be listed in required; got {required:?}"
        );

        // "name" property must be a string
        assert_eq!(
            schema["properties"]["name"]["type"].as_str(),
            Some("string"),
            "load_fixture: properties.name.type must be 'string'"
        );
    }

    #[test]
    fn fixture_relpath_resolves_catalogue() {
        // Known catalogue keys
        let known = [
            ("all_severities", "gui/test/fixtures/all_severities.ri"),
            ("small_cube",     "gui/test/fixtures/small_cube.ri"),
            ("empty",          "gui/test/fixtures/empty.ri"),
            ("broken_syntax",  "gui/test/fixtures/broken_syntax.ri"),
            ("large_assembly", "gui/test/fixtures/large_assembly.ri"),
            ("overflow",       "gui/test/fixtures/overflow.ri"),
        ];
        for (name, expected_relpath) in known {
            let result = fixture_relpath(name);
            assert_eq!(
                result.as_deref(),
                Some(expected_relpath),
                "fixture_relpath({name:?}) should return Some({expected_relpath:?}), got {result:?}"
            );
        }

        // Unknown name must return None
        let bogus = fixture_relpath("bogus_name");
        assert_eq!(
            bogus,
            None,
            "fixture_relpath(\"bogus_name\") should return None, got {bogus:?}"
        );
    }

    // --- F1: element_screenshot ---

    #[test]
    fn tool_defs_registers_element_screenshot() {
        let defs = tool_defs();
        let entry = defs
            .iter()
            .find(|d| d.name == "element_screenshot")
            .expect("element_screenshot must be present in tool_defs()");
        let schema = &entry.input_schema;

        // Non-empty description
        assert!(
            !entry.description.is_empty(),
            "element_screenshot: description must be non-empty"
        );

        // type == "object"
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "element_screenshot: input_schema.type must be 'object'"
        );

        // "testId" must be in required
        let required = schema["required"]
            .as_array()
            .expect("element_screenshot: input_schema.required must be an array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("testId")),
            "element_screenshot: 'testId' must be listed in required; got {required:?}"
        );

        // "testId" property must be a string
        assert_eq!(
            schema["properties"]["testId"]["type"].as_str(),
            Some("string"),
            "element_screenshot: properties.testId.type must be 'string'"
        );
    }

    #[test]
    fn is_image_tool_recognizes_element_screenshot() {
        assert!(
            is_image_tool("element_screenshot"),
            "element_screenshot must be recognised as an image tool"
        );
        // Regression: existing variants still pass
        assert!(is_image_tool("screenshot"));
        assert!(is_image_tool("screenshot_window"));
        // Non-image tools must not match
        assert!(!is_image_tool("health"));
        assert!(!is_image_tool(""));
    }
}
