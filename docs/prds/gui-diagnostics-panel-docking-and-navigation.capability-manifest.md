# Capability manifest — gui-diagnostics-panel-docking-and-navigation

Mechanizes G3 + G6 per leaf for `docs/prds/gui-diagnostics-panel-docking-and-navigation.md`. Each leaf's asserted capabilities are bound to evidence; no binding resolves to `declared-only | test-only | producer-downstream | producer-absent | fixture-ERROR | bound≤floor`, so the batch is clear to queue.

**Domain notes.** No novel `.ri` grammar → the G3 grammar-fixture form is **N/A** (no fixtures). No numeric/exactness/end-to-end accuracy claims → the G6 numeric-floor form is **N/A** (all signals are GUI-state assertions observed via reify-debug MCP, the overlay's sanctioned signal type). The relevant evidence forms here are **wired-on-main (anti-orphan)** and **field-population (empty-value sentinel)**.

---

## α — `DiagnosticInfo.has_location` wire field + 4 producers + TS type (intermediate / H-contract producer)

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| `reify_core::DiagnosticInfo` is an extensible serde struct that can carry an additive field | `crates/reify-core/src/diagnostics.rs:3133` (`pub struct DiagnosticInfo`, serde-derived; consumed via `reify-core/src/lib.rs` re-export) | **wired** |
| All 4 construction sites are on production paths (not test-only) | GUI: `gui/src-tauri/src/engine.rs:4603` (normal `diagnostics_to_info`), `:2178` (cold-start), `:2383` (live-edit) — all in `build_gui_state`; CLI: `crates/reify-cli/src/mcp_context.rs:239` | **wired-on-main** |
| `has_location` is derivable at the producer from span presence | `engine.rs:4593-4602` already branches on `diag.labels.first()` → `(1,1,1,1)` sentinel when empty; flag = `!diag.labels.is_empty()` | **wired** |
| Pinned wire-format tests exist to update (anti-mismatch for the contract) | `gui/src-tauri/src/tests/{engine,types,mcp_context}_tests.rs`, `crates/reify-mcp/tests/read_tools_tests.rs` reference `DiagnosticInfo` | **wired** |

Consumers (downstream, wired via deps): β, γ; plus MCP `get_diagnostics` (#4297, done) gains the field by passthrough. **No field-population risk** — the producer writes a real `bool` at every site; Rust's exhaustive struct-init forbids a silent miss.

## δ — Docked, collapsible Diagnostics region (leaf)

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| Layout store can carry new dimensions + persist forward-compatibly | `gui/src/stores/layoutStore.ts` (`createStore<PanelLayout>`), `gui/src/hooks/useLayoutPersistence.ts:38-40` (loader applies missing fields → defaults) | **wired** |
| A docked-pane + splitter pattern exists to reuse | existing `designTreeHeight`/`propertyHeight`/`constraintHeight` splitters in `App.tsx`; `clampPanelHeightsToFit` (`useLayoutPersistence.ts:76`) | **wired** |
| StatusBar badge has a toggle handler to rewire | `gui/src/panels/StatusBar.tsx:116,129` (`onToggleDiagnostics`), `App.tsx:537` (`handleToggleDiagnostics`) | **wired** |
| Panel internals (filter/group/list/rows) reusable under a new shell | `gui/src/panels/DiagnosticsPanel.tsx:235-324`, `diagnosticsView.ts` (`filterDiagnostics`/`groupDiagnostics`) | **wired** |
| reify-debug MCP can observe docked-vs-modal + fold + persistence | `screenshot`/`dom_query`/`store_state` tools; stable `data-testid` on diagnostic rows/splitters/panels added by **#4295 (done)** | **wired-on-main** |

## β — Span-less rows render non-navigable (leaf)

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| `has_location` is available on the frontend entry to gate rendering | produced by **α** (upstream, dep-wired); surfaced on `DiagnosticInfo`/`DiagnosticEntry` (`gui/src/types.ts:174`, `DiagnosticsPanel.tsx:18`) | **producer:α upstream (wired)** |
| Rows can be conditionally rendered non-interactive | `DiagnosticsPanel.tsx:286-322` renders `onClick`/`role="button"`/`tabindex` per row — conditional in SolidJS is trivial | **wired** |
| MCP can inject a span-less diagnostic and verify the rendered row | **#4303 (done)** `inject_diagnostics` (store-level synthetic, **labelled**; honest signal = UI *renders* the injected set via `ui_outline`/`element_screenshot`, not "store was set") | **wired-on-main** |

## γ — Cross-file navigate: open + activate + jump (leaf; re-lands #3358)

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| Open a not-yet-open file by path (disk read) | `gui/src/bridge.ts:75` `openFile(path)` → `invoke('open_file')` → `gui/src-tauri/src/commands.rs:255` `open_file_impl` (`std::fs::read_to_string`) | **wired** |
| Activate a file among open ones | `gui/src/stores/editorStore.ts:75` `setActiveFile`, `:39` `openFile`, `:20` `openFiles[]` | **wired** |
| Editor scrolls to a location once its file is active | `gui/src/editor/Editor.tsx:662-695` (`scrollToLocation` effect, `view.dispatch({selection, scrollIntoView})`); `:666` `isSameFile` guard is the seam to satisfy | **wired** |
| Span-less refusal in the handler | `has_location` from **α** (upstream, dep-wired) | **producer:α upstream (wired)** |
| MCP multi-file fixture to drive the assertion | `open_file`/`type_in_editor`/`editor_content`; multi-doc tab bar (`App.tsx:1509`); testid'd tabs (#4295) | **wired-on-main** |

## ε — End-to-end GUI integration gate (leaf; B+H integration gate)

| Capability asserted by signal | Evidence | Verdict |
|---|---|---|
| A scriptable reify-debug-MCP e2e harness exists | `gui/test/visual/run.ts` (generic `rpc<T>`, screenshot harness) per the code-intel PRD + reify-debug-mcp-expansion batch; `wait_for_idle`/`screenshot`/`dom_query`/`store_state` | **wired-on-main** |
| Inputs for the scripted run | `open_file`, `inject_diagnostics` (#4303), `keyboard`/`type_in_editor`, testid coverage (#4295) | **wired-on-main** |
| The behaviours to assert are produced by upstreams | β (span-less), γ (cross-file), δ (dock/fold) — all dep-wired | **producer:{β,γ,δ} upstream (wired)** |

---

### Resolution log

No binding required re-scoping, re-homing, or bound relaxation. The single contract seam (`DiagnosticInfo.has_location`) is additive + serde-defaulted (back-compat), its producer (α) is upstream of every consumer and dep-wired, and the integration gate (ε) exists and names the §Boundary-test sketch as its signal. Batch clear to queue.
