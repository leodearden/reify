# GUI Event Channel Inventory

This file is the **canonical, machine-grep-friendly source of truth** for Tauri-side event channel names in the Reify GUI. It is generated from and kept in lockstep with [`docs/prds/v0_3/gui-event-channel-inventory.md`](prds/v0_3/gui-event-channel-inventory.md) — §2 of that PRD is the cross-referenced human-readable form; this file is the grep target. On any PRD §2 prose change, this file is updated in the same commit.

For the naming/payload convention governing new entries see §3 of the source PRD. Every **event channel** name in column 1 of §1 and §2 is wrapped in single backticks so the regex `\| \`[a-z0-9-]+\` \|` matches every event-channel row machine-grep-style. This grep contract covers §1/§2 only — §2a command rows use **bold** first-column formatting (e.g. `| **solver-cancel-request** |`) and are intentionally NOT matched by the regex; §3 RPC names use snake_case and are also intentionally outside it.

## §1 — Wired channels (production today)

| Channel | Payload | Producer | Consumer | Notes |
|---|---|---|---|---|
| `mesh-update` | `MeshData` (per-entity) | `gui/src-tauri/src/diff.rs::delta_to_events` | `bridge.ts::onMeshUpdate` | Per-entity delta; convertRawMesh on TS side |
| `mesh-removed` | `String` (entity_path) | `delta_to_events` | `onMeshRemoved` | |
| `value-update` | `ValueData` | `delta_to_events` | `onValueUpdate` | |
| `value-removed` | `String` (cell_id) | `delta_to_events` | `onValueRemoved` | |
| `constraint-update` | `ConstraintData` | `delta_to_events` | `onConstraintUpdate` | |
| `constraint-removed` | `String` (node_id) | `delta_to_events` | `onConstraintRemoved` | |
| `tessellation-diagnostics` | `Vec<DiagnosticInfo>` (full list) | `delta_to_events` | `onTessellationDiagnostics` | Full-snapshot semantics |
| `compile-diagnostics` | `Vec<DiagnosticInfo>` (full list) | `delta_to_events` | `onCompileDiagnostics` | Full-snapshot semantics |
| `evaluation-status` | `{phase: String, progress: Option<f32>}` | `main.rs::emit_status` | `onEvaluationStatus` | RAII IdleGuard emits `idle` on Drop |
| `kernel-status` | `KernelStatus {available, message}` | `main.rs` Tauri `setup()` | `onKernelStatus` | One-shot at startup |
| `diagnostics` | `{uri, diagnostics}` (LSP-shaped) | `main.rs::TauriNotificationSink` | `onDiagnostics` | LSP-routed |
| `file-changed` | `FileData {path, content}` | `main.rs::create_watcher` | `onFileChanged` | File-watcher-driven |
| `file-removed` | `{path: String}` | `main.rs::create_watcher` | `bridge.ts::onFileRemoved` | File-watcher-driven; signals file deleted on disk |
| `focus-entity` | `String` (entity_path) | `focus_entity` command + MCP `focus_entity` tool | `onFocusEntity` | Bidirectional (UI ↔ MCP) |
| `navigate-to-source` | `{file, line, column, end_line, end_column}` | MCP `navigate_to_source` tool | `onNavigateToSource` | MCP-driven |
| `serialization-error` | `SerializationError` | `diff.rs::push_serialized_event` | `onSerializationError` | Replaces a payload that failed to serialize |
| `claude-text-delta` | `{id, content}` | `claude_bridge.rs::outbound_to_event` | `subscribeToClaudeEvents` | Channel name constructed at `claude_bridge.rs::outbound_to_event` (lines 1096, 1100, …); emitted at dynamic `app.emit(&event_name, …)` closure site in `main.rs` (line 438) — closure passed as `event_emitter` arg to `spawn_sidecar_impl`. Literal-string grep lands in `claude_bridge.rs`; `app.emit(…)` grep lands in `main.rs`. |
| `claude-thinking-delta` | `{id, content}` | same | same | same |
| `claude-tool-call` | `{id, tool_use_id, tool_name, tool_input}` | same | same | same |
| `claude-tool-result` | `{id, tool_name, result}` | same | same | same |
| `claude-done` | `{id}` | same | same | same |
| `claude-error` | `{id, message}` | same | same | same |
| `claude-notice` | `{id, code, message}` | same | same | same |
| `claude-ready` | `()` | same | same | same |
| `claude-permission-request` | `{id, request_id, tool_name, tool_input}` | same | same | same |
| `claude-sidecar-crashed` | `{reason: String}` | `claude_bridge.rs` `on_exit` hook | `subscribeToSidecarCrashed` | |
| `debug-request` | (variant; see `debug.rs`) | `debug.rs::emit` | `gui/src` debug-bridge | REIFY_DEBUG=1 only; internal Tauri-event-routed RPC pattern |

