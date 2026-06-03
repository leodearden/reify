# PRD: `reify-debug` MCP expansion — agent-grade GUI inspection + interaction surface

**Status:** Draft · **Author session:** 2026-06-03 · **Milestone:** v0_6
**Consumer north-star:** an **agent assessing / debugging the reify-gui UI/UX** — "what is on screen, how is it laid out, how does it respond."
**Substrate under change:** `gui/src-tauri/src/debug_server.rs` (tool-defs + dispatch), `gui/src/debug/bridge.ts` (frontend handlers), `gui/src/debug/types.ts` (ctx shape), `gui/test/visual/run.ts` (e2e harness), plus targeted exposure edits in `gui/src/App.tsx`, `gui/src/stores/`, `gui/src/panels/`, `gui/src/main.tsx`, `gui/src-tauri/capabilities/default.json`.
**Forward stub for the deferred half:** true platform accessibility-tree auditing (§5, deferred tracker AX‑1).

---

## §0 — Scope boundary: agent-grade *inspection + synthetic interaction*, NOT real-input/AX-audit

The expansion has two cleanly separable ambitions. This PRD owns the first; the second is deferred.

| Layer | What | This PRD? |
|---|---|---|
| **Agent inspection + synthetic interaction** — read the DOM/layout/style/diagnostics/console/semantics; drive the UI via *synthetic* DOM/pointer/keyboard events and the live Three.js camera; named-fixture + synthetic-state loaders; LSP probes — all over the existing `:3939/mcp` transport | **YES** |
| **Real OS-input driving + true accessibility-tree auditing** — injecting real OS pointer/keyboard via a CDP/WebDriver layer (needed for CSS `:hover`, native drag-and-drop, OS hit-testing), and a *true* platform AX tree (Chrome `Accessibility.getFullAXTree`-equivalent) | **NO — deferred**, §5 + tracker AX‑1 |

**Why the split.** The webview is **WebKitGTK** on Linux; it does not expose Chrome's CDP `Accessibility` domain, so a *true* AX tree is effectively blocked, and the urgent need (per the design session) is "better agent assessment/debug of UI/UX," not accessibility-conformance auditing. The pragmatic answer for the agent is a **DOM-derived semantic snapshot** (`ui_outline`), shipped here; real AX auditing — which also needs real ARIA in the app first — is a separate forward effort. Likewise, **synthetic events are deterministic and sufficient for agent UI driving** but have documented fidelity gaps (no CSS `:hover`, no native DnD); real-input injection is not worth the CDP/WebDriver dependency for this consumer and is deferred.

Every tool added here is a **frontend handler in `bridge.ts`** (full live DOM + Three.js + Solid-store + CodeMirror access) or a **`run_on_engine` Rust handler** in `debug_server.rs` — the two existing seams. No new transport, no new in-engine kernel seam.

---

## §1 — Consumer (G1)

**Named consumers (all three ratified in the design session):**

1. **Interactive Claude debugging the GUI live** — the primary day-to-day consumer; calls the tools over `:3939/mcp` to inspect and drive a running `reify-gui`.
2. **The `/verify` and `/review` skills** — drive the GUI to confirm a change works / assess UI/UX regressions; this PRD ships a usage recipe wiring the new tools into those skills (P5).
3. **A real-GUI e2e assertion suite** (`gui/test/visual/run.ts`, extended) — the **hard G1 anti-orphan proof**: every tool added here is exercised by at least one scenario that boots the real app and asserts behavior.

Every mechanism names its consumer:

