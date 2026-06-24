# PRD (forward-stub): Cross-scope / whole-model objective coupling (incl. subtree cost)

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub — **design-it-now on dispatch** · **Date:** 2026-06-24
**Parent:** `continuous-cost-minimisation.md` §10 (out-of-scope row 1). **Cluster:** `cost-optimisation`.

## Why deferred (not yet specifiable)

The eventual "minimise the cost of **this whole (sub)assembly**" — a single objective spanning auto params across **nested scopes** (e.g. `minimize cost(self.descendants)` driving child dimensions). Deferred because it is structurally impossible today and has a missing **semantic** precondition, not just a solver-scaling one:
- A `minimize` objective optimises **only its own scope's** auto params: `build_solver_problem` collects only own-template auto cells, **drops cross-scope reads**, and child scopes **freeze before** parents solve (verified). A parent's subtree-cost objective therefore sees child costs as **frozen constants**.
- Scope resolution is **source-order, freeze-as-you-go** — the spec's "leaf-first" contract is aspirational, not enforced.
- Spec §10.5 **objective inheritance / narrowest-scope-wins does not exist** — the semantic prerequisite for any whole-model objective.
- A merged whole-model solve is high-dimensional mixed continuous/discrete — likely beyond the single-start Nelder-Mead back-end.

## Substrate (verified 2026-06-24)

- `detect_scope_coupling` (`engine_eval.rs:562`) already emits `W_SCOPE_COUPLING` as an **advisory** read-set analysis — the right *sensor*, wrong *actuator* (it must graduate to a pre-solve clustering pass).
- `continuous-cost-minimisation.md` ships the in-scope `Money`-objective + robustness machinery, scaled here to spanning scopes.
- The BOM **report** chain — `structural-query` γ #3988 (`self.descendants`) / δ #3991 (`filter(_, Trait)`) / `reify report --bom` #4292 — provides the descendants walk + occurrence roll-up this PRD's subtree-cost objective consumes. (These are the γ/δ/#4292 the cost program "wires onto" — here, not in the in-scope PRD.)

## Pre-conditions for activating (real dep edges, wired when prereq IDs exist)
- `continuous-cost-minimisation.md` landed.
- **`objective-scope-inheritance.md` (F-inherit)** — spec §10.5 objective inheritance + enforced dependency-ordered scope resolution.
- structural-query **γ #3988** + **δ #3991** landed (descendants walk + trait filter) and **#4292** (BOM roll-up engine) — the subtree aggregation substrate.
- Likely **`ranked-solve-result.md` (F-result)** if the merged solve adopts a global/MINLP back-end.

## Decomposition (when activated — NOT filed now)
α coupling-detection → pre-solve clustering pass · β merged cross-scope `ResolutionProblem` builder (cluster-aware freeze model) · γ `cost(self.descendants)` subtree-cost objective over the clustered solve · δ optimiser back-end choice/scale-up · ε CI `.ri` minimising whole-assembly cost across nested scopes.

## Dispatch behaviour
The tracking `[MILESTONE]` task is PENDING, dep-wired on the preconditions above. **On dispatch the agent escalates to L2 for `/prd` expansion** (not implement). First expanded deliverable: a **design doc choosing the optimiser back-end** (Nelder-Mead + clustering caps vs a scalable global/MINLP solver) — that fork governs whether arbitrary-scope is achievable.
