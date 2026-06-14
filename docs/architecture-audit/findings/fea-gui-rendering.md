# Audit: GUI Rendering of FEA Results

**PRD path:** `docs/prds/v0_3/fea-gui-rendering.md`
**Auditor:** audit-fea-gui-rendering
**Date:** 2026-05-12
**Mechanism count:** 22
**Gap count:** 14 (8 WIRED, 4 PARTIAL, 8 FICTION, 0 TODO, 2 DRIFT, 0 ORPHAN — gap count = total − WIRED − ORPHAN = 14)

## Top concerns

- **`screenshot_window` MCP tool is FICTION but task 2954 is marked done via a docs-only commit** (`9db99f222e`). The visual-regression harness this PRD treats as a hard prerequisite cannot capture full-window state. No code in `gui/src-tauri/src/debug_server.rs` or `gui/src/debug/bridge.ts` references `screenshot_window`, and the PRD-prescribed `html-to-image` dependency is not in `gui/package.json`.
- **No FEA→IPC bridge exists.** `gui/src-tauri/src/engine.rs:949-950` constructs every `MeshData` with `scalar_channels: HashMap::new()` and `displaced_positions: None`. There is no code path that consumes an `ElasticResult` and writes von-Mises/displacement-magnitude into those slots. All the frontend FEA-mode plumbing (colormap baking, deformation, FEA toolbar, auto-enable) is WIRED but has no real producer — fed only by test fixtures.
- **Several "done" GUI tasks ship pure UI with no producer wired.** Task 2967 (auto-resolve panel) is marked done, and `gui/src/bridge.ts:580` even self-documents: "The backend event source is wired in a later task". The Tauri side has no emitter of `auto-resolve-start` / `auto-resolve-iteration` / `auto-resolve-complete` events. Same pattern as 2954: task marked done while only one side of a contract exists.
- **Visual-regression harness exists but is not CI-wired** (no `.github/workflows/`) and ships only ONE scenario (`m5_geometry_flange`), not the four FEA scenes the PRD test plan demands. Task 2968 (FEA baselines) is correctly pending; the harness's CI integration step is unowned.

## Mechanisms

### M-001: `screenshot_window` MCP tool (full-window DOM capture)

