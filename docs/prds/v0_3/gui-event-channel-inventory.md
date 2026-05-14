# GUI Event Channel Inventory

Status: contract + inventory (resolves cluster C-13 / GR-016 per `docs/architecture-audit/gap-register.md`). Authored 2026-05-12 in interactive session. Awaits Leo approval before queueing tasks.

## §0 — Purpose

This document is the **canonical inventory** of Tauri-side event channels that the Reify GUI subscribes to, plus the **convention** that governs adding new channels. It exists to resolve the C-13 failure mode: the audit found 6+ PRDs whose frontend listeners shipped with bridge-code comments like "backend event source is wired in a later task" while no Tauri-side emitter was ever filed.

This PRD owns:
1. The inventory document (this file's §2 table + per-channel specs in `docs/gui-event-channels/*.md`).
2. The convention rules (§3 naming/payload, §4 versioning, §5 error semantics, §6 test discipline, §7 subscription pattern).
3. The convention's shared infrastructure (§3.4 typed-emit Rust helper, §3.5 frontend `listen<T>` + `validatePayload` discipline, §6.3 mock-emitter test utility).
4. **All currently-absent backend emitter wiring** for true C-13 channels — the per-channel emitter task lives in this PRD's decomposition, gated on its upstream data source via real `add_dependency` edges set at decompose time as soon as the prereq task ID is known (per memory `preferences_cross_prd_deps_real_edges`, reversed 2026-05-12 — the orchestrator scheduler reads dep edges only, not metadata).

This PRD does **not** own:
- Payload-shape extensions to existing channels (e.g., adding `top/mid/bottom` stress keys to `mesh-update.scalar_channels`) — those stay with their citing PRDs as ordinary kernel/IPC-types work.
- Pure-frontend UI state (display-mode toggle, per-channel selector, document-local preferences) — those are panel/store work owned by the citing PRDs.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (memory `preferences_implementation_chain_naming`) — is what this contract is designed to prevent for the GUI/backend event boundary. Resolution mode is approach **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline.

## §1 — GR-016 chain analysis + audit correction

The gap-register's GR-016 row currently cites seven PRDs. On Phase-3-author-time tracing, one of those (PNv2 M-018, the Manifold `KernelAttributeHook` MeshGL walk) is not a GUI event channel at all — it is a backend-only producer stub already separately tracked under **GR-004** with disposition "Ownership → persistent-naming-v2." The chain from PNv2 M-018 to user-observable GUI behaviour has **at least three** broken backend links before any GUI event channel question arises:

1. **Link A — Manifold MeshGL walk** (PNv2 M-018 / cluster C-39 / GR-004). `KernelAttributeHook::propagate_attributes` returns `Ok(Discarded)` + `tracing::warn!(reason="task_9_pending")` at `crates/reify-kernel-manifold/src/kernel.rs:26-35`. **Backend producer.**
2. **Link B — Selector vocabulary v2 dispatch** (PNv2 M-019 / cluster C-10 / GR-013). 22+ `pub fn`s in `reify-eval/src/selector_vocabulary_v2.rs` not registered in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` (`reify-compiler/src/units.rs:166-186`). **Eval dispatch.**
3. **Link C — v0.1 selector dispatch arms** (PNv2 M-020 / task 2699 reopen). 11 names (`edges`, `faces`, `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `center_of_mass`, `moment_of_inertia`, `adjacent_faces`, `shared_edges`) not present in `try_eval_topology_selector` (`geometry_ops.rs:1701-1706`). **Eval dispatch.**
4. **Link D — GUI surfacing.** Once A–C land, the existing `mesh-update` channel + selection-state delta propagation handle re-render. **No dedicated event channel needed.**

This PRD therefore excludes PNv2 M-018 from the inventory and files a companion-correction task (§9 Phase 4, task λ) to strike the citation from GR-016's evidence row. The effective C-13 citing-PRD count is **6** (`fea-gui-rendering`, `fea-gui-rendering-shells`, `multi-load-case-fea`, `mesh-morphing`, `structural-stability-buckling`, `warm-state-eviction`), not 7.

## §2 — Channel inventory

