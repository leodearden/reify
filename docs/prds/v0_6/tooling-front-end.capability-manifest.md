# Capability manifest — tooling-front-end.md

Authored 2026-10-12 with the PRD (pass 1 of the `review-all-reify-20261010` program, stream
S27); amended 2026-10-12 at decomposition (pass 2) for the lead's rulings OQ1–OQ4
(PRD §7). Machine-readable twin:
`tooling-front-end.capability-manifest.yaml` (task ids stamped at `commit_planning`). Binds
each leaf's signal to substrate evidence (G3 + G6 mechanized). Leaves γ and δ carry `OPEN`
rows were resolved by the rulings: γ consumes `compile_program` (#5516) and δ is not
filed (#6665 deletes the MCP context). λ and φ are held leaves.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). Cite-by-symbol; re-locate
lines at implementation time.

**Probe provenance.** `target/debug/reify` built 2026-10-11 19:33 against main. Committed
fixtures (new, unregistered — no Rust test reads them yet; a leaf that adds one registers it
in `_RUST_COUPLED_RI_FIXTURES` in the same diff):

| fixture | tree-sitter `parse --quiet` | `reify check` |
|---|---|---|
| `tests/prd-gate/fixtures/tooling_fe_stdlib_enum.ri` | exit 0 | exit 0, `All constraints satisfied.` |
| `tests/prd-gate/fixtures/tooling_fe_unrepresentable_generic.ri` | exit 0 | exit 101, debug panic `unrepresentable cell_type: value cell 'Bearing.seal' has cell_type TypeParam("T")` |
| `tests/prd-gate/fixtures/tooling_fe_purpose_nested_members.ri` | exit 0 | exit 0 |
| `tests/prd-gate/fixtures/tooling_fe_hover_decl_kinds.ri` | exit 0 | exit 0 |

Other probes: `reify mcp-server tests/prd-gate/fixtures/tooling_fe_stdlib_enum.ri` →
`reify_get_diagnostics` returns `unresolved type: InfillPattern` and
`unknown enum type 'InfillPattern'`; a units-only module returns `[]` (units resolve
without a prelude, so the `reify_compiler::compile` rustdoc's units sentence is stale). The
stdio transport answers `notifications/initialized` with
`{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}`.
`reify doc --stdlib --out <dir>` emits 269 `&lt;default&gt;`.

`grammar_confirmed = true` on every leaf: no novel syntax.

**Delivered-check polarity.** Every `expect: present` grep pattern below matched 0 times at
`8e6ffcac9d` and every `expect: absent` pattern matched ≥ 1 (measured with `git grep -E -c`
over the listed paths), so each check changes verdict when its leaf lands. Fixture-existence
rows carry no delivered check: the fixtures land with the PRD and would be green on arrival.

## Per-leaf bindings

| leaf | capability | evidence | verdict | delivered check |
|---|---|---|---|---|
| α | ratchet member exists | producer-self; `scripts/lib_rust_production_view.sh` exists for the production view | PASS | path `tests/infra/test_tooling_front_end_ratchet.py` present |
| χ | decl-oracle module | producer-self; record h14 unit list | PASS | path `crates/reify-lsp/src/decl_oracle.rs` present |
| ψ | walk leaf module | producer-self; record h14 unit list | PASS | path `crates/reify-lsp/src/references/walk.rs` present |
| τ | check-verdict module | producer-self; record h14 unit list | PASS | path `crates/reify-cli/src/check_verdict.rs` present |
| υ | cmd modules | producer-self; after τ and #6665 | PASS | path `crates/reify-cli/src/cmd/check.rs` present |
| φ | engine-config module (HELD S1) | `KNOWN_SITES` in `scripts/check-compute-trampoline-registration.sh` pins `crates/reify-cli/src/main.rs` (S1-owned) | PASS | path `crates/reify-cli/src/engine_config.rs` present |
| ε | result carries `check_result` | `DiagnosticsResult` has three fields at main | PASS | grep `pub check_result: Option<` in `crates/reify-lsp/src` present |
| ε | wrapper, not a second pipeline | behavioural, PRD §5 B6 | PASS | manual |
| ω | tests module | producer-self; after ε | PASS | path `crates/reify-lsp/src/diagnostics/tests.rs` present |
| ζ | accessor substrate | producer #8743 (S9 ν), upstream edge; absent at main; ζ lowers S9 R4's reify-lsp/reify-cli rows (#8730 member) | PASS | grep `\.all_value_cells\(\)` in `crates/reify-lsp/src` present |
| ζ | guarded FEA-hint fixture | ζ's own deliverable; D3 probes the premise at decompose | OPEN | manual |
| η | `member_scopes` on `Declaration` | absent at main; `PurposeDef.structures` exists | PASS | grep `fn member_scopes` in `crates/reify-ast/src` present |
| η | purpose-nested fixture | `tooling_fe_purpose_nested_members.ri` parses, checks clean | PASS | — |
| θ | hover names `Joint` | no Joint/Field/Purpose/Constraint arm in `hover.rs` | PASS | grep `Declaration::Joint` in `crates/reify-lsp/src/hover.rs` present |
| θ | hover-kinds fixture | `tooling_fe_hover_decl_kinds.ri` parses, checks clean | PASS | — |
| κ | `LspMethod` enum | `handle_request` matches strings at main | PASS | grep `pub enum LspMethod` in `crates/reify-lsp/src` present |
| κ | GUI lane consumes it | `lane_for_method` lists 8 strings | PASS | grep `LspMethod` in `gui/src-tauri/src/lsp_bridge.rs` present |
| λ | generated TS method type (HELD S26) | S26's generator, wave 3 | OPEN | manual |
| ι | hover placed | hover/completion inline at main; run label `"hover"` | PASS | grep `\.run\("hover"` in `crates/reify-lsp/src/server.rs` present |
| μ | `dev_stdlib_root` home | literal 3× server.rs, 1× main.rs, 1× gui engine.rs | PASS | grep `fn dev_stdlib_root` in `crates/reify-compiler/src` present |
| π | family predicate | no `impl DiagnosticCode` at main; 9 `AutoTypeParam*` variants | PASS | grep `fn is_auto_type_param` in `crates/reify-core/src` present |
| π | parse-error code adopted | producer #8742 (S9 μ), upstream edge | PASS | grep `diagnostic_code\(\)` in `crates/reify-lsp/src/convert.rs` present |
| β | predicate in reify-eval | private to `reify-lsp` `diagnostics::eval_guard` at main | PASS | grep `pub fn first_unrepresentable_cell` in `crates/reify-eval/src` present |
| β | shape-two fixture | probe: exit 101 panic, zero compile diagnostics | PASS | — |
| γ | AnalysisContext on `compile_program` | producer #5516 (upstream edge); `compile_with_stdlib_checked` at main | PASS | grep `compile_program\(` in `crates/reify-lsp/src/analysis.rs` present |
| γ | module-name derivation gone | `pub fn module_name_from_uri` at main | PASS | grep `pub fn module_name_from_uri\(` in `crates/reify-lsp/src` absent |
| γ | stdlib-enum divergence | probe: check clean, MCP unresolved | PASS | — |
| ν | chunk table | three in-file lists; reify-mcp depends only on reify-core | PASS | grep `pub const CHUNKS` in `crates/reify-mcp/src` present |
| ν | no source-text scan | `TOPICS_ANCHOR` in `fence_gate.rs` at main | PASS | grep `TOPICS_ANCHOR` in `crates/reify-compiler/tests/harness_doc_chunks/fence_gate.rs` absent |
| ξ | false consumer claim gone | reify-config rustdoc; five dependent crates | PASS | grep `no other workspace crate consumes it` in `crates/reify-config/src` absent |
| ο | optional id | probe: `-32700` reply to a notification | PASS | grep `pub id: Option<` in `crates/reify-mcp/src/jsonrpc.rs` present |
| ρ | stdlib sources public | `stdlib_sources` is `pub(crate)`; probe 269 placeholders | PASS | grep `^pub fn stdlib_sources` in `crates/reify-compiler/src/stdlib_loader.rs` present |
| ρ | typed expression text | six strategies in `build.rs` | PASS | grep `enum ExprText` in `crates/reify-doc/src` present |
| σ | `NodeId: FromStr` | `Display` only at main | PASS | grep `FromStr for NodeId` in `crates/reify-eval/src` present |
| σ | hand parser deleted | `parse_node_id` in `dev.rs` | PASS | grep `fn parse_node_id` in `crates/reify-cli/src/dev.rs` absent |
| Ω | terminal stamp | docs-only | PASS | manual |
| ψ₂ (#9324, gap-fill 2026-10-16) | shared enumeration substrate | producer S9 ρ (#9321), upstream edge; ψ #8757 done | PASS | — |
| ψ₂ | walk.rs helpers deleted | `for_each_direct_subexpr` and four siblings in references/walk.rs; local.rs hand-matches at main | PASS | grep `fn for_each_direct_subexpr\b` in `crates/reify-lsp/src` absent |
| ψ₃ (#9325, gap-fill 2026-10-16) | per-feature cursor seams | `find_word_at_offset` seam; member-segment guard only in references; goto_def Phase C lexical (#7607, subsumed); no cursor.rs at main | PASS | — |
| ψ₃ | cursor module | producer-self | PASS | path `crates/reify-lsp/src/cursor.rs` present |
| ψ₃ | `CursorTarget` enum | producer-self | PASS | grep `enum CursorTarget\b` in `crates/reify-lsp/src` present |
| γ₂ (#9326, gap-fill 2026-10-16) | compile+check spelled thrice | from_parsed, compute_diagnostics_with_state, compute_diagnostics; per-request compile on hover/completion | PASS | — |
| γ₂ | `compile_program` entry | producer #8778 (γ), upstream edge | PASS | — |
| γ₂ | `analyse` constructor | producer-self | PASS | grep `fn analyse\(` in `crates/reify-lsp/src/analysis.rs` present |
| ε₂ (#9327, gap-fill 2026-10-16) | monolithic pipeline | ~495-line `compute_diagnostics_with_state`; prose EvalState ordering rule; string codes | PASS | — |
| ε₂ | typed LSP codes | producer-self | PASS | grep `enum LspDiagnosticCode\b` in `crates/reify-lsp/src` present |

**Accepted gaps.** `\.run\("hover"` binds ι to that label; the behavioural cover is §5 B5's
placement rig. `Declaration::Joint` proves θ's arm exists, not its content; B1 is the cover.
`LspMethod` in `lsp_bridge.rs` proves adoption, not routing equality; B4 is the cover.
