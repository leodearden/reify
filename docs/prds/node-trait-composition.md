# Node-Trait Composition

> **Superseded 2026-05-13** by [`docs/prds/v0_3/node-traits-unification.md`](v0_3/node-traits-unification.md) (GR-038 / cluster C-36 resolution under direction C′ — refined bridge).
>
> Acceptance criteria #1 (trait→priority mapping), #3 (config-file ingestion), and #5 (IMMEDIATE never-cancelled alignment) are absorbed and re-met under the unified surface. Tasks 2350 / 2353 / 2356 / 2358 / 2360 remain valid as foundation work this PRD originally produced; the parallel-taxonomies gap they left (`NodeArchKind` 7 variants vs `NodeKind` 5 variants; `NodeTraits` declarative-only) is closed by the unification PRD's bridges. This file remains as the foundation document; new work targets the v0_3 PRD.

## Goal

Add the four declarative node traits specified in `reify-implementation-architecture.md` §7.6 (lines 803–816) — `immediate`, `warm_startable`, `progressive`, `committable` — and make them compose orthogonally with the existing priority system. Per-node policy overrides must be expressible (e.g. "FEA only on final inputs", "solver progressive emission"), settable per node instance or per node type. Traits inform priority assignment but do not replace it.

## Background

- **Arch §7.6 (lines 803–816)** is the canonical definition. Each trait is a static declaration on the node type; priority is dynamic at scheduling time. Quote: "Traits are composable. Example: an FEA solver node might be `warm_startable + progressive + committable`."
- **Arch §7.3 (lines 751–767)** specifies per-node policy overrides via a "dedicated UI widget":
  - **Commit if slow** (default): dual-threshold commitment policy.
  - **Always cancel when stale**: never commit.
  - **Only run on final inputs**: don't evaluate on intermediate upstream results — gates on Freshness from the related PRD.
- **Arch §7.5 (lines 793–801)** specifies cancellation behaviour by priority. P0 / P1-fast nodes are never cancelled; P3 speculative is always cancelled on new snapshot. Trait `immediate` must align with P0/P1-fast.
- **Arch §3.5 (lines 432–436)** and §7.2 (line 743) describe the freshness-propagation mechanism that unlocks gated work — when the "only run on final inputs" policy applies, the node enters the value dirty set when freshness reaches `Final` on all inputs. This PRD's gating policies plug into that mechanism.
- **Arch §4.1** and §4.3 already define the `WarmStartable` protocol that the `warm_startable` trait corresponds to. Existing infrastructure from #27.
- **Today:** the eval/runtime crates have node types (`ValueCell`, `ConstraintNode`, `ResolutionNode`, `RealizationNode`, `ComputeNode`, `SchemaNode`, `SourceNode` per arch §2.1) but no first-class trait declaration on them. Priority assignment is implicit from node kind. There is no per-node override mechanism.

## Scope