## §2 — Channels this PRD adds (WIRED via GR-016 task δ / task 3539)

| Channel | Payload | Producer | Consumer | Upstream prereq | Owning slice | Spec |
|---|---|---|---|---|---|---|
| `auto-resolve-start` | `()` | `gui/src-tauri/src/engine.rs::emit_auto_resolve_if_any` (called from `EngineSession::{load_from_source, set_parameter, update_source}`) | `bridge.ts::onAutoResolveStart` → `AutoResolvePanel` | C-05 fix-now (param-x-auto wired into compile pipeline) | Phase 2 (proof slice) | [`auto-resolve-start.md`](gui-event-channels/auto-resolve-start.md) |
| `auto-resolve-iteration` | `AutoResolveIteration {iteration: u32, parameters: Map<String, AutoResolveParameterValue>, constraints: Map<String, AutoResolveConstraintProgress>, driving_metric?: String, driving_metric_value?: f64}` | same | `bridge.ts::onAutoResolveIteration` → AutoResolvePanel chart | same | Phase 2 | [`auto-resolve-iteration.md`](gui-event-channels/auto-resolve-iteration.md) |
| `auto-resolve-complete` | `()` | same | `bridge.ts::onAutoResolveComplete` | same | Phase 2 | [`auto-resolve-complete.md`](gui-event-channels/auto-resolve-complete.md) |
| `warm-pool-event` | `WarmPoolEvent {kind: 'evicted'\|'donated', size_bytes: u64, node_id: String}` | `gui/src-tauri/src/main.rs::TauriWarmPoolEventEmitter::emit` via `event_bus::emit_typed`; drained by `EngineSession::drain_and_emit_warm_pool_events` after each engine call (wired by task 3541) | `bridge.ts::onWarmPoolEvent` → `WarmPoolDebugPanel` (REIFY_DEBUG=1 only; `gui/src/debug/WarmPoolDebugPanel.tsx`) | warm-state-eviction M-010 (drainer wiring subsumed) | Phase 3 | [`warm-pool-event.md`](gui-event-channels/warm-pool-event.md) |
| `solver-progress` | `{solver_kind: String, iter: u32, residual: f64, eta_ms: Option<u64>}` | `reify-solver-elastic` `cg_loop` iteration-end callback (kernel seam wired by task 3543); engine-boundary `app.emit` call is a follow-on stub | `bridge.ts::onSolverProgress` → `SolverProgressOverlay` (props-driven; engineStore subscription wiring is a follow-on) | task 2923 (FEA progressive framework); task 2965 (overlay component) | Phase 3 | [`solver-progress.md`](gui-event-channels/solver-progress.md) |
| `fea-case-changed` | `{active_case_id: String, available_cases: Vec<String>}` | `gui/src-tauri/src/engine.rs::EngineSession::emit_fea_case_if_any` via `main.rs::TauriFeaCaseEmitter` | `bridge.ts::onFeaCaseChanged` → `FeaCasePickerDropdown` | task 3026 (multi-load case engine wiring; emitter scaffolded by task 3545) | Phase 3 | [`fea-case-changed.md`](gui-event-channels/fea-case-changed.md) |
| `mode-shape-frame` | `{mode_index: u8, phase: f32, displaced_positions: Vec<f32>, eigenvalue: Option<f64>}` (eigenvalue absent on base frame, present on each peak frame) | buckling solver post-process animation feed | (new) `BucklingPanel` animator | `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι (GR-024); eigenvalue field added by task 4072 | **ACTIVE** — owned by `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι (3458) (GR-024 / Phase 9: backend emitter + BucklingPanel animator) |

### §2a — Tauri commands (frontend → backend; not events; lint-exempt from Phase 5 script)