The canonical machine-grep-friendly form lives at `docs/gui-event-channels.md` (filed in §9 task α). This section is the cross-referenced human-readable form.

### §2.1 — Wired channels (production today)

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
| `focus-entity` | `String` (entity_path) | `focus_entity` command + MCP `focus_entity` tool | `onFocusEntity` | Bidirectional (UI ↔ MCP) |
| `navigate-to-source` | `{file, line, column, end_line, end_column}` | MCP `navigate_to_source` tool | `onNavigateToSource` | MCP-driven |
| `serialization-error` | `SerializationError` | `diff.rs::push_serialized_event` | `onSerializationError` | Replaces a payload that failed to serialize |
| `claude-text-delta` | `{id, content}` | `claude_bridge.rs::spawn_sidecar_impl` | `subscribeToClaudeEvents` | Sidecar message stream |
| `claude-thinking-delta` | `{id, content}` | same | same | |
| `claude-tool-call` | `{id, tool_use_id, tool_name, tool_input}` | same | same | |
| `claude-tool-result` | `{id, tool_name, result}` | same | same | |
| `claude-done` | `{id}` | same | same | |
| `claude-error` | `{id, message}` | same | same | |
| `claude-notice` | `{id, code, message}` | same | same | |
| `claude-ready` | `()` | same | same | |
| `claude-permission-request` | `{id, request_id, tool_name, tool_input}` | same | same | |
| `claude-sidecar-crashed` | `{reason: String}` | `claude_bridge.rs` `on_exit` hook | `subscribeToSidecarCrashed` | |
| `debug-request` | (variant; see `debug.rs`) | `debug.rs::emit` | `gui/src` debug-bridge | REIFY_DEBUG=1 only; internal Tauri-event-routed RPC pattern |

### §2.2 — Channels this PRD adds (FICTION → WIRED via this PRD's decomposition)

| Channel | Payload (proposed) | Producer (proposed) | Consumer (already shipped) | Upstream prereq | Owning slice |
|---|---|---|---|---|---|
| `auto-resolve-start` | `()` | `reify-eval` auto-resolve orchestrator entry | `bridge.ts::onAutoResolveStart` → `AutoResolvePanel` | C-05 fix-now (param-x-auto wired into compile pipeline) | Phase 2 (proof slice) |
| `auto-resolve-iteration` | `AutoResolveIteration {iteration: u32, parameters: Map<String,f64>, constraints: Map<String,f64>}` | same | `bridge.ts::onAutoResolveIteration` → AutoResolvePanel chart | same | Phase 2 |
| `auto-resolve-complete` | `()` | same | `bridge.ts::onAutoResolveComplete` | same | Phase 2 |
| `warm-pool-event` | `WarmPoolEvent {kind: 'evicted'\|'donated', size_bytes: u64, node_id: String}` | `reify-eval` `WarmStatePool::drain_events()` → journal translator at eval boundary | (new) `WarmPoolDebugPanel` in `gui/src/debug/` | warm-state-eviction M-010 (drainer wiring) | Phase 3 |
| `solver-progress` | `{solver_kind: String, iter: u32, residual: f64, eta_ms: Option<u64>}` | `reify-solver-elastic` CG callback at iteration boundary | (new) `SolverProgressOverlay` | task 2923 (FEA progressive framework); task 2965 (overlay component) | Phase 3 |
| `solver-cancel-request` | `{solver_kind: String, run_id: String}` | (frontend → backend) `cancel_solve` Tauri command, NOT an event — listed for documentation | `reify-solver-elastic` cancellation handle | tasks above | Phase 3 |
| `fea-case-changed` | `{active_case_id: String, available_cases: Vec<String>}` | `reify-eval` multi-case ElasticResult dispatch at case-switch | (new) `FeaCasePickerDropdown` | task 3026 (multi-load case engine wiring) | Phase 3 |
| `mode-shape-frame` | `{mode_index: u8, phase: f32, displaced_positions: Vec<f32>}` | buckling solver post-process animation feed | (new) `BucklingPanel` animator | `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι (GR-024) | **ACTIVE** — owned by `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι (GR-024 / Phase 9: backend emitter + BucklingPanel animator) |