| Mechanism (tool group) | Consumer |
|---|---|
| **Inspection** — `query_selector`/`_all`, `get_layout_metrics` (+overflow/clipped-text), `get_computed_style`, `get_window_state`, `active_element`, `get_diagnostics`, `ui_outline`, `list_console_errors`, `wait_for`/`wait_for_selector` | An agent/`/review` asking "what's on screen, is anything clipped/overflowing, what diagnostics/console-errors are present, did the UI settle" — answered via the MCP, asserted in the e2e suite |
| **Synthetic interaction** — `click_at`/`drag`/`hover`, `focus_element`, `scroll`, `pick_entity_at`, `orbit`/`pan`/`zoom_camera` | An agent/`/verify` driving the UI by coordinate to reproduce a user flow; the 3D camera helpers exercise the real `OrbitControls` bindings |
| **App chrome + layout** — `open_menu`/`menu_state`, `press_tab`/`tab_order`, `resize_panes`, tree-node `expand`/`collapse`, `set_window_size` | An agent testing menus, keyboard focus order, responsive layout, and deterministic viewport sizing for golden screenshots |
| **Fixtures + LSP probes** — named-fixture `load_fixture`, `inject_diagnostics` (synthetic, labelled), `reset_app_state`, `element_screenshot`, `hover_at`/`completion_at`/`definition_at` | An agent reproducing known good/broken/large states and probing the editor's LSP surface (the overlay blesses "LSP behaviour" + "GUI state change observable via debug MCP" as signal types) |
| **Substrate exposure** — `layoutStore` lift, `data-testid`/signal exposure sweep, e2e value-assertion harness, coordinate/transport contract | The interaction + layout tools above; the harness is the shared signal substrate for every leaf |

These are **not in-engine seams** (no kernel module / dispatcher / realization-kind / `KernelAttributeHook`), so the `engine-integration-norm.md` §3 sub-check does not apply. The consumer is the debug-MCP user surface, first-class observable.

---

## §2 — Sketch of approach (the "what changes")

Mechanically minimal because the transport, the frontend handler registry, the engine-locked Rust path, the Three.js scene/camera/`OrbitControls`/raycaster, the CodeMirror view, the Solid stores, the LSP `lsp_request` bridge, and the real-GUI e2e harness **all already exist** (verified — §3). A new tool is: a tool-def + dispatch arm in `debug_server.rs`, and a handler in `bridge.ts` (or a `run_on_engine` closure). The interesting work is the **substrate-exposure prerequisites** and getting the **contracts** right, not the per-tool plumbing.

### 2.1 Shared foundations (P0)