1. Define the four traits as a typed declaration (Rust trait, bitflags, or enum-set — implementation choice). Each node type declares its default trait set.
2. Compose orthogonally with priority: a node with `immediate` trait is P0/P1-fast; `progressive + committable` informs the scheduler that intermediate results are expected and commitment policy applies; etc. Document the trait→priority mapping table.
3. Add a per-node policy override surface: a struct or builder allowing instance-level or type-level overrides for the three policies named in §7.3. Settable from project configuration and/or programmatically.
4. Wire `only_run_on_final_inputs` into the scheduler so a node with this override does not enter the dirty set until all its inputs are `Final` (depends on the Freshness PRD's freshness-propagation walk).
5. Wire `always_cancel_when_stale` into the cancellation refinement (§7.5) so the node is never committed and is always cancelled when its dirty cone fires.
6. Wire `progressive` so the scheduler expects multiple cache updates per evaluation (intermediate emission). Confirm interaction with the freshness-propagation logic from the Freshness PRD.
7. Tests: unit tests for trait composition (e.g. assert that `RealizationNode` declares `warm_startable + committable` and lacks `progressive` by default; a hypothetical FEA node declares `warm_startable + progressive + committable`); scheduler integration tests for each override; instance-level vs type-level override precedence test.

## Out of scope

- The "dedicated UI widget" for policy overrides (arch §7.3) — provide the data structure and config-file path; UI surfacing is a follow-up.
- New trait kinds beyond the four named in §7.6.
- Replacing the priority system — traits inform it, do not replace it.
- The actual FEA / specialized solver nodes the traits will eventually decorate — only the trait machinery is in scope. Existing node types (Realization, Resolution, Compute) are decorated with sensible defaults.
- Cross-trait dependencies (e.g. "progressive implies warm_startable") — traits are independent; if a combination is invalid, runtime can reject, but no compile-time relationship enforcement.

## Acceptance criteria

- The four traits are declared as a first-class type (e.g. `bitflags::bitflags! struct NodeTraits { IMMEDIATE, WARM_STARTABLE, PROGRESSIVE, COMMITTABLE }` or equivalent enum-set). Doc-comments cite arch §7.6 with line numbers.
- Each existing node kind (`ValueCell`, `ConstraintNode`, `ResolutionNode`, `RealizationNode`, `ComputeNode`, `SchemaNode`, `SourceNode`) declares its default `NodeTraits` set. Document the assignments inline:
  - `ValueCell` (scalar): `IMMEDIATE`
  - `ConstraintNode`: depends — typically nothing, or `IMMEDIATE` for cheap predicates
  - `ResolutionNode`: `WARM_STARTABLE | COMMITTABLE` (potentially `PROGRESSIVE` for iterative solvers)
  - `RealizationNode`: `WARM_STARTABLE | COMMITTABLE`
  - `ComputeNode`: `WARM_STARTABLE | COMMITTABLE` (FEA-like nodes add `PROGRESSIVE`)
  - `SchemaNode`: `IMMEDIATE`
  - `SourceNode`: `IMMEDIATE`
- A `NodePolicyOverrides { commitment: CommitPolicy, gating: GatingPolicy }` struct (or equivalent) supports per-instance and per-type overrides. Three named policy values match §7.3: `CommitIfSlow` (default), `AlwaysCancelWhenStale`, `OnlyRunOnFinalInputs`.
- Override resolution order: instance > type > default. Tested.
- Scheduler honours `OnlyRunOnFinalInputs`: a node with this override is not scheduled while any input is `Intermediate` / `Pending`. Verified by integration test using a synthetic graph with mixed-freshness inputs.
- Scheduler honours `AlwaysCancelWhenStale`: a node with this override is never committed (commitment thresholds from §7.3 do not apply), and is always cancelled when in the dirty cone.
- `PROGRESSIVE` trait causes the scheduler to expect multiple cache updates per evaluation (does not error on intermediate emission).
- `cargo test -p reify-eval -p reify-runtime` green; new tests cover trait composition, override precedence, and gating behaviour.

## Task breakdown (queueing aim: 4–6 tasks)

1. **Declare the `NodeTraits` type and assign defaults to each node kind**. Land as no-op data — nothing yet consumes the traits. Includes inline documentation citing arch §7.6 with line numbers.
2. **Declare `NodePolicyOverrides` and the three policy values** (`CommitIfSlow`, `AlwaysCancelWhenStale`, `OnlyRunOnFinalInputs`). Include override-resolution logic (instance > type > default). Unit tests for override precedence.
3. **Wire `OnlyRunOnFinalInputs` into the scheduler**: a node with this override is gated by input freshness reaching `Final`. Plugs into the freshness-propagation walk from the Freshness PRD. Integration test with synthetic mixed-freshness graph.
4. **Wire `AlwaysCancelWhenStale` into cancellation refinement** (arch §7.5): override the §7.3 commitment thresholds for tagged nodes; always cancel when dirty cone fires. Integration test.
5. **Wire `PROGRESSIVE` trait** so the scheduler expects intermediate cache updates from tagged nodes. Cross-check with freshness-propagation: each intermediate emission updates the cache and triggers a freshness-only downstream walk. Integration test with a synthetic progressive node emitting 3 intermediates then `Final`.
6. **(Optional)** End-to-end test: a fixture node with `warm_startable + progressive + committable` (a stand-in for a future FEA solver) exercises the full trait composition. Confirm warm state preserved across re-evaluations, intermediates emitted, commitment threshold honoured.

## Dependencies

- Tasks 3 and 5 depend on the Freshness 4-Variant PRD's freshness-propagation walk (specifically the freshness-only downstream walk and `Pending` / `Intermediate` semantics). Without that machinery, gating on freshness has nothing to gate on. Wire `add_dependency` accordingly.
- Builds on existing `WarmStartable` protocol from #27 — `WARM_STARTABLE` trait is a declarative tag that corresponds to implementing the existing protocol.
