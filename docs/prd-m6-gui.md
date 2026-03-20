# PRD: Milestone 6 — "Visual" GUI for Alpha Testing

**Version:** 0.3
**Date:** 2026-03-19
**Author:** Leo (with Claude)
**Status:** Ready — all decisions resolved
**Depends on:** M4 "Living Design" (complete), M5 "Language Breadth" (in progress)

---

## 1. Problem Statement

Reify's engine is headless: the only way to see geometry is to export STEP/STL and open it in an external viewer. This creates a slow feedback loop — edit `.ri` source, run `reify build`, open CAD viewer, inspect, repeat. For alpha testing and day-to-day use, engineers need a live viewport that updates as they type.

The engine already supports this architecturally — demand-driven evaluation, tessellation, snapshot-versioned state, priority scheduling — but no visual surface exists to consume it.

## 2. Goals

1. **Live 3D viewport** — tessellated geometry rendered in real-time, updating within ~200ms of a parameter edit.
2. **Property editor** — read and write ValueCells directly, driving the engine's demand-driven evaluation.
3. **Constraint status panel** — show constraint satisfaction/violation at a glance.
4. **Integrated code editor** — built-in `.ri` editor with syntax highlighting, LSP diagnostics, and completions. Self-contained experience without requiring an external editor.
5. **Source ↔ viewport bidirectional awareness** — clicking geometry highlights source; cursor position in source highlights geometry.
6. **Zero-install alpha distribution** — alpha testers can try Reify without building from source.

## 3. Non-Goals (explicitly out of scope)

- **Full CAD GUI** — no sketcher, no feature tree, no dimension dragging, no assembly mate UI. This is a *viewer + property editor*, not a modeling environment.
- **2D drawing generation** — no orthographic projections or annotation.
- **Collaboration / multi-user** — single-user only.
- **Mobile / tablet** — desktop only.
- **Custom theming / branding** — functional appearance is fine.
- **Plugin / extension system** — hardcoded panels only.
- **Geometry selection for modeling** — face/edge picking for *queries* (hover info) is in scope; picking to define operations (e.g., "fillet this edge") is not.

## 4. Technology Decision

### 4.1 Options Considered

| Option | Stack | Pros | Cons |
|--------|-------|------|------|
| **A: wgpu + egui** | Pure Rust. wgpu for 3D, egui for panels. Single binary. | Same language as engine. No IPC. Native perf. Single distributable binary. Compiles to WASM later. | egui immediate-mode UI is less polished for complex panels. More rendering code to write (camera, lighting, selection). |
| **B: Tauri + Three.js** | Rust backend (Tauri), web frontend (TypeScript + Three.js + React/Solid). | Mature 3D ecosystem (Three.js orbit controls, lighting, selection). Fast UI iteration with web tech. Tauri keeps it lightweight. | Two languages. IPC serialization overhead. Harder to compile to pure WASM later. |
| **C: Web-native** | Engine compiled to WASM, or engine as local HTTP server + WebSocket. Browser UI with Three.js. | Zero install (open a URL). Most accessible for alpha testers. Proven 3D stack. | OCCT-to-WASM is painful (100+ MB). Server-mode adds networking complexity. Latency for geometry updates. |
| **D: Hybrid — Tauri + wgpu** | Rust wgpu viewport embedded in Tauri webview. Web UI for panels, Rust for 3D. | Best of both: native rendering + web UI ergonomics. | Tauri wgpu embedding is experimental. Two rendering contexts. |

### 4.2 Recommendation: **Option B — Tauri + Three.js**

**Rationale:**

- **Fastest path to alpha.** Three.js provides orbit controls, lighting, raycasting, and glTF/mesh rendering out of the box. The engine integration is the hard part, not the rendering.
- **Property editor and constraint panel are forms.** Web tech excels at data-dense panels with scroll, search, filtering, and responsive layout — where egui requires significantly more code for equivalent polish.
- **Tauri is lightweight.** ~10 MB overhead, not Electron's 100+ MB. Rust backend runs the engine in-process (no serialization for compute — only for UI updates).
- **IPC is bounded.** The data crossing the bridge is small: tessellated meshes (vertices + indices), ValueCell snapshots (name/value/determinacy tuples), constraint status (satisfied/violated enum). Not full evaluation state. Transfer is per-snapshot-delta, not per-frame.
- **Web-native upgrade path.** The Three.js viewport and web UI can later be served from a local or cloud server, enabling browser-based access without rewriting the frontend.
- **WASM is not foreclosed.** The engine crates remain pure Rust. If Truck or another WASM-compatible kernel matures, the engine itself can compile to WASM and the frontend connects directly — the web UI doesn't change.

