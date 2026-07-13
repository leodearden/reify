# PRD: Whole-model joint-drive solver seam (objective-position cost expansion + per-trial SCC recompute)

**Milestone:** v0_6 · **Status:** active (contract resolving a decomposition gap in `whole-model-objective-coupling.md`) · **Date:** 2026-07-13 · **Milestone lineage:** 4785 (M-WHOLE) · **Approach:** B + H (ConstraintSolver §3.5 seam)

## 1. Goal

A parent scope's `minimize cost(self.descendants)` objective becomes a **live function of coupled child `auto` dimensions** under the real `reify_constraints::DimensionalSolver`. A single `reify eval` drives a child `auto` dimension to the value that minimises the *whole-assembly* cost — differing from the value the bottom-up freeze-as-you-go cascade would pin it at — and the merged whole-assembly cost is strictly less than that frozen-cascade baseline.

This is the joint-drive mechanism M-WHOLE assumes but no task built.

## 2. Background — the decomposition gap this closes

`whole-model-objective-coupling.md` (M-WHOLE) was decomposed into α=5013 (cluster detection), β=5014 (merged cross-scope `ResolutionProblem` builder + N-scope write-back), γ=5015 (`cost(self.descendants)` subtree-cost aggregate as an **eval/value-position** semantic), δ=5016 (Nelder-Mead + deterministic multistart back-end), and ε=5017 (the CI-example integration gate asserting BT3 + BT4). β/γ/δ all landed and are marked done.

The M-WHOLE **capability manifest** exposes the gap in its own bookkeeping: **BT4 — "a parent `minimize cost(self.descendants)` jointly drives coupled child `auto` dimensions; merged cost < frozen baseline" — is the only capability lacking a `producer:task` binding.** It is justified instead by an achievability argument ("the merged solve optimises over a *superset* of free variables ⇒ merged optimum ≤ frozen") that *presupposes* the very seam nobody built. β delivers cluster **union + write-back**; γ delivers `cost()` as a **value** (its observable is `reify eval` showing the summed `Money`, in Let/eval position); δ **keeps** the existing Nelder-Mead solver and adds multistart. None owns the two seams that make the objective a live function of child autos.

Both gaps were verified by direct code inspection (three independent passes, 2026-07-13; base `2c8d7212`, unchanged on current main):

- **Gap A — objective-position `cost()` is never expanded.** `apply_cost_aggregation` (`crates/reify-eval/src/structural_query.rs:633`) has exactly one caller — the Let-cell loop (`engine_eval.rs:3890`, gated `if !matches!(cell.kind, ValueCellKind::Let) { continue }`). `build_solver_problem` (`engine_eval.rs:1541`) and `build_merged_solver_problem` (`:1728`) plain-clone objective terms into the `ResolutionProblem` with no structural-query pass. So `minimize cost(self.descendants)` reaches the solver as a raw `FunctionCall{name:"cost"}` → `eval_builtin("cost")` returns `Value::Undef` → `eval_objective_set` (`solver.rs:815`) returns `None` → `UNDEF_OBJECTIVE_PENALTY` (`f64::MAX/2`, `solver.rs:24`, applied `:877`): a flat constant, zero gradient, child autos undriven. Constraint-position `cost()` is likewise unexpanded (`expand_constraint_expr`, `structural_query.rs:718`, never calls `apply_cost_aggregation`).

- **Gap B — dependent Let cells stay frozen per trial** (the *deeper* blocker). `build_trial_values` (`crates/reify-constraints/src/solver.rs:138-150`) clones the base `ValueMap` and inserts **only** the trial auto-param scalars. The objective `minimize total` compiles to a bare `ValueRef(total)` (not inlined); `cost(self.descendants)` expands (once A is fixed) to a sum of `ValueRef(line_cost)` — and `line_cost`/`total` are **child Let cells** `build_trial_values` never touches. The topological Let recompute (`evaluate_let_bindings`, invoked at `engine_eval.rs:4351`) runs **post-solve only**. So even with Gap A fixed, the objective is constant w.r.t. the autos the solver varies. **Both A and B are required.**

