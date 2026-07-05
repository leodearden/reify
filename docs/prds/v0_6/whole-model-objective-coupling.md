# PRD: Cross-scope / whole-model objective coupling (incl. subtree cost)

**Milestone:** v0_6 · **Status:** ACTIVE — expanded from forward-stub on dispatch (esc-4785-8) · **Date:** 2026-07-05
**Parent:** `continuous-cost-minimisation.md` §10 (out-of-scope row 1, M-WHOLE). **Cluster:** `cost-optimisation`.
**Approach:** B + H (contract + two-way boundary tests) — load-bearing ConstraintSolver seam, blast radius ≥ 3 crates.

## §0 — Provenance & supersession

This PRD was a DEFERRED forward-stub (2026-06-24) that reached dispatch on 2026-07-05 when all preconditions landed (task 4785 `[MILESTONE]`, esc-4785-8). The governing **optimiser back-end fork** was resolved interactively with Leo (2026-07-05); this document is the design-doc deliverable the stub's dispatch behaviour required. The stub's "why deferred" reasoning is retained below as §1.3 (now historical) because it names the exact structural barriers this PRD dismantles.

The capability: **"minimise the cost of this whole (sub)assembly"** — a single objective spanning `auto` params across **nested scopes** (e.g. a parent `minimize cost(self.descendants)` that jointly drives child dimensions). This is the cross-scope successor to `continuous-cost-minimisation.md` (PRD 1), absorbing the subtree/whole-model cost-as-objective deliberately deferred out of PRD 1 §10 row 1.

## §1 — Goal & consumer (G1)

### §1.1 — User-observable goal
A design author writes a nested assembly whose **parent** scope declares one cost objective over its descendants, and a single `reify eval` resolves **all** coupled child dimensions to the values that minimise that whole-assembly cost — not each child's own local optimum. Concretely, in the shipped CI example (task ε):
- `reify eval` shows a child `auto` dimension resolved to the value that minimises the **parent's** `cost(self.descendants)`, **differing** from the value the current bottom-up cascade would freeze it at;
- a scope that **reads another scope's solved `auto` cell** surfaces the **co-solved** value under `reify eval` (the surface-`.ri` **BT3** observable, deferred here from F-inherit ζ #4826, which proved it is unobservable *without* a merged solve);
- an eval test asserts the merged whole-assembly cost is **strictly lower** than the frozen-cascade baseline on the same model.

### §1.2 — Consumer (G1) — the ConstraintSolver seam + the CI example
- **In-engine seam:** the solver-side mechanisms plug into the catalogued **§3.5 ConstraintSolver** seam (`docs/prds/v0_3/engine-integration-norm.md`). β extends problem construction + the resolution driver; δ extends the `DimensionalSolver` back-end and reuses the F-result-added `solve_ranked` on the trait. **No new seam** and no orphan-producible `pub fn` in a `kernel-*` crate.
- **User-observable consumer:** the committed CI example (task ε) `examples/whole_model_cost_min.ri` + its eval test — the vertical-slice integration gate. This is the named consumer that makes the merged-solve machinery non-orphan.

### §1.3 — Structural barriers this PRD dismantles (historical, verified 2026-06-24/2026-07-05)
- `build_solver_problem` (`crates/reify-eval/src/engine_eval.rs:1284`) collects **only the own-template's** `auto` cells and **drops cross-scope reads**; a constraint mixing own-auto with a child cell reads the child cell as a **frozen constant** (`current_values`, `:1377`).
- The resolution driver (`resolve_order` → per-template solve → freeze-into-`values`, `engine_eval.rs:3616/3747`) is a strict **freeze-as-you-go bottom-up cascade** with **no fixpoint** — a parent's subtree-cost objective sees child costs as constants and cannot push them.
- F-inherit (`objective-scope-inheritance.md`) deliberately stopped at governance: **INV-5** — it *never merges two templates' `auto` cells into one `ResolutionProblem`* (§6.3). It graduated the read-set sensor into a dependency-**orderer** and flags degenerate cross-scope aggregate objectives with `W_SCOPE_COUPLING`, but leaves the numeric joint-optimisation to this PRD. **That joint-optimisation is M-WHOLE's entire reason to exist.**