### §2.3 — Debug-MCP RPCs (not fire-and-forget events; convention applies)

| RPC | Request shape | Response shape | Producer | Consumer | Upstream prereq |
|---|---|---|---|---|---|
| `morph_stats` | `()` or `{body_id: String}` | `MorphStats {morph_count: u32, remesh_count: u32, last_rejection_reason: Option<String>, …}` | `reify-mesh-morph` runtime stats accessor | `mcp__reify-debug__morph_stats` (new tool registered in `gui/src-tauri/src/debug.rs` registry) | task 2949 (depends on 2948 depends on 2947) |

### §2.4 — Out-of-scope: payload extensions to existing channels

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

### §2.5 — Out-of-scope: pure-frontend state

Documented to explicitly head off mis-categorization as a backend-emitter gap:

- Display-mode toggle (`mid/extruded/both`) — Solid store in `gui/src/stores/`; persistence via existing `read_view_sidecar`/`write_view_sidecar` commands.
- Top/mid/bottom stress-channel toggle — same.
- Probe popup three-stacked-card UI — pure-frontend rendering over `MeshData.scalar_channels` once §2.4 keys land.
- Per-document persistence of GUI preferences — existing view-sidecar surface.

## §3 — Naming + payload convention

### §3.1 — Naming

- Event names are **kebab-case** ASCII, lower-cased, hyphen-separated.
- One channel per logical event family. Lifecycle trios (start / iteration / complete) get three sibling channels.
- The name lives in **two locations** that MUST be kept in lockstep: the Rust `app.emit("name", …)` call and the TypeScript `listen<T>("name", …)` call. The inventory document (`docs/gui-event-channels.md`) names the channel once; both sides reference it.

### §3.2 — Payload shape

