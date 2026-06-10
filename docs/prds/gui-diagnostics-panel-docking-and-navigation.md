# GUI diagnostics panel: docking, folding & jump-to-line completion

**Status:** active · version-agnostic GUI/tooling foundation · mixed approach (bare **B** for self-contained frontend work; **B+H** for the `DiagnosticInfo` wire-contract leaf) · 2026-06-08

## Goal

Bring the Reify GUI's diagnostics surface up to a modern-IDE "Problems panel" experience: a **docked, foldable** panel (replacing today's floating modal) that collapses to a header bar in place, and **complete** jump-to-line navigation so that clicking a diagnostic reliably reveals its source — including diagnostics in a **non-active file** (open + switch + jump), while diagnostics **not tied to a line** are honestly rendered as non-navigable rather than silently jumping to line 1.

User-observable end state: a designer sees a docked Diagnostics region under the editor; clicking its chevron folds it to a one-line header (count + filter chips) and unfolds it; clicking a line-tied row moves the editor cursor to that line — opening and switching to the file first if the diagnostic belongs to another open/importable file; a span-less diagnostic (e.g. a module-level reload error) renders greyed with no location and does not fake a jump.

## Background — scoped against the *real* current state

A survey of the GUI (2026-06-08) found **most of the literal ask already ships**. This PRD is scoped against the real gap, not the request as first phrased.

**Already shipped (do not rebuild):**
- **A diagnostics panel exists** — `gui/src/panels/DiagnosticsPanel.tsx`, currently a **floating modal dialog** (`overlay` + `aria-modal="true"`), toggled open/closed from the StatusBar compile/tessellation badges (`gui/src/panels/StatusBar.tsx:108-134`). It already has source/severity **filter chips**, a **"Collapse repeated"** dedup-by-count grouping toggle (`diagnosticsView.ts` `groupDiagnostics`), **resize** + size-persistence (`hooks/diagnosticsPanelPersistence.ts`, ResizeObserver), a **line-wrap** toggle, and Escape/overlay-click close. Shipped across tasks **#3229** (surface compile warnings), **#3350** (tessellation navigable), **#3353** (a11y/source chips), **#3393** (sizing/resize/wrap), **#4281** (high-warning triage UX).
- **Jump-to-line on click already works for in-file, line-tied diagnostics** — a row click (or Enter/Space) calls `onNavigate` → `handleNavigateToDiagnostic` (`gui/src/App.tsx:541`) → `setScrollToLocation({file_path, line, column, …})` → an Editor effect (`gui/src/editor/Editor.tsx:662-695`) dispatches `{selection, scrollIntoView: true}`.
- **Multi-document substrate exists** — `editorStore` holds `openFiles: FileData[]` + `activeFile`, with `openFile(FileData)`, `setActiveFile(path)` (`gui/src/stores/editorStore.ts:20-76`), a tab bar (`App.tsx:1509`), and a disk-read bridge `openFile(path) → invoke('open_file') → commands.open_file_impl` (`gui/src/bridge.ts:75`, `gui/src-tauri/src/commands.rs:255`).
- **Layout persistence is cleanly extensible** — `PanelLayout` + `layoutStore` (`gui/src/stores/layoutStore.ts`, `gui/src/hooks/useLayoutPersistence.ts`); the loader already tolerates missing fields → defaults, so adding panel dimensions is forward-compatible.

