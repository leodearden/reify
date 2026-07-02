# Capability manifest — eval-uniform-dependency-handling

Binds each leaf signal's asserted capabilities to evidence (mechanizing G3+G6). Evidence verified live at HEAD `85b6b88e07`..`bda8bc47cc` on 2026-07-02 (author/decompose session; three-stream agent deep dive + direct greps). D3 workflow verdict appended at the end. No numeric bounds anywhere in this batch (no floor checks); no novel grammar (`grammar_confirmed=true` batch-wide; fixture shapes reuse already-parsing constructs — probes below re-confirm).

## Task α — no-stale-Undef invariant checker + debug-gate harness

| Capability | Evidence | Verdict |
|---|---|---|
| Post-eval retained state exposes per-cell static deps | `grep:crates/reify-eval/src/lib.rs:385-392` — `EvaluationState { snapshot, reverse_index, trace_map }` stored on Engine; installed at `engine_edit.rs:3678-3682` (wired, production path) | PASS wired |
| Traces carry realization edges (the class the ordering graph drops) | `grep:crates/reify-eval/src/deps.rs:23-32` — `DependencyTrace { reads, realization_reads }`; populated via `set_realization_reads` (`deps.rs:406-433`, `engine_edit.rs:3670-3674`) | PASS wired |
| Auto-cell exclusion predicate | `grep:crates/reify-eval/src/graph.rs:769` — `is_auto_cell` | PASS wired |
| @optimized exclusion predicate reusable outside the R3e sweep | `grep:crates/reify-eval/src/engine_eval.rs:6723-6726` — `find_matching_compiled_function(...).optimized_target` in production path | PASS wired |
| Corpus can be green at α time (R3f class closed) | `producer:task-4946` upstream (dep edge wired at filing; pending at authoring) | PASS upstream |
| Checker itself observably fires (anti-silent-accept) | rejection-check obligation encoded in α's own signal: seeded-violation self-test (fabricated stale-Undef → non-empty report) is a RED-first member of α's suite | PASS (obligation, self-verifying) |

## Task δ — delete the compensation stack; capstone regression gate