- **Coordinate + transport contract (`τ0`).** One documented convention for every pixel tool: **CSS/logical pixels from the window top-left** (matching `getBoundingClientRect` and the canvas), with `devicePixelRatio` reported by `get_window_state` so callers can map to device pixels. Documents the **synthetic-event fidelity gaps** (synthetic events fire JS handlers but the browser does **not** apply the CSS `:hover` pseudo-class, and there is no native drag-and-drop or OS hit-testing). Ships a **two-way boundary test** (the H component, §G5): a Rust dispatch ↔ frontend-handler round-trip, and a coordinate round-trip (`get_layout_metrics(testId)` bounds → `click_at(center)` → the element's handler fires; `pick_entity_at(x,y)` agrees with the raycast).
- **e2e value-assertion harness (`H0`).** Extend `gui/test/visual/run.ts` (already boots the real GUI and calls `tools/call` over `:3939/mcp` via a generic `rpc<T>()`) with a **value-assertion scenario mode** (call a tool, assert on returned JSON — not only screenshot-diff) and a **named-fixture `.ri` catalogue** (`empty`, `small_cube`, `broken_syntax`, `large_assembly`, `all_severities`). This is the shared signal substrate every tool leaf routes through.
- **`layoutStore` lift (`L0`).** The pane/splitter sizes (`editorWidth`/`sideWidth`/`designTreeHeight`/`propertyHeight`/`constraintHeight`) are **component-local `createSignal`s in `App.tsx`** (verified, §3) — not reachable from the debug ctx. Lift them into a `gui/src/stores/layoutStore.ts` exposed on `window.__REIFY_DEBUG__`, preserving the existing `savedLayout` persistence. Prerequisite for `resize_panes`.
- **testid/exposure sweep (`T0`).** Add stable `data-testid` to menu buttons, splitter handles, panel sections, diagnostic rows, and file tabs; expose the `MenuBar` open-menu signal and the `DesignTree`/`ConstraintPanel` tree-expand state on the debug ctx. Prerequisite for menus, tab-order, and tree expand/collapse. (Query tools with nothing reliably queryable are half-wired orphans — this sweep is the anti-orphan substrate for the chrome layer.)

### 2.2 Tool layers (P1–P4)

- **Inspection (P1)** extends the patterns already in `bridge.ts:dom_query`/`list_elements` (which already call `getBoundingClientRect` + `getComputedStyle` and throw most of it away): return scroll metrics + `scrollWidth>clientWidth` ⇒ **overflow/clipped-text**; raw-CSS `query_selector`; a `get_computed_style` subset; `get_window_state` (size/pos/focused/`devicePixelRatio`); `get_diagnostics` reading the real `engineStore.compileDiagnostics`/`tessellationDiagnostics`; `ui_outline` (DOM-derived semantic text snapshot — roles where present, testid, text, enabled-state); `list_console_errors` (an **early-installed** ring buffer in `main.tsx` capturing `window.onerror`/`unhandledrejection`/`console.error|warn`); and the `wait_for`/`wait_for_selector` generalization of the existing `wait_for_idle`.
- **Synthetic interaction (P2)** dispatches `PointerEvent`/`MouseEvent` via `elementFromPoint` (`click_at`/`drag`/`hover`), `el.focus()` (`focus_element`), DOM/CodeMirror `scroll`; and on the canvas: a **query-only** `pick_entity_at` (raycast → entity path + world point, no selection mutation) and `orbit`/`pan`/`zoom_camera` that **drive the real `OrbitControls` via synthetic input** (so the control bindings are exercised, not just the camera re-set — `set_camera` already covers absolute poses).
- **App chrome + layout (P3)** drives the HTML `MenuBar` (`open_menu`/`menu_state`), walks focus by **driving** (`press_tab` moves focus and reports where it lands — preferred over reimplementing the WHATWG tabindex algorithm, which can diverge), `resize_panes` (via `L0`), tree-node `expand`/`collapse` (via `T0`), and `set_window_size` (`getCurrentWindow().setSize` + a `core:window:allow-set-size` capability grant).
- **Fixtures + LSP (P4)** adds `load_fixture(name)` (resolve a catalogue name → existing `open_file` path), `inject_diagnostics` (**store-level synthetic, explicitly labelled** — its honest signal is the diagnostics *UI rendering* the injected set, observed via `ui_outline`/screenshot, never "the store was set"), `reset_app_state`, `element_screenshot(testId)` (crop the window capture to an element's bounds), and `hover_at`/`completion_at`/`definition_at` over the existing `lsp_request` bridge.

### 2.3 What stays out (the deferred boundary)

Real OS-input injection and a true platform AX tree (§0, §5). Whole-**panel** collapse is *not an existing feature* (only tree-node expand/collapse exists) — building a panel-collapse affordance is out of scope; the tool targets the real tree-node state.

---

## §3 — Pre-conditions / substrate verification (G3 + G6)

Verified 2026-06-03 against the working tree (reads + greps; `bridge.ts` read in full). **There is no novel `.ri` grammar in this PRD** — the fixture `.ri` files use ordinary existing syntax — so the grammar gate is a no-op beyond "fixtures parse clean."

| Capability the tools need | Verdict | Evidence |
|---|---|---|
| Transport: Rust `emit("debug-request")` → `bridge.ts` handler → `invoke("debug_response")`; new tool = tool-def + dispatch arm + handler | **present** | `gui/src/debug/bridge.ts:75-477` (`buildHandlers`, `listen`); `gui/src-tauri/src/debug_server.rs` `tool_defs()`/`dispatch_tool` |
| DOM bounds + computed style already computed (extend for layout-metrics/computed-style/query_selector) | **present** | `bridge.ts:217-218` (`getBoundingClientRect`+`getComputedStyle` in `dom_query`); `:228-242` (`list_elements`) |
| Diagnostics (for `get_diagnostics`) read a **real populated** field | **present** | `engineStore` `compileDiagnostics`+`tessellationDiagnostics` set from `guiState` (`engineStore.ts:113`); typed `DiagnosticInfo{severity,message,code,range}` (`types.ts:174`) |
| Raycaster + `OrbitControls` (pick + camera nudges) | **present** | `viewport/selection.ts` raycast (BVH); `set_camera` already drives `controls` (`bridge.ts:322-376`) |
| `devicePixelRatio` (coordinate mapping) | **present** | `viewport/scene.ts:60` (`renderer.setPixelRatio(window.devicePixelRatio)`) |
| Editor/DOM scroll | **present** | CodeMirror `view.dispatch({scrollIntoView:true})` (`editor/Editor.tsx:136,170,557`) |
| LSP probes (`hover_at`/`completion_at`/`definition_at`) | **present** | `lsp_request` Tauri cmd + `editor/lspClient.ts` (`textDocument/hover`,`/completion`), `editor/gotoDefinition.ts` |
| Menus are HTML (for `open_menu`/`menu_state`) | **present** | `panels/MenuBar.tsx` (Solid, `MenuId='file'|'edit'|'view'|'help'`, open-state signal) |
| Real-GUI e2e harness over `:3939/mcp` with generic typed `rpc<T>()` | **present** | `gui/test/visual/run.ts` spawns `run-gui-dev.sh`, `tools/call` via `rpc<T>()` (`:60-95`); currently screenshot-diff only |
| **Pane/splitter sizes (`resize_panes`)** | ⚠️ **GAP → `L0`** | `editorWidth`/`sideWidth`/`designTreeHeight`/`propertyHeight`/`constraintHeight` are component-local `createSignal`s (`App.tsx:471-475`), not a store; a `savedLayout` persistence path exists to migrate |
| **Whole-panel collapse** | ⚠️ **absent feature** | only tree-node expand/collapse exists (`DesignTree.tsx:60` `expanded`, `ConstraintPanel.tsx:27` `expandedNodes`), component-local — tool targets these; panel-collapse out of scope |
| `set_window_size` | ⚠️ **needs capability grant** | feasible via `getCurrentWindow().setSize` (`@tauri-apps/api ^2.0.0` is a dep); `capabilities/default.json` grants only `core:window:default` → add `core:window:allow-set-size` |
| **CI gating of GUI e2e** | ⚠️ **does not exist** | **no `.github/workflows/`**; `scripts/verify.sh:451` builds the OCCT-clean `reify-gui` crate but does **not** run the GUI/visual harness. "Green in CI" is therefore aspirational — see G6 below |

**G6 — premises validated, no false numeric/capability claim asserted.** The few risky premises are bounded, not asserted as full-fidelity:
- *"synthetic click == user click"* is **false in specific documented ways** (no CSS `:hover`, no native DnD, no OS hit-testing) — `τ0` documents the gaps; the tools assert JS-handler firing, not pseudo-class styling.
- *"`ui_outline` == accessibility tree"* is **false** — it is a DOM-derived approximation, explicitly labelled; a true AX tree is deferred (blocked on WebKitGTK lacking the CDP `Accessibility` domain).
- *"signals are CI-gated"* is **false today** — there is no CI workflow; the honest signal is "passes via the real-GUI e2e harness run (`npm run test:visual`/new `test:e2e`), which boots the actual app." CI-wiring a display-enabled GUI job is separate infra (§5). Asserting a CI gate that does not exist would be exactly the false-premise trap the overlay warns against.
- *`inject_diagnostics`* is **synthetic store input** — its leaf signal is the **real diagnostics-UI render** of the injected set (via `ui_outline`/screenshot), never "the store was set" (which would be the rejected "unit test against synthetic input" pattern at the UI layer).
- No closed-form numeric floors are asserted (coordinates are exact pixel arithmetic; `devicePixelRatio` is reported, not guessed) — this PRD is off the G6 numeric-bound branch.

---

## §4 — Resolved design decisions

1. **Comprehensive but phased.** One PRD; P0 foundations (contract, harness, `layoutStore`, testid sweep) gate the tool layers; substrate-heavy items (layout control, true AX) sit behind their prerequisites rather than being faked. (Design-session choice: "comprehensive, phased.")
2. **Agent-assessment north-star; AX auditing deferred.** The urgent need is an agent understanding/driving the UI/UX, so ship the pragmatic `ui_outline` semantic snapshot now; a true platform AX tree (and the real ARIA it needs) is a nice-to-have-later forward stub (AX‑1).
3. **Both fixture styles, explicitly labelled.** A named **real-`.ri` fixture catalogue** that *produces* states end-to-end, **plus** a store-level `inject_diagnostics` for testing the diagnostics UI in isolation — each tool documents which it is, and the synthetic one's signal is the real UI render. (Design-session choice.)
4. **Synthetic events, fidelity gaps documented — not real-input injection.** Deterministic and sufficient for agent driving; the CDP/WebDriver dependency for real OS input is not worth it for this consumer.
5. **Drive the real controls, observe by driving.** Camera helpers drive `OrbitControls`; `press_tab` walks real focus; menus/tree-expand drive real component state — rather than reimplementing browser/platform algorithms that silently diverge. `pick_entity_at` is a **pure query** (no selection mutation); `select_entity` already covers the mutate case.
6. **One coordinate/transport contract for all pixel tools** — CSS/logical px from window origin, `devicePixelRatio` reported, synthetic-event gaps documented, validated by a two-way boundary test (the narrow H component, §G5).
7. **`layoutStore` lift + testid/exposure sweep are honest prerequisites**, not folded into the tools that need them — so a tool doesn't land "declared-only" against state it can't reach.
8. **`set_window_size` via the frontend** + a one-line `core:window:allow-set-size` capability grant — not a `DebugServerState` app-handle refactor.
9. **Whole-panel collapse is out of scope** (not an existing feature); the tool targets the real tree-node expand/collapse state only.
10. **"Green via the real-GUI e2e harness," not "green in CI"** — because no CI exists yet; CI-wiring is tracked as separate infra, and the suite is the regression guard once it lands.

---

## §5 — Out of scope

- **Real OS-input injection** (CDP/WebDriver/`tauri-driver`) for true `:hover` pseudo-class, native drag-and-drop, OS hit-testing — synthetic events suffice for the agent consumer.
- **True platform accessibility-tree auditing** — blocked on WebKitGTK lacking Chrome's `Accessibility.getFullAXTree`, and needs real ARIA in the app first. Deferred to tracker **AX‑1**; `ui_outline` is the pragmatic stand-in.
- **Whole-panel collapse/expand affordance** — not an existing UI feature; building it is product work, not debug tooling.
- **Standing up a display-enabled CI job** for the GUI e2e suite — there is no `.github/workflows/` yet; wiring GUI e2e (xvfb + GUI build + OCCT libs) into CI is a separate infra effort. The suite here is runnable (`npm run` + display) and becomes the regression guard once CI exists.
- **`reset_app_state` to a fully pristine process** — it resets app-level state (open files, selection, camera, layout, injected diagnostics), not the OCCT engine process.

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

This PRD is largely self-contained GUI-tooling; no contested-ownership pair from the breadcrumb map is touched.

| Seam | Owner | Resolution |
|---|---|---|
| `debug_server.rs` tool-defs + dispatch, `bridge.ts` handlers, `types.ts` ctx | **this PRD** | All new tools; the two hub files serialize under the orchestrator's file locks (expected). |
| `gui/test/visual/run.ts` e2e harness | **this PRD** (extends) | Adds value-assertion mode + fixture catalogue; reuses the existing `:3939/mcp` `rpc<T>()` transport. |
| `gui/src/stores/layoutStore.ts` (new) + `App.tsx` layout-signal lift | **this PRD** (`L0`) | Self-contained; preserves `savedLayout` persistence. |
| `lsp_request` / `lspClient.ts` LSP surface | `gui` editor (existing) | Read-only reuse by `hover_at`/`completion_at`/`definition_at`; no protocol change. |
| `capabilities/default.json` window permission | **this PRD** (`C2`) | Adds `core:window:allow-set-size`; no other capability change. |
| True AX-tree auditing | **AX‑1** (deferred tracker) | This PRD's `ui_outline` is the upstream stand-in; AX‑1 consumes the same testid/role exposure. |

No new in-engine seam is introduced, so no `engine-integration-norm.md` extension is needed.

---

## §G5 — Design-first / boundary-test decision

This is **not** an FEA/ComputeNode/persistent-naming/multi-kernel/grammar seam, and its blast radius is one crate + the frontend, so it does **not** warrant full B+H. The one place H pays is the **shared coordinate/transport contract (`τ0`)**: many pixel tools depend on a single convention, and a wrong one silently corrupts all of them. So H is applied **narrowly** — a contract doc + a two-way boundary test (Rust dispatch ↔ frontend handler round-trip; pixel-coordinate consistency `get_layout_metrics`→`click_at`→handler, and `pick_entity_at`↔raycast) — rather than across every leaf.

---

## §7 — Decomposition plan (one bullet per task → observable signal)

Every tool leaf's signal is a **value-assertion (or screenshot) scenario in the real-GUI e2e harness** (`H0`), run via `npm run test:visual`/`test:e2e` against an actual `reify-gui` over `:3939/mcp`. The two hub files (`debug_server.rs`, `bridge.ts`) are touched by every tool task and serialize under the orchestrator's file locks — that is expected, not a dependency.

**P0 — foundations (parallel; gate the tool layers):**

- **`τ0` — coordinate/transport contract + two-way boundary test (H).** Commit `docs/debug-mcp-contract.md` (CSS-logical-px-from-window-origin, `devicePixelRatio` reporting, synthetic-event fidelity gaps, tool wiring pattern, error envelope) + a boundary test (dispatch↔handler round-trip; coordinate round-trip). **Signal:** the boundary test passes (Rust + frontend) and the contract doc is committed. *Deps: none.*
- **`H0` — e2e value-assertion harness + named-fixture `.ri` catalogue.** Extend `gui/test/visual/run.ts` with a value-assertion scenario mode (assert returned JSON) and add a `test:e2e` npm script; commit the fixture catalogue (`empty`/`small_cube`/`broken_syntax`/`large_assembly`/`all_severities`). **Signal:** `npm run test:e2e` boots the real GUI and a value-assertion scenario (e.g. `store_state` meshCount on `small_cube`) passes. *Deps: none.*
- **`L0` — `layoutStore` lift.** Move the 5 layout signals from `App.tsx` into `gui/src/stores/layoutStore.ts`, exposed on the debug ctx; preserve `savedLayout` persistence. **Signal:** e2e asserts a pane-size read via the ctx; existing layout-persistence behavior unchanged (resize survives reload). *Deps: none.*
- **`T0` — testid/exposure sweep.** Add `data-testid` to menu buttons, splitter handles, panel sections, diagnostic rows, file tabs; expose `MenuBar` open-menu signal + `DesignTree`/`ConstraintPanel` expand state on the ctx. **Signal:** `list_elements` enumerates the newly-tagged surfaces (e2e asserts their presence). *Deps: none.*

**P1 — read-only inspection (the core agent-assessment value):**

- **`R1` — DOM/style/layout/window inspection.** `query_selector`/`_all`, `get_layout_metrics` (bounds + scroll + `scrollWidth>clientWidth` overflow/clipped-text), `get_computed_style` (subset), `active_element`, `get_window_state` (size/pos/focused/`devicePixelRatio`). **Signal:** e2e on a known fixture asserts metrics + an overflow/clip detection on a deliberately-narrowed element; `get_window_state` reports `devicePixelRatio`. *Deps: `H0`.*
- **`R2` — diagnostics + semantic snapshot.** `get_diagnostics` (real `compileDiagnostics`/`tessellationDiagnostics`, structured), `ui_outline` (DOM-derived semantic text snapshot). **Signal:** e2e loads `broken_syntax` → `get_diagnostics` returns the real error set with codes/ranges; `ui_outline` lists panel/tree/tab structure with text + enabled-state. *Deps: `H0`.*
- **`R3` — console capture + wait primitives.** `list_console_errors` (early ring-buffer install in `main.tsx`: `window.onerror`/`unhandledrejection`/patched `console.error|warn`) + `wait_for`/`wait_for_selector`. **Signal:** e2e triggers a frontend error → `list_console_errors` returns it with message+stack; `wait_for_selector(testId,{visible})` blocks then resolves when an element appears. *Deps: `H0`.*

**P2 — synthetic interaction:**

- **`I1` — pointer + scroll + focus.** `click_at`/`drag`/`hover` (synthetic `PointerEvent` via `elementFromPoint`, documented `:hover` caveat), `focus_element`, `scroll` (DOM + CodeMirror). **Signal:** e2e `click_at` on a button's computed center fires its handler (observable store/diagnostic delta); `scroll` moves a panel and `get_layout_metrics` confirms `scrollTop`. *Deps: `τ0`, `H0`.*
- **`I2` — canvas pick + camera.** `pick_entity_at` (query-only raycast → entity path + world point), `orbit`/`pan`/`zoom_camera` (drive real `OrbitControls` via synthetic input). **Signal:** e2e on `small_cube` — `pick_entity_at` at the cube's screen center returns its entity path; `orbit_camera` changes camera azimuth (a `viewport_state` delta). *Deps: `τ0`, `H0`.*

**P3 — app chrome + layout (need the P0 exposure):**

- **`C1` — menus + focus order.** `open_menu`/`menu_state` (drive the HTML `MenuBar`), `press_tab`/`tab_order` (observe-by-driving). **Signal:** e2e `open_menu("file")` → `menu_state` reports File open with per-item enabled-state; `press_tab` moves `active_element` through the documented order. *Deps: `T0`, `H0`.*
- **`C2` — layout control.** `resize_panes` (via `layoutStore`), tree-node `expand`/`collapse` (via the exposed tree state), `set_window_size` (+ `core:window:allow-set-size` capability grant). **Signal:** e2e `resize_panes({editorWidth:N})` then `get_layout_metrics` confirms; `expand_tree_node` on a `DesignTree` node → `ui_outline` shows its children; `set_window_size(w,h)` → `get_window_state` reports the new size. *Deps: `L0`, `T0`, `R1`, `H0`.*

**P4 — fixtures, injection, capture, LSP:**

- **`F1` — fixtures + synthetic injection + capture.** `load_fixture(name)` (catalogue → `open_file`), `inject_diagnostics` (store-level synthetic, **labelled**), `reset_app_state`, `element_screenshot(testId)` (crop). **Signal:** e2e `load_fixture("all_severities")` → `get_diagnostics` shows the real produced set; `inject_diagnostics([...])` → `ui_outline`/`element_screenshot` of the diagnostics panel shows the injected set **rendered by the real UI** (not merely store-set); `reset_app_state` returns `store_state` to baseline. *Deps: `H0`, `R2`.*
- **`F2` — LSP probes.** `hover_at(line,col)`, `completion_at(line,col)`, `definition_at(line,col)` over the existing `lsp_request`. **Signal:** e2e — `hover_at` over a known symbol returns hover markdown; `completion_at` returns a non-empty item list; `definition_at` returns the target range. *Deps: `H0`.*

**P5 — consumer integration (G1 hard proof):**

- **`E1` — e2e consumer suite + `/verify`+`/review` recipe.** A suite of value-assertion + screenshot scenarios exercising ≥1 tool from each of R1/R2/R3/I1/I2/C1/C2/F1/F2 against the real GUI, plus a committed recipe doc wiring the new tools into `/verify` and `/review`. **Signal:** `npm run test:e2e` runs the full suite green against a real `reify-gui`; the recipe doc is committed and referenced from the skills. *Deps: `R1,R2,R3,I1,I2,C1,C2,F1,F2`.*

**Deferred tracker (filed `deferred`, NOT activated):**

- **`AX‑1` — true platform accessibility-tree auditing.** A real AX tree (role/name/state per the ARIA accname algorithm, ideally platform-backed) for accessibility-conformance auditing — needs real ARIA in the app and a CDP-equivalent the WebKitGTK webview lacks. Forward stub; consumes the same testid/role exposure as `ui_outline`. *Deferred; no active deps.*

---

## §8 — Open (tactical / implementation-time) questions

1. **`ui_outline` shape (R2):** flat list (testid + role + text + enabled) vs. a nested tree mirroring DOM containment? Nested is richer for "what's on screen"; flat is cheaper to assert. Tactical — decide at R2; the consumer suite (E1) drives which reads best.
2. **`get_computed_style` default property set (R1):** return a curated subset (display/visibility/color/font/overflow/…) by default with an optional caller-supplied property list, rather than dumping ~400 props. Confirm the curated set at R1.
3. **`hover(x,y)` semantics (I1):** since synthetic events can't set CSS `:hover`, should `hover` additionally toggle a debug-only `data-debug-hover` attribute so CSS-hover *screenshots* are testable? Possible additive; default is JS-handler-only hover with the caveat documented.
4. **`orbit`/`pan`/`zoom` units (I2):** orbit in radians of azimuth/elevation delta vs. synthetic pixel-drag magnitude? Pixel-drag exercises the real binding most faithfully but is resolution-dependent; radians are reproducible. Decide at I2.
5. **`inject_diagnostics` scope (F1):** diagnostics only, or a general `inject_state(store, patch)`? Start diagnostics-only (the named urgent case); generalize later if other synthetic UI states are needed.
6. **`element_screenshot` cropping (F1):** crop the full `screenshot_window` capture by `getBoundingClientRect` bounds (simple, DPR-aware) — confirm the DPR scaling against `get_window_state.devicePixelRatio`.
7. **Fixture catalogue location:** `gui/test/fixtures/*.ri` vs. reuse `examples/`. Prefer a dedicated `gui/test/fixtures/` so broken/large fixtures don't pollute the runnable `examples/` corpus. Decide at H0.
8. **CI-wiring (E1, noted out of scope):** when a display-enabled CI job lands, `test:e2e` is the gate; until then the suite is operator-/skill-run. Re-confirm the run command in the `/verify` recipe.