## §2 — Sketch of approach & mechanisms (G1, G3)

The clusters are **already computed**: `resolve_order` (`crates/reify-eval/src/resolve_order.rs`) builds the cross-scope auto-read DAG from every constraint **and objective** term and Tarjan-partitions it into SCCs; today a non-trivial SCC merely emits `W_SCOPE_COUPLING` and falls back to source order (`resolve_order.rs:354`). Each non-trivial SCC **is exactly the set of scopes that must be co-solved**. The mechanism set:

1. **α — pre-solve clustering pass + over-cap degrade.** Consume `sccs_topo` as clusters rather than warnings. A cluster = a non-trivial SCC (scopes coupled by a spanning objective/constraint). Compute each cluster's merged dimensionality against a cap `WHOLE_MODEL_CLUSTER_DIM_CAP`. Within-cap clusters are marked for a merged solve; **over-cap** (or otherwise un-mergeable) clusters fall back to today's bottom-up frozen-approximate solve and emit the **named** diagnostic `W_COUPLING_APPROXIMATED` (a graduation of `W_SCOPE_COUPLING` — never silent; honours `feedback_silent_defaults_pattern`).
2. **β — merged cross-scope `ResolutionProblem` builder (cluster-aware freeze).** For a within-cap cluster, **union** the cluster scopes' `auto` cells, constraints, and objectives into **one** `ResolutionProblem`; solve once; **write back** the solved values to all cluster scopes. This undoes both the own-template-only collection in `build_solver_problem` and the per-template freeze (the specific thing F-inherit INV-5 refused). Scopes **outside** the cluster remain frozen constants exactly as today.
3. **γ — `cost(self.descendants)` subtree-cost objective.** The aggregate `Money` objective over descendants — the `cost(collection)` semantic (desugared form `sum(flat_map(filter(self.descendants, Costed), |c| [c.line_cost]))`), consuming the landed `self.descendants` (#3988) / `filter(_, Trait)` (#3991) / BOM roll-up (#4292) substrate. Over a merged cluster its descendant `line_cost`s are **live variables**, not frozen constants.
4. **δ — optimiser back-end: NM + clustering + deterministic multistart.** Keep the existing argmin `DimensionalSolver` (Nelder-Mead). For a merged cluster, run **best-of-K** from a **fixed deterministic** start set (corner/LHS grid — **no RNG**, preserving today's determinism-by-absence-of-stochasticity). Emit `RankedSolveResult` (best-of-K candidates + `OptimalityStatus::BestFound`) via the landed F-result carrier. The `Box<dyn ConstraintSolver>` boundary stays pluggable so a **seeded** per-cluster global solver (argmin PSO/SA in-tree, or the `cmaes` crate) can escalate later **if** real clusters exceed the cap — **not built in this PRD** (see §10).
5. **ε — CI `.ri` end-to-end** (§1.1) — the vertical-slice integration gate, including the BT3 surface observable.

**Money-objective machinery is reused, scaled to spanning scopes.** A whole-model `cost(self.descendants)` objective is **`Money`-dimensioned**, so the merged solve inherits continuous-cost §2.2's **robustness-floor default** (every inequality slack ≥ margin), `E_ROBUSTNESS_FLOOR_INFEASIBLE`, and the `cost_robustness_tradeoff(cost_expr, λ)` special-form — unchanged, now over the merged cluster.

### §2.1 — What is NOT novel substrate (G3 result)
- **Grammar.** `minimize cost(self.descendants)` and `let x : Money = cost(self.descendants)` **parse today** — continuous-cost §4 ran the grammar gate on exactly this family and marked it OK; the desugared `sum(flat_map(filter(self.descendants, Costed), |c| [c.line_cost]))` parses too. No grammar work. (`grammar_confirmed = true` for all leaves.)
- **The aggregation *semantic*** (`cost(...)` over a collection) was **explicitly reserved for M-WHOLE** by continuous-cost §2.1 ("No `cost(...)` aggregation builtin in this PRD … cross-scope → M-WHOLE"). It is **this PRD's owned work** (γ), built over landed substrate — **not** an external fiction.
- Landed substrate: `resolve_order` SCC condensation (F-inherit β #4822); §10.5 narrowest-scope-wins inheritance (F-inherit ζ #4826); `self.descendants` #3988 / `filter` #3991 / BOM roll-up #4292; Money-objective + robustness (#4795); `RankedSolveResult`/`solve_ranked` carrier (F-result). **G3: no unverified substrate.**

## §3 — Resolved design decisions (the fork)

Resolved with Leo 2026-07-05 (fused-memory `decisions_and_rationale`, esc-4785-8):

1. **Continuous-only merged clustered solve; MINLP rejected.** Discreteness (material choice, integer part-count, `Enum` `auto`) is architecturally staged **out** of M-WHOLE into the separate, unauthored **`discrete-cost-minimisation.md` (PRD 2)** over F-result — a **CP-SAT enumeration override of `solve_ranked`**, i.e. an **outer enumeration wrapping this continuous inner solve** (parent §10 rows 1–2; `ranked-solve-result.md` §6). A monolithic MINLP (SCIP/`russcip`) would re-absorb PRD 2's scope, carry the heaviest native dependency, and has the **worst determinism** story (B&B is version/platform-sensitive). **Rejected.**
2. **Back-end = Nelder-Mead + α-clustering + deterministic multistart** (fixed start set, no RNG). The `Box<dyn ConstraintSolver>` trait stays pluggable for a **future** seeded per-cluster global escalation — but that escalation is **not built here** and the seed+RNG-pin+single-thread-reduction determinism invariant is **not taken on** now. Rationale: coupling is sparse (α keeps clusters low-dimensional, where Nelder-Mead is fine); determinism is the hard constraint and this option preserves today's regime for free.
3. **δ emits `RankedSolveResult`** (multistart best-of-K *is* a ranked candidate set — F-result §0.1 names "a multi-start continuous sweep" as an anticipated producer) ⇒ a **soft dependency on the landed F-result carrier** (types + defaulted trait method already on main; no new F-result work required).
4. **Over-cap / too-large irreducible clusters degrade to bottom-up frozen-approximate + a named diagnostic** (`W_COUPLING_APPROXIMATED`, a graduation of `W_SCOPE_COUPLING`) — **never silent**.

**The load-bearing insight:** the back-end (δ) is the *cheap, late-bindable* part (clean trait, single call site); the high-effort work is α clustering + β merged builder + N-scope write-back — undoing the freeze-as-you-go architecture. δ's choice does not gate whether arbitrary-scope is achievable; **β does.**

## §5 — Contract (H)

### §5.1 — Clustering pass (α)
- **Input:** `resolve_order`'s `sccs_topo` (the SCC condensation of the cross-scope auto-read DAG) + the per-template `auto` cell counts.
- **A cluster** is a non-trivial SCC (≥ 2 scopes mutually coupled by a spanning objective/constraint), plus any scope whose objective is an inherited **aggregate** over descendants whose `auto` cells it reads (the F-inherit "degenerate aggregate" case, §3.2). A single scope with only own-auto reads is **not** a cluster (unchanged path).
- **Cap:** `WHOLE_MODEL_CLUSTER_DIM_CAP` (merged `auto`-var count). Within-cap → `MergedSolve`. Over-cap or un-mergeable → `ApproximatedFallback` + emit `W_COUPLING_APPROXIMATED { cluster_scopes, dim, cap }` once per cluster (deduped).
- **Invariant (back-compat, from resolve_order INV-2):** a model with **no** cross-scope auto reads yields **zero** clusters and a byte-identical resolution order + result to today.

### §5.2 — Merged `ResolutionProblem` builder (β)
- **Signature intent:** `build_merged_solver_problem(cluster: &Cluster, module, values) -> ResolutionProblem` — `auto_params` = union of all cluster scopes' regular auto cells; `constraints` = the union filtered to those reading ≥ 1 cluster auto id (cross-cluster reads stay frozen via `current_values`); `objective` = the cluster's spanning objective (γ), with descendant `line_cost`s now referencing in-cluster auto cells (live).
- **Write-back:** on `Solved`, each solved value is written to its **owning** scope's cell and marked `Determined`; downstream (non-cluster) scopes then read them as constants exactly as today.
- **Objective fold is consumed abstractly.** β assembles the `ObjectiveSet` and hands it to the solver's weighted fold; it must **not** hard-code raw-`f64` weight semantics. If sibling 4786 flips `ObjectiveTerm.weight` `f64 → Value::Scalar`, β/γ/δ remain compatible (they scale a single-aspect `Money` objective, dimensionally safe under either representation). **This is why 4785's fold-touching tasks are sequenced *after* 4786** (§8, decision from Leo).
- **Determinism:** the merged problem's variable ordering is a stable function of scope resolution order + source index (no map iteration order leaks).

### §5.3 — Back-end (δ)
- Best-of-K multistart from a **fixed** deterministic start set derived from `default_bounds_for` corners/midpoints; K and the start set are pure functions of the problem (no RNG, no clock, no seed to maintain). Result = the best feasible candidate by objective; ties broken deterministically by candidate index.
- Emits `RankedSolveResult { candidates: [best-of-K…], optimality: BestFound }` — never `ProvenOptimal` (derivative-free, budget-bounded; unchanged invariant I3).
- Inherits the `Money`-dimension robustness floor (continuous-cost §2.2) unchanged over the merged cluster.

## §6 — Boundary-test sketch (H)

| # | Facing | Scenario | Preconditions | Postconditions |
|---|---|---|---|---|
| BT1 | producer (α) | Two child parts + a parent `minimize cost(self.descendants)` → **one** cluster of the coupled scopes | nested `.ri`, parent aggregate objective over both children's `auto` dims | α yields exactly one cluster containing {parent, childA, childB}; within cap → `MergedSolve` |
| BT2 | producer (α) | Over-cap cluster degrades, not silently | a fixture whose coupled cluster exceeds `WHOLE_MODEL_CLUSTER_DIM_CAP` | `reify check` emits `W_COUPLING_APPROXIMATED` naming the cluster + dim + cap; result falls back to bottom-up approximate |
| BT3 | consumer (ε) | **Cross-scope solved-auto surface read** (the F-inherit ζ #4826 deferral) | a scope reads another scope's `auto` cell that the merged solve resolves | `reify eval` surfaces the **co-solved** value (not a frozen/undef) — impossible before β |
| BT4 | consumer (β+δ) | Merged solve beats the frozen cascade | same model solved both ways | merged whole-assembly `cost(self.descendants)` **< strictly** the bottom-up frozen baseline; the co-optimised child `auto` differs from its frozen value |
| BT5 | consumer (determinism) | Same input → identical result | any cluster `.ri` | two `reify eval` runs produce bit-identical resolved values + identical `RankedSolveResult` candidate ordering |
| BT6 | back-compat | Uncoupled model unchanged | a `.ri` with no cross-scope auto reads | resolution order + every resolved value byte-identical to pre-α main (resolve_order INV-2 preserved) |

The **integration-gate task ε** names BT3+BT4 (the CI `.ri` + eval test) as its user-observable signal, closing the G2 loop.

## §7 — Pre-conditions for activating

**All landed on main** (flip condition met — task 4785 dispatched):
- F-inherit `objective-scope-inheritance.md`: `resolve_order.rs` enforced dependency-ordered scope resolution + §10.5 narrowest-scope-wins (ζ #4826, β #4822 done).
- `structural-query` γ #3988 (`self.descendants`) / δ #3991 (`filter(_, Trait)`) / #4292 (BOM roll-up).
- `continuous-cost-minimisation.md` #4795 (Money-objective + robustness machinery; terminal).
- `ranked-solve-result.md` (F-result) carrier + defaulted `solve_ranked`.

**Cross-PRD sequencing (hard, Leo 2026-07-05):** 4785's **fold-touching tasks (β, γ, δ, ε)** depend on sibling **4786** `multi-aspect-objective-units-coherence.md`'s **dimensioned-weight (γ) terminal task** (the `ObjectiveTerm.weight` `f64 → Value::Scalar` decision). The WeightedSum fold is **triplicated** (`solver.rs` `eval_objective_set`, `engine_eval.rs` provenance fold, `registry.rs` `eval_rank_cost`) and all sites change together — sequencing 4785 after 4786 avoids a mid-flight breaking IR change. α (clustering + degrade diagnostic) is **not** fold-touching and may proceed independently.

## §8 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `continuous-cost-minimisation.md` (PRD 1) | consumes | `Money`-objective + robustness-floor + `cost_robustness_tradeoff`, scaled to spanning scopes | PRD 1 (this PRD reuses) | landed |
| `objective-scope-inheritance.md` (F-inherit) | consumes | `resolve_order` SCC condensation + §10.5 inheritance; this PRD **graduates** the SCC sensor into a clustering **actuator** (the exact INV-5 boundary F-inherit drew) | this PRD owns the clustering actuator | landed substrate |
| `ranked-solve-result.md` (F-result) | consumes | `RankedSolveResult` / `solve_ranked` carrier for δ's best-of-K multistart | F-result (this PRD produces into it) | landed |
| `multi-aspect-objective-units-coherence.md` (M-UNITS, 4786) | **sequenced-after** | `ObjectiveTerm.weight` dimensioning (`f64 → Value::Scalar`); the triplicated WeightedSum fold β/γ/δ consume | **4786** owns the weight decision; 4785 consumes the fold abstractly and lands after | **blocked-on** — 4786 not yet decomposed |
| `discrete-cost-minimisation.md` (PRD 2, unauthored) | sibling / non-seam | discrete/mixed cost via CP-SAT enumeration override of `solve_ranked` — the **outer loop** wrapping this continuous inner solve | PRD 2 (future) | future; M-WHOLE stays continuous |
| `structural-query-traversal.md` (#3988/#3991) + `io-lifecycle-bom-cost.md` (#4292) | consumes | descendants walk + occurrence roll-up feeding γ's aggregate objective | those PRDs | landed |

No reciprocal-ownership ambiguity: 4786 unambiguously owns the weight-dimensioning IR change; this PRD owns the clustering actuator + merged builder + back-end and consumes the fold. The §3.5 ConstraintSolver seam's contract owner is unchanged (`kinematic-constraints-*`); this PRD extends the `DimensionalSolver` impl, not the seam shape.

## §9 — Decomposition plan

Greek labels; task IDs assigned at decompose time. **α is fold-independent** (proceeds now); **β, γ, δ, ε are sequenced after 4786's dimensioned-weight task.**

- **α — pre-solve clustering pass + over-cap degrade diagnostic.** Modules: `reify-eval` (`resolve_order.rs`, `engine_eval.rs`), `reify-core` (diagnostic code). Consume `sccs_topo` as clusters; cap check; over-cap → `ApproximatedFallback` + `W_COUPLING_APPROXIMATED`. **Observable (leaf):** `reify check` on an over-cap nested fixture emits `W_COUPLING_APPROXIMATED` naming the cluster/dim/cap. **Also unlocks β.** Prereq: landed `resolve_order`.
- **β — merged cross-scope `ResolutionProblem` builder (cluster-aware freeze).** Modules: `reify-eval` (`engine_eval.rs` builder + `resolve_order` driver loop), `reify-ir` (`ResolutionProblem`). Union cluster auto cells/constraints/objectives into one problem; write back N scopes. **Intermediate — unlocks γ-coupling / δ / ε.** Prereq: **α**; cross-PRD **4786-weight**.
- **γ — `cost(self.descendants)` subtree-cost objective.** Modules: `reify-eval` / `reify-compiler` (aggregate `cost(collection)` semantic over #3988/#3991/#4292). **Observable (leaf):** `reify eval` shows `cost(self.descendants)` evaluating to the summed `Money` over descendants. **Also unlocks ε.** Prereq: landed #3988/#3991/#4292; cross-PRD **4786-weight**.
- **δ — optimiser back-end: NM + clustering + deterministic multistart, emits `RankedSolveResult`.** Modules: `reify-constraints` (`solver.rs`, `registry.rs`). Best-of-K fixed-start multistart over merged clusters; emit ranked candidates + `BestFound`; keep trait pluggable (no seeded solver built). **Observable (leaf):** `reify` surfaces a `RankedSolveResult` with K candidates + `BestFound` on a merged cluster. Prereq: **β**; landed F-result; cross-PRD **4786-weight**.
- **ε — CI `.ri` end-to-end (integration-gate leaf).** Modules: `examples/`, `crates/reify-eval/tests`. Committed `examples/whole_model_cost_min.ri` + eval test: parent `minimize cost(self.descendants)` jointly drives child dims; `reify eval` shows the co-solved child `auto` (differs from frozen), the **BT3** cross-scope solved-auto surface read, and merged cost **<** frozen baseline. **Observable (leaf):** the CI example + eval test (BT3+BT4). Prereq: **β, γ, δ**; cross-PRD **4786** (transitive).

**B+H shape:** α = foundation-with-observable (the degrade diagnostic); β = the contract/seam linchpin; γ/δ = incremental slices; ε = the vertical-slice integration gate whose signal is the boundary-test sketch. No separate companion-correction phase needed — the only cross-PRD prose touched (§3.5 seam) is already extended by F-result.

## §10 — Out of scope for this PRD

- **Discrete / mixed-integer cost** (material choice, integer part-count, `Enum` `auto`) → `discrete-cost-minimisation.md` (PRD 2) over F-result, as a CP-SAT enumeration **outer loop** wrapping this continuous inner solve. **MINLP is rejected** (§3.1).
- **A seeded global optimiser** (CMA-ES / argmin PSO/SA) for clusters exceeding `WHOLE_MODEL_CLUSTER_DIM_CAP` — the trait stays pluggable for it, but it is not built here; over-cap clusters degrade (§3.4). Building it (and taking on its seed+RNG-pin+single-thread-reduction determinism invariant) is a future PRD, triggered only if real models produce over-cap clusters.
- **Multi-aspect (cost + mass + …) objectives** → 4786 M-UNITS (this PRD stays single-aspect `Money`).
- **Geometry-dependent material/waste cost** (outer candidate loop) → M-WASTE.

## §11 — Open questions (tactical — decide at implementation time)

1. **`cost(self.descendants)` surface: builtin vs desugared.** γ can ship a `cost(collection)` aggregate builtin/overload, or rely on the already-parsing desugared `sum(flat_map(filter(self.descendants, Costed), |c| [c.line_cost]))`. **Suggested:** ship the `cost(collection)` builtin as sugar over the desugared form (better UX, same eval path). Decide during γ.
2. **`WHOLE_MODEL_CLUSTER_DIM_CAP` value.** The Nelder-Mead degradation knee is ~10–15 vars/cluster (simplex collapse; the solver already hits the 5000-iter cap at n ≥ 9). **Suggested:** start at 12; tune against real fixtures. Not a design decision — a scalar constant. Decide during α.
3. **`auto(free)` vs strict `auto` in ε's example.** The `continuous_cost_min.ri` precedent uses `auto(free)` to skip the uniqueness re-solve (which would mask the floor signal). ε likely does the same per-child. Decide during ε.
4. **Multistart K and start-set shape** (corner grid vs LHS-of-fixed-points). Pure determinism either way; K trades cost for basin coverage. **Suggested:** K = 2·(dim+1) corner/midpoint points. Decide during δ.