**The framing (why B, not objective-flattening).** Reify keeps its value-dependency graph acyclic via the freeze-as-you-go discipline: each solve is condensed to one opaque node (freeze inputs → solve → write outputs). But with joint optimisation there is a genuine **strongly-connected component**: `auto params → line_cost → total → objective → (solver re-picks) auto params`; the only cyclic edge is the solver's feedback edge. The fix is to **condense that SCC into a first-class optimisation node and unroll its interior as a topologically-ordered sub-DAG re-evaluated per trial.** This gives a *principled* definition of "which cells recompute per trial" — exactly the induced sub-DAG on every value-dependency path from an optimisation's auto params to its objective/constraints (the SCC minus the auto params) — and reuses the same topological recompute the engine already trusts post-solve. The considered alternative, objective **flattening/substitution** (inline each `ValueRef(line_cost)` down to auto params so no per-trial recompute is needed), was rejected: it re-introduces the diamond-duplication / `O(2ⁿ)` blow-up the codebase already fights (`solver.rs:731`), must separately flatten every constraint, and produces a *different* expression shape than the post-solve write-back recomputes — a drift hazard.

## 3. Relationship to M-WHOLE

This PRD **owns** the joint-drive seam that M-WHOLE ε (5017) consumes as its BT3/BT4 substrate. It does **not** re-open β/γ/δ (their delivered extents — union+write-back, value-position `cost()`, multistart back-end — are correct and reused verbatim). It corrects the M-WHOLE *decomposition*, not its design thesis. Once this PRD's vertical-slice leaf lands, ε's BT3/BT4 premise becomes true and 5017 re-dispatches unchanged (a real `add_dependency` edge `5017 → β`).

## 4. Sketch of approach

Two composed seams, each anchored at a confirmed integration point (in-engine consumer = §3.5 ConstraintSolver, per `engine-integration-norm.md`):

1. **(A) Objective/constraint-position `cost()` + structural-query expansion.** In `build_solver_problem` / `build_merged_solver_problem`, run the same `apply_cost_aggregation` + `expand_structural_query` + `apply_trait_filters` pass the Let-cell loop already runs, over each objective term and each constraint expression, *before* they enter the `ResolutionProblem`. This populates the objective's expression graph with the `ValueRef(child.line_cost)` nodes so they can *be* in the SCC.

2. **(B) Per-trial topological recompute (SCC-unroll).** Extend `ResolutionProblem` with a topologically-ordered `dependent_cells: Vec<(ValueCellId, CompiledExpr)>` — the induced sub-DAG of non-auto, non-`@optimized` cells that transitively feed the (expanded) objective/constraints and read ≥1 cluster auto param. `build_trial_values` folds this list in via `reify_expr::eval_expr` *after* inserting the trial autos, so `line_cost`/`total` reflect the trial. Because the objective, every constraint, and the feasibility/robustness gates all read the same `build_trial_values` output, one fold fixes them all.

The **authoritative cross-scope topological order** is built once, engine-side, from the existing primitives (`extract_value_deps` → transitive closure over child `default_expr`s → `topological_sort`), and is used by **both** the per-trial fold and the post-solve write-back (`engine_eval.rs:4351`) so the solver never optimises a different cell order than the engine later materialises.

## 5. Resolved design decisions

