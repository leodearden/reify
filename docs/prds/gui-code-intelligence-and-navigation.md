# GUI code-intelligence & navigation (IDE affordances)

**Status:** active · version-agnostic GUI/tooling foundation · approach **B+H** (contract + two-way boundary tests) · 2026-06-02

## Goal

Bring the Reify GUI editor up to modern-IDE parity for *reading, navigating, and refactoring* `.ri` source: parse-aware **rename**, **find-uses / references**, **structural folding**, an **occurrence-highlight**, a **command palette + symbol jump**, **navigation hotkeys with back/forward history**, and **hover-sync** correspondence between code ↔ viewport ↔ outline. The user-observable end state: a designer editing a `.ri` can press `F2` to safely rename a parameter everywhere it's used in scope, `Shift+F12` to list its uses, fold/unfold scopes from the gutter or keyboard, `Ctrl+Shift+P` to run any command, and hover a row in the outline to see the matching body light up in the viewport.

## Background

A survey of the GUI (2026-06-02) found the substrate is **much richer than assumed**, and several "missing" features already partly exist. This PRD is scoped against the *real* current state, not the gap as first reported:

**Already shipped (do not rebuild):**
- **Navigate-to-definition** — `Ctrl/Cmd+Click` → `textDocument/definition` via the real `reify-lsp` server, incl. cross-file import resolution (`gui/src/editor/gotoDefinition.ts`, `crates/reify-lsp/src/goto_def.rs`). Only a keyboard binding (`F12`) is missing.
- **Cross-pane correspondence** — task **#3880** (done 2026-05-27) wired viewport-click→editor-reveal, editor-cursor→viewport-fly, outline→viewport, viewport→outline (`gui/src/hooks/useEditorSelectionSync.ts`, `App.tsx` `handleViewportSelect`/`handleDesignTreeSelect`). The gap that remains is **hover**-sync and code-token→viewport on plain selection.
- **Shortcut infrastructure** — a real registry (`gui/src/shortcuts.ts`), a `?`-toggle **KeyboardHelp** overlay (`gui/src/components/KeyboardHelp.tsx`), and a MenuBar with hotkey annotations (task #1766). The registered *set* is sparse (open/save/export/F5/chat/reload) — no rename/find-refs/goto/fold/nav-history keys, and no command palette.
- **Folding metadata** — `foldNodeProp` is configured for `Block` nodes (`gui/src/editor/reifyLanguage.ts:15`), but the Editor never adds `codeFolding()` / `foldGutter()` / `foldKeymap`, so there is fold metadata but no fold UI or keybinding.
- **Full LSP server** — `reify-lsp` advertises hover + definition + completion + diagnostics, bridged through the `lsp_request` Tauri command (`gui/src-tauri/src/main.rs:434`, `gui/src/editor/lspClient.ts`).

**Genuinely missing (this PRD builds it):**
- **Parse-aware rename** and **find-references** — `reify-lsp` has **no** `references`/`rename`/`documentHighlight` providers, and **no retained binding map**: the compiler resolves names on-the-fly during type-checking (`crates/reify-compiler/src/scope.rs` `CompilationScope::resolve`) and discards spans + identifier names in the compiled IR (`crates/reify-ir/src/expr.rs` — `ValueRef(ValueCellId)`, no span). Reference-collection must therefore be **built from scratch as a scope-aware walker over the parsed AST**.

The good news for the build: identifier **uses carry spans** (`crates/reify-ast/src/ast.rs:14` — every `Expr` has `span`; `ExprKind::Ident(String)` at :34), the parsed module is fully available, `enclosing_decl_at` + `find_named_member_span` already locate declarations (`crates/reify-lsp/src/analysis.rs`, `goto_def.rs`), `CompilationScope` already encodes the precedence rules to mirror (params→lets→autos, guarded-block visibility, innermost-shadowing), and goto_def's `resolve_import` closure is the framework for the cross-file phase.

**Out of this PRD (filed separately):** four viewport/outline **defects** the user is hitting — outline overlay-scrollbar covering the eye-icons (regression of #3394), empty def-preview pane on viewport realization-click, grid z-fighting over the X/Y axes, and missing XYZ axis labels. These need fixing, not designing, so they are a standalone bug batch, not part of this PRD.

## Consumer & user-observable surface (G1)

Every mechanism below has a named, user-observable consumer. The LSP is **not** an engine seam (the `engine-integration-norm.md` §3 catalogue is for kernel/dispatch seams), so the relevant seam is the **`reify-lsp` ↔ frontend `lsp_request` contract** — owned entirely within this PRD (see §Contract).

| Mechanism | Consumer (user-observable surface) |
|---|---|
| `collect_references` (scope-aware AST walker, `reify-lsp`) | the references / rename / documentHighlight handlers below (in-PRD); never user-facing directly |
| `textDocument/references` provider | Find-uses panel: `Shift+F12` lists all in-scope uses; click navigates |
| `textDocument/rename` + `prepareRename` providers | `F2` inline rename rewrites all in-scope occurrences; refuses on non-renameable positions |
| `textDocument/documentHighlight` provider | occurrence highlight: placing the cursor on a symbol subtly highlights its other occurrences |
| `textDocument/documentSymbol` provider | command-palette symbol-jump (`@` / `Ctrl+Shift+O`) |
| folding wiring (frontend) | fold arrows in the gutter; `Ctrl+Shift+[` / `]`; fold-all/unfold-all |
| navigation history (frontend) | `F12` go-to-def; `Alt+←` / `Alt+→` back/forward across edit positions |
| command palette (frontend) | `Ctrl+Shift+P` runs any registered command |
| hover-sync (frontend) | hovering an outline row / code token highlights the matching mesh in the viewport (and vice-versa) |

## Sketch of approach

**Backend half (`reify-lsp`).** One new module, `references.rs`, exposing `collect_references` over the **parsed** `ParsedModule` (the compiled IR is unusable here — it has no spans/names). It mirrors `CompilationScope`'s precedence rules to decide, for each `Ident` occurrence, whether it binds to the declaration under the cursor. Four thin LSP handlers wrap it (`references`, `prepareRename`, `rename`, `documentHighlight`); a fifth (`documentSymbol`) reuses the existing parsed-module/`analysis.rs` walk. New capabilities are advertised in `server.rs` `initialize`. Everything routes through the existing `lsp_request` Tauri command — no new IPC channel.

**Frontend half (`gui/src`).** Each backend capability gets a thin consumer: a references panel + `Shift+F12`, an `F2` inline-rename flow that applies the returned `WorkspaceEdit` through the editor's existing `update_source`/dispatch path, a CodeMirror decoration for occurrence highlight, a command-palette component over `shortcuts.ts` + documentSymbol, the folding-extension wiring, and a navigation-history stack. Hover-sync reuses the already-shipped `selectionStore.hoverEntity` + viewport `setHovered` (`gui/src/viewport/selection.ts:150`) + `get_entity_at_source_location`.

**Phasing.** Single-file rename/find-refs ships first (the hard correctness work is the scope walker, which is identical single-file vs cross-file). Cross-file is a later phase that extends the walker over the import graph using goto_def's `resolve_import`, and is the one place with real substrate risk (the in-process LSP must hold a multi-document workspace view — that task owns building it).

## Resolved design decisions

1. **Reference-collection works on the parsed AST, not the compiled IR.** The IR discards spans and identifier names; the parsed AST retains both. The collector mirrors `CompilationScope` precedence rather than reusing it (the compiler's scope is tuple-state local to expr-compilation and not reusable as-is). *Risk:* drift between the two scope models — mitigated by the boundary-test fixtures (§Boundary tests) and a note in `references.rs` pointing at `scope.rs` as the source of truth for precedence.
2. **`prepareRename` is mandatory and conservative.** It returns `null` for any position that is not a safely-renameable user symbol (keywords, literals, builtins, and — in the single-file phase — symbols whose binding crosses a module boundary). The editor refuses the rename rather than producing a partial/unsafe edit. This is the primary guard against the "rename silently misses a use" failure.
3. **Rename validity is asserted by re-parse.** A `rename` `WorkspaceEdit` must yield a buffer that re-parses with **0 new ERROR nodes** and where the new name resolves identically. This is a boundary-test postcondition, not just a hope.
4. **Single-file first; cross-file is an explicit later phase, not silently deferred.** A single-file-only rename that misses cross-file uses is dangerous, so cross-file is in-scope for this PRD (phase κ), gated behind the multi-document-workspace substrate it builds.
5. **`documentSymbol` (syntactic) is distinct from the outline's `get_entity_tree` (semantic/realization).** The palette's symbol-jump uses documentSymbol; the outline keeps its entity tree. They are not merged.
6. **Occurrence-highlight and find-references share one collector** — documentHighlight is `collect_references(includeDeclaration=true)` filtered to the active document, rendered as CM decorations instead of a panel.
7. **The command palette is registry-driven.** Commands come from `shortcuts.ts` (executing the same callbacks `useKeyboardShortcuts` dispatches), so the palette and the keymap cannot drift. Symbol-jump mode (`@`) is a second source backed by documentSymbol.
8. **Navigation history is editor-position-based** (a bounded back/forward stack of `{uri, offset}`), pushed on goto-def / find-refs-navigate / cross-pane reveal. `Alt+←` / `Alt+→`.

## Pre-conditions for activating

- No upstream PRD blockers. All substrate verified present (parsed-AST spans, `analysis.rs` lookups, `lsp_request` bridge, CodeMirror folding exports confirmed at `@codemirror/language@^6.10`).
- The cross-file phase (κ) depends only on in-PRD tasks (α/β/γ) plus the multi-document-workspace substrate it builds itself.

## Cross-PRD relationship

No contested cross-PRD seams (checked against the overlay's three known contested pairs — none touch the LSP/editor). The only seam is **internal**: the `reify-lsp` ↔ frontend `lsp_request` contract, owned by this PRD (§Contract). The cross-pane correspondence work extends shipped task **#3880** (not a PRD) — additive, no ownership conflict.

| Relation | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #3880 (cross-view selection sync, done) | extends | `selectionStore.hoverEntity` / viewport `setHovered` | this-PRD (phase ι) | additive |
| `gui-event-channel-inventory.md` | unrelated | — | — | n/a |

## Contract (B+H) — the `reify-lsp` ↔ frontend seam

### Backend producer: `crates/reify-lsp/src/references.rs`

```rust
/// All source spans that refer to the SAME declaration as the symbol at `pos`,
/// within `parsed`. Scope-correct: excludes same-named symbols bound elsewhere.
/// Returns None if `pos` is not on a resolvable user symbol.
pub fn collect_references(
    parsed: &ParsedModule,
    pos: Position,              // LSP 0-based line/col
    include_declaration: bool,
) -> Option<ReferenceSet>;

pub struct ReferenceSet {
    pub name: String,
    pub kind: RefSymbolKind,   // Structure|Occurrence|Trait|Enum|Variant|Fn|Param|Let|Auto|Sub|Port
    pub declaration: SourceSpan,
    pub references: Vec<SourceSpan>,  // source order; excludes decl unless include_declaration
}

/// None ⇒ not renameable here (editor must refuse).
pub fn prepare_rename(parsed: &ParsedModule, pos: Position) -> Option<RenameTarget>; // {range, placeholder}

/// WorkspaceEdit covering exactly `declaration ∪ references`.
pub fn compute_rename(parsed: &ParsedModule, pos: Position, new_name: &str) -> Option<WorkspaceEdit>;
```

**Invariants.**
1. **Scope soundness (no false positives).** Every span in `references` lexically resolves to `declaration` under the same precedence `CompilationScope` uses: params→lets→autos in order, guarded-block (`where`/`else`) members visible only within their guard, innermost binding shadows outer. A same-named symbol in another structure/guard/scope is never included.
2. **Completeness (no false negatives) within the document** for the listed `RefSymbolKind`s.
3. **Determinism.** Output is source-ordered and idempotent.
4. **Rename refusal.** `prepare_rename` returns `None` for keywords, literals, builtins, and (single-file phase) cross-module bindings; the editor then shows "can't rename here" rather than editing.
5. **Edit validity.** Applying `compute_rename`'s edit yields a buffer that re-parses with **0 new ERROR nodes** and in which `new_name` resolves identically to the old binding.

### Frontend seam: LSP methods over `lsp_request`

| Method | Params | Result |
|---|---|---|
| `textDocument/references` | `{textDocument, position, context:{includeDeclaration}}` | `Location[]` |
| `textDocument/prepareRename` | `{textDocument, position}` | `{range, placeholder}` \| `null` |
| `textDocument/rename` | `{textDocument, position, newName}` | `WorkspaceEdit` |
| `textDocument/documentHighlight` | `{textDocument, position}` | `DocumentHighlight[]` (kind Text) |
| `textDocument/documentSymbol` | `{textDocument}` | `DocumentSymbol[]` (hierarchical) |

Capabilities advertised in `server.rs` `initialize`: `references_provider`, `rename_provider{prepareProvider:true}`, `document_highlight_provider`, `document_symbol_provider`.

## Boundary-test sketch (B+H)

Each row faces **both** the collector (unit test in `reify-lsp`) and the frontend (apply-edit / populate-panel via vitest or reify-debug MCP).

| Scenario | Precondition | Postcondition (asserts) |
|---|---|---|
| Same name, two structures | `width` declared in `A` and `B` | rename `A.width` → only `A`'s spans change; `B` untouched |
| Let shadows param | `param x` then `let x` in same body | rename targets only the innermost binding's uses; the other binding + its uses untouched |
| Guarded-block member | member in `where C {…} else {…}` | uses inside the guard resolve to the guarded decl; outside-guard same-name does not match |
| prepareRename refusal | cursor on keyword / number / builtin | `prepareRename` → `null`; editor shows non-renameable, makes no edit |
| Rename re-parses clean | rename a param used 3× | resulting buffer parses with 0 new ERROR nodes; new name resolves identically |
| includeDeclaration toggle | find-refs on a member | declaration present iff `includeDeclaration=true` |
| documentHighlight ≡ in-doc references | cursor on a member | highlight set == references set restricted to active doc |
| Cross-file rename (phase κ) | structure used in an imported file | `WorkspaceEdit` touches both files; both re-parse clean |
| Cross-module refusal (single-file phase) | cursor on imported symbol | `prepareRename` → `null` until phase κ lands |

The collector contract test (rows 1–5) + the find-refs end-to-end leaf (β) are the **integration gate** for the seam; the explicit end-to-end GUI leaf (λ) re-asserts it through the running app.

## Decomposition plan

Greek labels are PRD-local; task IDs are assigned at decompose. "Modules" = crates/dirs touched.

**Phase 0 — Foundation (the H-contract producer)**
- **α — Scope-aware reference-collector (`reify-lsp/references.rs`), single-file.** Modules: `crates/reify-lsp`. Builds `collect_references`/`prepare_rename`/`compute_rename` over the parsed AST, mirroring `CompilationScope` precedence. *Intermediate* — unlocks β/γ/δ. **Signal (as unlocker):** the §Boundary-test rows 1–5 pass as `reify-lsp` unit tests (scope soundness + completeness + rename-reparse). Prereqs: none.

**Phase 1 — Find references (first vertical slice + contract integration gate)**
- **β — `textDocument/references` + Find-uses panel (`Shift+F12`).** Modules: `crates/reify-lsp`, `gui/src/editor`, `gui/src`. Advertise `references_provider`; frontend panel lists results, click navigates (pushes nav history). **Signal:** in the running GUI, cursor on a member used N×, `Shift+F12` → panel lists N occurrences; clicking one moves the editor cursor there (reify-debug MCP: `type_in_editor`/`keyboard`/`editor_content`). Prereqs: α.

**Phase 2 — Rename**
- **γ — `prepareRename`+`rename` + `F2` inline rename.** Modules: `crates/reify-lsp`, `gui/src/editor`, `gui/src`. Apply `WorkspaceEdit` via the editor's `update_source`/dispatch path. **Signal:** `F2` on a param, type new name → all in-scope occurrences update, out-of-scope same-name untouched, buffer re-parses clean, file persists (reify-debug MCP `editor_content` before/after). Prereqs: α.

**Phase 3 — Occurrence highlight**
- **δ — `documentHighlight` + CM decoration on cursor-idle.** Modules: `crates/reify-lsp`, `gui/src/editor`. **Signal:** placing the cursor on a member name subtly highlights its other in-scope occurrences in the buffer; moving off clears them. Prereqs: α.

**Phase 4 — Folding (frontend-only)**
- **ε — Wire `codeFolding()`+`foldGutter()`+`foldKeymap` + fold-all/unfold-all.** Modules: `gui/src/editor`, `gui/src/shortcuts.ts`, `gui/src/components/KeyboardHelp.tsx`. **Signal:** fold arrows appear in the gutter beside `{` blocks; clicking folds the scope; `Ctrl+Shift+[`/`]` fold/unfold; the new keys show in the `?` overlay. Prereqs: none (foldNodeProp already configured).

**Phase 5 — Navigation hotkeys + history (frontend)**
- **ζ — `F12` go-to-def + back/forward nav stack (`Alt+←`/`Alt+→`).** Modules: `gui/src/editor`, `gui/src/hooks`, `gui/src/shortcuts.ts`. `F12` reuses the shipped goto path; the nav stack is the net-new mechanism, fed by goto/find-refs/cross-pane reveal. **Signal:** `F12` jumps to definition; `Alt+←` returns to the prior position; new keys appear in the `?` overlay. Prereqs: none (β/γ register their own `Shift+F12`/`F2`).

**Phase 6 — Command palette + symbol jump**
- **η — `textDocument/documentSymbol` (parsed-module walk).** Modules: `crates/reify-lsp`. *Intermediate* — unlocks θ. **Signal (as unlocker):** the provider returns the hierarchical symbol list for a fixture, asserted by a `reify-lsp` unit test. Prereqs: none.
- **θ — Command palette (`Ctrl+Shift+P`) + symbol-jump (`@`/`Ctrl+Shift+O`).** Modules: `gui/src/components`, `gui/src`. Commands from `shortcuts.ts`; symbol mode consumes η. **Signal:** `Ctrl+Shift+P` opens a fuzzy-filter palette of registered commands, Enter runs the command; typing `@` switches to symbol list, Enter moves the cursor to that symbol. Prereqs: η.

**Phase 7 — Hover-sync correspondence**
- **ι — Outline/code hover → viewport+outline highlight.** Modules: `gui/src`, `gui/src/panels`, `gui/src/viewport`. Reuse `selectionStore.hoverEntity` + viewport `setHovered` + `get_entity_at_source_location`. **Signal:** hovering an outline row highlights the matching mesh (emissive) in the viewport; hovering a structure name in code highlights its mesh + outline row; hover off clears. Prereqs: none.

**Phase 8 — Cross-file extension (the substrate-risk phase)**
- **κ — Cross-file references + rename over the import graph.** Modules: `crates/reify-lsp`, `gui/src-tauri`, `gui/src`. Extends α/β/γ using goto_def's `resolve_import`; **builds the multi-document workspace view** the in-process LSP currently lacks; `WorkspaceEdit` spans files. **Signal:** rename a structure used in an imported file → both files update and re-parse clean; find-refs lists uses across files. Prereqs: α, β, γ. *(G3-flagged: this task owns the workspace-document substrate; no other task assumes it.)*

**Phase 9 — End-to-end integration gate (B+H)**
- **λ — End-to-end GUI integration test.** Modules: `gui` (test harness). Drives the running app via reify-debug MCP (`open_file`→`type_in_editor`→`keyboard`→`editor_content`/`screenshot`) to exercise rename, find-refs, folding, palette, and hover-sync against a real `.ri`. **Signal:** the scripted session shows rename rewriting the buffer, the refs panel populating, a scope folding, the palette running a command, and an outline-hover lighting the viewport — all in one CI-able run. Prereqs: β, γ, δ, ε, θ, ι.

**DAG:** α→{β,γ,δ}; β,γ→κ; η→θ; {β,γ,δ,ε,θ,ι}→λ. ε, ζ, η, ι are independent of α.

## Out of scope (future PRDs)

- Multi-cursor / column selection, signature help, code actions / quick-fixes, fuzzy *file* search (Reify GUI is largely single-document today), breadcrumbs, minimap. Deferred per the "named asks + high-value staples" scope decision.
- Refactors beyond rename (extract-structure, inline). 
- The four viewport/outline **defects** — filed as a standalone bug batch (outline overlay-scrollbar / empty def-preview / grid-over-axes / axis labels), not designed here.

## Open questions (tactical — defer to implementation)

1. **Rename UX surface.** Inline CodeMirror rename field vs. a small modal. *Suggested:* inline field (matches VS Code `F2`). Decide in γ.
2. **References presentation.** Dedicated bottom panel vs. CodeMirror peek-inline. *Suggested:* reuse the existing diagnostics-panel docking pattern for consistency. Decide in β.
3. **Occurrence-highlight trigger debounce.** ms before documentHighlight fires on cursor-idle. *Suggested:* ~150 ms, matching `useEditorSelectionSync`. Decide in δ.
4. **Nav-history depth + whether cross-pane reveals push entries.** *Suggested:* bounded 50, cross-pane reveals push. Decide in ζ.
5. **Palette symbol-jump key.** `Ctrl+Shift+O` vs `@`-prefix-in-`Ctrl+Shift+P` vs both. *Suggested:* both. Decide in θ.
