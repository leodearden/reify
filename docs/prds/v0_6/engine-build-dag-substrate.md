# PRD — Engine Build-DAG Substrate (edge-completeness + cell-eval determinacy unification)

**Status:** deferred → ready to activate. v0_6. **Approach B + H** (high-stakes core-engine seam; contract + two-way boundary test below). Authored 2026-06-07.
**Design source:** `docs/design/engine-unified-build-dag-option-a.md` (multi-agent design + 27-agent red-team, verdict GO-WITH-CHANGES). This PRD is **Part 1 of 2** — the zero-behavior-change substrate the unified driver rides on. Part 2 = `docs/prds/v0_6/engine-unified-build-dag.md`.
**Code anchors** are as of `HEAD b0077500f5`; main moves fast — **re-locate every symbol at implementation time** (cite-by-symbol, the line is a hint).

---

## 1. Goal — the correctness substrate, landable with zero user-visible behavior change

Two independent foundations the unified Build-DAG (Part 2) requires, both shippable **without** any scheduler flag flip:

1. **Edge-completeness.** Today the dependency graph does **not** contain Realization→Realization, Constraint→Realization, or cross-sub `GeomRef::Sub` edges (`extract_realization_dependencies` returns `realization_reads = Vec::new()` unconditionally; the `Boolean { .. } => continue` arm drops both operands at `deps.rs:394`). Build the `GeomRef`-resolution edge-extraction pass so the graph is complete — then prove it with a debug-only `assert_dag_complete` that runs on **every legacy build** and verifies the unified DAG *would* schedule every `named_steps` producer before its consumer.
2. **Cell-eval determinacy unification.** The bare (`.with_determinacy`-less) cell-eval exists at **five** warm/edit sites (`engine_eval.rs:3252`, `:3068`; `concurrent.rs:481`; `engine_edit.rs:1053`, `:2487`). The `DeterminacyPredicate` None-branch silently returns `Undef` in debug *and* release. One private `cell_eval_ctx` constructor that always carries `.with_meta + .with_determinacy + .with_runtime_diagnostics`, routed through every cell-expr eval, makes there be no call site that can drop determinacy.

Both are pure-substrate: (1) adds graph edges + a debug assertion (no scheduler change); (2) fixes a real warm-path bug *and* removes a structural footgun. Neither flips behavior on the default cold `build()` path.

## 2. User-observable surface (what proves it landed)

- **β (edge graph):** the `assert_dag_complete` debug assertion passes across the **entire** existing `reify-eval/tests/` + `tests/golden` corpus on every legacy build — and is **upgraded** to check realization→realization and constraint→realization reachability, not just value-cell reachability. A new edge surfaced as missing = a test failure. (This is the H boundary test for the edge graph; see §6.)
- **γ (determinacy):** a warm-path RED→GREEN regression — a `let r = determined(x)` cell evaluated through `edit_param` / `edit_source` / `eval_cached` now yields the correct `DeterminacyPredicate` result instead of silent `Undef`. Observable through the editor/LSP read path.

## 3. Sketch of approach