1. **B (per-trial recompute), not B2 (objective flattening).** Reasons in §2. Reuses the same `eval_expr(default_expr, EvalContext::new(values, functions))` the engine already trusts; no expression blow-up; one trial map fixes objective + constraints + gates together; no representational drift from the post-solve recompute.
2. **The recompute set is graph-derived, not hand-listed.** It is exactly the induced sub-DAG (SCC minus autos): cells transitively feeding the objective/constraints that read ≥1 cluster auto. This is a principled, deterministic definition.
3. **One authoritative cross-scope topo order, shared by per-trial fold and post-solve write-back.** This is the primary correctness invariant (§6). The child `line_cost` cells live in child scopes; unioning them into one combined DAG across the cluster is the single new engine-side assembly.
4. **Constraint-position `cost()` is expanded in the same pass as objective-position.** Constraints have the identical frozen-Let bug (`compute_total_violation` reads the same trial map), and B's fold fixes them for free — so A must cover both to keep them consistent.
5. **`@optimized` / ComputeNode-dispatch cells are excluded from the per-trial fold** and left at their frozen value (re-running them through plain `eval_expr` would bypass the compute-dispatch registry — the load-bearing exclusion at `engine_eval.rs:7878-7884/7966-7972`). A Costed child whose `line_cost` is `@optimized` therefore does not couple; this is a documented limitation, surfaced in §11.
6. **SCC-admissibility guardrail.** A value-dependency SCC is admissible **only** when its sole back-edge is a solver feedback edge (an auto param chosen to optimise an objective that transitively depends on it). Any other cycle (`a → b → a` with no optimisation closing it) keeps failing as the existing cycle / `W_SCOPE_COUPLING` diagnostic. This prevents the SCC-unroll from silently admitting illegal data cycles.
7. **Scope = one optimisation cycle.** A single parent objective over child autos across a merged cluster. The condensation-DAG framing generalises to nested/sibling optimisation cycles (each optimisation SCC condenses to a node; nested optimisations are nodes-within-nodes), but building/testing that generality is a future leaf (§11).

## 6. Contract (H)

### 6.1 `ResolutionProblem` extension (`crates/reify-ir/src/constraint.rs:364`)

```rust
pub struct ResolutionProblem {
    // ...existing: auto_params, constraints, current_values, objective, functions...
    /// Topologically-ordered dependent cells re-evaluated on every solver trial.
    /// INVARIANT (ordering): evaluating these in sequence against a ValueMap that
    /// already holds the trial auto scalars yields exactly the values the post-solve
    /// write-back (`evaluate_let_bindings`) produces for the same autos.
    /// INVARIANT (membership): exactly the non-auto, non-@optimized ValueCells that
    /// (a) transitively feed the expanded objective or any constraint, AND
    /// (b) transitively read ≥1 auto_param in this problem.
    /// Empty for problems with no coupled dependent cells (identical to today's behaviour).
    pub dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
}
```

- **Data, not a closure.** `ResolutionProblem` is `Clone` (multistart clones it); the field is plain IR data, evaluated with the `reify-expr` context the solver already constructs. No new crate dependency (`reify-constraints` already depends on `reify-expr` and calls `eval_expr`).

### 6.2 Per-trial fold (`crates/reify-constraints/src/solver.rs:138`)

`build_trial_values(base, params, x)` → after inserting the auto scalars, for each `(id, expr)` in `problem.dependent_cells` **in order**: `values.insert(id, eval_expr(expr, EvalContext::new(&values, functions)))`. The result flows unchanged into `compute_total_violation` (`:871`) and `eval_objective_set` (`:877`).
- **INVARIANT:** an auto param is never overwritten by a dependent cell (membership excludes autos).
- **INVARIANT (empty-case identity):** with `dependent_cells == []` the function is byte-identical to today.

### 6.3 Engine-side assembly (`crates/reify-eval/src/engine_eval.rs:1537` and `:1802`)

One helper produces the ordered `dependent_cells` from the expanded objective/constraint terms, and the **same** ordered list feeds the post-solve write-back at `:4351`.
- **INVARIANT (single authority):** the per-trial order and the post-solve write-back order are produced by one call — they cannot diverge.
- **INVARIANT (expansion-before-assembly):** objective/constraint terms pass through `apply_cost_aggregation` + `expand_structural_query` + `apply_trait_filters` before the induced sub-DAG is computed (else `cost()` hides its `line_cost` deps).

### 6.4 Admissibility (`W_SCOPE_COUPLING` / cycle detection)