- Rust-side: a `#[derive(Serialize, Deserialize, Debug, Clone)]` struct or enum in `reify-gui::types` (the existing module for shared IPC types). Hand-written `serde_json::json!` literals are permitted only for inherently variant-shaped payloads (LSP diagnostics, MCP `debug-request`).
- TypeScript-side: an interface in `gui/src/types.ts` matching the Rust struct field-for-field. **Field names match exactly** (no case-renaming via `#[serde(rename_all)]` — verbosity over disguise).
- Optional fields use `Option<T>` (Rust) / `T | undefined` (TS); both serialize-skip `None`/undefined.
- No payload may carry a closure, lifetime, or non-`Serialize` type. Payloads cross a process boundary even when in-process (Tauri's event bus is JSON-serialized).

### §3.3 — Versioning

- **No version field by default.** Existing channels do not carry one; adding it everywhere would mass-rewrite stable code for hypothetical future flexibility.
- **Shape changes** to an existing payload are landed in **lockstep Rust+TS commits**, with both sides updated atomically. The convention forbids landing a Rust-side breaking change without the TS-side counterpart in the same task.
- **A `version: u32` field is added per-channel ONLY when** (a) a payload migration must support old + new shape simultaneously across releases, OR (b) the channel is consumed by external tooling (none today; this is forward-looking). When added, the field is the first key; consumers dispatch on it.

### §3.4 — Rust-side typed-emit helper

A `reify-gui::event_bus` module adds:

```rust
pub fn emit_typed<T: Serialize>(
    app: &tauri::AppHandle,
    channel: &str,
    payload: &T,
) -> Result<(), tauri::Error> {
    app.emit(channel, payload)
}
```

This is intentionally thin. Its value-add is (a) a single call site to grep for when surveying emit calls, (b) a place to land future cross-cutting behaviour (telemetry, debug-build assertions, channel-name validation against an in-process registry built from the inventory). Phase 1 implementation is the trivial wrapper; later phases may extend it without touching call sites.

Hand-call to `app.emit()` is permitted for variant-shaped payloads (LSP diagnostics, MCP-routed events).

### §3.5 — Frontend listen + validatePayload discipline

Frontend subscribers continue the existing `bridge.ts` pattern: each channel gets a named `on<Name>(callback): Promise<UnlistenFn>` wrapper. The wrapper:

1. Calls `listen<PayloadType>(channelName, cb)` from `@tauri-apps/api/event`.
2. **For hand-shaped payloads** (variant LSP shapes, MCP-routed events): runs `validatePayload(name, event.payload, REQUIRED_KEYS_ARRAY)` and drops malformed payloads with a `console.warn`.
3. **For typed-serde payloads** (most channels): trusts the type at compile time; runtime malformation is treated as a contract violation handled per §5.

The arrays of required keys (`KEYS_ID_CONTENT`, etc.) live hoisted at module level in `bridge.ts` to avoid per-call allocations; see the existing `KEYS_*` constants for the pattern.

## §4 — Versioning + migration policy

Covered in §3.3 above. Reiteration: lockstep commits are the default; per-channel `version: u32` is added only when a migration spans releases. No central registry of channel versions today; the inventory document is the registry of names.

## §5 — Error semantics

### §5.1 — Missing emitter (frontend listener subscribes but no backend emits)

Current behaviour (preserved as convention):

- `bridge.ts::subscribeToEvents` uses `Promise.allSettled` over the listener registration array. A missing channel name is not actually surfaceable at registration time (Tauri's `listen` always succeeds; absent emit calls are silent), so the failure mode here is **dead silence**, not an error.
- The convention's defense against silent-dead-channel drift is **CI gates** (§6) requiring a roundtrip test for every entry in the inventory.

### §5.2 — Malformed payload (emit fires; consumer cannot parse)

Per Leo's resolution: **hard-fail in debug builds, console.warn in release.**

- Frontend `validatePayload()` returns `null` on shape mismatch.
  - In `cfg(debug_assertions)` builds (development): the caller `throw`s with a descriptive error, crashing the panel. Forces a developer-loop fix before merge.
  - In release builds: `console.warn(`[<channel>] malformed payload; dropping event`, payload)`, drop the event, continue. User-visible degradation is "the panel didn't update" rather than "the GUI crashed."
- Typed-serde payloads: malformation here means the Rust producer changed shape without updating TS. The TypeScript `listen<T>` cast is unchecked at runtime; downstream NPE crashes the panel in development naturally. CI catches via §6.

### §5.3 — Producer-side emit failure

`app.emit()` returns `Result<(), tauri::Error>`. Convention: log via `tracing::warn!` and continue. Emit failures are rare (closed window, etc.) and not user-actionable; don't fail the surrounding operation.

## §6 — Test discipline

Every new channel filed under this PRD's decomposition (§9) ships with:

### §6.1 — Rust-side roundtrip test

`crates/reify-gui-tests/` or sibling: build the payload, serialize via `serde_json::to_value`, assert the JSON shape matches a frozen-fixture snapshot. Catches accidental field renames / additions.

### §6.2 — TypeScript-side shape test

`gui/src/__tests__/bridge/<channel>.test.ts`: construct a representative payload object, pass to the `on<Name>` wrapper via a stubbed `listen` (see §6.3), assert the callback receives correctly-typed data. For hand-shaped payloads, also pass malformed shapes and assert `validatePayload`-driven dropping (release-mode behaviour) and throwing (debug-mode behaviour).

### §6.3 — Mock-emitter test utility

A new `gui/src/__tests__/test_utils/mockEvents.ts` exports:

```typescript
export function mockTauriEvent<T>(channel: string): {
  emit: (payload: T) => void;
  reset: () => void;
};
```

Stubs `@tauri-apps/api/event::listen` via vitest module mock; tests call `mockTauriEvent('foo').emit(...)` to deliver synthetic events to the system under test. Convention: every consumer test uses this utility.

### §6.4 — Cross-language schema test (optional, channel-specific)

For high-stakes channels (auto-resolve, solver-progress), an integration test in `gui/test/` runs a real Tauri test harness, emits from Rust, asserts the TypeScript subscriber receives the expected shape. Slow; opt-in per channel. The Phase 1 helpers explicitly support this pattern but do not mandate it for every channel.

### §6.5 — CI gates

- `cargo test --workspace` runs Rust roundtrip tests.
- `npm test` runs TS shape tests.
- `npm run typecheck` catches Rust-TS interface drift via the shared `types.ts` definitions.
- No new CI infrastructure required.

## §7 — Subscription registration pattern

Existing pattern (preserved):

1. `bridge.ts` exports one `on<Name>(callback): Promise<UnlistenFn>` wrapper per channel.
2. Feature-specific stores or panels (`engineStore.ts`, `AutoResolvePanel.tsx`, etc.) consume the wrappers and own the unlisten lifecycle.
3. Multi-channel batch registration uses the rollback pattern from `subscribeToClaudeEvents` (sequential `listen` calls with rollback on any failure).

Convention: **panel-local subscription** is the default for panels with cohesive event sets (Claude bridge, FEA-mode toolbar). **Global subscription via `engineStore.subscribeToEvents`** is the default for cross-cutting state-sync events (mesh-update, value-update, etc.). The split is pragmatic, not architectural.

## §8 — Boundary-test sketch (B+H)

Per `references/gates.md` §G5, the H-component requires test scenarios facing **both** producer (Rust) and consumer (TypeScript) sides.

### §8.1 — Producer-side (Rust)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Typed emit roundtrip.** New channel emits a representative payload. | A `tauri::AppHandle` test harness. | `serde_json::to_value(&payload)` matches frozen JSON snapshot byte-for-byte. |
| **Emit during evaluation.** Engine-side trampoline emits `auto-resolve-iteration` mid-solve. | Mock auto-resolve orchestrator running. | Event count = iteration count; each event's `iteration` field strictly increasing. |
| **Emit under cancellation.** Solver cancellation observed mid-emit. | Concurrent `cancel()` while `solver-progress` events fire. | No emit-after-cancel observed past the poll budget (per ComputeNode contract §2 SLA). |
| **Closed window.** App handle's window torn down before emit. | `AppHandle` reference still valid; window closed. | `emit()` returns `Err(tauri::Error::WebviewWindowNotFound)`; `tracing::warn!` logged; no panic. |

### §8.2 — Consumer-side (TypeScript)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Typed listen happy-path.** Mock-emit a valid payload. | `mockTauriEvent` configured. | Subscriber callback fires once with payload deserialized into expected TS type. |
| **Malformed payload in debug build.** Mock-emit `{}` against a channel requiring fields. | Dev-mode build (`import.meta.env.DEV`). | `validatePayload` throws; surrounding panel renders an error boundary; test asserts the throw. |
| **Malformed payload in release build.** Same. | Release build. | `validatePayload` returns null; `console.warn` logged with channel name + payload; callback not invoked. |
| **Listen rollback.** Subscribe to 3 channels; the second `listen()` throws. | Stub `listen` to throw on the second call. | First listener's unlisten is invoked before the throw propagates; no listener remains active. |
| **Missing-emitter degradation.** Subscribe to a channel; never emit. | `mockTauriEvent` registered but never `.emit()`-ed. | No callback fires; no error; test passes (covers the "wired ahead of time" pattern's degraded state). |

The boundary tests above are the **observable signal** for the convention-helpers task (§9 task β); they're the proof that the convention infrastructure works before any channel's emitter is wired.

## §9 — Decomposition DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)**.

### Phase 1 — Foundation (convention + inventory)

- **Task α** — Commit canonical inventory at `docs/gui-event-channels.md`.
  - **Observable signal:** File committed; cross-references from each citing PRD's §"Cross-PRD relationship" point at this file (executed by Phase 4 task μ).
  - **Prereqs:** None.
  - **Crates touched:** docs only.

- **Task β** — Convention helpers landed: `reify-gui::event_bus::emit_typed`, frontend `validatePayload`-driven `on<Name>` template, `mockTauriEvent` test utility.
  - **Observable signal:** §8.1 and §8.2 boundary tests pass (a "convention smoke" test fixture, not tied to any production channel yet). `cargo test -p reify-gui-tests convention_smoke` and `npm test bridge/convention_smoke` both pass.
  - **Prereqs:** α (the inventory is the spec).
  - **Crates touched:** `gui/src-tauri/src/event_bus.rs` (new), `gui/src/bridge.ts` (add validatePayload-typed listener template), `gui/src/__tests__/test_utils/mockEvents.ts` (new).
  - **Note:** This is the C-as-integration-gate paired leaf — Phase 2's auto-resolve slice is the consumer that validates α + β.

- **Task γ** — Per-channel spec template at `docs/gui-event-channels/_template.md`.
  - **Observable signal:** File committed; Phase 3 channel specs (one per channel) instantiate this template.
  - **Prereqs:** α.
  - **Crates touched:** docs only.

### Phase 2 — Proof-of-convention vertical slice: auto-resolve trio

- **Task δ** — Emit `auto-resolve-start` / `auto-resolve-iteration` / `auto-resolve-complete` at the param-x-auto orchestrator's entry/loop/exit. Author per-channel specs at `docs/gui-event-channels/auto-resolve-*.md`.
  - **Observable signal:** Open `examples/auto_resolve_bracket.ri` (or sibling) in dev-mode GUI with `param x = auto`; `AutoResolvePanel` renders live iteration count + sparkline chart; `auto-resolve-complete` fires once at solver exit. Debug-MCP `wait_for_event("auto-resolve-complete")` returns within a bounded timeout for a known-converging fixture. The `bridge.ts:580-583` "wired in a later task" comment is removed.
  - **Prereqs:** α, β, γ. Plus C-05 fix-now (param-x-auto orchestrator wired into compile pipeline — listed in `phase-3-files-synthesis.md` as fix-now, may already be partially landed).
  - **Crates touched:** `reify-eval` (auto-resolve orchestrator emit calls), `reify-gui::types` (`AutoResolveIteration` typed struct, already exists in TS — add the Rust counterpart), `gui/src-tauri/src/main.rs` (thread the AppHandle into the orchestrator if not already routed).

### Phase 3 — Per-channel emitter slices (each gated on its upstream prereq)

- **Task ε** — Emit `warm-pool-event` via `WarmStatePool::drain_events()` translator at eval boundary; new `WarmPoolDebugPanel`.
  - **Observable signal:** Open a `.ri` file with `--warm-state-budget=50MB` in dev-mode GUI; edit-loop triggers evictions; debug panel shows evict/donate counts updating live; matches `EventJournal::count_evicted()` accessor return.
  - **Prereqs:** α, β, γ. Plus warm-state-eviction M-010 (drain_events wired at eval boundary — listed fix-now per audit; this PRD's task ε can subsume it as a paired step OR list it as a separate upstream task in the citing PRD's decomposition. Recommended: subsume here since the surfacing channel is centralized here, and the drain-translator is small enough to fit one task with the emitter wire).
  - **Crates touched:** `reify-eval` (engine boundary translator, `journal.rs` extension), `gui/src-tauri/src/event_bus.rs` (no change beyond convention), `gui/src/debug/WarmPoolDebugPanel.tsx` (new).

- **Task ζ** — Emit `solver-progress` from FEA solver iteration callback; new `SolverProgressOverlay`.
  - **Observable signal:** Open a large FEA model (≥10K dofs) in dev-mode GUI; trigger `solve_elastic_static`; overlay renders live CG iter + residual + ETA; cancel button cancels the solve within 2× poll budget (per ComputeNode contract §2 SLA).
  - **Prereqs:** α, β, γ. Plus task 2923 (FEA progressive framework — currently pending) AND task 2965 (overlay component — currently pending). Cross-PRD dep edges set via `add_dependency` at decompose time per `preferences_cross_prd_deps_real_edges`.
  - **Crates touched:** `reify-solver-elastic` (iteration callback), `gui/src/panels/SolverProgressOverlay.tsx` (new), `gui/src-tauri/src/commands.rs` (`cancel_solve` command exposing CancellationHandle).

- **Task η** — Emit `fea-case-changed` for multi-load case discrimination; new `FeaCasePickerDropdown`.
  - **Observable signal:** Open a multi-load `.ri` fixture (e.g. `examples/m6/multi_load_bracket.ri` once it exists) in dev-mode GUI; dropdown lists case names; selecting a case swaps the viewport contour to that case's `ElasticResult`; debug-MCP `dom_query('FeaCasePickerDropdown')` returns the active case ID.
  - **Prereqs:** α, β, γ. Plus task 3026 (multi-load case engine wiring — currently pending) AND multi-load-case-fea M-016 upstream data wiring.
  - **Crates touched:** `reify-eval` (case-dispatch emit at engine boundary), `gui/src/panels/FeaCasePickerDropdown.tsx` (new), `gui/src/stores/feaModeStore.ts` (add `activeCaseId` field), `gui/src/types.ts` (`MultiCaseResult` TS counterpart).

- **Task θ** — Register `morph_stats` debug-MCP RPC.
  - **Observable signal:** Open a parametric `.ri` file with mesh-morph eligibility in REIFY_DEBUG=1 GUI; MCP `mcp__reify-debug__morph_stats` returns counts matching expected morph activity after a parameter sweep.
  - **Prereqs:** α, β, γ. Plus task 2949 (depends on 2948 depends on 2947 — mesh-morph engine wiring chain).
  - **Crates touched:** `gui/src-tauri/src/debug.rs` (new tool registration), `reify-mesh-morph` (stats accessor — currently absent per audit M-013), `gui/src-tauri/src/mcp_context.rs` (tool wiring).

### Phase 4 — Companion correction tasks

- **Task ι** — Strike PNv2 M-018 citation from `docs/architecture-audit/gap-register.md` GR-016 evidence row; add a one-line note pointing to GR-004 for the actual chain.
  - **Observable signal:** `git diff docs/architecture-audit/gap-register.md` shows the strike + the note; the PRD count in GR-016 §"Cited by PRDs" reduces to 6.
  - **Prereqs:** None (independent doc edit).
  - **Crates touched:** docs only.

- **Task κ** — Update each of the 6 citing PRDs' cross-PRD section to reference this inventory as the seam-owner for backend event channels.
  - **Observable signal:** `git diff` covers `docs/prds/v0_3/fea-gui-rendering.md`, `docs/prds/v0_4/fea-gui-rendering-shells.md`, `docs/prds/v0_3/multi-load-case-fea.md`, `docs/prds/v0_3/mesh-morphing.md`, `docs/prds/v0_5/structural-stability-buckling.md` (deferred — note as forward-reference), `docs/prds/v0_3/warm-state-eviction.md`. Each PRD's cross-PRD section gets a row pointing at this PRD as the owner of its backend event channel.
  - **Prereqs:** α (this PRD's path must exist for citation).
  - **Crates touched:** docs only.

- **Task λ (SUPERSEDED by `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι)** — `mode-shape-frame` channel + producer.
  - GR-024's task ι (Phase 9 — backend `mode-shape-frame` emitter + frontend `BucklingPanel.tsx` animator) absorbed this bookmark when the buckling-eigensolver PRD was decomposed. See `docs/architecture-audit/gr024-buckling-eigensolver-filing-log.md` for the filing trail.
  - **Observable signal:** (superseded — see `docs/prds/v0_5/buckling-eigensolver.md` §13 task ι for the active observable signal).
  - **Prereqs:** (superseded).

### Phase 5 — Convention enforcement (optional; lower priority)

- **Task μ** — Add a contributor-doc note (or git pre-commit lint script) reminding new emitter call sites must update the inventory.
  - **Observable signal:** `docs/contributor/event-channels.md` exists; references `docs/gui-event-channels.md` as the source of truth; optionally a small `scripts/check_event_inventory.sh` that greps for new `app.emit("foo", …)` call sites and warns if `foo` isn't in the inventory.
  - **Prereqs:** α.
  - **Crates touched:** docs + (optionally) scripts.

### Dependency view

```
α ──┬──→ β ──┬──→ δ (auto-resolve, Phase 2 proof slice)
    │       │
    │       ├──→ ε (warm-pool)        ←── warm-state-eviction M-010
    │       ├──→ ζ (solver-progress)  ←── task 2923, 2965
    │       ├──→ η (fea-case)         ←── task 3026
    │       └──→ θ (morph-stats RPC)  ←── task 2949 → 2948 → 2947
    │
    └──→ γ (template)
    
ι (independent doc edit)
κ (depends on α only)
λ (superseded by buckling-eigensolver.md §13 task ι)
μ (depends on α; optional)
```

## §10 — Out of scope

- **Retroactive convention enforcement on existing wired channels.** The 25 channels in §2.1 already work; the convention applies to NEW channels and channels modified by §2.2 work. A future audit may sweep §2.1 into conformance, but this PRD doesn't.
- **Payload-shape extensions to existing channels** (§2.4). Owned by citing PRDs.
- **Pure-frontend UI state** (§2.5).
- **Cross-window or multi-instance event routing.** Tauri 2 supports multi-window; we have a single-window GUI today. Future-PRD scope.
- **Persistent event log / journal-to-disk.** Future-PRD scope; tangentially related to the warm-state journal which is a different artifact.
- **External-tooling subscription** (third-party listeners outside the Reify GUI). Not a use case today.
- **Mid-flight payload migration.** The lockstep-commit convention precludes this; if a real migration need emerges, §3.3's `version: u32` opt-in is the escape hatch.
- **Resolution of PNv2 M-018 / GR-004 (Manifold MeshGL walk).** Handled by GR-004's owning PRD (persistent-naming-v2). This PRD's task ι only corrects the citation, not the chain.

## §11 — Open questions (surfaced but not decided in this session)

1. **WarmPool surfacing: separate channel vs. journal-event channel.** `EventJournal` already carries Evicted/Donated event kinds (memory `journal.rs:48-63`); the buffered-events pattern in `warm_pool.rs` is a sibling not-yet-translated stream. Open: is the GUI surface a new dedicated `warm-pool-event` channel, or does the journal itself get a Tauri subscription that surfaces all journal events generically? **Suggested resolution:** dedicated channel for now (kept narrow); revisit if other journal-event consumers need GUI surfaces — at that point, a generic `engine-journal-event` channel may make sense. Decide during §9 task ε.

2. **`solver-cancel-request` shape: event or command?** Listed in §2.2 as a command for clarity, but Tauri can route cancellation either as a regular `#[tauri::command]` or as a frontend-side `emit` that the backend listens for. **Suggested resolution:** Tauri command (clearer call site; easier to test). Decide during §9 task ζ.

3. **`auto-resolve-iteration` constraint/parameter map shape.** Current frontend type uses `Record<string, number>` for both `parameters` and `constraints`. The audit didn't pin Rust-side shapes. Open: do we keep `Map<String, f64>` or use a typed-key enum for known param/constraint names? **Suggested resolution:** `BTreeMap<String, f64>` for Rust ergonomics; sorted-key serialization for deterministic JSON. Decide during §9 task δ.

4. **CI gate for new emit call sites.** §9 task μ proposes a lint script; this is optional. Open: should we make it mandatory (block CI on unregistered channel names)? **Suggested resolution:** start as a warning; promote to mandatory after one release cycle if drift is observed. Decide during §9 task μ (or skip task μ entirely).

5. **Multi-case ElasticResult Tauri serialization.** Task η depends on `MultiCaseResult` being Tauri-serializable. The multi-load-case-fea PRD's M-017 finding notes the type is currently produced engine-side as a kind-tagged Map (GR-001 dependency). Open: does the η task need a separate "MultiCaseResult IPC type" subtask, or does multi-load-case-fea PRD ship that? **Suggested resolution:** multi-load-case-fea PRD owns the IPC type; task η references it as a prerequisite. Decide during task η decomposition planning.

6. **REIFY_DEBUG-gated channels.** `debug-request` is REIFY_DEBUG=1-gated today. Does `warm-pool-event` follow the same gating (debug-only) or is it always-on? **Suggested resolution:** debug-only initially (it's a debug-panel feed); promote to always-on if a production GUI surface emerges. Decide during §9 task ε.

7. **Tauri 2 Channel<T> API.** Tauri 2 offers a `Channel<T>` type that's an alternative to `emit`/`listen`. It's better-typed but less flexible. Open: should the convention deprecate `emit`/`listen` in favour of `Channel<T>` for new channels? **Suggested resolution:** stick with `emit`/`listen` (matches all existing channels; Channel<T> would force a parallel mechanism). Revisit if Channel<T>'s typing benefits prove decisive. Decide as part of §9 task β if it comes up.