**What we give up:** single-binary distribution (Tauri produces an installer, not a single EXE), and a second language in the repo (TypeScript). Acceptable for alpha.

**Decision: Confirmed.** Option B selected.

---

## 5. User Stories

### 5.1 Core Loop: Edit → See

> As an engineer, I edit a `.ri` file in my editor (VS Code with Reify LSP), and the Reify viewport updates to show the new geometry within a few hundred milliseconds.

**Flow:**
1. User edits `bracket.ri` in VS Code — changes `width` from `80mm` to `100mm`.
2. LSP server picks up the file change, triggers re-parse → re-elaborate → re-evaluate.
3. Dirty/demand cone intersects the RealizationNode for the bracket (viewport has it demanded).
4. Geometry re-evaluates, OCCT produces new shape, tessellation produces new mesh.
5. GUI receives mesh delta via Tauri IPC, Three.js viewport updates.
6. Total latency: <500ms for simple parameter changes, <2s for complex re-realizations.

### 5.2 Property Editor

> As an engineer, I see all parameters of the selected structure in a panel, with their current values, units, determinacy states, and types. I can edit `determined` parameters directly, which triggers re-evaluation.

**Details:**
- Parameters grouped by structure (tree view matching scope hierarchy).
- Each parameter shows: name, value, unit, determinacy badge (`determined` / `auto` / `constrained` / `undef`).
- Editable fields for `determined` parameters — typing a new value is equivalent to editing the source.
- `auto` parameters are read-only with a tooltip showing which constraints/solver resolved them.
- Live update as the engine re-evaluates (intermediate values shown with a "computing..." indicator).

### 5.3 Constraint Panel

> As an engineer, I see all constraints at a glance: green for satisfied, red for violated, gray for indeterminate. Clicking a constraint highlights the relevant parameters and geometry.

**Details:**
- Flat list or grouped by scope, sortable by status.
- Each constraint shows: expression text (from source), status badge, contributing parameter values.
- Violated constraints expand to show which predicate failed and by how much.
- Click → highlight relevant ValueCells in the property editor and geometry in the viewport.

### 5.4 Viewport Interaction

> As an engineer, I orbit, pan, and zoom the 3D viewport. I hover over geometry to see which structure it belongs to, and click to select it (highlighting in the property editor and source).

**Details:**
- Standard CAD orbit/pan/zoom (Three.js OrbitControls).
- Hover: highlight face/edge under cursor, show tooltip with structure name and key dimensions.
- Click: select structure, populate property editor, jump to source location in editor (via LSP).
- Multi-body display: each top-level structure rendered as a separate mesh with distinct color/material.
- Visual indicators: wireframe overlay for selected structure, transparency for non-selected.

### 5.5 Source ↔ Viewport Navigation

> As an engineer, I double-click a structure name in the built-in editor and the viewport flies to that structure. Conversely, clicking geometry in the viewport jumps my cursor to the corresponding source line.

**Flow (source → viewport):** Cursor on structure declaration or Ctrl+click → GUI resolves EntityPath → camera animates to bounding box of target structure.

**Flow (viewport → source):** Click geometry → resolve to EntityPath → resolve to SourceSpan → built-in editor scrolls to line and highlights.

**External editor support:** If using VS Code alongside, the same flows work via LSP (`textDocument/showDocument`). The built-in editor is primary; external editor is optional.

### 5.6 Integrated Code Editor

> As an engineer, I write and edit `.ri` source directly in the Reify GUI, with syntax highlighting, live diagnostics, and completions — without needing a separate editor open.