### 3.1 Edge-extraction pass (`deps.rs`)
A new `GeomRef`-resolution pass feeding `build_trace_map_and_fields` + `ReverseDependencyIndex::build_from_graph_and_fields`:
- walk `Boolean.left/right`, `Modify`/`Transform`/`Pattern.target`, `Sweep.profiles`;
- treat `GeomRef::Step` as **intra-node** (no edge — sibling geom inlined into the consuming realization's op list);
- map `GeomRef::Sub(name)` → producing `RealizationNodeId`, register via `add_realization` **counted in in-degree** (the existing `realization_index`/`realization_dependents_of` substrate, `deps.rs:67/104/115`, is confirmed present — this *populates* it, it does not invent it);
- **fix the `deps.rs:394` Boolean-operand drop**;
- populate `realization_reads` for **selector/query Let cells AND geometry-reading constraints** (the latter detected via `is_geometry_query_call`, `geometry_ops.rs:1795`).

Three new edge types result: selector/query-cell→realization, constraint→realization, realization→realization.

### 3.2 `assert_dag_complete` (debug-only, every legacy build)
A debug assertion that, on each legacy build, computes the unified DAG's topological order from the edge graph and asserts the **legacy execution order is a valid linear extension** of it — i.e. every `named_steps` producer is scheduled before every consumer that reads it. Upgraded from a value-cell-only reachability check to also cover realization→realization and constraint→realization reachability. Zero user impact; the only effect is converting a *missing edge* into a loud test failure instead of a future mis-ordering under Part 2's lexicographic pop order.

### 3.3 `cell_eval_ctx` single constructor (`engine_eval.rs`)
One private `Engine::cell_eval_ctx(values, snapshot_values, runtime_sink)` that *always* carries `.with_meta + .with_determinacy + .with_runtime_diagnostics`. Migrate all five bare sites to route through it; `concurrent.rs:481` additionally needs `snapshot_values` plumbed. `eval_cached` stays expr-only by **executor selection** in Part 2, not by a separate context — so there is no code path that can drop determinacy.

> Refuted over-claim recorded (red-team): dropping `.with_determinacy` does **not** leak `Undef` past the `Determined` gate for *ordinary* `Let` cells (cold and warm both stamp `(val, Determined)`). The genuine residue is warm **`DeterminacyPredicate`** cells — that is the bug γ fixes. This is a parity fix, not a gate-leak fix.

## 4. Resolved design decisions

- **D1 — Edge-extraction is the spine, not a footnote.** The red-team's headline correction: the original "bodies-unchanged / one-line addition" framing was false. This is a substantial pass and it is *its own task* (α) precisely so Part 2's driver is never built against an incomplete graph.
- **D2 — Prove edges with `assert_dag_complete` on the legacy path.** The edge graph is validated against the **legacy execution order as oracle** *before* any driver consumes it (the C-as-integration-gate pattern: α is the producer, β is the integration-gate leaf).
- **D3 — `cell_eval_ctx` gated on task 4317.** Task 4317 (`Fix eval-order bug: param-default referencing sibling let`) is `in-progress` and carries the `eval_cached` §8.2 parity locks (the "4317-twin"). γ **depends on 4317 landing** and re-applies `cell_eval_ctx` at post-merge line numbers, inheriting 4317's parity baseline rather than racing it on `engine_eval.rs`.
- **D4 — No scheduler flag in Part 1.** Everything here is additive on the default path. The `BuildScheduler` enum, `REIFY_BUILD_SCHEDULER` env, and `feature = "unified-dag"` all live in Part 2.

## 5. Pre-conditions for activating

- **Task 4317 merged to main** (hard prerequisite for γ; cross-batch dependency).
- No other gate — the substrate this builds on (`realization_index`, `NodeId` kinds, `DeterminacyState`, `#[non_exhaustive]` diagnostics) is confirmed present on main (design ledger C1–C15).
- **No novel substrate / no grammar change** — G3 grammar-gate N/A (no `.ri` syntax introduced).

## 6. Contract + two-way boundary test (H component)

**Contract — the edge-completeness invariant.** For every build `B`, let `L(B)` = legacy execution order of nodes and `T(B)` = any topological order of the unified DAG over the §3.1 edge graph. Invariant: **`L(B)` is a linear extension of the partial order induced by the edge graph** — formally, for every edge `u→v`, `u` precedes `v` in `L(B)`. `assert_dag_complete` asserts exactly this on every legacy build. A missing edge manifests as an `L(B)` that violates a (would-be) edge — but since the edge is missing, the *symptom* is that Part 2's `T(B)` reorders a producer after its consumer. So β asserts the **contrapositive**: every `named_steps` producer precedes every consumer in `L(B)`, across all edge kinds.

**Boundary-test sketch (faces both the producer = edge-extraction and the consumer = future driver):**

| Scenario | Precondition | Postcondition (β asserts) |
|---|---|---|
| Cross-sub assembly (`let x = self.comp.body`) | parent named lexicographically *before* children | child realization precedes parent's selector/consumer in `L`; the `GeomRef::Sub` edge exists |
| Boolean over two realizations (`a ∪ b`) | both operands are realizations | both operand edges present (the `:394` drop is fixed) — neither dropped |
| Selector→op chain (`fillet(b, edges(b), r)`) | selector reads a realization | selector-cell→realization edge present; selector precedes consumer in `L` |
| Geometry-reading constraint | `fits_build_volume(bounding_box(part), …)` | constraint→realization edge present; constraint after realization in `L` |
| Modify/Transform/Pattern/Sweep target | op targets a `GeomRef` | target edge extracted (not only `args` destructured) |
| Determinacy-predicate warm cell (γ) | `let r = determined(x)` edited via `edit_param` | `r` evaluates the predicate, not silent `Undef` |

These rows ARE β's and γ's observable signals at decompose time (closing G2's loop).

## 7. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/engine-unified-build-dag.md` (Part 2) | produces-for | the populated `ReverseDependencyIndex` edge graph + `cell_eval_ctx` | **this PRD** | queued |
| task 4317 (`param-default sibling-let order`) | consumes | `engine_eval.rs` cell-eval determinacy path | task 4317 | in-progress (γ gates on its merge) |

No reciprocal-ownership ambiguity: this PRD unambiguously **produces** the substrate; Part 2 **consumes** it.

## 8. Decomposition plan

DAG: **α → β**; **γ** independent (gated on 4317).

- **α — `deps.rs` GeomRef-resolution edge-extraction pass.** Walk Boolean/Modify/Transform/Pattern/Sweep `GeomRef` targets; fix `:394` Boolean-operand drop; `GeomRef::Sub`→Realization edges counted in in-degree; populate `realization_reads` for selector/query Let cells **and** geometry-reading constraints. *Modules:* `crates/reify-eval/src/deps.rs`. *Signal:* **intermediate** — unlocks β (and Part 2's δ driver). Unit tests assert each of the three new edge types is extracted for the §6 boundary-test idioms. *grammar_confirmed: true.*
- **β — `assert_dag_complete` integration gate.** Debug-only assertion on every legacy build; upgraded to realization→realization + constraint→realization reachability; run the full `reify-eval/tests/` + `tests/golden` corpus under it. *Modules:* `crates/reify-eval/src/dirty.rs` (or `engine_build.rs` assert site), `crates/reify-eval/tests/`. *Signal:* **leaf** — assertion passes across the entire existing corpus on every legacy build (CI); a missing edge = test failure. *Prereq:* α. *grammar_confirmed: true.*
- **γ — `cell_eval_ctx` determinacy unification (5 sites) + warm-path RED regression.** One `cell_eval_ctx` constructor; migrate `engine_eval.rs:3252/:3068`, `concurrent.rs:481` (+ `snapshot_values` plumb), `engine_edit.rs:1053/:2487`. *Modules:* `crates/reify-eval/src/engine_eval.rs`, `concurrent.rs`, `engine_edit.rs`. *Signal:* **leaf** — `let r = determined(x)` warm-evaluated via `edit_param`/`eval_cached` yields correct `DeterminacyPredicate` result, not silent `Undef` (RED→GREEN). *Prereq:* task 4317 (out-of-batch, gate on merge). *grammar_confirmed: true.*

## 9. Out of scope for this PRD (→ Part 2)

- The worklist driver (`engine_fixpoint.rs`), cycle contract (Tarjan-SCC + `E_EVAL_CYCLE`/`E_EVAL_UNRESOLVED`), the three geometry-path executors, `rewrite_geometry_queries` arm, cross-sub `resolve_geometry_handle_arg`, C7 retirement, the differential corpus, the 3205/4275 acceptance tests, warm/incremental unification, cutover + legacy removal — **all Part 2.**
- The `BuildScheduler` enum / env / feature flag — Part 2.

## 10. Open questions (tactical; not blocking)

1. **`assert_dag_complete` placement.** Whether it lives in `dirty.rs` beside the existing reachability helpers or as a debug hook in `engine_build.rs`'s legacy loop. **Suggested:** wherever it can read both `L(B)` and the edge graph cheapest. Decide during α/β.
2. **`assert_dag_complete` cost on large modules.** It runs a topo-sort per legacy build in debug. If it dominates debug build time on the golden corpus, gate it behind `debug_assertions && env REIFY_ASSERT_DAG`. Decide during β.