These are Tauri **commands**, not fire-and-forget events. Listed here because they were scoped alongside the Phase 3 channels above. The **bold** first-column formatting (rather than backticks) keeps these rows outside the `\| \`[a-z0-9-]+\` \|` regex contract mechanically — the Phase 5 lint script needs no special-case handling for this section. To find a command's invoke site, grep for the command name as a string in the frontend source rather than using the event-channel regex.

| Command | Payload | Direction | Backend handler | Upstream prereq | Owning slice |
|---|---|---|---|---|---|
| **solver-cancel-request** | `{solver_kind: String, run_id: String}` | frontend → backend (`cancel_solve` Tauri command; `bridge.ts::cancelSolve()`) | `gui/src-tauri/src/commands.rs::cancel_solve_impl` — reads `AppState::pending_solve_cancel`, calls `.cancel()` on the `CancellationHandle` if present, clears the slot (PRD §11 Q2); engine-side handle publishing is a follow-on task | task 2923 (FEA progressive framework); task 2965 (overlay component) | Phase 3 (wired by task 3543) |

## §3 — Debug-MCP RPCs (not fire-and-forget events; snake_case names; outside §1/§2 kebab-case grep contract)

| RPC | Request shape | Response shape | Producer | Consumer | Upstream prereq |
|---|---|---|---|---|---|
| `morph_stats` | `()` or `{body_id: String}` | `MorphStats {morph_count: u32, remesh_count: u32, last_rejection_reason: Option<String>, …}` | `reify-mesh-morph` runtime stats accessor | `mcp__reify-debug__morph_stats` (new tool registered in `gui/src-tauri/src/debug.rs` registry) | task 2949 (depends on 2948 depends on 2947) |

## §4 — Out-of-scope: payload extensions to existing channels

The following are documented for inventory completeness; they are **not new channels** and are owned by their citing PRDs as ordinary kernel/IPC-types work:

| Extension | Existing channel | Owning PRD |
|---|---|---|
| `MeshData.scalar_channels` adds `vonMises_top`/`vonMises_mid`/`vonMises_bottom` keys | `mesh-update` | `structural-analysis-shells.md` (T18–T20) |
| `MeshData.element_kind` per-element tagging for mixed shell/tet bodies | `mesh-update` | `structural-analysis-shells.md` |
| `MeshData.region_tags` from `segmentation::SegmentationResult` | `mesh-update` | `structural-analysis-shells.md` (T20) |
| `MeshData.vector_channels` for shell-normal arrow overlay + rigid-body-mode arrows | `mesh-update` | `structural-analysis-shells.md` + `fea-gui-rendering.md` (M-014) |
| `MeshData.displaced_positions` populated under real load | `mesh-update` | FEA stack (task 2924) |
| `ValueData.case_id` discriminator for multi-case ElasticResult | `value-update` | `multi-load-case-fea.md` (M-016) |
| `MeshData.thickness` per-vertex channel for thickness heat-map | `mesh-update` | `varying-thickness-shells.md` (v0.5+) |

## §5 — Out-of-scope: pure-frontend state

Documented to explicitly head off mis-categorization as a backend-emitter gap:

- Display-mode toggle (`mid/extruded/both`) — Solid store in `gui/src/stores/`; persistence via existing `read_view_sidecar`/`write_view_sidecar` commands.
- Top/mid/bottom stress-channel toggle — same.
- Probe popup three-stacked-card UI — pure-frontend rendering over `MeshData.scalar_channels` once §2.4 keys land.
- Per-document persistence of GUI preferences — existing view-sidecar surface.

## How to extend this inventory

To add a new channel:

1. Follow the naming/payload convention in [`docs/prds/v0_3/gui-event-channel-inventory.md`](prds/v0_3/gui-event-channel-inventory.md) §3. Channel names are kebab-case ASCII; payloads cross a JSON boundary and must be `Serialize`/`Deserialize` on the Rust side with a matching TypeScript interface.
2. Add a row to the appropriate section above (§1 if wiring an existing fiction channel, or a new §2-style section if filing a new PRD-sponsored channel) and add the matching row to PRD §2 — **both files change in the same commit**.
3. When the Phase 5 lint script (PRD §9 task μ) lands, it will validate that every `app.emit("name", …)` call site in the Tauri source has a corresponding row here. Until then, grep for `app.emit(` in `gui/src-tauri/` and manually verify coverage.
