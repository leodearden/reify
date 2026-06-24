# PRD (forward-stub): Multi-aspect objective units-coherence

**Milestone:** v0_6 · **Status:** DEFERRED forward-stub — **design-it-now on dispatch** · **Date:** 2026-06-24
**Parent:** `continuous-cost-minimisation.md` §10 (out-of-scope row 4). **Cluster:** `cost-optimisation`.

## Why deferred (not yet specifiable)

Combining **multiple aspects** in one objective — cost **and** mass **and** part-count **and** waste — toward the eventual "all-aspects" optimisation. Deferred because of a **silent-correctness hazard** that needs a design-first resolution: the multi-objective `WeightedSum` fold sums terms as **raw f64 si-values**, stripping dimensions — so `minimize cost + mass` silently computes `36.88 (USD) + 2.4 (kg) = 39.28 (nonsense)`. This is the project's `feedback_silent_defaults_pattern` antipattern at the objective-combination seam. It is **multi-aspect-only** — it does NOT block the single-aspect (`Money`-only) `continuous-cost-minimisation.md`.

## Substrate (verified 2026-06-24)

- Each individual aggregate term is already dimensionally honest: `.sum`/`eval_add` preserve dimension and return `Undef` on mismatch (`reify-expr/src/lib.rs`).
- The **incoherence is only at combination**: `eval_objective_set` (`solver.rs:~630`) and the provenance fold (`engine_eval.rs:~2209`) both do `acc += weight * v` over bare `as_f64()` — the I3 fold is **triplicated** (comment at `engine_eval.rs:~2193`); all sites must change together.
- `Money`/`Mass`/etc. are first-class dimensions; `ObjectiveTerm.weight` is currently `f64`.

## Sketch (when activated) — the design fork to resolve
Three options, in increasing power/cost:
1. **Dimensionless-only terms (cheap floor):** require each `minimize` term to evaluate dimensionless (authors normalise: `minimize cost/1USD + mass/1kg`); the fold rejects mixed dimensions with a named diagnostic instead of silent f64. ~A guard at the (triplicated) fold sites.
2. **Dimensioned "shadow-price" weights (durable):** `ObjectiveTerm.weight` carries units `1/aspect_dim` (a price/exchange-rate), so `weight × value` is dimensionless by construction. **Breaking IR change** (`weight: f64 → Value::Scalar`).
3. **Project-everything-to-Money:** map each aspect to USD via per-aspect price models in stdlib.

## Pre-conditions for activating (real dep edges, wired when prereq IDs exist)
- `continuous-cost-minimisation.md` landed (single-aspect baseline).
- An aspect vocabulary beyond `Costed` (e.g. a `Massive { mass : Mass }` trait) — co-design here to avoid a "cost is special" trap.
- Relationship: the aspect-aggregation substrate overlaps `structural-query` δ #3991 + the #4292 roll-up.

## Decomposition (when activated — NOT filed now)
α units-coherence guard at the (triplicated) fold sites (option 1 floor) · β aspect-trait vocabulary (`Massive`, waste-value, …) · γ the dimensioned-weight / shadow-price model (option 2, breaking IR) — **the load-bearing, irreversible call** · δ CI `.ri` minimising a coherent multi-aspect objective.

## Dispatch behaviour
The tracking `[MILESTONE]` task is PENDING, dep-wired on the preconditions above. **On dispatch the agent escalates to L2 for `/prd` expansion** (not implement). First expanded deliverable: a **design doc choosing option 1/2/3** and whether `ObjectiveTerm.weight` becomes dimensioned (the single irreversible decision), to be made before any "all-aspects" objective ships.
