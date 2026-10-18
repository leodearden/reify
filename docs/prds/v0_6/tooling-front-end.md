# PRD: Tooling front end — one compile front end per surface, one LSP method table, one member-scope oracle

**Milestone:** v0_6 · **Status:** active — decomposed 2026-10-12 (pass 2); lead rulings OQ1–OQ4 applied (§7); gap-fill 2026-10-16: ψ₂ #9324, ψ₃ #9325, γ₂ #9326, ε₂ #9327 · **Approach:** B + H

Authored 2026-10-12 by the S27 `/prd` seat of program `review-all-reify-20261010`
(`plans/review-all-program-reify-20261012.md`, stream **S27 Tooling front end**, wave 1,
mode agent, M/medium). Ruling source: the program doc §Resolved decisions → "Uncontested
block: … S27 tooling front end (one compile_user_module used by CLI, MCP context, LSP)".
This PRD does not relitigate that ruling; §2 records two places where the code and the
task store at main contradict it, and §7 returns those to the lead.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12); the review's anchors were
verified at `a030ee7528` (2026-10-10) and `git diff a030ee7528..main` over every path below
shows only a doc-comment edit in `crates/reify-cli/src/mcp_context.rs` and additions under
`crates/reify-compiler/tests/harness_doc_chunks/` that do not touch the fence gate's
`TOPICS_ANCHOR` scan. Main moves fast — cite-by-symbol; re-locate lines at implementation
time. Runtime probes ran on `target/debug/reify` built 2026-10-11 19:33 against main.

---

## §0 — Goal and user-observable surface

The tooling front end is everything between "a `.ri` file or editor buffer" and "a
`CompiledModule` an engine evaluates": parse, module path, compile with a checker, the
module-path rule, and the question "is this graph safe to evaluate". Around it sit the
LSP's request plumbing, its declaration oracles, and four small registries (language
chunks, JSON-RPC envelope, diagnostic-code families, doc rendering). Today each is spelled
by hand per surface and the copies differ in substance (findings ledger, §1).

After this PRD lands, a user observes:

- An editor user hovering the name of a `field def`, `purpose`, `constraint def` or
  `joint` gets hover content. Today: nothing (`compute_hover_in_context` ends in `_ => {}`).
- Goto-definition, hover, find-references, rename and highlight on a member of a
  `structure def` nested inside a `purpose` resolve to that member
  (`tests/prd-gate/fixtures/tooling_fe_purpose_nested_members.ri`). Today they stop at the
  purpose's own members.
- A constraint that reaches an `@optimized` solver through a `where`-guarded `let` gets the
  `fea-not-evaluated` hint, and a guarded member's computation-failed/pending state is
  reported. Today guarded members are invisible to both emitters.
- Hover and completion no longer run compile + check on the stdio server's shared async
  worker.
- `reify doc --stdlib --out <dir>` renders real default text for stdlib params. Today the
  HTML carries 269 `&lt;default&gt;` placeholders (probe, §2.3).
- `reify dev inspect-node` accepts the node id it prints on its own `node:` line.
- An MCP client's `notifications/initialized` gets no reply line. Today the stdio
  transport answers it with `{"id":null,"error":{"code":-32700,"message":"Parse error"}}`
  (probe, §2.3).
- Adding an LSP method, a declaration kind, a doc chunk or an `AutoTypeParam*` code reds a
  test that names the missing site, instead of compiling cleanly through a `_` arm or a
  hand-copied list.

The compile-front-end outcome is the stream's high-severity goal. Per the lead's rulings
(§7): the one front end is resolution-unification's `compile_program` (#5516), which S27's γ
brings the LSP's `AnalysisContext` and remaining LSP compile sites onto; the CLI MCP context
is deleted by #6665 rather than migrated.

## §1 — Background: findings ledger

Every key the stream owns, with the leaf that closes it (§10). One key, one leaf.

