# ComputeNode Contract

Status: contract (resolves the open design questions accreted in `compute-node-infrastructure.md`). Authored 2026-05-12 in interactive session. Approved by Leo before queueing tasks.

Resolves cluster C-02 / gap GR-002 per `docs/architecture-audit/gap-register.md`.

## §0 — Purpose and supersession

This document is the **contract** for the ComputeNode dispatch seam: the surface where `@optimized("target")` lowering meets a Rust trampoline, and where the cache / warm-state / cancellation / pending / significance-filter machinery binds together. It supersedes the §"Open design questions" section of `docs/prds/v0_3/compute-node-infrastructure.md` (cancellation type, pending mechanism, dispatch-registry shape) and adds the OpaqueState transfer rules and the cross-cutting consumer policy that the prior PRD did not specify.

The foundation tasks 3380 / 3381 / 3382 / 3385 (P3.1 / P3.2 / P3.3 / P3.6) are **done** and stand. The still-pending tasks 3379 / 3383 / 3384 are **superseded** by the integration-phase DAG in §8 — they were authored before this contract; the DAG decomposes the work along vertical slices instead. Disposition of those task IDs (close as superseded vs. retitle in place) is recorded in §8.

This document is named in `docs/architecture-audit/gap-register.md` GR-002 and is the entry point for the integration-phase task chain.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (see `preferences_implementation_chain_naming` memory; supersedes the Phase-2 "scaffold without a caller" framing) — is what this contract is designed to prevent for the ComputeNode seam specifically. Resolution mode is approach **B + H** per `preferences_implementation_chain_portfolio`: vertical-slice decomposition under design-first/contracts/boundary-tests discipline.

## §1 — GR-001 summary

Struct-constructor runtime evaluation resolves via `Value::StructureInstance { type_id, fields }` with strictly nominal trait conformance and rewriting of existing builtin-dispatch constructors as stdlib `.ri` structure_defs. Full disposition recorded in `docs/architecture-audit/gap-register.md` § GR-001. Follow-up PRD: `docs/prds/v0_3/structure-instance-runtime.md` (to be authored separately; **not** part of this contract). This contract assumes GR-001's resolution lands before the first real consumer slice (§8 task η). The trampoline signature in §2 accepts inputs that are `Value` — once GR-001 lands, those inputs may be `Value::StructureInstance` instances (e.g. an `ElasticMaterial`); the trampoline cracks them open per its target's contract.

## §2 — Cancellation

**Type.** `CancellationHandle` is a thin wrapper around `Arc<AtomicBool>`:

```rust
#[derive(Debug, Clone)]
pub struct CancellationHandle {
    inner: Arc<AtomicBool>,
}

impl CancellationHandle {
    pub fn new() -> Self { /* AtomicBool::new(false) */ }
    pub fn cancel(&self) { /* store true, Relaxed */ }
    pub fn is_cancelled(&self) -> bool { /* load Relaxed */ }
}
```

This replaces the unit-struct placeholder at `crates/reify-eval/src/graph.rs:61-68` (audit M-003). Module-private until §8 task β lands; exported once the dispatch machinery (§8 task γ) consumes it.

**Why not `tokio_util::sync::CancellationToken`.** The solver path is sync; `is_cancelled()` is the only API needed at present. The value-adds of `CancellationToken` (`.cancelled().await`, tree-of-tokens) don't apply yet; the dep weight isn't justified. If a future case demands tree-of-cancellation, the `CancellationHandle` wrapper's internals can swap to a richer primitive without touching call sites — the wrapper exists for that retrofit option.

**Semantics — cooperative.** The trampoline is responsible for polling `cancellation.is_cancelled()` and bailing cleanly when set. Hard cancellation (thread kill) is not viable in safe Rust and is not pursued.

**Poll-discipline contract (producer-side obligation).** The trampoline must poll `is_cancelled()` at granularity ≤ a configurable budget. Default: **100 ms wall-clock between polls**. Each target documents its polling discipline:

- Iterative solvers (CG, eigensolvers): poll between iterations.
- Mesh ops (extraction, morph): poll between major phases (e.g. between assembly and solve).
- Imports: poll between chunked-read boundaries.

The FEA cancellation regression test (audit M-007 / task 2924) pins the SLA at the consumer side: rapid input changes against a long-running synthetic trampoline must observe cancellation within 2× the poll budget.

