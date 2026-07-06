# Capability manifest — `whole-model-objective-coupling.md` (M-WHOLE, task 4785)

Mechanizes G3 + G6 per leaf for the §9 decomposition (α β γ δ ε). Built at decompose
time (2026-07-05) by binding every capability each leaf's user-observable signal asserts
to on-main evidence (`grep:<file>:<line> wired` / `producer:task-<label> upstream` /
`grammar-fixture:<path> parses` / `floor:<bound> > <method-floor>`). A binding resolving
to any FAIL value (`declared-only` · `test-only` · `producer-absent` · `producer-extent-short`
· `producer-downstream` · `fixture-ERROR` · `bound≤floor` · `rejection-absent`) blocks the
batch. **Result: no FAIL bindings — batch clears the manifest gate.**

Substrate is **all landed**; `grammar_confirmed = true` for all leaves. New symbols
(`W_COUPLING_APPROXIMATED`, the merged `ResolutionProblem` builder, the `cost(collection)`
aggregate, best-of-K multistart) are **this-PRD-produced**, not assumed substrate — each is
bound to `producer:task-<label>` with the DAG-direction check (producer upstream of the leaf
that observes it).

Evidence commands: reify grep targets are the eval dispatch tables / `engine_eval.rs` walk,
the `reify-constraints` solver+registry, the `reify-ir` carriers, and the `structural_query`/
`bom_report` producers (overlay *Capability Manifest — reify evidence forms*). Empty-value
sentinel `Value::Undef` — **N/A here** (this PRD is solver/eval control-flow, no result-field
sampling; the FEA/modal field-population hot zone does not apply).

---

## β is the only pure-intermediate (no standalone signal)

β (merged cross-scope `ResolutionProblem` builder) has **no** standalone user-observable
signal — it is roped into the ε integration gate (C-as-integration-gate). Its substrate is
still bound here for the DAG:

| Capability | Evidence | Verdict |
|---|---|---|
| `build_solver_problem` own-template collection + `current_values` freeze (the two things β undoes) | `grep:crates/reify-eval/src/engine_eval.rs:1284` (builder), `:1377` (`current_values`) — wired | PASS |
| `ResolutionProblem` carrier to union cluster cells/constraints/objectives into | `grep:crates/reify-ir/src/constraint.rs:288` — wired | PASS |
| Cluster partition to union over | `producer:task-α` (upstream; `depends_on α`) | PASS |
| Merged builder + N-scope write-back | `producer:task-β` (β-owned; undoes F-inherit INV-5) | PASS |
| Objective fold consumed **abstractly** (no raw-f64 weight hard-coding; §5.2) | `grep:crates/reify-constraints/src/registry.rs:484` (`eval_rank_cost`), solver.rs `eval_objective_set` fold — β hands `ObjectiveSet` to the existing weighted fold | PASS |

Consumer: ε (integration gate) + δ (back-end). No orphan.

---

## α — `reify check` on an over-cap nested fixture emits `W_COUPLING_APPROXIMATED`

Leaf signal (BT2): `reify check` emits `W_COUPLING_APPROXIMATED { cluster_scopes, dim, cap }`
naming the cluster + dim + cap; result falls back to bottom-up approximate. Also intermediate
(unlocks β).

