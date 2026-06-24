# PRD: N-pane viewport — model-driven multi-pane GUI layout

**Milestone:** v0_6 · **Status:** active (design-first) · **Approach:** **B+H** (frontend architecture rewrite + a cross-PRD engine→GUI seam + two-way boundary tests) · **Date:** 2026-06-24

**Umbrella:** task **4291** (`io-display-output-viewport.md` forward-stub). **Batch:** PRD-3 of 3 (PRD-1 `appearance-substrate`, PRD-2 `appearance-viewport-egress`, PRD-3 this). **Owns:** the **pane** half of `DisplayOutput` consumption in the GUI snapshot path; shares the Output-walk seam with PRD-2 (which owns the **style** half).

---

## 1. Goal — what a user observes

Open a `.ri` that contains several `DisplayOutput(subject: …, pane: k)` occurrences in the dev GUI:

- Each subject renders in its **assigned** pane; the viewport shows a **tiled N-pane layout** (2-up, 4-up grid). Subjects sharing a pane index group into the same pane.
- Each pane has **independent camera** (orbit / pan / zoom). The **layout** (pane arrangement + sizes) and **per-pane cameras** persist across reloads via the existing view-sidecar.
- `pane: 0` (the default) behaves as today's single design view (`design-main`) — **back-compat**; the existing `def-preview` viewport continues to work unchanged.

This unblocks **multi-pane comparison** — variant A vs B, full view vs section — model-encoded and version-controlled, replacing the two hardcoded viewports (`design-main`, `def-preview`) the GUI ships today.

## 2. Background — verified substrate

All anchors **re-verified 2026-06-24** (lines drift; prefer the named symbols).

### Frontend (the N-pane rewrite surface)
- **Two fixed viewports, hardcoded.** `defaultViewports()` (`gui/src/stores/viewportStore.ts:68`) literally returns `design-main` + `def-preview`; the test `gui/src/__tests__/viewportStore.test.ts:7` asserts **exactly two**. `ViewportState.type` is the closed union `'design' | 'def-preview'` (`viewportStore.ts:19`).
- **`DualViewport` (218 LOC, `gui/src/viewport/DualViewport.tsx`) is hardwired to two panes**: separate `engineStore` / `defPreviewStore` props (no pane array/loop), a single scalar `splitRatio` (`:114`), two literal `<Viewport viewportId="def-preview"/>` (`:157`) and `="design-main"` (`:192`) instances.
- **`App.tsx`** hardcodes `assignView('design-main', …)` (`:378`) and passes the two stores as literal props to `DualViewport` (`:1692–1710`).
- **`viewportStore` is already keyed by string ID** — `state.viewports: Record<string, ViewportState>` (`viewportStore.ts:34`). Adding panes is just more map entries; the arity is not structural.
- **Per-viewport camera state already exists** — `CameraState` (`viewportStore.ts:8`).
- **Camera persistence already loops over arbitrary viewport count** — `App.tsx:300` iterates `Object.entries(viewportStore.state.viewports)` into a `viewportCameras` record on `PersistentViewState` (`types.ts`). **N-ready today**; the gap is per-pane *layout/size* state (only the scalar `splitRatio` exists).
- **`Viewport` is already parameterized by `viewportId`** (`gui/src/viewport/Viewport.tsx:26`) — needs **no change** for N.
- **Visibility flows as a SEPARATE entity→state map**, not on `MeshData`: `viewStateStore.getAllEffective()` (`App.tsx:508`) → `entityVisibility` prop (`:1708`). This is the precedent the pane map mirrors (ratified §3.2).
- **`MeshData` (TS mirror, `gui/src/types.ts`)** carries no pane/style fields.
- **Other hardcoded-two sites** to relax: `useDefPreviewActivation.ts:111,115` (`def-preview` literals — stay valid, no change needed).