- **INVARIANT:** a value-dependency SCC is admitted iff its only back-edge is a solver feedback edge; every other cycle still errors with the existing diagnostic.

## 7. Boundary-test sketch (H) — facing both the solver side and the engine side

| # | Scenario | Preconditions | Postcondition asserted |
|---|---|---|---|
| BT-1 (solver) | Per-trial recompute makes the objective non-constant | `ResolutionProblem{ auto_params:[q], dependent_cells:[(line_cost, unit_cost*q)], objective: sum(line_cost) }` | `eval_objective_set` returns different values for two different trial `q` vectors (objective is a live function of `q`; not the `UNDEF_OBJECTIVE_PENALTY` constant) |
| BT-2 (solver) | Empty-case identity | `dependent_cells == []` | `build_trial_values` output byte-identical to pre-change; regression suite green |
| BT-3 (engine) | Objective-position `cost()` expands | model with `minimize cost(self.descendants)` | the `ObjectiveSet` entering the `ResolutionProblem` contains `ValueRef(child.line_cost)` terms, not a raw `cost` `FunctionCall` |
| BT-4 (engine) | Per-trial order == post-solve write-back order | merged cluster with cross-scope child `line_cost` cells | the `dependent_cells` order equals the order `evaluate_let_bindings` uses at `:4351` (same authority) |
| BT-5 (e2e, **leaf**) | Joint drive | `examples/whole_model_joint_drive.ri` (2 scopes, parent `minimize cost(self.descendants)`, child `auto`) | child `auto` resolves to the whole-model cost-min value, **≠** its bottom-up frozen freeze; merged cost **strictly <** frozen-cascade baseline |
| BT-6 (guardrail, negative) | Illegal data cycle still errors | a non-optimisation `a→b→a` value cycle | `reify eval` emits the existing cycle / `W_SCOPE_COUPLING` diagnostic (rejection **observed** to fire) |
| BT-7 (guardrail) | `@optimized` cell excluded | Costed child with `@optimized` `line_cost` | that cell is absent from `dependent_cells`; no compute-dispatch bypass |

BT-5 is the vertical-slice leaf's user-observable signal (closing G2). BT-1/-3/-4 face the producer (engine) side; BT-2/-6/-7 are the safety backstops.

## 8. Pre-conditions for activating

- **Landed substrate (no dep edge needed; confirmed on main 2026-07-13):** `apply_cost_aggregation` (`structural_query.rs:633`), `ResolutionProblem` (`constraint.rs:364`), `build_trial_values` (`solver.rs:138`), `topological_sort` (`dirty.rs:153`), `extract_value_deps` (`deps.rs:2634`), `evaluate_let_bindings` (`engine_eval.rs:8067`), `build_merged_solver_problem` (β, 5014), value-position `cost()` (γ, 5015), NM+multistart back-end (δ, 5016).
- **Grammar:** `minimize cost(self.descendants)` over children with `auto(free)` dims parses today (`tree-sitter parse --quiet` exit 0; fixture verified 2026-07-13). `grammar_confirmed=true`.

## 9. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `whole-model-objective-coupling.md` (M-WHOLE) | produces-for | joint-drive under §3.5 ConstraintSolver (objective-position `cost()` expansion + per-trial SCC recompute); binds M-WHOLE manifest **BT4** | **this PRD** | queued (γ correction task) |
| task 5017 (M-WHOLE ε integration gate) | consumed-by | ε's BT3/BT4 read the co-solved child auto this PRD makes movable | this PRD (β leaf); 5017 gets a real `add_dependency` edge onto β | queued |

No new contested-ownership pair (not among the three known seams in `phase-3-breadcrumb-map.md` §3).

## 10. Decomposition plan

**Phase 1 — engine foundation (intermediate, roped into β via C-as-integration-gate):**