- **State:** FICTION
- **Failure mode:** F1 (task marked done; mechanism absent)
- **Evidence:** PRD §"Prerequisite: visual regression infrastructure" item 1; task 2954 `done_provenance.commit = 9db99f222e` (a docs-only commit that updates the PRD prose, no implementation); `grep -rn screenshot_window /home/leo/src/reify/gui` returns zero hits in code (only the task's own description); `gui/package.json` has no `html-to-image` dependency; `gui/src/viewport/scene.ts` has no `preserveDrawingBuffer: true` (per task 2954 step 2).
- **Blocks:** 2968 (FEA visual baselines), full-window probe-popup/diagnostic-overlay regression coverage
- **Note:** Task description prescribes a concrete frontend-mediated implementation; only the prescription landed. The PRD's `WebviewWindow::capture()` note (line 49) was patched in the docs-only commit but no follow-on code commit ever shipped.

### M-002: `wait_for_idle` MCP tool

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src-tauri/src/debug_server.rs:217-228` (tool def), `:302` (dispatch), `:559-589` (handler with frontend round-trip and timeout); `gui/src/debug/bridge.ts:343-ff` (frontend handler). Tests at `:719-724`.
- **Blocks:** —
- **Note:** Frontend-mediated; takes optional `timeout_ms`. Used by `gui/test/visual/run.ts`.

### M-003: `set_camera` MCP tool

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src-tauri/src/debug_server.rs:154-188` (tool def with position/target/up/zoom schema), `gui/src/debug/bridge.ts:255-ff` (handler). Tests at `:650-655`.
- **Blocks:** —

### M-004: `set_test_mode` MCP tool (CSS-animation freeze)

- **State:** PARTIAL
- **Failure mode:** F3 (scope narrower than PRD implies)
- **Evidence:** `gui/src-tauri/src/debug_server.rs:204-216` (tool def description explicitly says "Does NOT pause JS-driven animations or the Three.js render loop"); `gui/src/debug/bridge.ts:310-ff` (handler). PRD §Prerequisite item 4 says "freeze any animated UI (spinners, pulsing toasts) during a shot so animation phase doesn't introduce diffs" — the CSS-only scope handles the spinner/toast case but not e.g. animated WebGL helpers.
- **Blocks:** —
- **Note:** Acceptable narrowing for current scenes; flag in case future FEA overlay uses JS-driven animation (arrow pulses, etc.).

### M-005: Visual-regression harness (Node-side diff driver)

- **State:** PARTIAL
- **Failure mode:** F3 (deliverable shipped without CI wiring and without FEA scenes)
- **Evidence:** `gui/test/visual/{run,rpc,diff,paths}.ts` exists; `gui/package.json` has `"test:visual": "tsx test/visual/run.ts"`; `gui/test/screenshots/` is empty; SCENARIOS list in `run.ts:51-58` contains only `m5_geometry_flange`; no `.github/workflows/` directory in repo (`find /home/leo/src/reify/.github` returns no entries). `run.ts:7-9` notes "CI integration: invoke this script in a CI job after the `cargo build` step, once `.github/workflows/` exists in the repo (task TBD)".
- **Blocks:** every visual-regression-gated FEA assertion in the PRD test plan
- **Note:** Harness primitives + SSIM diff + baseline-update env switch all present. Missing: CI workflow file + FEA scene fixtures + their baselines.

### M-006: Per-vertex scalar attribute IPC pipeline (`scalar_channels`)

- **State:** PARTIAL
- **Failure mode:** F2 (frontend wired, backend half wired — schema present but never populated)
- **Evidence:** `gui/src-tauri/src/types.rs:167-260` defines `MeshData.scalar_channels: HashMap<String, Vec<f32>>` with validation; `gui/src/types.ts:32-84` mirrors it; `gui/src-tauri/src/engine.rs:949` explicitly constructs every emitted `MeshData` with `scalar_channels: HashMap::new()`. No code path queries an `ElasticResult` for vertex stress.
- **Blocks:** 2962 (FEA GUI contour wiring; pending)
- **Note:** Task 2959 is marked done — accurately for the schema/IPC layer, but a reader of the done flag would assume end-to-end works. Producer side is unowned within 2959's scope.

### M-007: Packed `displaced_positions` channel (deformed shape)

- **State:** PARTIAL
- **Failure mode:** F2 (same shape as M-006: schema/render wired, producer absent)
- **Evidence:** `gui/src-tauri/src/types.rs:204` declares `displaced_positions: Option<Vec<f32>>`; `gui/src/viewport/meshManager.ts:128-129` consumes it; `gui/src-tauri/src/engine.rs:950` always emits `None`. Task 2963 marked done with a large `metadata.files` list including kernel files, but the engine.rs emit-site is still hardcoded `None`.
- **Blocks:** end-to-end deformed-shape regression
- **Note:** 2963's done state plausibly covers UI + meshManager.setDeformation correctness on test data; live wiring from `ElasticResult.displacement` to `displaced_positions` is not in code.

### M-008: Colormap utility (viridis, magma, engineering-rainbow)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src/viewport/colormaps/{viridis,magma,rainbow}.ts` (256-entry LUTs); `gui/src/viewport/colormap.ts` (Palette, RangeSpec, `bakeColours`); task 2960 done commit `2bc872b0d6`.
- **Blocks:** —

### M-009: FEA-mode store + toolbar (channel/palette/range/warp UI)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src/stores/feaModeStore.ts` (state + setters + `tryAutoEnable`); `gui/src/viewport/FeaModeToolbar.tsx`; `gui/src/__tests__/feaModeStore.test.ts`; task 2961 done commit `2f345a9951`. `Viewport.tsx:216` invokes `feaStore.tryAutoEnable(channel)` when a scalar channel arrives — auto-promote semantics per PRD §Resolved are present.
- **Blocks:** —
- **Note:** "Lock Current" handler is implemented per task 2961 description. (RESOLVED 2026-06-11: a stale cross-reference here to an empty TODO at `Viewport.tsx:384-386` was removed — no such TODO exists.)

### M-010: Stress contour rendering — end-to-end wiring

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 1 + §Resolved "rendering pipeline" assume an end-to-end path; only IPC schema + UI/baker exist)
- **Evidence:** Task 2962 pending (depends on 2920/2924/2961). `ElasticResult` typed contract (#2911) and result-interpolation (#2920) are done in the kernel; engine integration (#2924) is pending and depends on 14 prerequisites including 3426/3379/3383/3384 (compute-node-infrastructure subset). Without 2924, no ElasticResult ever reaches the GUI. Even if 2924 lands, 2962 must do the sample-stress-at-surface-vertex translation that doesn't exist yet (`engine.rs:949` proves it).
- **Blocks:** entire FEA visualization headline deliverable
- **Note:** This is the largest concrete chain. Transitively gated by GR-001 only via 2924's input-instantiation needs (Material struct ctors etc.); the GUI-side wire itself doesn't touch struct ctors.

### M-011: Deformed-shape view (warp slider + undeformed overlay)

- **State:** PARTIAL
- **Failure mode:** F2 (consumer-side wired; producer absent)
- **Evidence:** Task 2963 done commit `a315768d5e`. `Viewport.tsx:176-184` track-then-act effect calls `meshManager.setDeformation({ warpFactor })`; meshManager handles undeformed-overlay translucency; FEA toolbar exposes the warp slider. But `engine.rs:950` always emits `displaced_positions: None`, so under real load the deformed branch never engages.
- **Blocks:** end-to-end correctness; same root cause as M-007.
- **Note:** Reasonable to mark 2963 "done" for the work-stream it owns, but the gap-from-done is real for an outside reader.

### M-012: Probe-point query system (raycast → values → pinned probes)

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 3 + §Resolved "probe persistence" call for a full system; no code)
- **Evidence:** Task 2964 pending. `grep -rn "ProbeSystem\|probe_at" /home/leo/src/reify/gui/src` returns no hits. No `probe_at` Tauri command in `crates/` or `gui/src-tauri/src/`. The pinning identity `(entity_path, face_id, barycentric_uv)` (PRD §Resolved) has no representation in any store.
- **Blocks:** 2964 + the "pressurised cylinder + probe popup" visual baseline (2968)
- **Note:** Greyed-stale-marker logic and re-pin affordance are entirely unimplemented.

