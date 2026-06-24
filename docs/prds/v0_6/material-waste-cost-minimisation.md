# PRD (forward-stub): Material & waste cost minimisation (geometry-dependent, "regime B")

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub — **design-it-now on dispatch** · **Date:** 2026-06-24
**Parent:** `continuous-cost-minimisation.md` §10 (out-of-scope row 3). **Cluster:** `cost-optimisation`.

## Why deferred (not yet specifiable)

This is the eventual goal of the cost-optimisation program: minimise **material and waste** cost, where cost depends on **realised geometry** (material cost = density × *kernel volume*; offcut / nesting waste). It is deferred because the solver's objective is evaluated over the param `ValueMap` **only** (`eval_objective_set` never sees kernel output — verified), so a geometry-dependent cost objective requires an **outer candidate-sweep loop**: pick candidate params → full eval (geometry realisation + cost read) → iterate. Two foundations must exist before that loop is tractable, and the loop architecture itself is a design fork — so this stub escalates for `/prd` expansion when its preconditions land, rather than being decomposed now.

Waste/offcut/nesting cost is **fully greenfield** (zero precedent in-tree; no closed-form surrogate — intrinsically kernel-bound).

## Substrate (verified 2026-06-24)

- Geometry queries (`volume`, `area`, mass via `volume × density`, `moment_of_inertia`, `center_of_mass`) **evaluate today** at top level (post-realisation), test-pinned (`examples/ambient_default_material/…`, `examples/kernel_queries/…`; GHR-ζ task 3608). They are NOT available in the solver inner loop.
- The realisation cache is **flushed wholesale on every param edit** (`engine_edit.rs:746`) — so every geometry-varying candidate is a full kernel rebuild absent a finer cache key. (The finer key is the input-cone-hash eviction being built by `selective-realization-eviction.md`, the "F-cache" precondition below.)
- `continuous-cost-minimisation.md` ships the in-scope `Money`-objective + robustness machinery this PRD reuses for the outer loop's inner objective.

## Sketch (when activated)
1. An **outer candidate-sweep** eval mode that re-evals geometry + reads a `Money` cost per candidate and selects the optimum (returns ranked alternatives via F-result).
2. A geometry-dependent cost vocabulary (material cost from volume×density; a waste/offcut model — greenfield).
3. Feasibility via a finer realisation-cache key (F-cache) so candidates that differ only in non-geometric params reuse geometry.

## Pre-conditions for activating (real dep edges, wired when prereq IDs exist)
- `continuous-cost-minimisation.md` landed (in-scope `Money`-objective + robustness).
- **`ranked-solve-result.md` (F-result)** — a result carrier for top-N candidates + optimality status (the outer loop produces a set).
- **`selective-realization-eviction.md` (the "F-cache" capability)** — param-input-cone-keyed realisation cache (else the outer loop is a full rebuild per candidate). Input-cone-hash keyed eviction replaces the wholesale `clear_realization_cache()` flush, so candidates differing only in non-geometric params reuse geometry. Terminal frontier δ (4731) + ε (4732); wired on `[MILESTONE]` task 4787. *No separate `realization-cache-input-cone-rekey.md` PRD: that mechanism is fully delivered here (tasks 4728→4729→4730→{4731,4732}) — dedup catch, /prd session 2026-06-24.*

## Decomposition (when activated — NOT filed now)
α outer-sweep eval mode · β material-cost-from-geometry vocabulary · γ waste/offcut cost model (greenfield) · δ outer-loop ↔ F-cache incrementality · ε CI `.ri` example minimising true material cost.

## Dispatch behaviour
The tracking `[MILESTONE]` task is PENDING, dep-wired on the preconditions above. **On dispatch (deps met) the agent escalates to L2 for `/prd` expansion** — it does NOT implement. First expanded deliverable: a **design doc choosing the outer-loop architecture** (sweep granularity, cache-reuse strategy, ranked-result integration).
