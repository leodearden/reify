# Capability manifest — undef-self-describing

Mechanizes G3 + G6 per task for `docs/prds/v0_6/undef-self-describing.md` (decompose batch `undef-self-describing-2026-06-03`). Each binding: capability → evidence (`grep:file:line wired` / `producer:label upstream` / `grammar-fixture` / `field-population` / `floor`). Any binding resolving to `declared-only | test-only | producer-absent | producer-downstream | fixture-ERROR | bound≤floor` blocks the batch.

**Result: no FAIL bindings — batch clears.** No numeric bounds anywhere (G6 branches 1/2 N/A); no new grammar (G3 grammar gate vacuous, `grammar_confirmed=true` all tasks). The one substrate gap (no CLI param-printer) is a *build target of δ*, not an assumed-existing capability — δ builds the surface on the existing diagnostics print loop.

Leaf/intermediate is by the dependency definition (leaf = nothing in-batch depends on it). η is the sole structural leaf; δ/ε/ζ/γ are intermediates that nonetheless each carry a genuine user-observable signal (robustness). Bindings given for all 7 tasks.

---

## α — reason model + cell-origin capture (intermediate → β, γ)

| Capability asserted by α's signal | Evidence | Verdict |
|---|---|---|
| Cell-compute boundary writes `(Value, DeterminacyState)` per cell on the production eval path (the capture hook site) | `grep:crates/reify-eval/src/engine_eval.rs:201` — `snapshot_values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>` (production eval) | PASS (wired) |
| `DeterminacyState` readable to classify origin kind (Undetermined vs Auto vs Provisional) | `grep:crates/reify-ir/src/value.rs:3053-3062` | PASS (wired) |
| `UndefCause` side-map **populated with a non-sentinel cause** on the production path (field-population) | `producer:α` — α writes a real `UndefCause` (not `None`/placeholder) for undef-origin cells; signal asserts capture-correctness + transparency on a **real eval** of a committed `.ri`, not a synthetic-input unit test | PASS (producer:α populates) |
| DAG-direction | α is a root; consumers β/γ are downstream | PASS (upstream) |
| Transparency: capture does not change value/determinacy/content-hash/cache | `grep:crates/reify-ir/src/value.rs:962-965` (precedent: identity-excluded metadata) + invariant A1; verified by α's capture-on-vs-off byte-identical run | PASS (additive metadata) |

## β — undef tracer, all-causes DAG walk (intermediate → δ, ε, ζ, γ)

| Capability | Evidence | Verdict |
|---|---|---|
| Forward cell dependency edges to walk from an undef cell to its inputs | `grep:crates/reify-eval/src/deps.rs:1329-1379` — `DependencyMap { forward: HashMap<ValueCellId, Vec<ValueCellId>>, … }`, built `from_graph` | PASS (wired) |
| Expression → referenced cells (leaf attribution) | `grep:crates/reify-ir/src/expr.rs:823-958` — `CompiledExpr::collect_value_refs`; `grep:crates/reify-eval/src/deps.rs:1184-1190` — `extract_value_deps` | PASS (wired) |
| Per-cell origin causes to collect | `producer:α` upstream (side-map) | PASS (upstream) |
| All-causes completeness + cycle safety | invariants B1–B4; visited-set guard; signal = real-eval tests (`c=a+b` ⇒ both; chain collapse; cyclic terminates) | PASS (end-to-end on real eval) |

## γ — op/builtin contract-failure reason sink (intermediate → η)

| Capability | Evidence | Verdict |
|---|---|---|
| reify-expr local op/builtin return-`Undef` sites (input-determined contract failures) to instrument | `grep:crates/reify-expr/src/lib.rs:2122-2160` (eval_binop undef return); `grep:crates/reify-stdlib/src/geometry.rs:225-228` (`Some(Value::Undef)` contract-fail) | PASS (wired sites exist) |
| `DiagnosticCode` to attach to `OpContractFailed` | `grep:crates/reify-core/src/diagnostics.rs` (DiagnosticCode enum, the codes these sites already emit) | PASS (wired) |
| Sink drains into side-map; reported by tracer | `producer:α` (side-map) + `producer:β` (tracer) upstream | PASS (upstream) |
| Sink off ⇒ eval byte-identical (transparency at expr layer) | invariant G3; opt-in `Option<&mut …>` sink | PASS (additive) |

## δ — CLI surface (intermediate → η; carries a user-observable leaf signal)

| Capability | Evidence | Verdict |
|---|---|---|
| A print channel to surface the cause to the user | `grep:crates/reify-cli/src/main.rs:1199-1210` — `report_eval_output` diagnostics loop (`writeln!(err, "{}: {}", diag.severity, diag.message)` at :1207) | PASS (wired) |
| **CLI undef-cause surface itself** (the §2/§8.4 gap: no param-printer exists today) | `producer:δ` — δ **builds** the surface on the existing diagnostics loop (informational note); not an assumed-existing capability | PASS (δ's deliverable, built on existing substrate) |
| Tracer output to render | `producer:β` upstream | PASS (upstream) |
| `examples/undef_self_describing.ri` parses (existing syntax only) | `grammar-fixture` — uses the `undef` literal, which parses: `grep:tree-sitter-reify/grammar.js:1482` + task **#3918 done** (undef writable literal); no novel syntax | PASS (0 ERROR / existing) |

## ε — GUI hover / param-panel surface (intermediate → η; carries a user-observable leaf signal)

| Capability | Evidence | Verdict |
|---|---|---|
| Param payload type extensible with a `reason` field | `grep:crates/reify-mcp/src/types.rs:53-61` — `ParameterInfo` already carries `determinacy: String` (additive `reason` field) | PASS (wired, extensible) |
| GUI payload builder to thread the field | `grep:gui/src-tauri/src/engine.rs:1472-1501` (`build_parameters_payload`) | PASS (wired) |
| Observe the panel/hover content through the product (not storage) | `reify-debug` MCP present (`dom_query` / `store_state` / hover) — debug MCP tools live this session | PASS (product read path) |
| Tracer output to render | `producer:β` upstream | PASS (upstream) |

## ζ — LSP hover surface (intermediate → η; carries a user-observable leaf signal)

| Capability | Evidence | Verdict |
|---|---|---|
| Hover content assembly point | `grep:crates/reify-lsp/src/hover.rs:43-52` (markdown assembly) via `grep:crates/reify-lsp/src/bridge.rs:179-186` (`textDocument/hover`) | PASS (wired) |
| Tracer output to render | `producer:β` upstream | PASS (upstream) |

## η — B+H integration gate + §9.2.9 docs (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| All boundary scenarios' deliverables (BT1–BT12) | `producer:δ, ε, ζ, γ` (transitively α, β) — all **upstream** of η | PASS (upstream) |
| `reify-language-spec.md` §9.2.9 text to reconcile | `grep:docs/reify-language-spec.md:1875` — the section exists; docs edit, no runtime substrate | PASS (doc) |
| Integration-gate signal = boundary suite (G2 escape-hatch closure / G5 H component) | PRD §5 BT1–BT12 (incl. transparency BT8, surface-agreement BT12) | PASS (named signal) |
