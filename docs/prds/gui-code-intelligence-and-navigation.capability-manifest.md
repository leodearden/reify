# Capability manifest — gui-code-intelligence-and-navigation

Mechanizes G3 (assumed-substrate verified / wired) + G6 (premise validity) per leaf. Each binding: capability → evidence. Any FAIL blocks the batch. Verified 2026-06-02 against `main`.

PRD: `docs/prds/gui-code-intelligence-and-navigation.md`. Task IDs: α=4201, β=4202, γ=4203, δ=4204, ε=4205, ζ=4206, η=4207, θ=4208, ι=4209, κ=4210, λ=4211.

**Domain note (G6 branches 1/2):** no numeric bounds or closed-form-exactness claims in this PRD — branches 1/2 are N/A throughout. The load-bearing G6 check here is branch 3 (end-to-end capability / DAG-direction) plus the rename **scope-soundness** premise, validated by boundary-test fixtures rather than a numeric floor. Empty-value sentinel (`Value::Undef`) field-population check is N/A (no result-field sampling).

---

### α — reference-collector (4201) · INTERMEDIATE
| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Identifier USES carry source spans on the parsed AST | substrate-exists | `crates/reify-ast/src/ast.rs:14` (`Expr.span`); `ExprKind::Ident(String)` :34 | PASS |
| Enclosing-decl + member-by-name lookup to build on | substrate-exists | `crates/reify-lsp/src/analysis.rs` `enclosing_decl_at` / `find_named_member_span`; `goto_def.rs` | PASS |
| Scope precedence model to mirror | substrate-exists | `crates/reify-compiler/src/scope.rs` `CompilationScope` (params→lets→autos, guarded `where`/`else`, shadowing) | PASS |
| Compiled IR is NOT a viable source (no spans/names) | anti-premise (drove "use parsed AST") | `crates/reify-ir/src/expr.rs` `ValueRef(ValueCellId)`, no span field | PASS (design avoids it) |
| Scope soundness (no cross-scope false positives) | G6 branch-3 premise | the collector IS the producer; boundary-test rows 1–3 are its RED signal | PASS (producer=this task) |

### β — references provider + Find-uses panel (4202) · LEAF / integration gate
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `collect_references` | capability→producer | `producer:task-4201` (α), upstream via dep edge | PASS |
| `lsp_request` IPC bridge wired on main | wired-on-main | `gui/src-tauri/src/main.rs:434` `lsp_request`; `gui/src/editor/lspClient.ts` | PASS |
| capability-advertise path | wired-on-main | `crates/reify-lsp/src/server.rs` initialize capabilities | PASS |
| shortcut/KeyboardHelp registration | wired-on-main | `gui/src/shortcuts.ts`, `gui/src/components/KeyboardHelp.tsx` | PASS |
| DAG-direction | anti-inversion | α (4201) is upstream of β | PASS |

### γ — prepareRename + rename + F2 (4203) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `prepare_rename`/`compute_rename` | capability→producer | `producer:task-4201` (α), upstream | PASS |
| Buffer write path to apply WorkspaceEdit | wired-on-main | `gui/src-tauri/src/main.rs:266` `update_source` + editor dispatch | PASS |
| prepareRename refusal guards partial edits | G6 branch-3 premise | invariant 4 (§Contract); RED test = "rename on keyword → no edit" | PASS |
| Edit re-parses clean | G6 premise | invariant 5; RED test = "0 new ERROR nodes after rename" | PASS (verifiable via parser) |
| DAG-direction | anti-inversion | α upstream of γ | PASS |

### δ — documentHighlight + decorations (4204) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `collect_references` (in-doc filter) | capability→producer | `producer:task-4201` (α), upstream | PASS |
| CM decoration extension surface | wired-on-main | `gui/src/editor/` (CodeMirror 6 `@codemirror/view` Decoration) | PASS |
| DAG-direction | anti-inversion | α upstream of δ | PASS |

### ε — folding wiring (4205) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `foldNodeProp` configured for Block | grammar/substrate | `gui/src/editor/reifyLanguage.ts:15` | PASS |
| `codeFolding`/`foldGutter`/`foldKeymap`/`foldAll` exported | substrate-exists | runtime check 2026-06-02: all present in `@codemirror/language@^6.10` | PASS |

### ζ — F12 + nav history (4206) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| goto-def resolution path to reuse for F12 | wired-on-main | `gui/src/editor/gotoDefinition.ts` (Ctrl+Click → textDocument/definition, shipped) | PASS |
| editor position API for the nav stack | substrate-exists | CodeMirror `EditorView` selection/scroll | PASS |

### η — documentSymbol provider (4207) · INTERMEDIATE
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| parsed-module declaration/member walk | substrate-exists | `crates/reify-lsp/src/analysis.rs` declaration traversal | PASS |
| DAG-direction (feeds θ) | anti-inversion | η upstream of θ | PASS |

### θ — command palette + symbol jump (4208) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| command source = shortcut registry | wired-on-main | `gui/src/shortcuts.ts` SHORTCUTS + `useKeyboardShortcuts.ts` ID→callback | PASS |
| symbol source = documentSymbol | capability→producer | `producer:task-4207` (η), upstream | PASS |
| DAG-direction | anti-inversion | η upstream of θ | PASS |

### ι — hover-sync (4209) · LEAF
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `selectionStore.hoverEntity` | wired-on-main | `gui/src/stores/selectionStore.ts` | PASS |
| viewport `setHovered` emissive highlight | wired-on-main | `gui/src/viewport/selection.ts:150` | PASS |
| code-token → entity resolution | wired-on-main | `get_entity_at_source_location` Tauri command (`gui/src-tauri/src/main.rs`) | PASS |

### κ — cross-file references + rename (4210) · LEAF · G3-FLAG
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| import resolution framework | substrate-exists | `crates/reify-lsp/src/goto_def.rs:73` `resolve_import` (definition only today) | PASS |
| multi-document LSP workspace view | substrate-ABSENT → built here | the in-process LSP holds one document; **this task owns building the workspace set** through `gui/src-tauri/src/lsp_bridge.rs`. No other task assumes it. | PASS (queued upstream deps α/β/γ; substrate built by this leaf) |
| DAG-direction | anti-inversion | α,β,γ upstream of κ | PASS |

### λ — end-to-end integration gate (4211) · LEAF / B+H gate
| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| GUI-driving harness | substrate-exists | reify-debug MCP (`open_file`/`type_in_editor`/`keyboard`/`editor_content`/`screenshot`) | PASS |
| all exercised affordances exist upstream | capability→producer | `producer` β,γ,δ,ε,θ,ι — all upstream via dep edges | PASS |
| DAG-direction | anti-inversion | β,γ,δ,ε,θ,ι upstream of λ | PASS |

---

**Result:** no FAIL bindings. The single substrate-ABSENT item (κ's multi-document workspace) is explicitly owned and built by κ itself with α/β/γ wired upstream — it is a queued prerequisite-internal-to-the-task, not an unverified assumption. Batch cleared to queue.