- **α — Objective/constraint-position `cost()` expansion + `dependent_cells` assembly.**
  - Modules: `crates/reify-ir/src/constraint.rs`, `crates/reify-eval/src/engine_eval.rs`.
  - Does: (1) expand objective + constraint terms via `apply_cost_aggregation`/`expand_structural_query`/`apply_trait_filters` in `build_solver_problem` (`:1537`) and `build_merged_solver_problem` (`:1802`); (2) add `ResolutionProblem.dependent_cells`; (3) build the authoritative cross-scope induced-sub-DAG topo order and route it to **both** the problem builder and the post-solve write-back (`:4351`). Boundary tests BT-3, BT-4.
  - Observable: **intermediate** — no standalone user signal; unlocks β.
  - Prereq: β=5014, γ=5015, δ=5016 (landed).

**Phase 2 — vertical slice (LEAF, integration gate):**

- **β — Per-trial SCC recompute in the solver + joint-drive example + gate.**
  - Modules: `crates/reify-constraints/src/solver.rs`, `examples/whole_model_joint_drive.ri`, `crates/reify-constraints/tests/` (or `crates/reify-eval/tests/`).
  - Does: fold `dependent_cells` into `build_trial_values` (`:138`) after the autos; add the SCC-admissibility guardrail; commit the minimal 2-scope example + eval test + boundary tests BT-1, BT-2, BT-5, BT-6, BT-7.
  - Observable (**user-observable leaf**): `reify eval examples/whole_model_joint_drive.ri` resolves a child `auto` to the whole-model cost-min value (≠ its frozen freeze) and merged cost strictly < the frozen-cascade baseline — CLI output difference + CI eval test.
  - Prereq: α.

**Phase 3 — companion correction (cross-PRD prose):**

- **γ — Bind M-WHOLE manifest BT4 → this PRD's β leaf.**
  - Modules: `docs/prds/v0_6/whole-model-objective-coupling.md`, `docs/prds/v0_6/whole-model-objective-coupling.capability-manifest.md`.
  - Does: record `producer:task-β` on BT4 (and BT3's dependence on the moved child auto); note the joint-drive seam is owned by this PRD; cross-link.
  - Observable: the M-WHOLE manifest's BT4 line now carries a `producer:task` binding (the missing binding the escalation exposed). **Correction task**, not a user-observable code leaf.
  - Prereq: β (the leaf it binds to must be named/filed).

**Downstream (wired at resolution time, not part of this batch):** `add_dependency(5017 → β)`; 5017 returns to `pending`.

## 11. Out of scope for this PRD

- **Nested / sibling optimisation cycles.** The condensation-DAG framing supports N cycles, but only the single parent-objective-over-child-autos cycle is built/tested here. Multiple objectives across a cluster, or a child scope carrying its own inner objective inside a parent optimisation, is a future M-WHOLE leaf.
- **Per-trial recompute of `@optimized` / ComputeNode-dispatch cells.** Excluded by design decision 5 (compute-dispatch bypass hazard). A Costed child whose `line_cost` is `@optimized` will not couple; a future task could add a dispatch-safe per-trial path.
- **Seeded global solver / MINLP.** Unchanged from M-WHOLE δ §10 — the NM + deterministic-multistart back-end is reused as-is.

## 12. Open questions (tactical — decide at impl)

1. **Where the induced-sub-DAG helper lives** — a new fn in `engine_eval.rs` vs extending `detect_let_cycle`/`build_combined_param_let_graph` to a cross-template combined graph. Suggested: a new helper reusing `extract_value_deps` + `topological_sort`, so `detect_let_cycle`'s single-template contract is untouched. Decide in α.
2. **Multistart interaction (δ).** Per-trial recompute lives inside each Nelder-Mead run, so best-of-K multistart should compose transparently; confirm no start-set caching assumes a frozen objective. Decide in β.
3. **Gate-resident test registration.** If β's eval test lands as a `tests/infra/test_*.sh` or a heavy/wall-clock-bounded crate test, its drift-guard registration (`run-all-classification.manifest` / `test_no_new_wallclock_upper_bounds.sh` / `.config/nextest.toml`) must ride the **same diff** (overlay rule). A plain non-heavy crate eval test needs none. Decide in β.
