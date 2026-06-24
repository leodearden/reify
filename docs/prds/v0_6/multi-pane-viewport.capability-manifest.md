# Capability manifest — `multi-pane-viewport.md`

Mechanizes G3 + G6 per leaf (gates.md → *Capability Manifest*). Each binding is `capability → evidence`
with a verdict. Any `declared-only | test-only | producer-absent | producer-downstream |
producer-extent-short | fixture-ERROR | bound≤floor | rejection-absent` **blocks** the batch. All bindings
below resolve **PASS** or **producer-upstream**. Re-verified 2026-06-24 against `target/release/reify`
+ the source tree (symbol-anchored; line numbers drift — prefer the named symbols).

Evidence commands: grammar/semantic gate = `tree-sitter parse --quiet <fixture>` + `reify check <fixture>`;
wiring greps = paths below. The pane map is a **transport descriptor list** on `GuiState`
(precedent `tensegrity_wires`), **not** a reify-eval result field — so the `Value::Undef` field-population
sentinel does not apply here (no compute result; `DisplayDirective.pane` is the `Int` read off the already-
evaluated `DisplayOutput` instance, `DisplayDirective.subject` is the realization's `entity_path` string).

DAG: `α, γ → β(α) → δ(α,β,γ) → ε(δ) → ζ(δ,ε)`. **ζ is the sole leaf** (integration gate). Every NEW
capability is produced **upstream of or by** the task that asserts it — G6 branch 3 (dependency-correctness)
is the only branch that fires, and it holds.

---

## α — `viewportStore` N-generalization + `store_state` viewport-map exposure + exactly-two→exactly-N test

| Capability | Evidence | Verdict |
|---|---|---|
| `viewportStore` is keyed by string ID (arity is not structural — N panes = more map entries) | `grep:gui/src/stores/viewportStore.ts:34` `viewports: Record<string, ViewportState>` | **PASS (wired on main)** |
| per-viewport `CameraState` already exists | `grep:gui/src/stores/viewportStore.ts:8` `interface CameraState` | **PASS (wired on main)** |
| `'pane'` variant added to the closed `ViewportState.type` union (additive to `'design' \| 'def-preview'`) + per-pane layout/size state replacing scalar `splitRatio` | `grep:viewportStore.ts:19` (union), `:36` (scalar `splitRatio`) — NEW additive variant + state, producer:task-α (this leaf) | **PASS (α-owned, additive)** |
| `store_state` exposes the viewport map (currently engine/editor/selection/claude only) | `grep:gui/src/debug/bridge.ts:326` `store_state` — the viewport map (`ctx.viewports`) is already in hand at `pickViewport` (`bridge.ts:90`); α surfaces it — producer:task-α | **PASS (α-owned, substrate present)** |
| exactly-two test relaxed to assert the two **defaults** survive while allowing N | `grep:gui/src/__tests__/viewportStore.test.ts:7` (`'has exactly two viewport entries'`, `toHaveLength(2)` at `:11`) — producer:task-α updates it | **PASS (α-owned)** |

*Signal:* `reify-debug` `store_state` reports the viewport map; vitest asserts N viewports round-trip + the two
defaults survive. No novel substrate.

## γ — shared engine Output-walk → `GuiState.display_panes` (+ TS mirror) — **the seam PRD-2 extends**

| Capability | Evidence | Verdict |
|---|---|---|
| `build_outputs`' four-gate enumeration is reusable (walk `module.templates`→`sub_components`, gate `EntityKind::Occurrence` + `conforms_to_output`, resolve `ValueCellId` in `values`, read `instance.fields`) | `grep:crates/reify-eval/src/engine_build.rs:3765` `build_outputs` + `:3889` `OutputTarget::DisplayDeferred` (recognizes `DisplayOutput`); `grep:crates/reify-eval/src/tolerance_combine.rs:488` `conforms_to_output`, `:522` `OutputTarget`, `:586` `extract_output_export_spec` | **PASS (wired on main)** |
| `build_gui_state` already holds `result.values` (the `ValueMap` walk input) | `grep:gui/src-tauri/src/engine.rs:2416` `build_gui_state` → `tessellate_snapshot(c)` at `:2533` | **PASS (wired on main)** |
| `DisplayOutput.pane` is a readable `Int` field off the `Value::StructureInstance` | `grep:crates/reify-compiler/stdlib/io.ri:161` `occurrence def DisplayOutput : Output`, `:163` `param pane : Int = 0`; field-read pattern is `extract_output_export_spec` (`tolerance_combine.rs:586`) | **PASS (wired on main)** |
| `collect_display_routing(module, values) → Vec<DisplayDirective>` (reuses the four-gate enumeration, emits a routing map not export artifacts) | NEW Rust over the substrate above — producer:task-γ (this leaf) | **producer-upstream (γ-owned)** |
| `DisplayDirective { subject: String, pane: i32 }` serde struct (PRD-2 adds `style`) | NEW Rust serde-derive — producer:task-γ | **producer-upstream (γ-owned)** |
| `GuiState.display_panes: Vec<DisplayDirective>` `#[serde(default)]` (model-level descriptor list on the existing snapshot path, NOT a per-`MeshData` field) | NEW field; **exact precedent** `grep:gui/src-tauri/src/types.rs:203` `tensegrity_wires: Vec<TensegrityWireData>` (`#[serde(default)]`, model-level list extracted from value cells) on `GuiState` (`:162`) — producer:task-γ | **producer-upstream (γ-owned, `tensegrity_wires` precedent)** |
| TS mirror in `gui/src/types.ts` (GR-016 §3.2 lockstep) | NEW lockstep type — producer:task-γ; `display_panes` absent today (grep returns nothing) | **producer-upstream (γ-owned)** |

*Signal:* a `.ri` with `DisplayOutput(pane:1)` yields a non-empty `display_panes` whose `subject` equals the
rendered mesh's `entity_path` (Rust serde round-trip test + the join surfaced). **Join-key identity** (inv. 1)
— `DisplayDirective.subject` is the same `entity_path` string carried by `MeshData` (`grep:gui/src/types.ts:8`
`entity_path: string`); both produced from the same realization. Not a numeric/exactness assertion — string
identity, verifiable at γ. **No new engine §3 seam** — `engine-integration-norm.md §3.10` excludes the
GUI/Tauri seam; this is GR-016 §2.4 payload-extension, owned by this PRD.

## β — `DualViewport` → `MultiViewport` (depends α)

| Capability | Evidence | Verdict |
|---|---|---|
| `Viewport` is already parameterized by `viewportId` (no change for N) | `grep:gui/src/viewport/Viewport.tsx:26` `viewportId: string` | **PASS (wired on main)** |
| `MultiViewport.tsx` grid/tiling layout rendering N panes from a config array, per-pane resize replacing scalar `splitRatio` | `DualViewport.tsx` (218 LOC, hardwired-to-two) is the refactor base (`grep:gui/src/viewport/DualViewport.tsx` present; `MultiViewport.tsx` absent today) — producer:task-β (this leaf) | **producer-upstream (β-owned)** |
| per-pane layout/size state consumed from `viewportStore` | producer:task-α (upstream) | **producer-upstream (α)** |

*Signal:* renders N panes from a config array (vitest + GUI); the existing dual layout still works as a
degenerate two-pane grid.

## δ — `App.tsx` N-pane wiring (depends α, β, γ)

| Capability | Evidence | Verdict |
|---|---|---|
| `display_panes` present on the `GuiState` snapshot | producer:task-γ (upstream) | **producer-upstream (γ)** |
| join `display_panes`↔meshes by `entity_path` (the load-bearing seam, inv. 1) | NEW wiring; join key `DisplayDirective.subject == MeshData.entity_path` (`grep:gui/src/types.ts:8`) — producer:task-δ, γ upstream | **producer-upstream (δ-owned, γ upstream)** |
| generalize `assignView` off the hardcoded `'design-main'` | `grep:gui/src/App.tsx:378` `assignView('design-main', …)` — producer:task-δ | **producer-upstream (δ-owned)** |
| `MultiViewport` available to mount N panes | producer:task-β (upstream) | **producer-upstream (β)** |
| N-pane `viewportStore` available | producer:task-α (upstream) | **producer-upstream (α)** |

*Signal:* opening a multi-pane `.ri` populates each pane with its assigned subject (`viewport_state` per pane).

## ε — N-pane layout persistence (depends δ)

| Capability | Evidence | Verdict |
|---|---|---|
| per-viewport cameras already round-trip over arbitrary N | `grep:gui/src/App.tsx:300/:907` `Object.entries(viewportStore.state.viewports)` → `viewportCameras`; restore loop `grep:App.tsx:849` `persisted.viewportCameras` | **PASS (wired on main, N-ready)** |
| `PersistentViewState` view-sidecar struct exists | `grep:gui/src/types.ts:560` `PersistentViewState`, `:573` `viewportCameras` | **PASS (wired on main)** |
| per-pane sizes + fold/arrangement persistence (the gap — only scalar `splitRatio` exists today) | NEW field on `PersistentViewState` extending the existing sidecar — producer:task-ε | **producer-upstream (ε-owned)** |

*Signal:* layout + per-pane camera persist across reload (`viewport_state` after reload).

## ζ — integration gate (LEAF) — scripted `reify-debug` MCP session against a committed `.ri` (depends δ, ε)

All ζ-required capabilities trace **upstream** (α, β, γ, δ, ε) — G6 branch 3 holds.

| Capability | Evidence | Verdict |
|---|---|---|
| `viewport_state({viewportId:'pane-1'})` returns per-pane camera + `meshCount` + per-mesh `entityPath` list (no MCP extension) | `grep:gui/src/debug/bridge.ts:374` `viewport_state` → `pickViewport` (`:90`) | **PASS (wired on main)** |
| `store_state` enumerates the viewport map → pane count (scenarios 1–2) | producer:task-α (upstream) | **producer-upstream (α)** |
| tiled N-pane layout renders (scenario 1) | producer:task-β (upstream) | **producer-upstream (β)** |
| `display_panes` produced by the engine walk; dangling subject **dropped + logged** (inv. 1, scenario 7) | producer:task-γ (walk drops directives whose subject has no realized mesh) + δ (frontend doesn't materialize an empty pane) — upstream | **producer-upstream (γ, δ)** |
| per-pane mesh assignment, join holds both ways (scenarios 1, 3) | producer:task-δ (upstream) | **producer-upstream (δ)** |
| layout + per-pane camera persistence across reload (scenario 6) | producer:task-ε (upstream) | **producer-upstream (ε)** |
| committed `.ri` fixture (`DisplayOutput(pane:0)` + `(pane:1)` on two subjects, + many-to-one + default-pane) parses + checks | grammar-fixture `docs/prds/v0_6/fixtures/multi_pane_surface.ri` → `tree-sitter parse --quiet` exit 0, **0 ERROR**; `reify check` exit 0, "All constraints satisfied." (this session). Committed CI proof `examples/io_formats.ri:19/28` (`DisplayOutput(…, pane: 1, …)` + default-pane) — tasks 4284/4287/4288 done | **PASS (grammar N/A — no novel syntax)** |
| `gui/test` harness (`open_file`/`wait_for_idle`/`viewport_state`/`store_state`/`screenshot`) drives the running GUI | `grep:gui/test/visual/` harness present (precedent: GUI-diagnostics ε #4404 used the same harness for its boundary-test gate) | **PASS (wired on main)** |
| no capability owned **downstream** of ζ | ζ is the sole leaf; nothing depends on it (DAG terminus) | **PASS (DAG-direction: no inversion)** |

*Signal:* one CI-able scripted `reify-debug` MCP session shows two subjects tiled across two panes with
per-pane mesh assignment + grouping + back-compat + independent cameras + persistence + dangling-drop — the
§8 boundary-test sketch realized. *Consumer:* end user viewing a multi-pane `.ri`.

---

**Numeric floor:** N/A — no FEA/solver/accuracy bound is asserted. Pane routing is exact integer assignment
(`DisplayOutput.pane: Int`) + exact string-key join (`entity_path`); no AABB/round-trip/numeric tolerance.
G6 branches 1 (numeric bound) and 2 (closed-form exactness) do not fire.

**G6 branch 3 (dependency-correctness):** every NEW capability is produced upstream of or by the task that
asserts it. ζ (sole leaf) asserts nothing its dependency set (α, β, γ, δ, ε) cannot produce — the inversion
that sank `multi-kernel-phase-3.md §8 ε` (esc-3436-210) does not recur here.

**Net:** 0 FAIL bindings. The NEW capabilities (`collect_display_routing` + `DisplayDirective` +
`GuiState.display_panes`, γ; `'pane'` type variant + `store_state` exposure, α; `MultiViewport`, β; the
`entity_path` join + `assignView` generalization, δ; per-pane layout persistence, ε) are each produced
**upstream of or by** the leaf that asserts them; every consumed substrate (the `build_outputs` four-gate
enumeration, `build_gui_state` holding `result.values`, the `DisplayOutput` surface, `viewport_state`/
`store_state`/`pickViewport`, `Viewport(viewportId)`, the camera round-trip, `PersistentViewState`,
`tensegrity_wires` as the `#[serde(default)]` precedent) is wired on main.