### Backend (the shared Output-walk seam)
- **GUI snapshot path:** `tessellate_snapshot` (`crates/reify-eval/src/engine_build.rs:8199`) → `TessellateResult { meshes: Vec<MeshSurface>, values: ValueMap, … }`; `build_gui_state` (`gui/src-tauri/src/engine.rs:2416`) maps `Vec<MeshSurface>` → `Vec<MeshData>` (`:2576`) and **already holds `result.values`** (`:2531`) — the evaluated `ValueMap` needed to walk Output occurrences.
- **`MeshData` (Rust, `gui/src-tauri/src/types.rs:363`)** has 9 fields, none pane/style. The comment `engine.rs:2574`: *"`MeshData` intentionally stays visibility-free"* — the invariant §3.2 preserves.
- **`MeshSurface` carries `default_visible: bool`** (`crates/reify-eval/src/lib.rs:1160`) — precedent for an engine→GUI display hint, but the pane map is a separate sibling map (not a `MeshSurface`/`MeshData` field — §3.2).
- **`GuiState` (`types.rs:162`)** is the snapshot payload (`meshes`, `values`, `constraints`, `files`, diagnostics, **`tensegrity_wires`**). `tensegrity_wires` is the **exact precedent for the pane map**: a model-level descriptor list extracted from value cells, added as a `#[serde(default)]` field (forward-compat), transported on the existing path — **not** a per-mesh field, **not** a new channel.
- **`DisplayOutput` surface is landed** (tasks 4287/4288, done): `occurrence def DisplayOutput : Output { param subject : Solid; param pane : Int = 0; param style : DisplayStyle = DisplayStyle(); param format : OutputFormat = OutputFormat.Display }` (`stdlib/io.ri:161`). **No `constraint determined(subject)`** (intentional — viewports may show in-progress geometry, `io.ri:156`).
- **`DisplayOutput.pane` is inert today** — no code reads it for effect.
- **The recognize-but-skip is in the HEADLESS CLI export driver, a DIFFERENT consumer.** `build_outputs` → `OutputTarget::DisplayDeferred` → `I_DISPLAY_OUTPUT_DEFERRED` info diagnostic + `continue` (`engine_build.rs:3889`). Headless `reify build` has no viewport — **that skip correctly stays a no-op.** This PRD drives the *GUI snapshot* path, not the CLI.
- **The reusable Output-walk primitives** `build_outputs` uses (`engine_build.rs:3846`): walk `module.templates` → `sub_components`, gate on `EntityKind::Occurrence` + `conforms_to_output(…)` (`tolerance_combine.rs:488`), resolve `ValueCellId` in `values`, read fields off the `Value::StructureInstance` (`instance.fields.get("pane")`). `OutputTarget` enum + `extract_output_export_spec` at `tolerance_combine.rs:522/586`.

### Tooling (the integration-gate verification substrate)
- **`reify-debug` MCP `viewport_state(params)` is already `viewportId`-parameterized** (`gui/src/debug/bridge.ts:374` → `pickViewport(ctx, params)`; tests `debugBridge.test.tsx:1264/1427`). Calling `viewport_state({viewportId: 'pane-1'})` returns that pane's `camera` + `meshCount` + per-mesh `entityPath` list — so per-pane mesh assignment is observable **without an MCP extension**.
- **`store_state()` (`bridge.ts:326`) does NOT yet expose the viewport map** (only engine/editor/selection/claude). Enumerating pane *count* needs a small addition — folded into task α.

## 3. Resolved design decisions (ratified by Leo — not re-litigable)

1. **Model declares logical pane *assignment*; GUI owns physical *layout*.** The `.ri` says which subjects group into which pane index (`DisplayOutput.pane`); the GUI decides tiling/arrangement/sizing and persists it. The model never dictates pixel geometry. (Leo: "model = overridable default", applied to layout.)
2. **Pane routing stays OFF `MeshData`.** Compute an `entity_path → pane` map **separately**, mirroring how visibility is handled (`getAllEffective()`). This keeps the `MeshData`-visibility-free invariant and avoids threading a `pane_id` through all four tessellation paths. The map is a new `#[serde(default)]` field on `GuiState` (precedent: `tensegrity_wires`).
3. **`DualViewport` → `MultiViewport`.** Generalize to an array of pane configs with a grid/tiling layout + per-pane sizing (replacing the single scalar `splitRatio`). `viewportStore` keys are already string IDs; relax `defaultViewports()` and the `type` union.
4. **Back-compat:** `pane: 0` == today's `design-main`; `def-preview` remains a special, non-numbered viewport. A model with no `DisplayOutput` renders exactly as today.
5. **This PRD owns the pane model.** The stub PRD's "contested with in-flight GUI-rendering work" framing is **stale**: investigation found no active PRD owns a multi-pane/viewport model (`fea-gui-rendering.md` explicitly *deferred* multi-result comparison: "DualViewport already exists… mostly state management, not new architecture"). Greenfield and uncontested.
6. **PRD-3 introduces the shared Output-walk; PRD-2 extends it.** (See §6 — the one decision this `/prd` session resolved, settling the reciprocal-ownership question the skeleton handed off.)

