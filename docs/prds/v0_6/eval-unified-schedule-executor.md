# PRD stub: eval consumes the unified-DAG schedule (uniform-dependency stage (b))

**Milestone:** v0_6 · **Status:** deferred — forward stub + [MILESTONE] task; do NOT decompose from this document
**Predecessor:** `docs/prds/v0_6/eval-uniform-dependency-handling.md` (stages (c)+(a), active) · **Substrate:** `docs/prds/v0_6/engine-unified-build-dag.md` (+ `engine-build-dag-substrate.md`)
Ratified 2026-07-02 (design session with Leo): staged (c) → (a) → **(b)**; (b) is the committed endgame, deliberately designed *after* (a) lands.

## 1. Goal (endgame statement)

The pure-eval surfaces (`eval`, `eval_cached`) consume the unified-DAG schedule (`run_unified_pass` / `run_unified_pass_seeded`, `crates/reify-eval/src/engine_fixpoint.rs`) via an **eval-side executor**, exactly as build and edit already do — one dependency graph, one deterministic scheduler, for every entry point. The symbolic mint becomes a **scheduled executor step** (at Realization / geometry-Value nodes) instead of walk-embedded special casing; ordering of mint before consumer is structural for every current and future value source.

## 2. What it deletes (the payoff)

- `build_combined_param_let_graph` + `topological_sort`-over-value-cells as a separate, second dependency graph (`engine_eval.rs:434-545` at HEAD `85b6b88e07`).
- The duplicated main-walk logic between `eval` and `eval_cached` (two hand-maintained walks today).
- The R3d in-walk mint arms as walk-special-cases — re-homed into the executor's node handling.
- The residual risk class entirely: any new value source (solver rounds, new compute targets) participates by construction because it is a graph node, not a convention enforced at N call-sites.

## 3. Why deferred (do not activate early)

1. **Stage (a) rewrites the substrate this design targets** — after `eval-uniform-dependency-handling.md` lands, every named geometry producer is a declared value cell and the compensation sweeps are gone. (b)'s executor design against the pre-(a) engine would be stale on arrival.
2. **Sequencing against unified-dag Stage-5 (#4727, legacy-scheduler deletion, pending)** — whether (b) designs against a one-scheduler or two-scheduler codebase materially changes its shape.
3. **G5:** this is a re-plumb of the hottest path in the engine; it earns its own design-first `/prd` session with fresh HEAD evidence, not leaves authored two rewrites in advance.

## 4. Flip condition (the [MILESTONE] task)

A single [MILESTONE] task is filed with the predecessor PRD's batch, depending on its capstone leaves (δ deletions + ε edit-path closure). When it becomes runnable:

> Run a `/prd` **author-mode design session** for this PRD: verify this stub's premises against HEAD (especially `engine_fixpoint.rs` shape, #4727 state, what (a) actually landed), design the eval-side executor (node-kind handling, symbolic mint step, @optimized/compute dispatch in executor context, cache write-back, `E_EVAL_UNRESOLVED` gating), then decompose.

The milestone's deliverable is the **design session occurring**, not implementation. If at flip time the demand signal has changed (e.g. no new misses, low pressure), Leo may explicitly re-defer — the stub stays the recorded direction either way.

## 5. Known design questions to resolve at flip time (recorded now, decided then)

- Executor shape: extend the three existing decomposed executors (Realization / selector-query-cell / Constraint) with an eval-mode, vs a fourth executor — and how demand scoping (`selective-demand.md` family) applies to pure eval.
- The planner is deliberately execution-free ("plans, does NOT execute"); where the eval executor lives so build/edit don't regress.
- `E_EVAL_UNRESOLVED` decline behavior (`engine_fixpoint.rs` residue handling) is gated off for production — the eval executor must not surface planner-level declines as user errors.
- Cache-coherence of executor write-backs (today's `reeval_cone_cell` contract: values + snapshot_values + `cache.record_evaluation`).
- Whether `concurrent.rs` moves in the same pass.
- Monotonicity guard: within one eval, cell state must only move Undef→resolved; the executor should assert this rather than assume it.

## 6. Cross-PRD relationship

| Other PRD | Direction | Mechanism | Owner | Status |
|---|---|---|---|---|
| `eval-uniform-dependency-handling.md` | consumes its end state (declared cells, deleted sweeps, invariant as convergence oracle) | [MILESTONE] task filed in its batch | predecessor PRD's batch | queued |
| `engine-unified-build-dag.md` | extends its driver to the last non-consuming surface | `run_unified_pass` consumption from `eval`/`eval_cached` | **this PRD** (at flip time) | deferred |
| unified-dag Stage-5 (#4727) | sequencing input | legacy-scheduler deletion state | #4727 (unchanged) | pending at authoring |

No cross-repo seams.