| key | anchor | sev | verdict | leaf |
|---|---|---|---|---|
| fk-f5264d76e3ad | `reify-cli mcp_context.rs::CliToolContext` — no single owner of the user-module front end | high | confirmed | γ (β contributing; MCP part closed by #6665) |
| fk-41819ea934bd | `reify-lsp diagnostics.rs::compute_diagnostics` — second, divergent pipeline | medium | confirmed | ε |
| fk-b420a7695f68 | `diagnostics.rs::compute_diagnostics_with_state` — guarded members missing from value-cell walks | medium | confirmed | ζ |
| fk-3a3ae797f979 | `references.rs::entity_members` — five wildcard member-list matches, no purpose-nested descent | medium | confirmed | η |
| fk-1a772d160168 | `server.rs::ReifyLanguageServer` — CPU placement decided per handler | medium | confirmed | ι |
| fk-74ce7cce677f | `server.rs::build_cross_file_rig` — three resolver closures, five stdlib-root literals | low | weakened | μ (subsumes #8557) |
| fk-5854202d4aed | `diagnostics.rs::EvalState` — tests read private fields | low | confirmed | ε |
| fk-d9b1ed83ed62 | `hover.rs::compute_hover_in_context` — 212-line fall-through, 7 of 11 kinds | medium | confirmed | θ |
| fk-7b9fad8a5d00 | `bridge.rs::InProcessLsp` — method set spelled in 5 Rust sites + 1 TS | medium | confirmed | κ (λ held for the TS half; subsumes #8447) |
| fk-2da836d68189 | `reify-mcp language_chunks.rs::get_chunk` — 3 in-file lists + cross-crate text scan | medium | confirmed | ν |
| fk-677d67cb2ac8 | `reify-config lib.rs::Manifest` — reify.toml inert, docs say otherwise | medium | weakened | ξ (subsumes #8336) |
| fk-bed8cadb429f | `reify-mcp jsonrpc.rs::JsonRpcRequest` — notifications answered with -32700 | low | confirmed | ο |
| fk-3e8bf5666555 | `reify-lsp convert.rs::convert_diagnostic` — 6 of 9 AutoTypeParam codes traced | low | confirmed | π |
| fk-10941c98934e | `reify-doc-build build.rs::build_doc_model` — six expression-text strategies | low | confirmed | ρ |
| fk-972b7d28a290 | `reify-cli dev.rs::parse_node_id` — hand parser, output not re-pasteable | low | confirmed | σ (subsumes #7720) |

The stream also performs the h14 partitions the record marks `stays_whole: false`:
`crates/reify-cli/src/main.rs` (7 units), `crates/reify-lsp/src/analysis.rs` (4),
`crates/reify-lsp/src/references.rs` (6), `crates/reify-lsp/src/diagnostics.rs` (2). The
symbol lists per unit are the record's h14 entries for those paths
(`plans/review-all-reify-20261010.json`); §10 τ–ω restate the unit names only.

Weakened findings re-read under the freshness clause: fk-74ce7cce677f's skeptic note (the
rig's doc does name its two consumers) removes the "believed shared" premise but not the
SPOT duplication, which is what #8557 already records — μ proceeds on the duplication.
fk-677d67cb2ac8's skeptic note corrects the dependent-crate count to five (reify-cli,
reify-compiler, reify-eval, reify-runtime, gui/src-tauri); ξ uses five.

## §2 — Premise corrections found at authoring (G6)

### §2.1 The CLI MCP context is ruled for deletion (task #6665)

The S27 ruling names "MCP context" as a front-end consumer. Task **#6665**
carries Leo's 2026-08-26 ruling: *delete `reify mcp-server` and `CliToolContext` outright —
dangerous cruft, zero consumers*, deleting `crates/reify-cli/src/mcp_context.rs` wholesale
and **keeping** the `reify-mcp` crate (registry, stdio and in-process transports, chunk
serving). `docs/prds/v0_6/driver-contract-implementation.md` §6 decision 11 ("This PRD does
no work on `reify mcp-server`. Its deletion is ratified.") and task #6851's 2026-09-26 note
both build on it. The S27 ruling and #6665 cannot both stand → **OQ1**.

### §2.2 A canonical compile entry point is already designed and filed (resolution-unification)

`docs/prds/v0_6/resolution-unification.md` (status active, B+H, authored 2026-07-24 with
Leo) §3.1 specifies **one** compiler entry point,
`compile_program(entry_path, entry_source, resolver, cfg, checker) -> CompiledProgram`,
whose internals are `parse_with_stdlib`, the cfg-gated DAG walk, compile and
`attach_module_path_diag` — i.e. it already owns module path from the entry path,
the module-path rule and the checker. Its P1 leaves route all seven CLI commands
(#5517 γ, #5518 δ), the GUI (#5519 ε) and LSP diagnostics (#5520 ζ) through it, behind
#5516 β (pending, high). #6797 (driver-contract ο) owns the LSP module-header assertion
and is ordered behind #5520. #6805 (driver-contract χ) is the cross-driver
diagnostic-set parity gate *including the LSP*, extending #6700 and #5521, and records
"DO NOT BUILD A FOURTH HARNESS".

A second function `compile_user_module(source, path, checker)` in reify-compiler would be
the eighth compile entry point resolution-unification exists to collapse, and the G4
table's "S27 owns the front-end function" contests resolution-unification's ownership of
the same region (`module_dag.rs`, `lib.rs`). → **OQ2**.

### §2.3 Probes (target/debug/reify at main)

| probe | result |
|---|---|
| `reify check tests/prd-gate/fixtures/tooling_fe_stdlib_enum.ri` | exit 0, `All constraints satisfied.` |
| `reify mcp-server tests/prd-gate/fixtures/tooling_fe_stdlib_enum.ri` → `reify_get_diagnostics` | `unresolved type: InfillPattern`, `unknown enum type 'InfillPattern'` — the no-prelude `compile` divergence is live for stdlib enums |
| same, a module using only `mm`/`deg` units | `[]` — units now resolve without a prelude, so the `reify_compiler::compile` rustdoc's "nine previously hard-coded units … unresolved-name diagnostics" sentence is stale; the finding's claim holds for stdlib enums and traits, not for units |
| `reify check` / `reify eval tests/prd-gate/fixtures/tooling_fe_unrepresentable_generic.ri` | exit 101, debug panic `unrepresentable cell_type: value cell 'Bearing.seal' has cell_type TypeParam("T")` — zero compile diagnostics, so no severity gate can catch it (#6851 "shape 2", re-verified here on the CLI) |
| `reify mcp-server` stdio: `initialize`, then `{"jsonrpc":"2.0","method":"notifications/initialized"}` | second output line `{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}` |
| `reify doc --stdlib --out <dir>` | 269 occurrences of `&lt;default&gt;` in the generated HTML |
| `reify check` on every `tests/prd-gate/fixtures/tooling_fe_*.ri` other than the generic | exit 0; all four parse with 0 ERROR nodes under `tree-sitter parse --quiet` |

## §3 — Sketch of approach

Four groups, each a small set of leaves:

1. **Front end** (β, γ; `compile_program` per OQ2-A). The surfaces stop spelling parse → module
   path → compile → module-path rule themselves; the eval-readiness question moves out of
   `reify-lsp`'s private `diagnostics::eval_guard` into `reify-eval`, the crate that owns
   the hazard, so every surface asks the same predicate and differs only in what it does
   with the answer.
2. **LSP internals** (ε, ζ, η, θ, ι, κ, μ, π). One diagnostics pipeline whose result carries
   the constraint outcomes; guarded members visible to every value-cell walk through S9's
   `TopologyTemplate::all_value_cells()`; one wildcard-free `Declaration::member_scopes()` in
   reify-ast; hover dispatching through an exhaustive match; one `LspMethod` table that
   owns the method set, its GUI concurrency class and its blocking-work placement; one
   import resolver with the stdlib root resolved once.
3. **Registries** (ν, ξ, ο, ρ, σ, π). Each list becomes one table its consumers derive from:
   the chunk table, an inert-status statement for reify.toml, a typed JSON-RPC id, one
   expression-text renderer, `NodeId: FromStr` as the inverse of its `Display`, and an
   `AutoTypeParam*` family predicate beside the enum.
4. **h14 partitions** (τ, υ, φ, χ, ψ, ω). Move-only splits along the record's unit lists,
   imports pointing downward only, tests moving with their unit. They land before the
   semantic leaves that edit the same files, so those leaves edit small files.

An adoption-ratchet member (α) lands first and carries one counted row per inline pattern
an extraction leaf retires (program INV candidate 5).

## §4 — Contract (H)

### §4.1 Eval-readiness predicate (β)

```rust
// reify-eval — homed beside the assertion it predicts
pub struct UnrepresentableCell { pub id: ValueCellId, pub cell_type: Type }
pub fn first_unrepresentable_cell(compiled: &CompiledModule) -> Option<UnrepresentableCell>;
```

- Same derivation as today's `eval_guard::first_unrepresentable_cell` (stage-one scan, then
  the `EvaluationGraph::from_templates` graph test against `is_representable_cell_type`);
  no third notion of "unrepresentable".
- Reporting stays per surface: the LSP keeps `SkippedEval` (log line + editor Warning); the
  throttling state stays in `reify-lsp`.
- `None` is the only answer that permits `Engine::eval`/`check` on that module from a
  surface that adopts it.

### §4.2 Compile front end (γ) — `compile_program` (resolution-unification §3.1)

The producer is resolution-unification β (#5516): `compile_program(entry_path,
entry_source, resolver, cfg, checker) -> CompiledProgram`. S27 adds no compile entry point.
The contract the LSP relies on, and γ asserts from the consumer side:

- one call per surface turns `(path or URI, source text, checker)` into the parsed module,
  the compiled module, and rendered parse errors; no surface derives a `ModulePath` from a
  path or URI itself;
- the module-path rule (`check_module_path_decl`) is applied inside that call, exactly once;
- the checker is a parameter (CLI and LSP pass `SimpleConstraintChecker`), never a hidden
  default;
- `AnalysisContext` builds its module through `compile_program` with the same resolver
  shape LSP diagnostics use after #5520, so hover/completion and diagnostics compile the
  same program; whether `AnalysisContext` keeps reusing the per-document parse cache or
  takes `compile_program`'s own parse is decided in γ against #5516's landed signature
  (§12 Q6) — a second parse per request is acceptable only if measured cheap.
- parse errors carry S9's `ParseErrorKind::diagnostic_code()` (#8742); reify-lsp's
  `convert_parse_error` copies it onto the LSP diagnostic (π).

### §4.3 LspMethod (κ)

```rust
// reify-lsp — one home for the method set
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LspMethod { Initialize, Initialized, DidOpen, DidChange, DidClose, Completion, Hover,
    Definition, DocumentSymbol, DocumentHighlight, PrepareRename, Rename, References, Shutdown }
pub enum LspConcurrency { Ordered, ConcurrentQuery }
impl LspMethod {
    pub const ALL: [LspMethod; 14];
    pub fn as_str(self) -> &'static str;          // the JSON-RPC method name
    pub fn concurrency(self) -> LspConcurrency;    // server-side property, exhaustive match
    pub fn runs_blocking_work(self) -> bool;       // exhaustive match; consumed by ι
    fn params_error_prefix(self) -> &'static str;  // replaces bridge::error_prefix consts
}
impl FromStr for LspMethod { type Err = UnsupportedLspMethod; }
```

- `InProcessLsp::handle_request` parses the string once and matches on `LspMethod`; an
  unknown name keeps today's `unsupported LSP method:` error text.
- `gui/src-tauri/src/lsp_bridge.rs::lane_for_method` routes on
  `LspMethod::from_str(m).map(LspMethod::concurrency)`; an unknown name keeps the ordered
  lane. Today's classification is preserved exactly (`rename` is `ConcurrentQuery`).
- `blocking_work::test_support::blocking_work_requests` and the three arrays in
  `gui/src-tauri/src/tests/lsp_lane_routing_tests.rs` iterate `LspMethod::ALL`.

### §4.4 Member scopes (η)

```rust
// reify-ast, beside Declaration
pub struct MemberScope<'a> { pub owner: &'a str, pub members: &'a [MemberDecl], pub span: SourceSpan }
impl Declaration { pub fn member_scopes(&self) -> impl Iterator<Item = MemberScope<'_>>; }
```

- Exhaustive over `Declaration` (no `_` arm): Structure, Occurrence, Trait yield one scope;
  Purpose yields its own members and then one scope per `PurposeDef.structures` entry;
  every other kind yields none, each named in the match.
- The five LSP lookups (`find_parsed_member_span_and_doc`, `enclosing_decl_name_at`, the two
  matches in `compute_goto_definition_with_parsed`, `references::entity_members`) and the
  outline (`symbol_children`) consume it; "innermost scope containing an offset" is a
  `reify-lsp` helper over it, not a sixth match.

### §4.5 Diagnostics result (ε)

```rust
pub struct DiagnosticsResult {
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    pub geometry_output: Option<Vec<u8>>,
    pub log_messages: Vec<LogLine>,
    /// None when parse errors or the eval-readiness predicate stopped the pipeline.
    pub check_result: Option<reify_eval::CheckResult>,
}
pub fn compute_diagnostics(source: &str, uri: &Url) -> Vec<lsp_types::Diagnostic> {
    compute_diagnostics_with_state(&mut EvalState::new(), source, uri).diagnostics
}
```

- Tests assert constraint outcomes through `check_result`, never through `EvalState`'s
  private `engine`, `last_content_hash` or `version_counter`; the `#[cfg(test)]`
  `ReifyLanguageServer::eval_state()` accessor is deleted if no test needs it after the port.

### §4.6 Registries

- **Chunks (ν):** `pub const CHUNKS: &[(&str, &str)] = &[("syntax", include_str!("chunks/syntax.md")), …]`
  in `reify-mcp`; `TOPICS`/`available_topics()` and `get_chunk` derive from it. The fence
  gate reaches the registry through `reify_mcp::tools::language_chunks::{available_topics,
  get_chunk}` as a dev-dependency of reify-compiler (reify-mcp depends only on reify-core,
  so no cycle) and proves reachability by content: every `chunks/*.md` file on disk equals
  the content `get_chunk` serves for some topic.
- **JSON-RPC (ο):** `JsonRpcRequest.id: Option<serde_json::Value>` with `#[serde(default)]`;
  `None` is a notification and produces no output line; responses go through one
  `ok`/`err` constructor pair; the literal `-32603` becomes `INTERNAL_ERROR`.
- **AutoTypeParam family (π):** `DiagnosticCode::AUTO_TYPE_PARAM: [DiagnosticCode; 9]` and
  `DiagnosticCode::is_auto_type_param(self)` derived from it, in reify-core beside the
  enum; `convert_diagnostic` calls the predicate; its test iterates the const.
- **Expression text (ρ):** one `ExprText` value per expression-valued doc field —
  `Source(String)` when a span and source exist, otherwise `Unavailable(Reason)` rendered
  identically by every formatter. `build_stdlib_doc_model` passes each stdlib module's real
  source (`stdlib_loader::stdlib_sources` made `pub`), so stdlib and user modules render the
  same way.
- **Node ids (σ):** `impl FromStr for NodeId` (reify-eval `cache.rs`) is the exact inverse
  of its `Display`, built from per-kind `FromStr` impls in reify-core `identity.rs`
  (`ValueCellId` and `RealizationNodeId` exist; `ConstraintNodeId`, `ResolutionNodeId`,
  `ComputeNodeId` are added). `reify dev inspect-node` parses with it and the hand parser
  `parse_node_id` is deleted.
- **reify.toml (ξ):** `Manifest`'s rustdoc states once that no shipped binary loads a
  reify.toml today; the per-section docs point at that statement; the false "no other
  workspace crate consumes it yet", the `warm_state_budget_bytes` "env > manifest > default"
  ladder sentence, the "callers MUST consume it via `Manifest::auto_type_params`" sentence,
  `docs/notes/reify-toml-schema.md` and the `review/briefing.yaml` consumption claim are
  corrected to match.

### §4.7 Import resolver and stdlib root (μ)

- `reify_compiler::module_dag::dev_stdlib_root(workspace_root: &Path) -> PathBuf` is the one
  home of the `crates/reify-compiler/stdlib` fallback.
- `ServerState` holds the resolved stdlib root, computed once at `initialize`;
  `goto_definition`, `prepare_rename`, `rename` and `references` share one
  `import_resolver(root, stdlib_root, open_docs)` and one document-lookup helper.

## §5 — Boundary-test sketch

| # | scenario | facing | preconditions | postconditions |
|---|---|---|---|---|
| B1 | hover on each named kind | LSP client ↔ hover | `tests/prd-gate/fixtures/tooling_fe_hover_decl_kinds.ri` open | non-empty hover on `heat`, `lightweight`, `Positive`, `pin_joint`; a test over `NAMED_DECL_SNIPPETS` reds when a row has no hover |
| B2 | purpose-nested member navigation | LSP client ↔ member_scopes | `tooling_fe_purpose_nested_members.ri` open | goto-def on the `depth` use in `doubled` lands on `param depth`; find-references returns both sites; outline unchanged |
| B3 | new `Declaration` variant | reify-ast ↔ every consumer | add a variant in a scratch build | `member_scopes` and hover's declaration match fail to compile until the variant is named |
| B4 | method table ↔ GUI lanes | reify-lsp ↔ gui/src-tauri | `LspMethod::ALL` | every member dispatches through `handle_request` without `unsupported`; lane equals `concurrency()`; an unknown name keeps the ordered lane and the unsupported error |
| B5 | placement | stdio server ↔ blocking pool | the existing `stdio_server_queues_its_blocking_work_behind_the_pool` rig | hover and completion are among the requests that queue behind the pool |
| B6 | one diagnostics pipeline | tests ↔ server path | any fixture with a parse error | `compute_diagnostics` returns exactly the parse-error diagnostics `compute_diagnostics_with_state` returns (today it also compiles the partial AST) |
| B7 | guarded members | compiler ↔ LSP emitters | fixture authored by ζ: an `@optimized` result reaching a constraint through a `where`-guarded `let` | `fea-not-evaluated` Info hint present; with the guard false the hint is absent |
| B8 | eval readiness | reify-eval ↔ LSP (and any adopting surface) | `tooling_fe_unrepresentable_generic.ri` | predicate answers `Bearing.seal`; the LSP emits its Warning and skips eval; a clean fixture answers `None` |
| B9 | chunk registry | reify-mcp ↔ fence gate | a chunk file added on disk but not to `CHUNKS` | the fence gate reds naming the file; reformatting `language_chunks.rs` changes nothing |
| B10 | JSON-RPC notification | client ↔ stdio transport | `initialize`, then `notifications/initialized` | exactly one output line (the initialize reply) |
| B11 | node id round trip | dev command ↔ itself | any node `inspect-node` prints | the printed id parses back to the same `NodeId` for all five kinds |
| B12 | stdlib docs | doc builder ↔ stdlib | `reify doc --stdlib --out <dir>` | zero `&lt;default&gt;` for params whose default has source text |

B1–B12 are owned by the leaf named in §10; the cross-surface compile parity row is
deliberately absent — it belongs to #6805 (§9).

## §6 — Resolved design decisions

1. **The eval-readiness predicate lives in reify-eval, not reify-compiler.** It is a graph
   question answered with `EvaluationGraph::from_templates` and `is_representable_cell_type`;
   reify-compiler cannot depend on reify-eval. "Beside the front end" means the next call a
   surface makes, with one definition.
2. **The LSP method's concurrency class is a server-side property owned by reify-lsp.**
   The GUI consumes it; it no longer declares it.
3. **`member_scopes` lives on `reify_ast::Declaration`**, not in reify-lsp, so reify-eval's
   `source_location` can adopt it later without a second copy (left to the stream that owns
   that file; cross-reference in the hand-back).
4. **reify.toml gets an inert-status statement, not a production loader** (subject to OQ4).
   A loader has no named consumer (G1), and wiring it means routing the CLI's and GUI's
   Engine construction through `with_registered_kernels_and_manifest(.., Some(..))`, which
   is S1's Engine-constructor seam.
5. **No new cross-surface harness.** The compile-parity signal for γ extends #6805's
   fixture set (`tooling_fe_stdlib_enum.ri`) rather than adding a fourth harness.
6. **h14 partitions are move-only and land before the semantic leaves in the same files.**
   Each partition leaf moves tests with their unit so no child module reaches back into a
   parent's privates through `use super::*`; imports point downward only (program INV
   candidate 7: no intra-crate module SCC grows).
7. **Every extraction leaf names its ratchet row in α's member** (INV candidate 5): front
   end (`reify_compiler::compile(` and hand module-name derivations in CLI/LSP production
   code), stdlib root (`"crates/reify-compiler/stdlib"` literals; `resolve_import_path`
   closures in `server.rs`), LspMethod (`"textDocument/` literals outside its home in
   reify-lsp and gui/src-tauri production code), member scopes (wildcard member-list matches
   over `Declaration` in reify-lsp). The `else_members.iter()` chains are counted by S9's
   ratchet for its `all_value_cells()` accessor, not a second copy here.
8. **The docs-truth gate is not triggered.** No grammar, builtin, stdlib signature or `.ri`
   semantics changes; the LSP, MCP and doc-output changes restore parity with the CLI. No
   chunk text changes (ν changes only the registry's shape and its content is pinned
   byte-for-byte).

## §7 — Lead rulings (2026-10-12)

Recorded in the program doc §"Resolved decisions — pass 1 open questions" → S27. The
options considered are in the pass-1 record; only the rulings are restated here.

- **OQ1 → (A).** #6665 stands: `reify mcp-server` and `crates/reify-cli/src/mcp_context.rs`
  are deleted. δ is not filed. The MCP portion of fk-f5264d76e3ad (three no-prelude
  `compile` calls, three parse-error policies, ungated `engine.eval`) and
  fk-5854202d4aed's `CliState.engine_construction_count` close by that deletion, recorded on
  #6665. ο lands at the `reify-mcp` stdio-transport level, which #6665 keeps.
- **OQ2 → (A).** `compile_program` (#5516) is the one front end. γ moves
  `AnalysisContext::from_parsed` and the remaining LSP compile sites onto it, deletes
  `module_name_from_uri` and the inline copy in `compute_diagnostics_with_state`, and depends
  on #5516 and #5520. The G4 row "reify_compiler compile entry points" is owned by
  resolution-unification β; S12's `CheckEnginePlan`/`MeasurementContext` layer under
  `compile_program`/`CompiledProgram`. τ creates no `front_end.rs`.
- **OQ3 → (A).** β moves the predicate to reify-eval and the LSP consumes it. The CLI is
  unchanged; its exit policy is #5403's and the shape-2 panic is #6851's.
- **OQ4 → (A).** reify.toml: docs only (decision 4).

## §8 — Pre-conditions for activating

- Lead rulings OQ1–OQ4 recorded in the program doc (done 2026-10-12).
- S9 ν #8743 (`TopologyTemplate::all_value_cells()`) is ζ's hard dependency; S9 μ #8742
  (`ParseErrorKind::diagnostic_code`) is π's.
- #5516 and #5520 (resolution-unification β, ζ) are γ's hard dependencies.
- α's wrapper takes a `tests/infra/run-all-classification.manifest` row in its own diff, in
  the format S19 owns.

## §9 — Cross-PRD relationship

| other PRD / stream | direction | seam mechanism | owner | status |
|---|---|---|---|---|
| `resolution-unification.md` (#5516 β, #5517–#5520) | consumes | `compile_program` / `CompiledProgram` | resolution-unification β (#5516) | γ depends on #5516, #5520 (OQ2-A) |
| task #6665 | consumes | deletion of `crates/reify-cli/src/mcp_context.rs`, `cmd_mcp_server` | #6665 | closes the MCP parts of fk-f5264d76e3ad and fk-5854202d4aed (OQ1-A); υ orders after it |
| `driver-contract-implementation.md` #6797 ο | adjacent | LSP module-header assertion | driver-contract | unchanged under OQ2-A |
| `driver-contract-implementation.md` #6805 χ (extends #6700, #5521) | consumes | cross-driver diagnostic-set parity | driver-contract | γ adds `tooling_fe_stdlib_enum.ri` to its corpus via cross-reference, not a new harness |
| `solver-driver-parity.md` #6695 η | adjacent | LSP engine construction sites | solver-driver-parity | ε makes `compute_diagnostics` a wrapper: three construction sites, not four |
| task #6851 | adjacent | unrepresentable-cell root fix | #6851 | β moves the predicate only (OQ3-A) |
| S9 compiler declaration passes | consumes | `TopologyTemplate::all_value_cells()` (ν #8743); ratchet row R4 in `tests/infra/test_compiler_decl_pass_ratchets.py` (α #8730); `ParseErrorKind::diagnostic_code` (μ #8742) | S9 | ζ depends on #8743 and lowers R4's reify-lsp/reify-cli rows; π depends on #8742 |
| S9 | adjacent | `MemberDecl` variants in reify-ast (S9) vs `Declaration::member_scopes` (η) | each its own | disjoint symbols in `decl.rs` |
| S1 Engine state partition (wave 3) | consumes | `scripts/check-compute-trampoline-registration.sh` `KNOWN_SITES` path for the CLI Engine site | S1 | φ held |
| S26 GUI frontend (wave 3) | consumes | generated TS wire types | S26 | λ held |
| S25 GUI backend (wave 2) | produces for | `dev_stdlib_root` | S27 | cross-reference only |
| S12 check surface (wave 2) | adjacent | S12 layers `CheckEnginePlan` under `compile_program` | resolution-unification | OQ2-A |
| S19 infra harness | consumes | `run-all-classification.manifest` row format | S19 | α adds its row |
| S3 + S7 | consumes | INV candidate 7 SCC baseline | S3/S7 | partitions must not grow an SCC |
| S18 verify.sh | consumes | `scripts/verify-pipeline-infra-tests.txt` row naming `crates/reify-cli/src/main.rs` | S18 | φ moves the row with the code; full gate |

G4 seams this stream owns: "gui lsp_bridge.rs lane_for_method and gui/src/editor/lspClient.ts
(consumers of LspMethod)" (κ, λ). "reify_compiler compile entry points" is re-pointed to
resolution-unification β by OQ2-A; S27 consumes it.

## §10 — Decomposition plan

Labels are Greek; task ids filed 2026-10-12 (pass 2). φ #8776 and λ #8777 are held (deferred, x_seam_wait) until S1 / S26 release them. Every leaf carries
`metadata.x_finding_key` (list), `metadata.x_finding_run = "review-all-reify-20261010"`,
`metadata.source = "review-all"`, priority = the highest severity among its keys.

**Phase 0 — ratchet**

- **α #8755 — Tooling front-end adoption ratchet member** *(intermediate → γ, η, κ, μ)*.
  `tests/infra/test_tooling_front_end_ratchet.py` (stdlib unittest, no wall-clock bound) +
  `test_tooling_front_end_ratchet.sh` wrapper + manifest row, same diff. One row per pattern
  in decision 7 (the guarded-member union is S9's R4 row in
  `tests/infra/test_compiler_decl_pass_ratchets.py`, #8730 — not a second member), each with today's measured production-code count as baseline, counted over
  `scripts/lib_rust_production_view.sh`'s production view (no second Rust lexer). Reds when a
  count rises; a vacuity floor reds when a pattern matches nothing it should. Signal: green
  on main; a scratch extra copy of each pattern reds naming the row. Keys: none (INV-5
  mechanism). Priority medium.

**Phase 1 — partitions (move-only)**

- **χ #8756 — Partition `reify-lsp/src/analysis.rs`** into `decl_oracle.rs` (module_name_from_uri,
  enclosing_decl_at, decl_name_and_span, nested_structures, purpose_nested_decl_names,
  name_token_span; imported downward by analysis, goto_def, references, symbols),
  `symbols.rs` (document symbols), `analysis.rs` (AnalysisContext and its summaries), tests
  moved with their unit. Signal: LSP suite green with no test edited beyond its `use` lines;
  `git grep "use crate::analysis::{.*enclosing_decl_at"` empty. Keys: none. medium.
- **ψ #8757 — Partition `reify-lsp/src/references.rs`** into `references/{mod.rs, walk.rs, local.rs,
  rename.rs, decl_uses.rs, cross_file.rs}` per the record; `walk.rs` imports reify-ast only;
  the mod root's `pub use` is the interface. Subsumes #8611 (its test-module move is the
  degenerate case of moving tests with their layer; cancel with reason "subsumed by ψ").
  Signal as χ. Keys: none. medium.
- **τ #8758 — Partition `reify-cli/src/main.rs`, layer units** into `args.rs`, `check_verdict.rs`,
  `report.rs`, `module_capabilities.rs` (no `front_end.rs`: OQ2-A), tests moved
  with their unit. Signal: CLI suite green; `main.rs` holds no `fn module_has_*`, no
  `CheckErrorAllowlist*`. Keys: none. medium.
- **υ #8775 — Partition `reify-cli/src/main.rs`, commands** into `cmd/<name>.rs`; main.rs dispatches
  only. After τ and after #6665 deletes `cmd_mcp_server`. Keys: none. medium.
- **φ #8776 — HELD on S1 — `reify-cli/src/engine_config.rs`** (register_compute_trampolines,
  configured_eval_engine). `x_seam_wait = {stream: S1, contract: "KNOWN_SITES entry for the
  CLI Engine site and S1's EngineConfig"}`. The diff moves the `KNOWN_SITES` path and the
  `scripts/verify-pipeline-infra-tests.txt` row with the code (verify-pipeline file: full
  gate). Keys: none. medium.

**Phase 2 — LSP internals**

- **ε #8759 — One diagnostics pipeline; `DiagnosticsResult.check_result`** (§4.5). Ports the 18
  `compute_diagnostics` test callers and the ~56 private-field reads to the result; deletes
  the posture lock's stand-in probe engine. Signal: B6; `git grep -n "state\.engine\b"` over
  diagnostics tests empty. Keys: fk-41819ea934bd, fk-5854202d4aed. medium.
- **ω #8770 — Partition `reify-lsp/src/diagnostics.rs`** into `diagnostics.rs` + `diagnostics/tests.rs`
  using only the public result. After ε. Keys: none. medium.
- **ζ #8771 — Guarded members in every LSP value-cell walk.** The freshness loop and the FEA
  hint's `value_cell_exprs` map in `compute_diagnostics_with_state`, the three chains in
  analysis, and `eval_guard`'s stage-one scan all call S9's `all_value_cells()`; a bare
  `value_cells` read survives only where excluding guarded members is the point, and the
  line says so; ζ lowers the reify-lsp and reify-cli rows of S9's R4 ratchet
  (`tests/infra/test_compiler_decl_pass_ratchets.py`) to 0. Signal: B7 (fixture authored by
  ζ). Depends: #8743, χ. Keys: fk-b420a7695f68. medium.
- **η #8772 — `Declaration::member_scopes()`** (§4.4) and the five lookups + outline through it.
  Signal: B2, B3. Ratchet row → 0. Depends: α, χ, ψ. Keys: fk-3a3ae797f979. medium.
- **θ #8773 — Hover: exhaustive declaration dispatch, per-class renderers.** Split
  `compute_hover_in_context` into one function per symbol class; declaration-name hover
  through a wildcard-free match on `Declaration`; add Field, Purpose, Constraint, Joint;
  hover becomes a `NAMED_DECL_SNIPPETS` consumer. Signal: B1, B3. Depends: η. Keys:
  fk-d9b1ed83ed62. medium.
- **κ #8760 — `LspMethod`** (§4.3) in reify-lsp, `handle_request` on the enum, `lane_for_method`
  and the routing tests on `ALL`; drop the "Breaking change" paragraph and correct the
  documented `Ok(Value)` list. Subsumes #8447 (cancel with reason "subsumed by κ"). Signal:
  B4. Ratchet row → 0 in reify-lsp and gui/src-tauri production code. Depends: α. Keys:
  fk-7b9fad8a5d00. medium.
- **λ #8777 — HELD on S26 — `lspClient.ts` method type from the generated wire types.**
  `x_seam_wait = {stream: S26, contract: "TS wire types generated from Rust, carrying
  LspMethod::ALL"}`. `lspRequest` takes the generated union, so a misspelt method fails
  `tsc`. Keys: none (the TS half of κ's key). low.
- **ι #8774 — Every AnalysisContext-building handler through `BlockingWorkPlacement`.** hover and
  completion move into `self.blocking_work.run`; `blocking_work_requests` derives from
  `LspMethod::runs_blocking_work`; the stale `server.rs:268-284` pointer in AnalysisContext's
  Send note becomes a symbol cite. didOpen/didChange per §12 Q2. Signal: B5. Depends: κ.
  Keys: fk-1a772d160168. medium.
- **μ #8769 — One import resolver; stdlib root resolved once** (§4.7). Subsumes #8557 (cancel
  with reason "subsumed by μ"). The CLI's literal in `parse_and_compile_with_cfg` moves to
  `dev_stdlib_root`, unless OQ2-A's #5517 has already removed that function; the GUI literal
  is S25's (cross-reference). Signal: a handler-level cross-file test for goto-definition
  and prepareRename against an open dirty buffer; ratchet rows → 1 home literal and 1
  resolver. Depends: α. Keys: fk-74ce7cce677f. low.
- **π #8762 — `AutoTypeParam*` family predicate** (§4.6); drops the "four codes" comment and the
  "Behavioral note" prose. `convert_parse_error` also carries S9's
  `ParseErrorKind::diagnostic_code()` onto the LSP diagnostic (S9's seam row: S9 produces,
  S27 adopts). Signal: a `BoundedInfeasible` diagnostic now emits the
  `reify_lsp::auto_type_param` trace event; the test iterates the family; an LSP parse-error
  diagnostic carries a code. Depends: #8742. Keys: fk-3e8bf5666555. low.

**Phase 3 — front end**

- **β #8761 — Eval-readiness predicate in reify-eval** (§4.1); `reify-lsp`'s `eval_guard` keeps
  reporting and asks reify-eval. CLI unchanged (OQ3-A). Signal: B8. Contributes to
  fk-f5264d76e3ad (closed by γ). high.
- **γ #8778 — LSP onto `compile_program`: AnalysisContext and the remaining compile sites** (§4.2).
  `AnalysisContext::from_parsed` and every LSP compile site still on
  `compile_with_stdlib_checked` after #5520 build through `compile_program`;
  `module_name_from_uri` and the inline copy in `compute_diagnostics_with_state` are
  deleted; the stale "nine previously hard-coded units" sentence in the
  `reify_compiler::compile` rustdoc is corrected. Signal: `tooling_fe_stdlib_enum.ri`
  produces the same diagnostic set through `reify check`, LSP diagnostics and hover's
  AnalysisContext, asserted by adding the fixture to #6805's corpus (no new harness); hover
  on an importing entry shows the imported sub's values; the LSP front-end ratchet rows → 0.
  Depends: α, β, χ, #5516, #5520. Keys: fk-f5264d76e3ad (the MCP part closes by #6665).
  high.
- **δ — not filed (OQ1-A).** The MCP context is deleted by #6665.

**Phase 4 — registries**

- **ν #8763 — Chunk table; fence gate through the public registry** (§4.6). Drops the 19-count
  test and `TOPICS_ANCHOR`. Signal: B9; `reify_language_reference` output byte-identical for
  every topic. Keys: fk-2da836d68189. medium.
- **ξ #8764 — reify.toml inert status stated once** (§4.6). Subsumes #8336 (its option (b);
  cancel with reason "subsumed by ξ"). Signal: `review/briefing.yaml` and
  `docs/notes/reify-toml-schema.md` state the inert status; the reify-config rustdoc no
  longer says "no other workspace crate consumes it yet". Keys: fk-677d67cb2ac8. medium.
- **ο #8765 — Typed JSON-RPC id; notifications get no reply** (§4.6). Signal: B10 through the
  stdio transport. Keys: fk-bed8cadb429f. low.
- **ρ #8767 — One expression-text renderer; stdlib sources in the doc model** (§4.6); corrects
  the "Structures then Occurrences" and `cross_refs` docs. Signal: B12. Keys:
  fk-10941c98934e. low.
- **σ #8768 — `NodeId: FromStr` as the inverse of `Display`; `inspect-node` round-trips** (§4.6).
  Subsumes #7720 (cancel with reason "subsumed by σ"). Signal: B11 through
  `crates/reify-cli/tests/harness_cli/cli_inspect_node.rs`. Keys: fk-972b7d28a290. low.

**Close**

- **Ω #8779 — PRD close: terminal stamp.** Depends on every other filed leaf (cancelled siblings
  count as satisfied). Sets the Status token, names the landed leaf ids, adds the
  AS-AUTHORED paragraph and the LIVE/AS-AUTHORED map here and in the capability manifest.
  low.

DAG summary: α → {γ, η, κ, μ}; χ → {ζ, η, γ}; ψ → η → θ; ε → ω; κ → {ι, λ}; τ → {υ, φ};
β → γ; #5516, #5520 → γ; #8743 → ζ; #8742 → π; #6665 → υ; S1 → φ; S26 → λ; every leaf → Ω. Drift-guard registrations ride in
the diff of the leaf adding the test (α's manifest row; any new nextest entry).

## §11 — Out of scope

- GUI compile paths (`compile_single_file_with_stdlib`, `compile_entry_with_imports` in
  `gui/src-tauri/src/engine.rs`) — resolution-unification ε (#5519) and S25.
- The `ReifyToolContext` trait and MCP tool behaviour beyond the front-end call — S25.
- The CLI exit policy for skipped evaluation — #5403 (OQ3-B would need a ruling there).
- reify-eval's `source_location` adopting `member_scopes` — left to the stream owning that
  file.
- `cmd_check` (544 lines) and `cmd_build` (479) internal decomposition beyond moving them —
  a later heuristic-4 pass.
- `completion.rs`'s `KEYWORDS` dependency from rename — another finding's.

## §12 — Open (tactical) questions

1. **Partition timing.** τ/υ move code that pending tasks (#5517, #5518, #6665 and several
   driver-contract leaves) list in `metadata.files` as `crates/reify-cli/src/main.rs`.
   Suggested: land each partition as one move-only commit while no main.rs task is in
   flight, and let architects re-acquire footprints (BRE). Decide at decompose.
2. **didOpen/didChange placement (ι).** They compile and evaluate under `eval_state`'s
   `std::sync::Mutex`. Suggested: move them into the blocking pool too, awaiting the work so
   the handler's ordering is unchanged; decide in ι after reading tower-lsp's notification
   ordering.
3. **`inspect-node`'s legacy `Kind(inner)` grammar (σ).** Suggested: accept only the
   `Display` form and rewrite `cli_inspect_node.rs`; keep an alias only if a script uses the
   old form (none found by `git grep inspect-node`).
4. **Hover content for the four new kinds (θ).** Suggested: the same header + doc-comment
   shape the existing declaration arms render.
5. **Fixture registration.** A leaf whose Rust test reads a `tests/prd-gate/fixtures/tooling_fe_*.ri`
   registers it in `_RUST_COUPLED_RI_FIXTURES` in the same diff (PG-DRIFT).
6. **AnalysisContext's parse (γ).** Reuse the per-document parse cache if `compile_program`
   as landed by #5516 accepts a parsed entry; otherwise take its own parse and measure the
   per-request cost against the cache. Decide in γ.

## Gate walk (author mode)

- **G1** — every mechanism names a consumer: `LspMethod` → handle_request, lane_for_method,
  routing tests, ι; `member_scopes` → five lookups + outline + θ; predicate → LSP;
  `check_result` → tests and ω; `CHUNKS` → `reify_language_reference` and the fence gate;
  `dev_stdlib_root` → LSP handlers and CLI; `ExprText` → three formatters; `NodeId: FromStr`
  → `inspect-node`; family predicate → `convert_diagnostic`. The front-end function's
  consumer list is OQ1/OQ2's subject.
- **G2** — leaf signals per §10; partitions are behaviour-preserving refactors whose signal
  is the unchanged suite plus a grep that the moved symbols left the parent.
- **G3** — no novel grammar: four fixtures parse with 0 ERROR nodes and `reify check` exits
  0 on three (the fourth panics by design, §2.3). Substrate verified at main: S9's accessor
  is absent today (ζ depends on it); `stdlib_sources` exists as `pub(crate)`;
  `BlockingWorkPlacement::run`, `ValueCellId: FromStr`, `RealizationNodeId: FromStr`,
  `with_registered_kernels_and_manifest` all exist; reify-mcp depends only on reify-core.
- **G4** — §9; two contested seams returned as OQ1/OQ2.
- **G5** — B+H: blast radius ≥ 3 crates (reify-cli, reify-lsp, reify-ast, reify-eval,
  reify-core, reify-compiler, reify-mcp, reify-config, reify-doc-build, gui); §4 + §5.
- **G6** — §2; no numeric or exactness claims; the rejection-style signals (B3, B9) are
  compile/test failures the leaf itself introduces.
- **G7 (advisory, reify INV-SF family)** — `placeholders-owned-and-loud`: ρ replaces six ad-hoc
  placeholders with one typed `Unavailable` state. `declared-intent-consumed-or-diagnosed`:
  reify.toml is declared intent nothing consumes → OQ4. `diagnostics-carry-codes`: the
  module-path diagnostics `check_module_path_decl` builds carry their code only in message
  text; γ does not change that (S12 / driver-contract μ territory) and records it.
  `undef-has-provenance`, `error-severity-exits-nonzero`, `indeterminate-attributable-transient`,
  `parse-is-value-faithful`: no hit. No waivers.
- **META** — complete and coherent: the rulings fix Phase 3's shape (γ consumes
  `compile_program`; δ not filed; β moves the predicate only).

## Gap-fill amendments (2026-10-16)

Rulings from the attended gap-fill sitting, `plans/gap-fill-deliberation-reify-20261016.md`. They add four leaves; the sections above are unchanged except the status line. Machine-readable rows: `tooling-front-end.capability-manifest.yaml` (ψ₂, ψ₃, γ₂, ε₂).

- **ψ₂ #9324 — member-segment matchers onto the shared enumeration** (§K2-D2; medium; part of fk-8d86383441a3, whose leaf is S9 ρ #9321). `cursor_on_member_segment` and `expr_member_segment_hit` (references/local.rs) and every references/walk.rs caller are rewritten onto S9 ρ's `Expr::for_each_child` / `MemberDecl::for_each_direct_expr` / `MemberDecl::child_scopes`; walk.rs's five child helpers are deleted; derived-sub prototype and operand uses become collected (rename RED tests). Depends on ψ #8757 (done) and S9 ρ #9321. Per the ruling this rewrite may land ahead of ψ₃.
- **ψ₃ #9325 — `cursor.rs`: `resolve_cursor -> CursorTarget`** (§K2-D3, the held "A" half; medium; fk-dd82750298e2, plus the systemic half of fk-406a3917737c). `CursorTarget { DeclName | ValueUse { binding } | MemberSegment { base } | TypeUse | Keyword | Trivia(Comment | String) | None }`, consumed by hover, goto_def, references, rename, highlight and completion's DotAccess/TypePosition as wildcard-free matches; goto_def Phase C and the member-segment guard subsumed. #7607 is cancelled with `x_subsumed_by` #9325 (its three measured cases and its flip-the-pin acceptance criterion are ported). Depends on S9 ρ #9321 and ψ₂ #9324. The ruling strikes §11's completion-context exclusion for this leaf; §11 as written carries no such line (its only completion bullet is the `KEYWORDS` dependency from rename, which stands), so this bullet records that completion's context classification is in ψ₃'s scope. The immediate `determine_context` named-argument fix is the standalone low task #9328 (K2-D3 B), which ψ₃ later deletes. The derived-sub reference walk (`derivation: _` in references/walk.rs) is covered by S9 ρ's `for_each_direct_expr` and ψ₂.
- **γ₂ #9326 — `analysis::analyse`: one compile+check per document version** (§K2-D4 leaf 1; medium; fk-4034ccf5b655; K1-D4's items folded in). `analyse(parsed, mode) -> Analysis { compiled, skipped, check, engine }` built on `compile_program` (OQ2-A), memoised per version on `DocumentState` beside `parsed_cache`, the only place naming the checker and calling the eval-readiness predicate; `AnalysisContext` becomes a view; `EngineMode` decides undef-cause capture once; ratchet row "reify-lsp compile-entry call sites = 1" in α #8755's member. Depends on γ #8778, β #8761, ε #8759, ι #8774 and α #8755. γ #8778 is not widened: analyse() wraps γ's `compile_program` call and γ's check stays satisfied (cross-reference recorded on #8778). ι #8774 keeps its scope; after γ₂ only cache misses go through `BlockingWorkPlacement`.
- **ε₂ #9327 — diagnostics stage split and typed codes** (§K2-D4 leaf 2; medium; fk-ce2e01ebfe43). `compute_diagnostics_with_state` becomes stage functions with `eval_stage` the sole `EvalState` writer committing on success; `enum LspDiagnosticCode` and one `lsp_diagnostic` constructor. Depends on ε #8759, ζ #8771, ω #8770 and γ₂ #9326.
- **Survey record.** fk-dd82750298e2 and fk-ce2e01ebfe43 had swapped anchor/verdict-notes fields in `docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json`; corrected 2026-10-16 before either key was stamped (keys kept as identities; see the record's `corrections` field).
- **Cross-references.** β #8761 (K1-D1): `first_unrepresentable_cell` and its LSP callers become deletable once the generic-instantiation stream's β makes its hydration check always-on; β lands unchanged first. #6012 / #8910 (K2-D8): completion.rs is re-measured after both land; a move-only split only if it is still above 2,000 raw lines and ψ₃ has not taken `determine_context`.
- **Close leaf.** Ω #8779 depends on every filed leaf; adding ψ₂, ψ₃, γ₂ and ε₂ to Ω's dependencies is left to the S27 lead (this sitting's applier did not write #8779).