## 4. Sketch of approach

A single backend walk produces a pane-assignment map; the frontend consumes it to drive an N-pane tiled layout.

1. **Engine (γ):** in `build_gui_state`, run a **Display-occurrence walk** (reusing `build_outputs`' four-gate enumeration) over `result.values`, producing `Vec<DisplayDirective> { subject: entity_path, pane: i32 }`. Surface it as a `#[serde(default)]` `GuiState.display_panes` field (Rust + TS lockstep) on the existing snapshot path — no new channel (GR-016 §2.4). **This walk is the seam PRD-2 extends** to also emit a style map.
2. **Store (α):** generalize `viewportStore` to hold N panes (relax `defaultViewports()` + the `type` union; add per-pane layout/size state), update the exactly-two test, and expose the viewport map in `store_state`.
3. **Layout (β):** `DualViewport` → `MultiViewport` — render N panes from a config array in a grid/tiling layout with per-pane resize.
4. **Wiring (δ):** `App.tsx` loops over panes, joins `display_panes` to meshes (by `entity_path`), and routes each subject's mesh into its assigned pane.
5. **Persistence (ε):** extend the view-sidecar for per-pane layout/size + fold state (cameras already round-trip).
6. **Integration gate (ζ):** a committed `.ri` opened in the dev GUI tiles two subjects across two panes; layout + per-pane cameras persist; verified via `reify-debug` MCP.

The engine-side `build_gui_state` extension is **not a new engine §3 seam** — `engine-integration-norm.md §3.10` explicitly **excludes** the GUI/Tauri IPC seam, handing it to `gui-event-channel-inventory.md` (GR-016). The payload extension (`GuiState.display_panes`) is "ordinary kernel/IPC-types work" owned by this PRD per GR-016 §2.4 — the same resolution PRD-2 uses for its appearance fields.

## 5. Pre-conditions for activating

- **Parent `io-export-import-completion.md` landed** — `DisplayOutput`/`DisplayStyle` surface + the `I_DISPLAY_OUTPUT_DEFERRED` recognition point (tasks 4287/4288, **done**). ✅
- **No grammar work** — `DisplayOutput(subject:…, pane: k)` already parses and type-checks; `examples/io_formats.ri:19` is the committed CI proof. G3 grammar: **N/A** (no novel syntax).
- **No dependency on PRD-1 or PRD-2** — PRD-3 is independent (owns the walk; PRD-2 depends on PRD-3, not vice-versa). This PRD can land first.

## 6. Cross-PRD relationship (G4)

| Other PRD / seam | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **PRD-2 `appearance-viewport-egress`** | **shares seam** | the `build_gui_state` Display-occurrence walk (γ): this reads `.pane`, PRD-2 reads `.style` — one walk, two maps on `GuiState` | **PRD-3 owns the walk scaffold + `DisplayDirective` struct + `GuiState.display_panes`; PRD-2 extends the same walk to add `display_appearance`** | **resolved — PRD-2 → PRD-3(γ) hard dep** |
| `io-display-output-viewport.md` (stub 4291) | activates | this PRD activates the **pane** half; PRD-2 the **style** half; together they meet 4291's preconditions | this (pane) + PRD-2 (style) | 4291 stays the umbrella tracker |
| `gui-event-channel-inventory.md` (GR-016) | extends | `GuiState.display_panes` payload field on the existing snapshot path | **this** owns (per §2.4 payload-extension precedent) | settled (no new channel) |
| `engine-integration-norm.md` (GR-017) | excluded | §3.10 hands the Tauri/GUI seam to GR-016; the `build_gui_state` extension is not a new §3 seam | this (extends existing surface) | settled |
| `fea-gui-rendering.md` (v0.3, deferred) | sibling | the multi-result comparison it deferred ("DualViewport exists, mostly state management") | **this** owns the N-pane model it punted | uncontested (reframe) |
| `gui-diagnostics-panel-docking-and-navigation.md` | sibling pattern | docked-panel/layout + sidecar-persistence + reify-debug-MCP integration-gate precedent | reuse pattern; no shared ownership | noted (ε/ζ follow its shape) |

**Shared-seam resolution (the one open decision this session settled).** Both PRD-2 and PRD-3 extend the `build_gui_state` Output-occurrence walk; the skeleton handed off "who introduces it." **Resolution: PRD-3 owns the walk scaffold.** Rationale: PRD-3 is *independent* (no dep on PRD-1's appearance types) and must be able to land first; if PRD-2 owned the walk, PRD-3 would inherit PRD-2 → PRD-1's appearance chain, defeating its independence. So PRD-3's γ introduces the walk, the `DisplayDirective` struct, and the `GuiState.display_panes` field; **PRD-2's γ adds an additive `style` field to `DisplayDirective` + a `display_appearance` map, depending on PRD-3's γ** (for the walk) **and PRD-1** (for the `Color`/`Finish`/`Appearance` types). No reciprocal ambiguity — both PRDs agree the walk is shared and PRD-3 introduces it. **Companion action:** the concurrently-authored PRD-2 must wire `add_dependency(PRD-2.γ → PRD-3.γ)` at its decompose time; PRD-3's γ task ID is published in this batch's hand-back.

## 7. Contract (B+H) — the engine→GUI seam

The load-bearing seam is the **join between the pane map and the meshes**. Specifying it up front prevents the integration task starving under the narrow-lock orchestrator.

### 7.1 The shared Output-walk (γ owns; PRD-2 extends)

```rust
// gui/src-tauri/src/engine.rs (or a sibling display_routing module).
// Mirrors build_outputs' four-gate enumeration (engine_build.rs:3846) but
// emits a routing map instead of export artifacts. Runs inside build_gui_state,
// over the `values: ValueMap` already in hand (engine.rs:2531).
fn collect_display_routing(module: &CompiledModule, values: &ValueMap) -> Vec<DisplayDirective>;

#[derive(Serialize, Deserialize)]  // mirrored in gui/src/types.ts (GR-016 §3.2 lockstep)
pub struct DisplayDirective {
    /// entity_path of the resolved DisplayOutput.subject — MUST equal the
    /// MeshData.entity_path of that subject's realization (the join key).
    pub subject: String,
    /// DisplayOutput.pane (default 0). pane 0 ⇒ design-main (back-compat).
    pub pane: i32,
    // PRD-2 adds: pub style: Option<DisplayStyleData>,  // same occurrence, same walk
}
```

`GuiState` (`types.rs:162`) gains:
```rust
#[serde(default)]                       // forward-compat, mirroring tensegrity_wires
pub display_panes: Vec<DisplayDirective>,
```

### 7.2 Invariants

1. **Join-key identity.** `DisplayDirective.subject` MUST be the same `entity_path` string used for `MeshData.entity_path` of the subject's realization. If a `DisplayOutput.subject` resolves to no realized mesh, its directive is **dropped** (logged), never dangling. *(This is the seam's single most failure-prone point — the boundary test §8 verifies it both ways.)*
2. **Default pane is back-compat.** `pane: 0` (explicit or defaulted) routes to `design-main`. A module with **no** `DisplayOutput` yields an empty `display_panes` and renders exactly as today.
3. **Many-to-one.** Multiple subjects with the same `pane` index group into one pane (the frontend groups by `pane`).
4. **`def-preview` is orthogonal.** It is never produced by the walk; it stays the special editor-cursor-driven viewport (`useDefPreviewActivation.ts`).
5. **`MeshData` stays pane/style-free** (§3.2). The map is the *only* carrier of pane assignment.
6. **Last-good semantics.** `display_panes` follows `meshes`' existing last-good-on-failure behavior in `build_gui_state` (no panes flicker to empty on a transient eval error).

### 7.3 Frontend pane model (α/β/δ)

- `viewportStore`: `ViewportState.type` gains a `'pane'` variant (numbered model panes) alongside `'design'`/`'def-preview'`; a pane viewport carries its `pane` index. `defaultViewports()` keeps `design-main` + `def-preview` as the two defaults; model panes (index ≥ 1) are added dynamically from `display_panes`.
- `MultiViewport` renders `viewportStore`'s panes in a grid (default tiling heuristic → Open Questions); per-pane sizes replace the scalar `splitRatio`.
- `App.tsx` joins `display_panes` to meshes by `entity_path` and assigns each subject's mesh to its pane's `Viewport`.

## 8. Boundary-test sketch (B+H) — the integration gate's signal

The integration gate ζ is a **single scripted `reify-debug` MCP session** against a committed `.ri`. Each row faces both the producer (engine walk → `display_panes`) and the consumer (frontend tiling). Reuses the existing `gui/test` harness (`open_file`/`wait_for_idle`/`viewport_state`/`store_state`/`screenshot`).

| # | Scenario | Preconditions | Postcondition (asserted) |
|---|---|---|---|
| 1 | Two subjects → two panes | `.ri` with `DisplayOutput(subject:a, pane:0)` + `(subject:b, pane:1)` | `store_state` shows ≥2 viewports incl. a `pane:1`; `viewport_state({viewportId:'pane-1'})` lists subject `b`'s mesh, `'design-main'` lists `a`'s — **join-key holds both ways** (inv. 1) |
| 2 | Pane count observable | scenario-1 scene | `store_state.viewports` enumerates the panes (α's exposure); pane count == 2 (+ def-preview) |
| 3 | Many-to-one grouping | a third `DisplayOutput(subject:c, pane:1)` | `viewport_state({viewportId:'pane-1'})` lists **both** `b` and `c` (inv. 3) |
| 4 | Back-compat / empty | a `.ri` with **no** `DisplayOutput` | `display_panes` empty; layout identical to today's single design view (inv. 2) |
| 5 | Independent cameras | scenario-1 scene | orbit pane-1 via the camera API; `viewport_state` per pane shows divergent cameras (pane-0 unchanged) |
| 6 | Layout + camera persistence | scenario-1 after a camera move + pane resize | reload; `viewport_state` per pane shows restored cameras; pane sizes restored (ε) |
| 7 | Dangling subject dropped | `DisplayOutput(subject:undetermined, pane:2)` whose subject has no realized mesh | no `pane:2` viewport materializes with a phantom mesh; directive dropped + logged (inv. 1) |

## 9. Decomposition plan

B+H shape: foundations (α, γ) → vertical slice (β, δ) → persistence (ε) → integration gate (ζ). Greek labels; real task IDs assigned at decompose.

- **α — `viewportStore` N-generalization + `store_state` exposure.** Relax `defaultViewports()` (keep the two defaults; allow N); add a `'pane'` `type` variant + per-pane layout/size state; update the exactly-two test to assert the two **defaults** while allowing N; expose the viewport map in `store_state`. *Modules:* `gui/src/stores/viewportStore.ts`, `gui/src/__tests__/viewportStore.test.ts`, `gui/src/debug/bridge.ts`. *Signal (observable):* `reify-debug` `store_state` reports the viewport map; vitest asserts N viewports round-trip + the two defaults survive. *Unlocks:* β, δ, ε, ζ.
- **γ — shared engine Output-walk → `GuiState.display_panes` (+ TS mirror).** Add `collect_display_routing` reusing the `build_outputs` four-gate enumeration; surface `display_panes: Vec<DisplayDirective>` (`#[serde(default)]`) on `GuiState`; lockstep TS type. **The seam PRD-2 extends.** *Modules:* `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/types.rs`, `gui/src/types.ts` (+ possibly a reify-eval re-export of the walk primitives). *Signal (observable):* a `.ri` with `DisplayOutput(pane:1)` yields a non-empty `display_panes` whose `subject` matches the rendered mesh's `entity_path` (Rust serde round-trip test + the join surfaced to the frontend). *Unlocks:* δ; **PRD-2.γ depends on this**.
- **β — `DualViewport` → `MultiViewport`.** Grid/tiling layout rendering N panes from a config array, per-pane resize replacing scalar `splitRatio`; the two-default case stays a degenerate grid. *Modules:* `gui/src/viewport/MultiViewport.tsx` (new), `gui/src/viewport/DualViewport.tsx`. *Signal:* renders N panes from a config array (vitest + GUI); the existing dual layout still works. *Unlocks:* ζ. *Depends:* α.
- **δ — `App.tsx` N-pane wiring.** Loop over panes; join `display_panes` to meshes by `entity_path`; route each subject's mesh to its assigned pane; generalize `assignView` off the hardcoded `'design-main'`. *Modules:* `gui/src/App.tsx`. *Signal:* opening a multi-pane `.ri` populates each pane with its assigned subject (`viewport_state` per pane). *Unlocks:* ζ. *Depends:* α, β, γ.
- **ε — N-pane layout persistence.** Extend the view-sidecar for per-pane sizes + fold/arrangement (cameras already round-trip, `App.tsx:300`). *Modules:* `gui/src/App.tsx`, `gui/src/types.ts` (`PersistentViewState`). *Signal:* layout + per-pane camera persist across reload (`viewport_state` after reload). *Unlocks:* ζ. *Depends:* δ.
- **ζ — integration gate (leaf).** Commit a `.ri` (`DisplayOutput(pane:0)` + `(pane:1)` on two subjects); a scripted `reify-debug` MCP session asserts the §8 boundary-test rows (tiling, per-pane mesh assignment, grouping, back-compat, independent cameras, persistence, dangling-drop). *Modules:* `examples/` (the committed `.ri`) + `gui/test/` harness. *Signal:* one CI-able scripted session shows two subjects tiled across two panes with per-pane mesh assignment + persistence — the §8 sketch realized. *Consumer:* end user viewing a multi-pane `.ri`. *Depends:* δ, ε (transitively α, β, γ).

DAG: α, γ are roots → β(α) → δ(α,β,γ) → ε(δ) → **ζ(δ,ε)** is the sole leaf.

## 10. Out of scope (named)

- **Per-mesh appearance/style** — PRD-1 (types/material) + PRD-2 (style egress). This PRD reads only `.pane`.
- **Free-form VSCode-style splitter trees** — start with simple grid/tiling.
- **Independent *documents* per pane** — panes view one model's subjects, not separate files. Cross-window / multi-instance routing.
- **Driving `def-preview` from the model** — it stays editor-cursor-driven.
- **Programmatic camera/section/exploded views from the `.ri`** — future PRD; the per-pane camera stays user-driven here.

## 11. Open questions (tactical — deferred to implementation)

1. **Grid tiling heuristic for N panes.** 2 → side-by-side or stacked? N → `ceil(sqrt(N))` columns? **Suggested:** `ceil(sqrt(N))`-column grid, 2 panes side-by-side. Decide in β; GUI owns layout (§3.1) so any choice stays coherent.
2. **Pane viewport ID scheme.** `'pane-1'`, `'pane-2'`, … keyed off the `pane` index. **Suggested:** `pane-{k}` for `k ≥ 1`; `pane-0` aliases `design-main`. Decide in α (the boundary test §8 assumes `'pane-1'`).
3. **Empty assigned pane.** A `pane: k` whose subject has no realized mesh (inv. 1 drops the directive) — does the pane still appear empty, or collapse? **Suggested:** don't materialize a pane with zero meshes (avoids the #4213 empty-pane confusion). Decide in δ.
4. **Eviction/relayout on edit.** When a `DisplayOutput` is removed from the `.ri`, does its pane disappear on the next snapshot? **Suggested:** yes — `display_panes` is recomputed each snapshot; the frontend reconciles the pane set. Decide in δ.