**What "cancelled" leaves behind.** A cancelled trampoline returns `ComputeOutcome::Cancelled` (see §5). The ComputeNode's prior `last_substantive` result stays in the cell; `Freshness::Pending` persists until the next dispatch completes successfully or fails. The prior warm state in the cache is **not** invalidated by cancellation — next dispatch reads it.

## §3 — Pending lifecycle

**Mechanism.** Reuse `Freshness::Pending { last_substantive: ResultRef }` (already defined at `crates/reify-types/src/value.rs:2429-2436`). No new `Value` or `Freshness` variant. The existing `last_substantive` field is the progressive-disclosure substrate — prior result stays observable while recomputation is in flight.

**Docstring broadening.** Current docstring reads "Gated; not recalculated, showing previous best." Update to: **"Current entry not authoritative; previous best on display (either gated on upstream, or recomputation in flight via a ComputeNode)."** The underlying invariants (downstream waits, prior best visible, transitions to Final on resolution) hold uniformly across both cases.

**Chain-root contract extension.** Today, `CacheStore::pending_cause` chain roots are `NodeId::Value(_)` or `NodeId::Failed`. Extend to admit `NodeId::Compute(_)` as a valid chain root. The existing forwarder semantics ("transitive Pending forwards the upstream chain root" — `cache.rs:147-156`) carry through unchanged. Test pins the new admission in §8 task α.

**Tooling discriminator.** UI distinguishes "computing" from "gated" via `pending_cause` introspection:

- `pending_cause == Some(NodeId::Compute(_))` → render "computing" badge (spinner / elapsed-time overlay).
- `pending_cause == Some(NodeId::Value(_))` → render neutral "waiting" badge.
- `pending_cause == None` → root cause (entry was the originating gated node).

No new state machine, no new variant.

**Atomic completion.** When a ComputeNode completes, the engine performs in a single critical section:

1. Write new value to output ValueCells.
2. Transition output ValueCells' freshness `Pending → Final`.
3. Clear `pending_cause` on the output ValueCells.
4. Donate warm state to cache (§5 step 4).

No consumer can observe an incoherent intermediate state (e.g. `Final` + stale value, or `Pending` + new value). The freshness walk visits the output cells; downstream propagation re-evaluates per the normal walk.

**Re-broadening sanity.** If consumers later need to *type-discriminate* (not just `pending_cause`-introspect) computing from gated — e.g. type-system level differentiation — retrofit a `Freshness::Recomputing` sibling variant at that time. Reversibility argument: broadening Pending is additive; un-broadening is breaking. Conservative direction is "tightest now."

## §4 — Dispatch registry

**Scope: per-Engine.** Aligns with the existing precedent `Engine::register_optimized_impl(target, Box<dyn OptimizedImpl>)` at `engine_admin.rs:415-422`. Test isolation: tests swap impls per-Engine without disturbing concurrent tests.

**Shape.**

```rust
pub struct ComputeDispatchRegistry {
    fns: HashMap<&'static str, ComputeFn>,
}

impl Engine {
    pub fn register_compute_fn(&mut self, target: &'static str, f: ComputeFn) { /* ... */ }
    pub(crate) fn compute_dispatch(&self, target: &str) -> Option<ComputeFn> { /* ... */ }
}
```

**Registration convention.** Each crate that defines compute targets exposes a `pub fn register_compute_fns(engine: &mut Engine)` called at engine construction. Order is deterministic (workspace-known crates registered in alphabetic order); duplicate registration is a hard error at construction time (panics with a clear message naming both registrants).