**Genuinely missing (this PRD builds it):**
1. **The panel is a modal, not foldable.** It is a floating overlay you open/close, not a **docked region** you fold/collapse in place. There is no panel-visibility/collapse/height state in `layoutStore` (only `editorWidth`/`sideWidth`/`designTreeHeight`/`propertyHeight`/`constraintHeight`).
2. **Span-less diagnostics fake a jump.** `diagnostics_to_info` (`gui/src-tauri/src/engine.rs:4593-4602`) and the two synthetic sites (`:2178` cold-start, `:2383` live-edit) emit the sentinel `(1,1,1,1)` with `code: None` when a diagnostic has **no label span** — indistinguishable on the wire from a genuine `1:1` diagnostic. Clicking such a row jumps to line 1, which is meaningless. There is no wire signal for "this diagnostic is tied to a source line." (The TS type notes a *separate* partial convention — `code === "unresolved-source"` ⇒ positions unreliable — `gui/src/types.ts:180-185` — but that covers only one case and is not even set by the producer here.)
3. **Cross-file navigation no-ops.** `handleNavigateToDiagnostic` (`App.tsx:541`) sets `scrollToLocation` and closes the panel but **does not switch the active file**; the Editor effect is then gated out by `if (!isSameFile(location.file_path, activeFile)) return;` (`Editor.tsx:666`). Compile diagnostics are global to the module set, so a diagnostic on an imported file routinely produces a click that does nothing. *(Task **#3358** "DiagnosticsPanel navigation drops cross-file targets" is marked `done`, but the current handler does not switch files — the fix regressed in a later `App.tsx` refactor or was never landed as described. This PRD re-lands it as a boundary-tested leaf.)*

## Consumer & user-observable surface (G1)

The LSP is not involved — these diagnostics flow from the **engine build path** (`engineStore.compileDiagnostics` + `tessellationDiagnostics`) merged in `App.tsx:492`, not `textDocument/publishDiagnostics`. The only seam is the **`reify_core::DiagnosticInfo` wire contract** (Rust producer ↔ TS panel ↔ MCP `get_diagnostics` consumer ↔ pinned wire tests), owned entirely by this PRD (§Contract).

| Mechanism | Consumer (user-observable surface) |
|---|---|
| `DiagnosticInfo.has_location` wire field (`reify-core`) | the panel's span-less rendering + the navigate guard (in-PRD); also the MCP `get_diagnostics` JSON (#4297) gains an honest "line-tied?" signal |
| span-less row rendering (`DiagnosticsPanel`) | a diagnostic with no source span renders greyed, shows no `file:line:col`, and is not a clickable button |
| cross-file navigate (`handleNavigateToDiagnostic`) | clicking a line-tied diagnostic in another file opens/activates that file and moves the cursor to the line |
| docked foldable panel (`layoutStore` + `DiagnosticsPanel` shell + `App.tsx` layout) | a docked Diagnostics region under the editor that folds to a header bar and unfolds; resize + fold-state persist; the StatusBar badge expands+focuses it |
| end-to-end gate (`gui` test harness) | a scripted reify-debug-MCP session exercises fold/unfold, in-file jump, cross-file open+jump, and span-less non-navigability against a real multi-file `.ri` |

## Sketch of approach

**Backend half (one additive wire field).** Add `has_location: bool` to `reify_core::DiagnosticInfo` (`crates/reify-core/src/diagnostics.rs:3133`) with `#[serde(default = "…true")]` so older deserializers and any un-updated TS consumer treat a missing field as line-tied (preserving today's behavior). Set it at all **four** construction sites: `engine.rs:4603` ⇒ `!diag.labels.is_empty()`; the two synthetic GUI sites (`:2178`, `:2383`) ⇒ `false` (module-level reload errors, not line-tied); `reify-cli/src/mcp_context.rs:239` ⇒ per its own span availability. Mirror the field in `gui/src/types.ts`. The MCP `get_diagnostics` path (`gui/src-tauri/src/mcp_context.rs:127`, `crates/reify-mcp`) is pure passthrough — it gains the field with no logic change, but its wire-pinned fixtures update.

**Frontend half (`gui/src`).**
- *Span-less rendering* — in the panel, rows with `has_location === false` render greyed, show `—` instead of `file:line:col`, and drop `onClick`/`role="button"`/`tabindex`; `handleNavigateToDiagnostic` defensively refuses them.
- *Cross-file navigate* — for a line-tied diagnostic whose `file_path` ≠ `activeFile`: if it's in `openFiles`, `editorStore.setActiveFile(file_path)`; else `await bridgeOpenFile(file_path)` → `editorStore.openFile(fileData)`; then set `scrollToLocation`. The Editor effect re-runs on the `activeFile` swap and the `isSameFile` guard then passes. The open→activate→scroll **sequencing** is the one piece of real integration risk and is the boundary test (§Boundary tests).
- *Docked foldable panel* — add `problemsHeight: number` + `problemsCollapsed: boolean` to `PanelLayout`/`layoutStore`/persistence (the loader's missing-field tolerance keeps old layouts valid). Refactor `DiagnosticsPanel`'s **chrome** from modal-overlay to a **docked region at the bottom of the editor column** (per the chosen layout), reusing its existing filter/group/list/row internals: a header bar with a fold chevron (▼/▶), `Diagnostics (N)`, and the filter chips; collapsed ⇒ only the header shows; resizable via a splitter reusing the existing designTree/property/constraint splitter pattern, clamped against editor-column height. Rewire the StatusBar badges (`onToggleDiagnostics`) to expand+focus the docked panel (toggle `problemsCollapsed`) instead of opening a modal. Migrate the modal-specific tests (overlay/`aria-modal`/Escape/overlay-click/ResizeObserver-size) to the docked-region equivalents; line-wrap + filter + group tests carry over.

**Phasing.** The wire field (α) and the docking refactor (δ) are independent roots. Span-less rendering (β) and cross-file navigate (γ) build on both. A single end-to-end MCP-driven gate (ε) re-asserts the whole surface through the running app.

## Resolved design decisions

1. **Span-less is an additive boolean, not a nullable location.** `has_location: bool` keeps the existing `line`/`column`/`end_*` fields (so MCP/other consumers that read them keep working) and adds one honest "trust the location?" flag. Restructuring into `location: SourceLocation | null` was rejected: the wire format is **pinned by tests** and consumed by multiple MCP tools, so a breaking restructure costs far more than an additive flag. `#[serde(default → true)]` makes the change backward-compatible for any consumer that hasn't learned the field.
2. **`has_location` is computed at the producer, from label presence.** `!diag.labels.is_empty()` at the real conversion path; hard-`false` at the two synthetic GUI sites (cold-start / live-edit reload errors). It is *not* inferred frontend-side from `(line,col)==(1,1)`, which would misfire on legitimate top-of-file diagnostics.
3. **The modal is replaced, not duplicated.** The docked panel supersedes the floating dialog; there is no "modal vs docked" mode toggle. The chosen layout docks it at the **bottom of the editor column** (full-width under the editor, above the status bar).
4. **Default fold state is collapsed; the panel does not auto-expand on error.** Opening to a header bar avoids stealing editor space and avoids surprise layout jumps when diagnostics arrive; the count badge on the header (and the StatusBar badge) signals there is something to expand. *(Auto-expand-on-first-error is a tactical open question.)*
5. **Cross-file navigate reuses the existing disk-read + multi-doc substrate.** No new Tauri command, no new store; `bridgeOpenFile` + `editorStore.openFile`/`setActiveFile` already exist. This is wiring, not new substrate.
6. **Span-less rows are non-interactive, not merely guarded.** They are rendered without `onClick`/`role`/`tabindex` (not "clickable but no-op"), so screen-reader and keyboard users get an honest affordance; the handler guard is a defensive second line.
7. **Header naming stays "Diagnostics"** to match existing UI/testIds and the StatusBar "Compile"/"Tessellation" labels (the mockup's "Problems" is cosmetic). *(Final label is a tactical open question.)*

## Pre-conditions for activating

- No upstream PRD blockers. All substrate verified present: `reify_core::DiagnosticInfo` (additive field), all four producer sites, the `editorStore` multi-doc API + `bridgeOpenFile` disk read, the `layoutStore`/`PanelLayout` forward-compatible loader, and the existing splitter pattern.
- No novel `.ri` grammar (G3 grammar gate N/A — no fixtures needed).

## Cross-PRD relationship (G4)

No contested cross-PRD seams (checked against the overlay's three known contested pairs — none touch diagnostics/editor layout). The `DiagnosticInfo` field touches `reify-core`/`reify-mcp`/`reify-cli` but is **additive and owned entirely by this PRD**.

| Relation | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `gui-code-intelligence-and-navigation.md` (Find-uses references panel, open-Q #2) | this PRD **establishes** the docked-panel pattern that PRD said it would "reuse for consistency" | docked collapsible-region shell in `DiagnosticsPanel`/`layoutStore` | this-PRD | forward seam; additive, no conflict |
| #3358 (cross-file navigation, `done` but regressed) | this PRD **re-lands** it | `handleNavigateToDiagnostic` file-switch | this-PRD (γ) | supersedes |
| MCP `get_diagnostics` (#4297, done) | gains `has_location` field (passthrough) | `reify_core::DiagnosticInfo` JSON | this-PRD (α) | additive |

## Contract (B+H) — the `reify_core::DiagnosticInfo` wire seam

### Producer field

```rust
// crates/reify-core/src/diagnostics.rs
pub struct DiagnosticInfo {
    pub file_path: String,
    pub line: u32, pub column: u32, pub end_line: u32, pub end_column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
    #[serde(default = "default_true")]   // missing ⇒ treat as line-tied (back-compat)
    pub has_location: bool,              // false ⇒ (line,col) is the sentinel, not a real span
}
```

**Invariants.**
1. **Honesty.** `has_location == false` **iff** the diagnostic carries no source span (the producer used the `(1,1,1,1)` sentinel). A genuine diagnostic at line 1 col 1 has `has_location == true`.
2. **Completeness.** Every one of the four construction sites sets the field explicitly (Rust's exhaustive-struct-init forces this — no silent default-miss on the production path).
3. **Back-compat.** A JSON `DiagnosticInfo` without `has_location` deserializes as `true`; the TS consumer treats `has_location !== false` as line-tied.
4. **No phantom navigation.** The frontend renders `has_location === false` rows non-interactive and the navigate handler refuses them.

### Frontend seam

`gui/src/types.ts` `DiagnosticInfo` gains `has_location?: boolean`. `DiagnosticEntry` (the `+source` wrapper) inherits it. `handleNavigateToDiagnostic` consumes both `has_location` (refuse if false) and `file_path` (open/switch if ≠ active).

## Boundary-test sketch (B+H)

| Scenario | Precondition | Postcondition (asserts) |
|---|---|---|
| Span-less ⇒ flag false | diagnostic with empty `labels` | `diagnostics_to_info` emits `has_location == false`; spanned diagnostic ⇒ `true` (reify-core / GUI Rust unit test) |
| Synthetic sites ⇒ flag false | cold-start (`:2178`) & live-edit (`:2383`) reload errors | both emit `has_location == false` |
| Wire back-compat | JSON without `has_location` | deserializes to `true`; pinned-wire fixtures updated to include the field |
| Span-less row non-interactive | inject a span-less diagnostic | row is greyed, shows `—`, has no `onClick`/`role="button"`; navigate handler is never reached / refuses (vitest or reify-debug MCP `inject_diagnostics`) |
| In-file jump still works | line-tied diagnostic in active file | clicking moves the cursor to its line (unchanged behavior) |
| Cross-file open+jump | main.ri active, line-tied diagnostic on imported helper.ri | clicking opens/activates helper.ri and moves the cursor to the line (reify-debug MCP, multi-file fixture) |
| Fold/unfold | docked panel rendered | chevron click collapses to header bar and restores; StatusBar badge expands+focuses |
| Layout persists | resize + fold, reload | `problemsHeight`/`problemsCollapsed` survive reload (`store_state`/persistence) |

The reify-core/GUI flag unit tests + the cross-file navigate leaf (γ) are the **integration gate** for the seam; the end-to-end GUI leaf (ε) re-asserts the whole surface through the running app.

## Decomposition plan

Greek labels are PRD-local; task IDs assigned at decompose. "Modules" = crates/dirs touched.

**Phase 0 — Wire contract (the H-contract producer)**
- **α — `DiagnosticInfo.has_location` field + all four producers + TS type.** Modules: `crates/reify-core`, `gui/src-tauri/src/engine.rs` (3 sites), `crates/reify-cli/src/mcp_context.rs`, `gui/src/types.ts`. Add the `#[serde(default→true)]` bool; set it at every construction site; mirror in TS; update pinned-wire fixtures (`reify-mcp` read-tool tests, GUI wire tests). *Intermediate* — unlocks β/γ. **Signal:** a Rust unit test asserts `diagnostics_to_info` emits `has_location=false` for a label-less diagnostic and `true` for a spanned one, both synthetic sites emit `false`, and `reify check`/MCP `get_diagnostics` JSON includes the field. Prereqs: none.

**Phase 1 — Docked foldable panel (independent root)**
- **δ — Replace the modal with a docked, collapsible Diagnostics region.** Modules: `gui/src/stores/layoutStore.ts`, `gui/src/hooks/useLayoutPersistence.ts`, `gui/src/panels/DiagnosticsPanel.tsx` (shell), `gui/src/App.tsx` (layout JSX + splitter), `gui/src/panels/StatusBar.tsx`. Add `problemsHeight`+`problemsCollapsed`; refactor the panel chrome to a docked bottom-of-editor region with a fold-chevron header (count + filter chips), collapse-to-header, splitter resize clamped to editor-column height; rewire StatusBar badges to expand+focus; migrate modal-only tests to docked equivalents; preserve filter/group/line-wrap internals. **Signal:** in the running GUI the Diagnostics region is docked (not a modal overlay) under the editor; clicking the chevron folds it to a one-line header and unfolds it; the StatusBar badge expands it; a resize + fold survives reload (reify-debug MCP `screenshot`/`store_state`/`dom_query`). Prereqs: none.

**Phase 2 — Span-less honesty (consumes α inside δ's panel)**
- **β — Span-less rows render non-navigable.** Modules: `gui/src/panels/DiagnosticsPanel.tsx`. Rows with `has_location === false` render greyed, show `—`, drop `onClick`/`role`/`tabindex`. **Signal:** inject a span-less diagnostic (reify-debug MCP `inject_diagnostics`) → its row is non-clickable and shows no location; a line-tied row still navigates. Prereqs: α, δ.

**Phase 3 — Cross-file navigation (consumes α; re-lands #3358)**
- **γ — `handleNavigateToDiagnostic`: span-less refusal + open/activate/jump.** Modules: `gui/src/App.tsx`, `gui/src/editor` (sequencing). Refuse `has_location === false`; for a line-tied diagnostic in another file, `setActiveFile` (if open) or `bridgeOpenFile`→`editorStore.openFile` (if not), then `setScrollToLocation`; ensure the Editor effect fires post-swap. Stop closing the (now docked) panel on navigate. **Signal:** with main.ri active and a line-tied diagnostic on imported helper.ri, clicking the row opens/activates helper.ri and moves the cursor to the diagnostic's line (reify-debug MCP, multi-file fixture). Prereqs: α, δ.

**Phase 4 — End-to-end integration gate (B+H)**
- **ε — End-to-end GUI integration test.** Modules: `gui` (test harness). Drive the running app via reify-debug MCP against a multi-file `.ri` fixture carrying line-tied diagnostics in two files plus a span-less one: assert fold/unfold, in-file jump, cross-file open+jump, span-less non-navigability, and layout persistence — in one CI-able run. **Signal:** the scripted session shows the panel folding, an in-file row jumping, a cross-file row opening+jumping, a span-less row inert, and the layout surviving reload. Prereqs: β, γ, δ.

**DAG:** α→{β,γ}; δ→{β,γ}; {β,γ,δ}→ε. (α and δ are independent roots; β edits the panel component, γ edits `App.tsx` — no same-file lock collision; both serialize after δ which owns the panel shell + the `App.tsx` layout JSX.)

## Out of scope (future PRDs / separate work)

- LSP `publishDiagnostics`-sourced inline squiggles (these diagnostics come from the engine build path, not the LSP).
- The `code === "unresolved-source"` "positions unreliable but file known" case — related to but distinct from span-less; today it still carries a (wrong-file) location. Could later reuse `has_location`-style honesty, but is not in this PRD.
- A keyboard shortcut to toggle the panel (the `shortcuts.ts` registry exists) — tactical, see open questions.
- Diagnostic quick-fixes / code actions, severity sorting, "go to next/previous diagnostic" (F8) navigation.

## Open questions (tactical — defer to implementation)

1. **Auto-expand on first error?** Suggested: no (avoid layout jumps); badge + header count signal instead. Decide in δ.
2. **Header label** "Diagnostics" vs "Problems". Suggested: keep "Diagnostics" (matches testIds/StatusBar). Decide in δ.
3. **Toggle shortcut.** Add `Ctrl+Shift+M` (VS Code Problems) to `shortcuts.ts` + the `?` overlay? Suggested: yes, cheap. Decide in δ.
4. **Cross-file open failure** (file deleted/unreadable since the diagnostic was produced) — toast vs silent. Suggested: toast via the existing `showToast`. Decide in γ.
5. **Default `problemsHeight`** and min/clamp against editor-column height. Suggested: ~160px, min ~80px, reuse `clampPanelHeightsToFit`-style clamping. Decide in δ.