| Capability | Evidence | Verdict |
|---|---|---|
| Symbols to delete exist at HEAD | `grep:crates/reify-eval/src/engine_eval.rs:6662` `re_eval_consumers_of_in_walk_mints`; `:6759` graph twin; `minted_in_walk` (`:6042` decl + arms); `engine_build.rs:8499` `mint_symbolic_geometry_handles_into_values`; `geometry_ops.rs:5133` `mint_symbolic_topology_selectors_into_values` | PASS |
| `reeval_cone_cell` KEPT is safe (other consumer exists) | `grep:crates/reify-eval/src/engine_eval.rs:4061` — guarded-member downstream cone consumes it independently of the deleted sweeps | PASS wired |
| R3E trio regression exists on main | `grep:crates/reify-eval/tests/value_eval_consumes_minted_selector.rs` (file exists, verified 2026-07-02) | PASS |
| Let-backed regression member exists at δ time | `producer:task-4946` upstream (its deliverable; dep edge α←4946 + chain α→γ→δ puts it transitively upstream) | PASS upstream |
| `printer_print_envelope_eval_e2e` exists on main at δ time | `producer:task-4655` upstream (direct dep edge δ←4655 wired at filing) | PASS upstream |
| Ordering-by-construction replaces the sweeps (γ's deliverable) | `producer:task-γ` upstream (intra-batch edge); precedent that the pair-shape orders correctly: solid-param cells mint in-walk today (Param arm `engine_eval.rs:6121-6169`) — the R3E trio passes via exactly this path | PASS upstream + precedent |

## Task ε — edit-path closure

| Capability | Evidence | Verdict |
|---|---|---|
| Edit path has graph-native trace machinery (no TopologyTemplate) | `grep:crates/reify-eval/src/engine_eval.rs:6759-6791` — `_from_graph` builds nodes/traces from `graph.value_cells`; `trace_map` installed `engine_edit.rs:3678-3682` | PASS wired |
| Edit-path in-walk mint exists | `grep:crates/reify-eval/src/engine_edit.rs:969` | PASS wired |
| Stale backstop comment exists to remove (premise of the cleanup) | `grep:crates/reify-eval/src/engine_edit.rs:964-966` — comment claims an "idempotent post-eval backstop mint pass"; no `mint_symbolic_*` call in the `edit_param` body at HEAD (only `:969` in-walk; `:3695/:3705` are `edit_source`) | PASS (discrepancy confirmed real) |
| Geometry-let cells ride the dirty cone at ε time | `producer:task-γ` upstream (intra-batch edge) | PASS upstream |

## Intermediates (β, γ) — load-bearing bindings (checked though not leaves)

| Capability | Evidence | Verdict |
|---|---|---|
| β: filter site keys on graph-presence today | `grep:crates/reify-eval/src/engine_eval.rs:6315-6338` — `snapshot.graph.value_cells.contains_key(target_cell)` + task #4726 comment | PASS |
| β: value-bucket panic on absent input / double-count risk when present | `grep:crates/reify-eval/src/compute_cache_key.rs:96-118` — panic `"value_input {:?} not present in graph"`; geometry flows via `realization_inputs` | PASS |
| γ: geometry-let skip site to change | `grep:crates/reify-compiler/src/entity.rs:1991-1995` — `if is_geometry_let(...) { continue; }` | PASS |
| γ: solid-param cell+realization precedent shape | `grep:crates/reify-compiler/src/entity.rs:1902-1988` (geometry-typed Param cell) + `crates/reify-eval/src/graph.rs:420-426` (`geometry_cell` name-match link) | PASS wired |
| γ: symbolic handle representation | `grep:crates/reify-ir/src/value.rs` — `GeometryHandleRef.kernel_handle: Option<GeometryHandleId>`; `PartialEq`/`content_hash` exclude it (GHR-β); landed by #4652 (merge `f944fdc48a`) | PASS |
| γ: differential corpus exists | `producer:task-4359` (done — legacy-vs-unified equivalence suites) | PASS |

## μ — [MILESTONE] stage-(b) design session

No substrate capabilities asserted (deliverable is a design session; stub PRD `eval-unified-schedule-executor.md` records premises to re-verify at flip time). Exempt.

## D3 workflow verdict (scripts/prd-decompose-verify.mjs, run wf_d6291824-b19, 2026-07-02)

**Verdict: PASS — blocks: false, zero FAIL/UNPROVABLE/HARNESS_ERROR.** (Harness note: the leaf array reached the script stringified, so the three leaves were processed as one combined premise set — the Enumerator/Prover/Adversary nevertheless probed each leaf's premises individually; journal inspected to confirm.)

Probes executed (Prover, re-run independently by Adversary — all PASS):
- `tree-sitter parse --quiet tests/prd-gate/fixtures/geometry_let_selector_consumer.ri` — exit 0 (fixture committed beside this manifest; structure def with geometry let + `faces_by_normal` selector + consumer let).
- `reify check` on the same fixture — exit 0, `All constraints satisfied.` (plus benign `W_MODULE_DECL_MISSING` warning).

Adversary missed-premise hunt (all PASS):
- `EvaluationState{snapshot,reverse_index,trace_map}` retained — installed by `eval()` (`engine_eval.rs:4410`), `eval_cached()` (`:5666`), `edit_source` (`engine_edit.rs:3678`); `edit_param` rebuilds trace_map in-body (~`:2180`, install ~`:2255-2260`). **Finding (task work, not a premise failure): `Engine.eval_state` is PRIVATE (`lib.rs:449`) — α's checker needs in-crate access (supports the in-crate `pub` module placement, PRD §11 Q1).**
- `DependencyTrace { reads, realization_reads }` confirmed (`deps.rs:23-33`).
- `is_auto_cell` confirmed (`graph.rs:769`).
- @optimized predicate reusable outside the sweep: `find_matching_compiled_function` is `pub` cross-crate (`reify-expr/src/lib.rs:1514`); `optimized_target` is a `pub` field (`reify-ir/src/expr.rs:402`).
- Upstream producers #4946 and #4655 both live/non-terminal.
- All five deletion-target symbols + full call-site inventory exist at HEAD.
- ε's stale-backstop-comment discrepancy is REAL at HEAD (scoped to `edit_param`).