**`@optimized fn` lowering.** When a stdlib `.ri` `fn` is annotated `@optimized("target::name")`, the compiler records `optimized_target` on `CompiledFunction` (already wired per audit M-014's mention of `functions.rs:106-122`). At evaluation time, `eval_user_function_call` (`reify-expr/src/lib.rs:719-769`) inspects `optimized_target`: if `Some(target)`, lower to `Engine::insert_compute_node(target, value_inputs, realization_inputs, options_value_hash, output_value_cells)` instead of inlining the body. The body becomes the fallback used when no trampoline is registered (clean diagnostic emitted; arguably a hard error in production builds — defer that policy to §9).

**Trampoline signature.**

```rust
pub type ComputeFn = fn(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,            // post-GR-001: a Value::StructureInstance carrying the target's options struct
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome;

pub enum ComputeOutcome {
    Completed {
        result: Value,
        new_warm_state: Option<OpaqueState>,
        cost_per_byte: f64,             // for warm-state eviction; derived from solve_time_ms / size_bytes
        diagnostics: Vec<Diagnostic>,
    },
    Cancelled,
    Failed { diagnostics: Vec<Diagnostic> },
}
```

`RealizationReadHandle` is a read-only borrow over a realization node's content (mesh data, BRep handle, etc.). Exact type design is left to §8 task γ — minimum-viable could be `(RealizationNodeId, &EvaluationGraph)`; ergonomic could be a typed view trait. The trampoline does not mutate realizations.

Target string is implicit in the registry entry (no need to pass to the fn).

## §5 — OpaqueState transfer

**Storage model: CacheStore is canonical at-rest; `ComputeNodeData.opaque_state` slot is a transient in-flight carry.**

The slot's Clone-drops-it semantics (`graph.rs:107-119`, audit M-012) already pre-commits to this shape — graph snapshots don't deep-clone warm state; CacheStore is the persistence layer.

**Lifecycle.**

1. **Dispatch-begin.** Engine calls `cache.get_warm_state(NodeId::Compute(N.id)) → Option<OpaqueState>`. Engine writes the result into `N.opaque_state` (transient slot population). Trampoline receives `prior_warm_state: Option<&OpaqueState>` as a borrow over the slot's contents.
2. **In-flight.** State is borrowed by the trampoline. If cancellation fires, the prior state in cache is untouched; the slot is cleared.
3. **Dispatch-complete.** Trampoline returns `ComputeOutcome::Completed { new_warm_state, cost_per_byte, ... }`. Engine atomically: (a) writes new value to output VCs, (b) flips freshness Pending → Final, (c) donates via `cache.donate_warm_state(NodeId::Compute(N.id), new_warm_state, cost_per_byte)`, (d) clears `N.opaque_state` slot. (Slot is cleared to maintain the audit-clean invariant "slot is transient" — cache is the only persistent store. Future fast-path optimization could keep the slot populated for adjacent re-dispatch without cache lookup; defer to §9.)
4. **ComputeNode removal.** Engine extends the existing donation hook at `engine_edit.rs:2275-2301` to cover `NodeId::Compute(_)` — the current hook misses it (audit M-008). On ComputeNode removal: donate slot contents (if populated) to cache, with cost metadata; the cache's eviction policy may discard immediately or retain per warm-state-eviction rules.
5. **Reappearance.** When a ComputeNode with the same `NodeId::Compute(X)` is re-inserted (undo/redo or sibling realization re-emergence), the engine queries cache at dispatch-begin per step 1 — same code path.

**Cost provenance.** The trampoline supplies `cost_per_byte` derived from its own measurement of solve time and result/state size. This unblocks warm-state-eviction M-005 (cost provenance) without separate timing infrastructure on the engine side. Cost-aware-LRU eviction (warm-state-eviction M-004 / cluster C-42) can flip its comparator independently — that's a warm-state-eviction-PRD concern, not this contract's.

**Cancellation interaction.** Per §2: cancellation does **not** transfer state. The prior cache entry remains; the slot is cleared; next dispatch reads the prior cache entry again. Idempotent under any number of cancel-and-redispatch cycles.

## §6 — Consumer policy

**Cross-seam meta-policy umbrella.** `engine-integration-norm.md` (GR-017 / cluster C-14) is the cross-seam meta-policy that catalogs all seven engine plug-in seams; its §3.4 is the listing entry for ComputeNode dispatch — listing-only, deferring all normative content to this contract. CN-contract retains sole authority over cancellation (§2), pending lifecycle (§3), OpaqueState (§5), consumer policy (this section), and trampoline signature (§4 registry / §8 implementation tasks); engine-integration-norm.md §3.4 does not redefine or supplement any of those. The relationship is hierarchical: **CN-contract** is the single-seam contract (gold-standard B+H exemplar for one seam); **engine-integration-norm** is the cross-seam umbrella (lists CN-contract as one of seven). For the two-seam composition where a ComputeNode wrap (§3.4) composes with a realization-kind dispatch branch (§3.2), see engine-integration-norm.md §7 (mesh-morph worked example).

**Rule:**

> A feature routes through ComputeNode (via `@optimized("target")` on a stdlib `fn`, or via direct `Engine::insert_compute_node` from internal-engine realization-dispatch code) iff **(1)** its result is a graph-participant Value or Realization output AND **(2)** it's expensive enough that the cache / warm-state / cancellation / significance-filter infrastructure pays off — heuristic ≥ ~50 ms wall-clock per invocation.

**Origin does not enter the rule.** Both user-visible stdlib fn calls and engine-internal realization dispatches can route through ComputeNode if (1)+(2) hold. The dispatch mechanism differs (annotation vs manual insert call); the seam is the same.

**Heuristic threshold.** "≥ ~50 ms" is back-of-envelope: under-caching at this level is recoverable cost (a few extra recomputes), over-caching is bounded waste (a few extra cache lookups). Instrumented sharpening per persistent-fea-cache PRD M-017 (task 2979); dynamic auto-tuning is future-PRD work.

**Per-feature disposition (v0.3 corpus):**

| Feature | Routes through? | Rationale |
|---|---|---|
| `solve_elastic_static` | Yes | Graph Value output (ElasticResult); FEA-scale, seconds. |
| `solve_load_cases` | Yes | Same; per-case cache reuse via cache-key composition is the explicit point. |
| `solve_buckling`, `solve_modal`, `solve_thermal` (future) | Yes | Same shape. |
| `error_estimate` (Z-Z recovery, a-posteriori PRD) | Yes | Same. |
| Mid-surface / shell extraction (T18, shells PRD future) | Yes | Realization output, geometry-expensive, cacheable on geom-hash. |
| Mesh / OpenVDB / HDF5 / CSV imports | Yes | Graph-participant Value/Realization, one-time load is expensive; persistent-cache tier across sessions is the big win. |
| **Mesh-morphing** | **Yes** | Realization output, mesh-size-expensive, warm-state-beneficial. The mesh-morphing PRD's "doesn't route through `@optimized solve_elastic_static`" reads as axis-2 (morph's internal composition does not dispatch through stdlib FEA-as-ComputeNode), not axis-1 (whether morph itself is a ComputeNode). Axis-1 = yes; axis-2 unchanged (morph keeps calling `solve_cg` directly). **Mesh-morphing PRD prose needs a small correction** to explicitly state axis-1 routing — filed as a separate follow-up task in §8. |
| Cheap stdlib fns: `worst_case`, `case_names`, envelope helpers (`envelope_max`, `envelope_min`), `max`/`min`/`argmax`/`argmin` on Fields, `von_mises` reduction | No | Graph-participant but cheap; overhead exceeds benefit. |
| Builtin OCCT ops (boolean fuse, fillet, chamfer) | No | Already realization-cached at content-hash granularity; no warm-state benefit; per-op duration below threshold. |
| CLI subcommands, doc generation, compile-time analysis | No | Not graph-participant. |

**Negative-case rationale (mesh-morphing PRD correction in §8 task μ):** The mesh-morph engine wiring (currently scoped under mesh-morphing PRD task 2947) will insert a ComputeNode at the morph dispatch point in `engine_build.rs::dispatch_volume_mesh`. The ComputeNode wraps the morph operation; the morph operation's body composes FEA primitives directly per the existing PRD. Both axes are consistent under this contract.

## §7 — Boundary test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level integration) and `crates/reify-eval/src/*.rs::tests` (unit, per-module). The seam is between `reify-eval` (graph + dispatch + cache + freshness) and the trampoline-providing crates (`reify-solver-elastic`, `reify-kernel-gmsh`, `reify-mesh-morph`, future `reify-kernel-openvdb` etc.). Tests must cross this seam from each side; both directions are listed.

### 7.1 Producer-side (reify-eval looks outward at trampolines)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Round-trip dispatch.** Insert ComputeNode with target T; engine dispatches the registered trampoline; result populates output VCs. | Engine has registered `T → trampoline`; inputs are Final; output VCs exist. | Output VCs hold `trampoline.result`; freshness Final; `pending_cause` cleared; warm state donated to cache; ComputeNodeData.opaque_state cleared. |
| **Unknown-target diagnostic.** Insert ComputeNode with unregistered target U. | Engine has no registration for U. | Engine emits a `Diagnostic::UnknownComputeTarget` (clean, named) and the output VCs transition to `Freshness::Failed`. No crash, no panic. |
| **Cancellation propagation.** Synthetic slow trampoline (sleeps polling cancellation every 50 ms). Insert ComputeNode; mid-dispatch, mark an upstream input dirty (triggering cancellation). | Trampoline registered; trampoline polls per the 100 ms SLA. | Cancellation observed within 2× poll budget (200 ms). Trampoline returns `ComputeOutcome::Cancelled`. Prior warm state in cache untouched. Output VCs remain in `Freshness::Pending` until the next dispatch completes. |
| **Cancellation cleans threads.** Drive 20 rapid input changes against the slow trampoline. | As above. | No more than one trampoline in-flight at a time; no orphaned threads (verified via thread-count probe); no leaked OpaqueState (verified via cache audit). |
| **Pending propagation.** Output VC of in-flight ComputeNode N has downstream VC D that consumes it. Read freshness of D mid-dispatch. | Trampoline is in-flight; D's evaluation depends on the output of N. | D's freshness reads `Freshness::Pending { last_substantive: prior_D_result }`; `cache.pending_cause(D)` returns `Some(NodeId::Compute(N.id))` (after walk) or forwards from the upstream pending chain. |
| **Atomic completion.** Read output VC of N at the moment trampoline returns Completed. | Trampoline returns successfully. | No observable intermediate state — caller sees either (Pending, prior value) or (Final, new value), never (Pending, new value) or (Final, prior value). Atomicity test forces a tight read loop and asserts no incoherence. |
| **OpaqueState round-trip across removal.** Insert ComputeNode N → dispatch → remove N → re-insert ComputeNode N' with same `NodeId::Compute` → dispatch. | Trampoline returns deterministic state. | N' dispatch's `prior_warm_state` equals N's final warm state. Cache survived the removal-reinsertion cycle. |
| **Significance filter at result boundary.** Re-dispatch ComputeNode with tolerance-equivalent (bit-different) inputs. | `is_opted_in("solver::elastic_static")` returns true; per-field tolerance policy matches `significance_filter.rs:144+`. | `FilterOutcome::Equivalent` is returned; downstream VCs are **not** invalidated; no consumer recomputation triggered. (Already pinned in `significance_filter.rs:160+`; integration test verifies through-engine path.) |

### 7.2 Consumer-side (trampoline-providing crates look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Annotated stdlib fn dispatches.** Declare `fn test_identity(x: Int) -> Int @optimized("test::identity")` in a fixture stdlib path; call from `.ri`. | Registry has `"test::identity"` mapped to a fn that returns its input. | Evaluation result equals the call argument. Engine inspection confirms a ComputeNode with `target = "test::identity"` was inserted into the graph. |
| **No-trampoline fallback emits diagnostic.** Declare `@optimized("nonexistent::target")` but don't register. | No registration for `"nonexistent::target"`. | Evaluation emits `Diagnostic::UnknownComputeTarget` (or inlines the function body — policy decision in §9). User-visible signal in CLI / GUI diagnostics. |
| **FEA round-trip.** Stdlib declares `fn solve_elastic_static(...) -> ElasticResult @optimized("solver::elastic_static")`; trampoline calls `reify-solver-elastic::solve_cg_with_warm_state`. `.ri` file solves a cantilever beam fixture; reads `result.max_von_mises`. | GR-001 resolved (Material/Load/Support runtime ctors produce `Value::StructureInstance`); reify-solver-elastic registered. | `result.max_von_mises` is within tolerance of the analytical solution. ComputeNode in the graph. Cache holds a warm state. Re-running the same `.ri` returns the cached result without trampoline call (verified via dispatch-count instrumentation). |
| **FEA cancellation under design loop.** `param thickness : Length = auto; minimize mass s.t. max_von_mises < yield_stress`. Auto-resolve drives thickness; FEA runs per iteration; between iterations the param changes mid-solve. | Same as above + auto-resolve loop wired. | No orphaned solver threads; per-iteration wall-clock is bounded; final design converges. Diagnostic stream shows cancellation events at expected cadence. |
| **Persistent-cache round-trip.** Solve a `.ri` file; exit engine; restart; reopen the file. | persistent-fea-cache integration (task 2974 / §8 task ι) landed. | First evaluation hits persistent cache. No trampoline call (verified via dispatch-count). `result.max_von_mises` matches. |
| **Mesh-morph as ComputeNode consumer.** `.ri` file with parametric design; vary a non-structural parameter; mesh-morph eligibility predicate holds; morph runs as a ComputeNode. | mesh-morph engine wiring routes through ComputeNode (§8 task κ); FEA warm-state preserved through morph BTreeMap discipline. | Morph completes faster than re-mesh (≥10× at 100K elements per mesh-morphing PRD). FEA warm-start state persists across the morph (warm-state regression test pins this — currently mesh-morph M-017). |
| **Type discrimination via pending_cause.** UI surface observes `Freshness::Pending` on a cell; reads `pending_cause`. | Test fixture in-flight trampoline; UI mock subscribed. | `pending_cause` returns `Some(NodeId::Compute(N))`. UI mock renders "computing" badge (vs. neutral "waiting"). |

## §8 — Integration DAG (proposed; not yet filed)

Decomposition style: **B (vertical slice) + H (design-first / interface contracts / boundary tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its **user-observable signal**. Producer-only tasks closed in isolation are no longer tolerable (`feedback_task_chain_user_observable`).

Tasks 3379 / 3383 / 3384 (still pending under the old PRD's P3.4 / P3.5 decomp) are **superseded** by this DAG. Disposition recommendation: close 3379/3383/3384 as `cancelled` with `reopen_reason = "Superseded by ComputeNode contract DAG, docs/prds/v0_3/compute-node-contract.md §8"`. Already-done foundation tasks 3380/3381/3382/3385 stand.

### Phase 1 — Foundation supplements (small; un-block the rest)

- **Task α** — `CacheStore::pending_cause` chain-root contract extension.
  - **Observable signal:** Unit test in `reify-eval/src/cache.rs::tests` pins: marking a `NodeId::Compute(N)` as Pending and a downstream `NodeId::Value(V)` as transitively-Pending forwards the chain root → reading `engine.pending_cause(V)` returns `Some(NodeId::Compute(N))`.
  - **Prereqs:** None (extends existing tests).
  - **Crates touched:** reify-eval (cache.rs, engine_admin.rs).

- **Task β** — Real `CancellationHandle` type.
  - **Observable signal:** Unit test pinning `cancel()` → `is_cancelled()` true; cloned handles share state; thread-safety guaranteed by `Arc<AtomicBool>`.
  - **Prereqs:** None.
  - **Crates touched:** reify-eval (graph.rs).

### Phase 2 — Vertical slice (minimum-viable end-to-end @optimized → ComputeNode → result)

- **Task γ** — Per-Engine dispatch registry + stdlib `@optimized("test::identity")` trampoline + lowering wire.
  - **Observable signal:** A test `.ri` file in `crates/reify-eval/tests/fixtures/` calling a stdlib `fn identity_compute_test(x: Int) -> Int @optimized("test::identity")` evaluates to the input value, AND engine inspection confirms a ComputeNode was inserted in the graph for that call (no inlining).
  - **Prereqs:** α, β. Per-Engine registry shape per §4.
  - **Crates touched:** reify-eval (new file `engine_compute.rs`; `engine_admin.rs` for `register_compute_fn`; `engine_eval.rs` for the inspection of `optimized_target` and lowering to `insert_compute_node`), reify-compiler/stdlib (add the test fixture stdlib decl).

### Phase 3 — Pending lifecycle slice

- **Task δ** — `Freshness::Pending` integration during in-flight ComputeNode dispatch.
  - **Observable signal:** A test fixture with a synthetic slow trampoline (sleeps in a polling loop) observes — from a separate thread — `engine.freshness(output_vc) == Freshness::Pending { last_substantive: prior_value_ref }` mid-execution, and `Freshness::Final { result_hash: new_value_hash }` after completion. Atomic-completion test asserts no `(Final, prior_value)` or `(Pending, new_value)` observations. Docstring update on `Freshness::Pending` per §3.
  - **Prereqs:** γ, α.
  - **Crates touched:** reify-eval (engine_compute.rs, cache.rs for atomic transition, freshness_walk.rs), reify-types (Freshness docstring).

### Phase 4 — Cancellation slice

- **Task ε** — Cancellation wiring through dispatch.
  - **Observable signal:** Engine integration test drives rapid input changes against a synthetic slow trampoline. Observable: (a) thread count remains bounded at 1 in-flight; (b) cancellation observed within 2× poll budget; (c) `ComputeOutcome::Cancelled` returned; (d) prior cache state intact. Output VCs remain Pending pending next dispatch.
  - **Prereqs:** γ, β, δ.
  - **Crates touched:** reify-eval (engine_compute.rs).

### Phase 5 — Warm-state slice

- **Task ζ** — OpaqueState lifecycle (read from cache, populate slot, trampoline reads, write back, clear slot, donate to cache).
  - **Observable signal:** Test fixture trampoline stores a counter in OpaqueState. First call: no prior state, counter = 0 returned. Second call: prior counter = 0, trampoline returns 1. ComputeNode removal-then-reinsert preserves the counter across the round-trip (verified via post-reinsert call returning 1, not 0). `cost_per_byte` reflected in cache eviction policy.
  - **Prereqs:** γ. Parallel with ε.
  - **Crates touched:** reify-eval (engine_compute.rs, engine_edit.rs for the donation hook extension covering `NodeId::Compute`), reify-types (OpaqueState — already exists).

### Phase 6 — First real consumer (FEA #16 done as a vertical slice through ComputeNode)

- **Task η** — `fn solve_elastic_static` stdlib declaration + `@optimized("solver::elastic_static")` + trampoline wrapping reify-solver-elastic.
  - **Observable signal:** A `.ri` file (`examples/fea_cantilever_smoke.ri` or similar) declares a steel cantilever beam with a tip load and a fixed support, calls `solve_elastic_static(...)`, and the returned `ElasticResult.max_von_mises` is within tolerance of the analytical solution (`σ_max = 6PL/(bh²)` for a rectangular section under tip load). CLI evaluation confirms; re-running the same file hits the in-memory ComputeNode cache (dispatch-count instrumentation).
  - **Prereqs:** δ, ε, ζ. **Plus GR-001 resolution** (Material/Load/Support runtime ctors yield `Value::StructureInstance`) — gates on `structure-instance-runtime.md` PRD landing.
  - **Crates touched:** reify-compiler/stdlib/solver_elastic.ri, reify-stdlib (trampoline registration via `register_compute_fns`), reify-solver-elastic (no changes — existing API), reify-eval (no changes beyond the slice).
  - **Supersedes:** task 2924 (FEA #16) acceptance.

### Phase 7 — Significance filter integration

- **Task θ** — Significance filter integrated into freshness-walk at output-VC boundary.
  - **Observable signal:** `.ri` file with `param thickness : Length = auto; minimize mass(bracket) s.t. max(von_mises(bracket)) < yield_stress(material)`. Auto-resolve runs FEA per iteration. Consumer constraint `max(von_mises) < yield_stress` is only re-evaluated when FEA result differs **beyond tolerance** (`displacement` field on a Pressure-less tolerance scale, others bit-exact per `significance_filter.rs:75-77`). Test instrumentation pins the no-recompute path.
  - **Prereqs:** η.
  - **Crates touched:** reify-eval (freshness_walk.rs, significance_filter.rs).

### Phase 8 — Persistent-cache hookup

- **Task ι** — ComputeNode → persistent-cache lookup/write integration.
  - **Observable signal:** A `.ri` FEA file evaluates; engine exits; engine restarts; same `.ri` re-opened — first evaluation hits persistent cache (no trampoline call). CLI `--verbose` instrumentation confirms.
  - **Prereqs:** η. Plus persistent-fea-cache PRD task 2974's existing scope (this task **replaces** the open work in 2974 — see persistent-fea-cache PRD audit M-011).
  - **Crates touched:** reify-eval (engine_compute.rs, persistent_cache.rs), reify-config (cache resolution wiring).
  - **Supersedes:** task 2974.

### Phase 9 — Mesh-morph as ComputeNode consumer

- **Task κ** — Mesh-morph engine wiring via ComputeNode at VolumeMesh realization dispatch.
  - **Observable signal:** A `.ri` parametric design (e.g. `prj/printer_v01/printer.ri` if appropriate; otherwise a synthetic fixture). Varying a non-structural parameter (Stage A + Stage B eligibility) triggers `dispatch_volume_mesh` → ComputeNode-wrapped morph → reused FEA warm-state on subsequent solve. CLI `--verbose` stats show `morphed: true` rather than `remeshed`; mesh-morph PRD's ≥10× wall-clock reduction benchmark passes at 100K elements (task 2953 acceptance).
  - **Prereqs:** η, ζ. Plus mesh-morphing PRD tasks 2945 (BoundaryAssociation producer side, currently absent per mesh-morph audit M-005) and 2946 (OCCT Projector concrete impl, currently absent per audit M-006).
  - **Crates touched:** reify-eval (engine_build.rs::dispatch_volume_mesh, engine_compute.rs), reify-mesh-morph (register_compute_fns), reify-kernel-occt (Projector impl), reify-kernel-gmsh (NodeAttachment emission).
  - **Supersedes:** task 2947 acceptance.

### Phase 10 — Companion correction tasks

- **Task μ** — Mesh-morphing PRD prose correction: explicitly state axis-1 = "morph routes through ComputeNode" and axis-2 = "morph's internal composition does not call stdlib FEA-as-ComputeNode" — they are orthogonal; current PRD prose is read-ambiguous, this contract resolves the ambiguity.
  - **Observable signal:** `docs/prds/v0_3/mesh-morphing.md` updated; cross-reference to this contract; no code changes; doc lint passes.
  - **Prereqs:** None (independent doc edit).

- **Task ν** — Foundation-task dispositions: close 3379 / 3383 / 3384 as `cancelled` with `reopen_reason` pointing at this contract; ensure their metadata.files are released.
  - **Observable signal:** Tasks visible in `mcp__fused-memory__get_tasks` as `status: cancelled` with the reopen_reason set; no orphaned worktree state.
  - **Prereqs:** Leo's approval of this DAG.

### Dependency view

```
α ─┐
   ├─→ γ ─┬─→ δ ─→ ε ─┐
β ─┘      │           ├─→ η ─┬─→ θ
          └─→ ζ ──────┘      ├─→ ι
                             └─→ κ ←── (mesh-morph 2945, 2946)
                             
μ (independent)
ν (post-DAG-approval; independent)
```

GR-001's `structure-instance-runtime.md` PRD must land before η. Tasks α–ζ are GR-001-independent.

## §9 — Open questions (surfaced but not decided in this session)

1. **No-trampoline lowering: hard error vs. body-inlining fallback.** When `@optimized("target")` is annotated but no trampoline is registered, what happens? Inline the body (development mode; allows incremental work), or hard error (production discipline; flags missing registration loudly). The trampoline mismatch is currently rare (today: zero registrations) but will be common during integration. **Suggested resolution:** lower to body-inlining in `cfg(debug_assertions)` builds; hard error in release. Decide during §8 task γ.

2. **OpaqueState slot post-dispatch — clear vs. keep for fast-path.** Contract specifies clearing per §5 step 3 for invariant cleanliness. Future fast-path optimization could keep the slot populated to avoid cache-lookup on adjacent re-dispatch. Cost: extra invariant ("slot may shadow cache"). Defer until profiling justifies.

3. **Tree-of-cancellation future.** If `solve_buckling` spawns sub-modal-analyses that should cancel together with the parent, the `CancellationHandle` wrapper internals can swap to a hierarchical primitive (custom or `tokio_util::sync::CancellationToken`). The wrapper API stays stable. No work needed now; flagged for when the case arises.

4. **Hybrid-1 (typed-only structural admission for `Value::StructureInstance`).** Deferred per GR-001 resolution. Reconsider only if `structure_def : TraitName` boilerplate proves a real friction.

5. **Heuristic threshold sharpening.** "≥ ~50 ms" is back-of-envelope. Persistent-fea-cache PRD M-017 (task 2979) instruments measurement. Dynamic auto-tuning is future-PRD work.

6. **`Freshness::Recomputing` retrofit.** If consumers eventually require type-level discrimination of "computing" from "gated" (beyond `pending_cause`-introspection), retrofit a sibling Freshness variant at that time. Reversibility argument applies; not anticipated to be needed soon.

7. **Significance-filter opt-in mechanism.** Currently hardcoded list (`is_opted_in("solver::elastic_static")` only). PRD decomp left this open ("marker trait vs annotation-driven vs hardcoded"). Contract treats this as orthogonal scope: extend the hardcoded list as new consumers land; revisit the mechanism only when it becomes friction.

8. **Realization read-handle type.** §4's `RealizationReadHandle` type is left to §8 task γ's design step. Minimum-viable: `(RealizationNodeId, &EvaluationGraph)` tuple. Ergonomic-future: a typed view trait. Defer; either works for the contract.