**Details:**
- CodeMirror 6 editor embedded in a resizable panel (left or bottom, user-draggable split).
- Connects to `reify-lsp` as an in-process LSP client (no separate LSP server process needed — the GUI's Rust backend hosts the LSP and bridges to CodeMirror via Tauri events).
- Syntax highlighting via a hand-written Lezer grammar (~200-300 lines) providing incremental parsing, bracket matching, code folding, and structural indent — all client-side without LSP round-trips.
- Live diagnostics: red/yellow squiggles inline, diagnostic list in a sub-panel.
- Completions: parameter names, structure types, unit suffixes, standard library functions.
- Go-to-definition: Ctrl+click on an identifier jumps to its declaration.
- Hover: type info and documentation on hover.
- File tabs for multi-file projects.
- Standard editor features: undo/redo, find/replace, line numbers, minimap (optional).
- On edit: the engine re-evaluates immediately (same path as external file save, but without the filesystem round-trip — edits go directly from CodeMirror → engine via Tauri command).

**Why CodeMirror over Monaco:**
- CodeMirror 6 is ~150 KB (vs Monaco's ~5 MB). In a Tauri app where the webview is lightweight, this matters.
- CodeMirror's architecture is more composable and easier to integrate with custom LSP bridges.
- Monaco carries VS Code assumptions (AMD loader, worker threads) that add friction in a Tauri context.

### 5.7 Export from GUI

> As an engineer, I click "Export" and choose STEP, STL, or 3MF. The file is written and I get a success notification.

Already implemented in the CLI — the GUI wraps the same `reify-geometry` export API.

---

## 6. Architecture

### 6.1 Component Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                        Tauri Application                          │
│                                                                   │
│  ┌──────────────────────────┐   ┌──────────────────────────────┐ │
│  │    Rust Backend           │   │   Web Frontend (Solid)       │ │
│  │                           │   │                              │ │
│  │  ┌─────────────────────┐  │   │  ┌────────────────────────┐ │ │
│  │  │ reify-gui (new)     │  │   │  │ Three.js Viewport      │ │ │
│  │  │                     │  │   │  │ (orbit/pan/zoom,        │ │ │
│  │  │ • Engine session    │◄─┼───┼─►│  raycasting, mesh       │ │ │
│  │  │ • In-process LSP    │  │   │  │  rendering)             │ │ │
│  │  │ • File watcher      │  │   │  └────────────────────────┘ │ │
│  │  │ • Snapshot diffing   │  │   │  ┌────────────────────────┐ │ │
│  │  │ • Tauri commands    │  │   │  │ CodeMirror 6 Editor    │ │ │
│  │  └────────┬────────────┘  │   │  │ (syntax hl, LSP diag,  │ │ │
│  │           │               │   │  │  completions, hover)    │ │ │
│  │  ┌────────▼────────────┐  │   │  └────────────────────────┘ │ │
│  │  │ Existing Engine      │  │   │  ┌────────────────────────┐ │ │
│  │  │ (reify-eval,         │  │   │  │ Property Editor        │ │ │
│  │  │  reify-runtime,      │  │   │  │ (parameter tree,       │ │ │
│  │  │  reify-geometry,     │  │   │  │  value editing)         │ │ │
│  │  │  reify-kernel-occt)  │  │   │  └────────────────────────┘ │ │
│  │  └─────────────────────┘  │   │  ┌────────────────────────┐ │ │
│  │                           │   │  │ Constraint Panel        │ │ │
│  └──────────────────────────┘   │  │ (status badges,          │ │ │
│                                  │  │  cross-highlighting)     │ │ │
│                                  │  └────────────────────────┘ │ │
│                                  └──────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
         ▲                              ▲
         │ File system                  │ LSP (optional)
         │ (watch .ri files)            │ (external editor, e.g. VS Code)
         ▼                              ▼
    ┌──────────┐                  ┌──────────────────┐
    │ .ri files│                  │ External Editor   │
    └──────────┘                  │ (optional)        │
                                  └──────────────────┘
```

### 6.2 New Crate: `reify-gui`

A thin Rust crate that:

1. **Owns the engine session.** Instantiates `reify-eval::Engine`, loads `.ri` files, registers demands.
2. **Watches the filesystem.** Detects `.ri` file changes (via `notify` crate), triggers re-parse/re-evaluate.
3. **Computes snapshot diffs.** Compares consecutive snapshots to produce minimal UI update messages:
   - `MeshUpdate { entity_path, vertices, indices, normals }` — only for changed RealizationNodes.
   - `ValueUpdate { cell_id, value, determinacy, freshness }` — only for changed ValueCells.
   - `ConstraintUpdate { node_id, satisfaction, diagnostics }` — only for changed ConstraintNodes.
4. **Exposes Tauri commands.** The IPC surface:

```rust
// Tauri commands exposed to the frontend

#[tauri::command]
fn get_initial_state() -> GuiState;
// Full state dump on app launch: all meshes, all values, all constraints, file contents.

#[tauri::command]
fn set_parameter(cell_id: String, value: String) -> Result<(), String>;
// Edit a ValueCell via property editor. Two-way binding: updates source text at the
// corresponding declaration site, then triggers re-parse → re-evaluate.

#[tauri::command]
fn update_source(file_path: String, content: String) -> Result<(), String>;
// Editor content changed. Triggers re-parse → re-evaluate (no filesystem round-trip).

#[tauri::command]
fn save_file(file_path: String, content: String) -> Result<(), String>;
// Persist editor content to disk.

#[tauri::command]
fn open_file(path: String) -> Result<FileContent, String>;
// Open a .ri file in the editor.

#[tauri::command]
fn export(format: String, path: String) -> Result<(), String>;
// STEP/STL/3MF export.

#[tauri::command]
fn get_source_location(entity_path: String) -> Option<SourceLocation>;
// Resolve entity → source span for viewport→source navigation.

#[tauri::command]
fn focus_entity(entity_path: String);
// Source→viewport navigation. Emits a `focus` event to the frontend.

#[tauri::command]
fn lsp_request(method: String, params: serde_json::Value) -> Result<serde_json::Value, String>;
// Generic LSP request bridge for completions, hover, go-to-def, etc.

// Tauri events (backend → frontend, push-based)
// "mesh-update"        → MeshUpdate
// "value-update"       → ValueUpdate[]
// "constraint-update"  → ConstraintUpdate[]
// "evaluation-status"  → { phase: "idle" | "evaluating" | "resolving", progress?: f32 }
// "diagnostics"        → PublishDiagnosticsParams (LSP diagnostic push)
// "file-changed"       → { path: String } (external file change detected by watcher)
```

### 6.3 Frontend Structure

```
gui/
  src-tauri/              # Rust backend (reify-gui crate)
    src/
      main.rs             # Tauri app setup, engine init
      commands.rs         # Tauri command handlers
      diff.rs             # Snapshot diffing logic
      bridge.rs           # Serialize engine types → JSON for frontend
      lsp_bridge.rs       # In-process LSP ↔ Tauri event bridge
  src/                    # Web frontend (Solid + TypeScript)
    index.tsx             # Solid app root, layout
    App.tsx               # Top-level layout: editor | viewport | panels
    viewport/
      Viewport.tsx        # Three.js canvas component (Solid onMount lifecycle)
      scene.ts            # Three.js scene setup (camera, lights, renderer)
      controls.ts         # Orbit/pan/zoom (thin wrapper on OrbitControls)
      meshManager.ts      # Receive MeshUpdate events, update Three.js meshes
      selection.ts        # Raycasting, hover highlight, click-to-select
    editor/
      Editor.tsx          # CodeMirror 6 wrapper component
      lspClient.ts        # Bridge Tauri commands to CodeMirror LSP extensions
      reifyLanguage.ts    # CodeMirror language mode (Lezer grammar)
      diagnostics.ts      # Inline diagnostic rendering from LSP publishDiagnostics
      completions.ts      # Completion source backed by LSP textDocument/completion
    panels/
      PropertyEditor.tsx  # Parameter tree, inline editing
      ConstraintPanel.tsx # Constraint list with status badges
      Toolbar.tsx         # Export button, view controls
      StatusBar.tsx       # Eval time, triangle count, constraint summary
    stores/
      engineStore.ts      # Solid store: meshes, values, constraints, eval status
      selectionStore.ts   # Solid store: selected entity, hovered entity
      editorStore.ts      # Solid store: open files, dirty state, cursor position
    bridge.ts             # Tauri invoke/listen wrappers, typed event handlers
    types.ts              # TypeScript types matching Rust bridge types
    theme.ts              # Dark mode theme tokens (CSS custom properties)
```

**Frontend framework:** SolidJS. Solid's fine-grained reactivity maps well to the engine's delta-based updates — a `MeshUpdate` event updates only the affected signal, which updates only the affected Three.js mesh object, with no virtual DOM diffing. The property editor's tree structure and real-time value updates benefit from Solid's granular subscriptions. Three.js and CodeMirror 6 are the heavyweight dependencies.

**Decision: Confirmed.** SolidJS selected.

### 6.4 Integration with Existing Engine

The GUI consumes existing engine interfaces — no engine modifications needed for core functionality:

| Engine API | GUI Usage |
|------------|-----------|
| `Engine::load_file()` | Load `.ri` on startup and file change |
| `Engine::subscribe_events()` | Receive evaluation completion events |
| `DemandRegistry::register()` | Register viewport-visible RealizationNodes as always-demanded |
| `Snapshot::values()` | Read ValueCells for property editor |
| `Snapshot::constraint_status()` | Read ConstraintNode satisfaction for constraint panel |
| `GeometryKernel::tessellate()` | Produce meshes for viewport |
| `GeometryKernel::export()` | STEP/STL/3MF export from GUI |
| `Engine::edit_value_cell()` | Property editor writes |

**New engine surface needed** (minimal):
- `Engine::entity_source_span(EntityPath) -> Option<SourceSpan>` — for viewport→source navigation.
- `Engine::entity_bounding_box(EntityPath) -> Option<BoundingBox>` — for source→viewport camera focus.
- `Engine::update_source(path, content)` — accept source text directly from the editor without requiring a filesystem write. This avoids the round-trip of write→watch→read and enables sub-keystroke latency.
- In-process LSP hosting: `reify-lsp` must support being driven programmatically (not just over stdio/TCP). This may already be possible via `tower-lsp`'s `LspService` — needs verification.
- Structured event emission for mesh/value/constraint deltas (may already be covered by the event journal; needs verification).

### 6.5 Two-Way Property ↔ Source Binding

When a parameter is edited in the property editor, the source text is updated to match:

1. **Property editor** → user changes `width` from `80mm` to `100mm`.
2. **`set_parameter` command** → Rust backend resolves `cell_id` to the AST node containing the literal value.
3. **Source text edit** → the backend performs a targeted text replacement at the declaration site (e.g., `width: Length = 80mm` → `width: Length = 100mm`).
4. **Editor sync** → the `source-updated` event pushes the new text to CodeMirror, which applies it as an edit (preserving undo history).
5. **Engine re-evaluation** → the updated source feeds through the normal re-parse → re-elaborate → re-evaluate pipeline.

This means source is always canonical. The property editor is a structured view *into* the source, not a parallel state. The dirty-file indicator reflects property editor changes just like manual edits.

**Edge cases:**
- `auto` parameters are read-only in the property editor (resolved by the solver, not user-editable).
- If the user is actively editing the same line in the code editor, the property editor edit is rejected with a conflict notification (source editor takes priority).
- Computed expressions (e.g., `width: Length = base_width * 2`) show the computed value in the property editor but the expression in the source. Editing replaces the expression with a literal.

### 6.6 Demand Registration Strategy

The viewport drives what gets computed:

1. **On file load:** Register all top-level structure RealizationNodes as always-demanded.
2. **On structure visibility toggle:** Add/remove from demand set (future: per-structure visibility checkboxes).
3. **On property editor focus:** The focused structure's ValueCells are P0-demanded for responsive editing.
4. **On viewport occlusion (future, not M6):** Off-screen structures could be deprioritized to P3. Not implementing in alpha — all visible structures are P1.

### 6.6 Mesh Transfer Protocol

Meshes are the largest data crossing the Tauri IPC bridge. Optimization matters for interactive feel:

1. **Binary transfer.** Vertex/index buffers transferred as `ArrayBuffer` via Tauri's binary event channel, not JSON-serialized floats.
2. **Delta updates.** Only changed meshes are re-sent. Each mesh is keyed by `EntityPath`; on snapshot change, only re-tessellate and re-send meshes whose RealizationNode content hash changed.
3. **Progressive LOD (future, not M6).** Coarse mesh first for fast visual update, high-res mesh follows. The tessellation tolerance parameter controls this. Alpha ships with a single tolerance level.
4. **Size budget.** Typical bracket-class model: ~10K triangles, ~120 KB per mesh. Acceptable for IPC even without optimization. Complex assemblies (100K+ triangles) may need chunked transfer — deferred unless alpha testers hit it.

---

## 7. Detailed Requirements

### 7.1 3D Viewport

| ID | Requirement | Priority |
|----|-------------|----------|
| V-1 | Render tessellated meshes from RealizationNodes | Must |
| V-2 | Orbit, pan, zoom with mouse (OrbitControls) | Must |
| V-3 | Ambient + directional lighting, shadows optional | Must |
| V-4 | Distinct colors per top-level structure | Must |
| V-5 | Hover highlight (face/edge glow) | Should |
| V-6 | Click to select structure | Must |
| V-7 | Wireframe overlay on selected structure | Should |
| V-8 | Fit-to-view button (auto-frame all geometry) | Must |
| V-9 | Background color: neutral gray gradient | Must |
| V-10 | Grid / ground plane | Should |
| V-11 | Axis indicator (XYZ gizmo in corner) | Should |
| V-12 | "Evaluating..." spinner overlay during re-evaluation | Must |

### 7.2 Property Editor

| ID | Requirement | Priority |
|----|-------------|----------|
| P-1 | Tree view of structures → parameters | Must |
| P-2 | Show value, unit, and determinacy badge per parameter | Must |
| P-3 | Inline edit for `determined` parameters | Must |
| P-4 | Read-only display for `auto`/`constrained` parameters | Must |
| P-5 | `auto` tooltip: "resolved by [solver] via [constraints]" | Should |
| P-6 | Live update during re-evaluation (intermediate values) | Must |
| P-7 | Search/filter parameters by name | Should |
| P-8 | Collapse/expand structure groups | Must |
| P-9 | Highlight parameter when corresponding geometry is selected | Should |

### 7.3 Constraint Panel

| ID | Requirement | Priority |
|----|-------------|----------|
| C-1 | List all constraints with status badge (green/red/gray) | Must |
| C-2 | Show constraint expression text | Must |
| C-3 | Show contributing parameter values for each constraint | Should |
| C-4 | Expand violated constraints to show failure details | Must |
| C-5 | Click constraint → highlight relevant parameters and geometry | Should |
| C-6 | Sort by status (violated first) | Should |

### 7.4 Navigation

| ID | Requirement | Priority |
|----|-------------|----------|
| N-1 | Click geometry → jump to source line in editor (via LSP) | Must |
| N-2 | LSP command → fly viewport camera to structure | Should |
| N-3 | Double-click structure in property editor → fly to in viewport | Should |

### 7.5 Code Editor

| ID | Requirement | Priority |
|----|-------------|----------|
| D-1 | CodeMirror 6 editor with Reify syntax highlighting | Must |
| D-2 | Live diagnostics (inline squiggles from LSP publishDiagnostics) | Must |
| D-3 | Completions (parameter names, types, units, stdlib functions) | Must |
| D-4 | Hover info (type, dimension, documentation) | Must |
| D-5 | Go-to-definition (Ctrl+click) | Must |
| D-6 | File tabs for multi-file projects | Must |
| D-7 | Undo/redo, find/replace, line numbers | Must |
| D-8 | Dirty-file indicator and Ctrl+S save | Must |
| D-9 | Direct engine update on edit (no filesystem round-trip) | Must |
| D-10 | External file change detection and reload prompt | Should |
| D-11 | Minimap | Should |
| D-12 | Bracket matching and auto-indent | Should |
| D-13 | File browser sidebar for multi-module projects | Must |

### 7.6 Export

| ID | Requirement | Priority |
|----|-------------|----------|
| E-1 | Export button with format selector (STEP, STL, 3MF) | Must |
| E-2 | File save dialog for output path | Must |
| E-3 | Progress indicator for large exports | Should |
| E-4 | Success/failure notification | Must |

### 7.6 Application Shell

| ID | Requirement | Priority |
|----|-------------|----------|
| A-1 | Open `.ri` file or directory via file dialog or CLI argument | Must |
| A-2 | Watch `.ri` files for external changes, prompt reload in editor | Must |
| A-3 | Window title shows file name and evaluation status | Must |
| A-4 | Resizable panels (editor / viewport / property editor / constraint panel) | Must |
| A-5 | Keyboard shortcuts: Ctrl+O (open), F5 (force re-evaluate), Ctrl+E (export), Ctrl+S (save) | Must |
| A-6 | Status bar: evaluation time, triangle count, constraint summary | Should |
| A-7 | Dark mode default, consistent dark theme across all panels | Must |
| A-8 | Layout: editor left, viewport center, property editor + constraint panel right (adjustable) | Must |

---

## 8. Performance Requirements

| Metric | Target | Rationale |
|--------|--------|-----------|
| Parameter edit → viewport update | < 500ms | Interactive feel. Engine already targets this via warm-start + caching. |
| File save → viewport update | < 1s | Re-parse + re-elaborate + re-evaluate + re-tessellate + transfer. |
| Viewport frame rate | ≥ 30 FPS | Smooth orbit/pan/zoom. Three.js handles this for <1M triangle scenes. |
| Startup time (load + first render) | < 3s | For a typical single-file project. |
| Memory (GUI overhead) | < 200 MB | On top of engine memory. Three.js + Tauri webview baseline. |
| IPC mesh transfer | < 50ms per mesh | Binary ArrayBuffer transfer, not JSON. |

---

## 9. Distribution

### 9.1 Alpha Distribution

- **macOS:** `.dmg` via Tauri bundler.
- **Linux:** `.AppImage` or `.deb` via Tauri bundler.
- **Windows:** `.msi` via Tauri bundler.

Tauri's built-in bundler handles all three platforms. OCCT is statically linked into the Rust binary (already the case for CLI).

### 9.2 CLI Integration

```
reify gui <file.ri>          # launch GUI with file loaded
reify gui .                  # launch GUI watching current directory
```

The `reify gui` command is a new subcommand of `reify-cli` that launches the Tauri application. WebSocket mode (`--port`) deferred to post-alpha.

---

## 10. Implementation Plan

### Phase 1: Scaffold + Viewport (1 day)

- [ ] Tauri project scaffolding (`gui/` directory with `src-tauri/` and `src/`).
- [ ] SolidJS app shell with dark theme, resizable panel layout (editor | viewport | side panels).
- [ ] `reify-gui` crate: engine session initialization, file loading, basic Tauri commands.
- [ ] Three.js scene setup: camera, lights, OrbitControls, grid, axis indicator, dark background.
- [ ] `get_initial_state` command: load `.ri` file, tessellate, send mesh to frontend.
- [ ] Solid stores for engine state, selection, editor state.
- [ ] **Exit criterion:** `reify gui examples/bracket.ri` opens a window showing the bracket mesh with orbit/pan/zoom in a dark-themed shell.

### Phase 2: Code Editor + Live Updates (1.5 days)

- [ ] CodeMirror 6 integration: editor component, Lezer grammar for Reify (~200-300 lines).
- [ ] In-process LSP bridge: `reify-lsp` hosted in the Rust backend, bridged to CodeMirror via Tauri events.
- [ ] Live diagnostics: LSP `publishDiagnostics` → CodeMirror inline squiggles.
- [ ] Completions and hover info via `lsp_request` command.
- [ ] Go-to-definition (Ctrl+click).
- [ ] `update_source` command: editor edits go directly to engine (no filesystem round-trip).
- [ ] File watcher (`notify` crate) for external changes → reload prompt.
- [ ] Snapshot diffing: detect changed RealizationNodes, re-tessellate, send mesh delta.
- [ ] Tauri event emission for mesh/value/constraint updates.
- [ ] "Evaluating..." overlay during re-evaluation.
- [ ] File tabs, dirty indicators, Ctrl+S save.
- [ ] **Exit criterion:** Edit `.ri` in the built-in editor → viewport updates within 500ms. Diagnostics, completions, and hover work.

### Phase 3: Property Editor + Constraint Panel (1 day)

- [ ] Parameter tree view populated from ValueCell snapshot.
- [ ] Determinacy badges, unit display, inline editing for `determined` parameters.
- [ ] `set_parameter` command with two-way source binding → source text updates → re-evaluation → viewport update.
- [ ] Live value updates during re-evaluation (intermediate values).
- [ ] Constraint list with status badges (green/red/gray).
- [ ] Violated constraint expansion with failure details.
- [ ] Cross-highlighting: click constraint → highlight parameters; click geometry → highlight in editor + property editor.
- [ ] **Exit criterion:** Change `width` in property editor → geometry updates, constraints re-check. Constraint panel shows live status.

### Phase 4: Navigation + Selection (0.5 day)

- [ ] Click geometry in viewport → editor scrolls to source line, property editor shows parameters.
- [ ] Click structure in editor → viewport flies to bounding box.
- [ ] Hover highlight in viewport (face/edge glow, tooltip with structure name).
- [ ] Wireframe overlay on selected structure.
- [ ] Fit-to-view button.
- [ ] **Exit criterion:** Full bidirectional navigation between editor, viewport, and panels.

### Phase 5: Polish + Distribution (1 day)

- [ ] Export dialog (STEP/STL/3MF) with file save dialog and notifications.
- [ ] Keyboard shortcuts (Ctrl+O, F5, Ctrl+E, Ctrl+S).
- [ ] Status bar (eval time, triangle count, constraint summary).
- [ ] File browser sidebar for multi-file projects.
- [ ] Bracket matching, auto-indent, find/replace in editor.
- [ ] Tauri bundler config for macOS/Linux/Windows.
- [ ] Smoke test on all three platforms.
- [ ] **Exit criterion:** `reify gui` distributable works on macOS and Linux. Full edit→view→export loop functional.

**Total estimated: 5 days** (upper end of the 3–5 day M6 estimate, reflecting the added code editor scope).

---

## 11. Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| **Tauri wgpu embedding needed later** | Would require viewport rewrite if egui/wgpu viewport is desired in Tauri shell | Low (Three.js is sufficient for alpha) | Accept Three.js for alpha. Evaluate native viewport for v0.2 if perf requires it. |
| **OCCT thread safety for tessellation** | Tessellation must run on the dedicated kernel thread (known M4 issue) | High (known) | Already mitigated: M4 established dedicated OCCT thread. Tessellation requests go through the same channel. |
| **Large mesh IPC bottleneck** | Complex assemblies could stall the UI during mesh transfer | Medium | Binary ArrayBuffer transfer. Delta-only updates. Defer LOD/chunking unless hit in practice. |
| **Tauri + TypeScript adds build complexity** | Two build systems (Cargo + npm/pnpm), CI complexity | Medium | Tauri CLI handles orchestration. Pin dependency versions. |
| **Three.js rendering quality** | Tessellated B-rep meshes may show faceting artifacts | Medium | Tessellation tolerance is adjustable. OCCT's `BRepMesh_IncrementalMesh` produces good results at reasonable tolerance. |
| **Cross-platform OCCT linking** | Static linking OCCT on Windows is historically painful | Medium | Already solved for CLI builds. GUI binary links the same way. |
| **CodeMirror LSP bridge complexity** | Bridging in-process LSP to CodeMirror via Tauri events adds latency and serialization | Medium | LSP responses are small JSON. Completion latency target is <100ms. If problematic, fall back to a local LSP socket connection instead of Tauri IPC. |
| **Code editor scope creep** | Built-in editor expectations grow (refactoring, multi-cursor, git integration) | Medium | Explicitly scope to D-1 through D-13 requirements. Defer advanced editor features to post-alpha. The editor is a convenience, not a VS Code replacement. |

---

## 12. Success Criteria

The GUI milestone is complete when:

1. `reify gui examples/bracket.ri` launches and displays the bracket in a dark-themed 3D viewport.
2. Editing `.ri` source in the built-in CodeMirror editor updates the viewport within 500ms.
3. The editor provides syntax highlighting, live diagnostics, completions, hover, and go-to-definition.
4. Changing a parameter in the property editor triggers re-evaluation and viewport update.
5. Constraint panel shows satisfaction status and updates live.
6. Clicking geometry in the viewport scrolls the editor to the corresponding source line.
7. Clicking a structure declaration in the editor flies the viewport camera to that structure.
8. Export to STEP/STL works from the GUI.
9. Distributable packages build for macOS and Linux.

---

## 13. Resolved Decisions

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| 1 | Frontend framework | **SolidJS** | Fine-grained reactivity maps to delta-based engine updates |
| 2 | Built-in code editor | **Yes, CodeMirror 6** | Self-contained experience, ~150 KB vs Monaco's ~5 MB |
| 3 | Dark mode | **Yes, dark mode default** | Standard for 3D/CAD tools |
| 4 | Technology stack | **Tauri v2 + Three.js + SolidJS + CodeMirror 6** | Confirmed |
| 5 | Multi-file projects | **Yes, file browser sidebar + file tabs** | Needed for multi-module projects in alpha |
| 6 | WebSocket mode | **Deferred to post-alpha** | Clean add-on later; Tauri command surface maps directly to WebSocket API |
| 7 | CodeMirror language mode | **Lezer grammar** | ~1.5 days more than TextMate, but enables bracket matching, code folding, structural indent without LSP round-trip. ~200-300 lines, comparable to existing Tree-sitter grammar. |
| 8 | Editor ↔ property editor sync | **Two-way binding** | Property editor edits update source text directly. Source is canonical; property editor is a view into it. |
| 9 | Tauri version | **Tauri v2** | Starting fresh (no migration cost), active development, multi-webview capability useful for viewport isolation, mobile not foreclosed |