### M-013: In-flight solver progress overlay (CG iter / residual / ETA / cancel)

- **State:** FICTION
- **Failure mode:** F1 (no overlay component; no event channel)
- **Evidence:** Task 2965 pending (depends on 2923). `grep -rn "SolverProgressOverlay\|cancel_solve" /home/leo/src/reify/gui` returns zero hits. Solver progressive framework (#2923) is a prerequisite; cancellation tokens exist in `reify-runtime/concurrent.rs` but no Tauri command wraps them for the GUI's "cancel" button.
- **Blocks:** designer experience during long FEA solves
- **Note:** Coarse-vs-fine intermediate rendering policy in PRD §Resolved would need backend-driven `result_partial` event channel; none exists.

### M-014: Diagnostic overlay layer (rigid-body-mode arrows, problem-elements, ghost selectors)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Task 2966 pending (depends on FEA kernel diagnostic task #2929). `grep -rn "DiagnosticOverlay\|ArrowHelper\|rigid_body_mode" /home/leo/src/reify/gui/src` returns zero hits. The existing `DiagnosticsPanel` is the generic compile/eval-diagnostics list, NOT the geometry-overlay layer this PRD describes.
- **Blocks:** "unconstrained body" baseline scene (2968)
- **Note:** Confusingly, the existing component name "DiagnosticsPanel" could be misread as already covering this PRD's diagnostic overlay; it does not.

### M-015: Auto-resolve loop progress panel (right-sidebar tab + chart)

- **State:** PARTIAL → DRIFT
- **Failure mode:** F2 (UI wired; producer absent) + F4 (task marked done with explicit "wired in a later task" self-comment)
- **Evidence:** Task 2967 done commit `5bd88b640b`. `gui/src/panels/AutoResolvePanel.tsx` renders iterations + sparkline chart; `engineStore.ts:151-207` exposes `beginAutoResolveLoop` / `applyAutoResolveIteration` / `endAutoResolveLoop`; `bridge.ts:580-615` subscribes to `auto-resolve-start` / `auto-resolve-iteration` / `auto-resolve-complete` Tauri events. **But:** `bridge.ts:580-583` comment: "The backend event source is wired in a later task. The GUI side is ready ahead of time. The engineStore.subscribeToEvents Promise.allSettled pattern means unavailable events degrade to a console.warn rather than a startup crash." No Tauri-side emitter of these events exists (`grep -rn 'auto-resolve-start' /home/leo/src/reify/gui/src-tauri /home/leo/src/reify/crates` returns no producer).
- **Blocks:** "bracket auto-resolve" baseline scene (2968); useful user experience
- **Note:** Bigger pattern: tasks 2954, 2959, 2963, 2967 each have a done-side that ships only one half of a contract. Each is individually defensible; the cumulative effect is a wall of done-marked work whose end-to-end behavior is unverified.

### M-016: `MeshPhongMaterial { vertexColors: true }` switch when scalars present

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src/viewport/meshManager.ts` (material swap conditional on scalar_channels presence); tests in `gui/src/__tests__/viewport/meshManager.test.ts`. PRD §Resolved per-vertex pipeline.
- **Note:** Listed separately because the PRD calls it out as a specific decision.

### M-017: Direct BufferAttribute mutation path (no meshManager full re-sync)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `gui/src/viewport/Viewport.tsx` colorize effect calls `bakeColours` and writes the colour BufferAttribute with `needsUpdate = true`; range / colormap changes do not trigger geometry re-sync (per PRD §Resolved field-rendering perf). Covered by `feaModeStore.test.ts` and viewport tests.

### M-018: Failed-solve viewport state (clear scalars, keep geometry, show diagnostic layer)

- **State:** PARTIAL
- **Failure mode:** F2 (clear-scalars half wired via feaModeStore.setEnabled(false) idiom; diagnostic-layer half absent — see M-014)
- **Evidence:** feaModeStore can disable on demand, reverting material; but no failure-state coordinator triggers it from a solver error. PRD §Resolved failed-solve viewport state.
- **Blocks:** —
- **Note:** Closely coupled to M-014.

### M-019: ElasticResult kernel→engine integration (`@optimized` + ComputeNode)

- **State:** FICTION (for this PRD's purposes)
- **Failure mode:** F1
- **Evidence:** Task 2924 pending; depends on 14 prerequisite tasks including ComputeNode infra (3377-3385) and 3426 (the pending stdlib `fn solve_elastic_static` decl). PRD §"Pre-conditions for activating" line 41 lists 2924 as required. Without it, GUI never sees ElasticResult regardless of GUI-side wiring.
- **Blocks:** M-006/M-007/M-010/M-011/M-012/M-013/M-014 (all downstream)
- **Note:** Cross-PRD breadcrumb: this PRD's headline feature gates on `structural-analysis-fea.md` decomposition #16 + `compute-node-infrastructure.md` Phase 3. GR-001 (struct ctor eval) is a transitive blocker via 3426.

### M-020: Surface mesh existing pipeline already accepts per-vertex scalars

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** PRD §"Pre-conditions for activating" line 44 — "Plumbed by task GUI-1" — refers to 2959 done.

### M-021: DualViewport existing for future multi-result comparison

- **State:** ORPHAN (informational; PRD calls it out-of-scope but as a future hook)
- **Failure mode:** N/A
- **Evidence:** `gui/src/viewport/DualViewport.tsx` exists; tests in `gui/src/__tests__/App.test.tsx` mock it. PRD §Resolved "Multi-result comparison" notes it exists; v0.3 keeps comparison out of scope.
- **Note:** Mentioned to document the PRD's claim accurately. Excluded from gap count.

### M-022: Tauri 2 `WebviewWindow::capture()` API existence

- **State:** DRIFT (PRD ground-truth-correction; was wrong, now corrected in PRD prose but not in implementation)
- **Failure mode:** F5 (PRD originally described a mechanism that doesn't exist in the chosen Tauri version)
- **Evidence:** Commit `9db99f222e` corrected `docs/prds/v0_3/fea-gui-rendering.md:49` from `WebviewWindow::capture()` to `html-to-image` after L1 escalation `esc-2954-99` discovered the API doesn't exist on tauri 2.10.3. PRD line 49 currently reads "Tauri 2 has no native `WebviewWindow::capture()` API" — corrected.
- **Note:** Drift is now between corrected PRD prose and the un-shipped implementation — covered by M-001. Logged separately so Phase 3 can see the "fact-checking the platform" failure shape independently from the "task marked done" failure shape.

## Cross-PRD breadcrumbs

- **`structural-analysis-fea.md`** owns ElasticResult, result interpolation, `@optimized` integration, progressive solve framework — direct prerequisites for almost every gap above (M-006/7/10/11/12/13/14/18/19). The state of 2924 (pending, 14 deps) gates this entire PRD.
- **`compute-node-infrastructure.md`** owns tasks 3377-3385 that 2924 chains through. Has its own audit slot.
- **`multi-load-case-fea.md`** explicitly noted in PRD as future user of probe + overlay design; LoadCase / MultiCaseResult constructors transitively block via GR-001.
- **`mesh-morphing.md`** named in PRD §Relationship as composing target for live rendering; not blocking but a coupling point.
- **`structural-analysis-shells.md`** noted as future extension for shell-element rendering (mid-surface + thickness); not blocking v0.3.
- **`prd-m6-gui.md`** is the field-rendering surface this PRD extends; not audited here.