| Capability | Evidence | Verdict |
|---|---|---|
| `sccs_topo` SCC condensation, consumable as clusters | `grep:crates/reify-eval/src/resolve_order.rs:259` (`sccs_topo` built) — wired (F-inherit β #4822) | PASS |
| `W_SCOPE_COUPLING` diagnostic to **graduate** (α turns the sensor into an actuator) | `grep:crates/reify-eval/src/resolve_order.rs:374` — wired | PASS |
| `W_COUPLING_APPROXIMATED` new named diagnostic | `producer:task-α` (α-owned; grep-absent today = correct — this leaf emits it) | PASS |
| `WHOLE_MODEL_CLUSTER_DIM_CAP` cap constant | `producer:task-α` (α-owned scalar; value tactical per §11 Q2) | PASS |
| Rejection/diagnostic **fires** on over-cap (G6 branch 4) | in-task: α builds **both** the cap check and the emitter → the observing leaf owns the mechanism (not a rejection of pre-existing substrate) | PASS |
| Back-compat: no-cross-scope-read model yields zero clusters, byte-identical result (resolve_order INV-2) | `grep:crates/reify-eval/src/resolve_order.rs:448` (acyclic-crossing test asserts no emission) — invariant already tested | PASS |

---

## γ — `reify eval` shows `cost(self.descendants)` summing `Money` over descendants

Leaf signal: `reify eval` shows `cost(self.descendants)` evaluating to the summed `Money`
over descendants. Also unlocks ε.

| Capability | Evidence | Verdict |
|---|---|---|
| `self.descendants` accessor (eval dispatch) | `grep:crates/reify-eval/src/structural_query.rs:462` (`enumerate_descendants`), `:491`; compiler `crates/reify-compiler/src/expr.rs:985` (`STRUCTURAL_QUERY_ACCESSORS`) — wired (#3988/#3982) | PASS |
| `filter(self.descendants, Trait)` | `grep:crates/reify-eval/src/structural_query.rs:559`; `crates/reify-compiler/src/expr.rs:2060` — wired (#3991) | PASS |
| `Costed` trait + `line_cost : Money` (per-line cost cell) | `grep:crates/reify-compiler/stdlib/io.ri:111` (`trait Costed`), `:113` (`line_cost : Money`) — wired (#4292) | PASS |
| BOM / cost roll-up over lifecycle traits | `grep:crates/reify-eval/src/bom_report.rs:1` — wired (#4292) | PASS |
| `cost(collection)` aggregate **semantic** (desugars to `sum(flat_map(filter(self.descendants, Costed), \|c\| [c.line_cost]))`) | `producer:task-γ` (γ-owned; continuous-cost §2.1 explicitly reserved this for M-WHOLE — owned work, not fiction) | PASS |
| Grammar reality | `grammar-fixture:examples/structural_query_filter.ri` (`filter(self.descendants, Bolt)` parses, 0 ERROR) + continuous-cost §4 grammar gate on the `cost(...)`/`let x:Money=cost(...)` family; `grammar_confirmed=true` | PASS |

G6: no absolute-accuracy / numeric-floor / field-population claim — `cost(...)` is an exact
`Money` sum over already-evaluable `line_cost` cells (linear reduction, no new numeric method).

---

## δ — `reify` surfaces a `RankedSolveResult` (K candidates + `BestFound`) on a merged cluster

Leaf signal: `reify` surfaces a `RankedSolveResult` with K best-of-K candidates + `BestFound`
optimality on a merged cluster. Prereq β + landed F-result.

| Capability | Evidence | Verdict |
|---|---|---|
| `RankedSolveResult` carrier | `grep:crates/reify-ir/src/ranked.rs:106` (`enum RankedSolveResult`) — landed F-result (#4801) | PASS |
| `solve_ranked` defaulted trait method (δ produces **into** it) | `grep:crates/reify-ir/src/constraint.rs:445` (defaulted); `crates/reify-constraints/src/registry.rs:297` (override) — wired | PASS |
| `OptimalityStatus::BestFound` (never `ProvenOptimal`; I3) | `grep:crates/reify-constraints/src/registry.rs:311`; `crates/reify-constraints/src/solver.rs:1624` — wired | PASS |
| Nelder-Mead `DimensionalSolver` back-end (kept; multistart wraps it) | `grep:crates/reify-constraints/src/solver.rs:6` (`argmin::solver::neldermead::NelderMead`) — wired | PASS |
| I3 weighted fold reused (consumed abstractly) | `grep:crates/reify-constraints/src/registry.rs:484` (`eval_rank_cost`); solver.rs `eval_objective_set` — wired | PASS |
| Merged cluster to solve over | `producer:task-β` (upstream; δ `depends_on β`) — DAG-direction OK | PASS |
| Best-of-K fixed deterministic start set (no RNG, no seed) | `producer:task-δ` (δ-owned; determinism-by-absence-of-stochasticity preserves today's regime) | PASS |

G6: no numeric floor — `BestFound` is a budget-bounded derivative-free status, not an
accuracy bound (I3 invariant already forbids `ProvenOptimal` on this path). No guessed threshold.

---

## ε — CI `.ri` end-to-end (integration gate): BT3 + BT4

Leaf signal: committed `examples/whole_model_cost_min.ri` + eval test —
**BT3** (a scope reads another scope's solved `auto` cell → `reify eval` surfaces the
**co-solved** value, impossible before β) and **BT4** (merged whole-assembly
`cost(self.descendants)` **strictly <** the bottom-up frozen baseline; the co-optimised child
`auto` differs from its frozen value). Prereq β + γ + δ.

| Capability | Evidence | Verdict |
|---|---|---|
| Merged cross-scope builder + N-scope write-back | `producer:task-β` (upstream; ε `depends_on β`) | PASS |
| `cost(self.descendants)` subtree objective | `producer:task-γ` (upstream; ε `depends_on γ`) | PASS |
| Best-of-K back-end emitting `RankedSolveResult` | `producer:task-δ` (upstream; ε `depends_on δ`) | PASS |
| **BT3** cross-scope solved-auto **surface read** (F-inherit ζ #4826 deferral, homed on M-WHOLE per commit 85580c7d3e) | `producer:task-β` (the merged write-back makes the other scope's solved cell readable; impossible pre-β) — DAG upstream | PASS |
| **BT4** `merged cost < strictly frozen baseline` (comparative, **not** an absolute bound) | achievability basis: the merged solve optimises over a **superset** of free variables vs the frozen cascade ⇒ merged optimum ≤ frozen; **strict** because the ε fixture is authored with **active** coupling (co-solved child `auto` ≠ its frozen freeze). No numeric-floor branch fires (no absolute-accuracy claim). | PASS |
| `examples/whole_model_cost_min.ri` new file | absent today (`ls` → not found) = ε creates it; parent `minimize cost(self.descendants)` parses (γ substrate + grammar gate); `grammar_confirmed=true` | PASS |

G6 branch 3 (end-to-end capability): every capability ε's signal requires is delivered by ε's
**upstream** prerequisites (β, γ, δ — all wired as `depends_on` edges) — none by a task that
**depends on** ε. No misattribution.

---

## Summary

| Leaf | Signal | Asserted-capability verdicts | Blocks batch? |
|---|---|---|---|
| α | `W_COUPLING_APPROXIMATED` on over-cap fixture | 6 PASS | no |
| γ | `cost(self.descendants)` sums `Money` | 6 PASS | no |
| δ | `RankedSolveResult` K-candidate + `BestFound` | 7 PASS | no |
| ε | BT3 co-solved surface read + BT4 merged<frozen | 6 PASS | no |
| β (intermediate) | roped into ε | 5 PASS | no |

**No FAIL bindings.** All assumed substrate is landed & wired on main; all novel capability is
`producer:task-<label>` with the producer **upstream** of every leaf that observes it. Batch
clears the manifest gate. No cross-PRD dependency edges (§8: 4785 consumes the fold abstractly,
proceeds independently of 4786; the shared fold-site edits are merge-churn, not a semantic dep).
