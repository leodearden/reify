# Audit: ComputeNode Infrastructure

**PRD path:** `docs/prds/v0_3/compute-node-infrastructure.md`
**Auditor:** audit-compute-node-infrastructure
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 10 (mechanisms not in WIRED state)

## Top concerns

- **The dispatch path is the entire load-bearing seam, and it is absent.** `ComputeNodeData` is constructed *only* in tests today; no production builder lowers an `@optimized("target")` function call into a ComputeNode insertion. P3.4 (task 3383, pending) and P3.5 (task 3384, pending) plus the trampoline P4 (task 3379, pending) form a chain that nothing on the consumer side has exercised end-to-end. Until P3.4 lands, every "wired" piece below is dead code that has only been unit-tested.
- **OpaqueState slot is structural only; warm-state transfer between graph and cache is not connected to ComputeNodes.** `ComputeNodeData.opaque_state: Option<OpaqueState>` exists; `CacheStore::{donate,get}_warm_state` exists keyed on `NodeId` (and `NodeId::Compute` is wired); but no code path moves state between them at dispatch boundaries. The Clone impl explicitly drops it. P3.5 (3384) owns the wiring decision.
- **`topology_fingerprint` silently omits `compute_nodes`.** The P3.1 docstring promises a P3.2 follow-up to add a ComputeNode bucket, but P3.2's actual scope was per-node `cache_key` composition (a different concern). This is a documented but unimplemented invariant — and at present harmless only because no production code constructs ComputeNodes.
- **`Pending` semantics are open.** P3.5's lifecycle ticket lists three options (new `Value::Pending` variant, freshness-flag reuse, sentinel-by-convention). No decision yet. Reify already has `Freshness::Pending` (with `pending_cause` chain) used elsewhere — drift risk if P3.5 picks a competing third mechanism.

## Mechanisms

### M-001: `ComputeNodeData` struct + `EvaluationGraph` integration (P3.1)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/graph.rs:75-121` (struct + manual Clone), `:163-164` (PersistentMap on EvaluationGraph), `:513-534` (insert/get/get_mut APIs). Round-trip + multi-node + field-exhaustive tests at `:942-1230`. Task 3380 done.
- **Blocks:** N/A (foundation for all other P3.x pieces; nothing waiting on it that isn't tracked below)
- **Note:** Field list, manual Clone semantics (`opaque_state` dropped on clone, `CancellationHandle` placeholder cloned by value), and explicit "P3.5 must revisit Clone semantics" comment all match the PRD spec exactly.

### M-002: `ComputeNodeId` + `NodeId::Compute(_)` variant

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/cache.rs:14-25` (NodeId enum), `:51-55` (From impl), `:57-66` (Display), `:1293-1346` (P3.3 step-1 pin tests). `ComputeNodeId` exported from `reify-types`.
- **Blocks:** N/A
- **Note:** P3.3 step-1; unified cache identifier surface ready for any future Compute-keyed cache wiring.

### M-003: `CancellationHandle` placeholder type

- **State:** PARTIAL
- **Failure mode:** F3 (placeholder unit-struct, real type deferred)
- **Evidence:** `crates/reify-eval/src/graph.rs:61-68` — explicit "P3.5 replaces it with the real cooperative-cancellation type (likely `Arc<AtomicBool>`)". Kept module-private to avoid API break. Task 3384 owns the real type.
- **Blocks:** 3384 (P3.5), 2924 (FEA #16 transitive)
- **Note:** Three options listed in 3384's body: `Arc<AtomicBool>`, `tokio_util::sync::CancellationToken`, custom type. Decision deferred to planning.

### M-004: Cache-key composition (P3.2)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/compute_cache_key.rs:91-158` (composition over `(target, sorted value-bucket, sorted realization-bucket, options_hash)`). Test suite at `:160-464`: determinism, value/realization-input cardinality, reordering invariance, target/options-delta, domain separation (`shared_H` at value-vs-realization positions), missing-input panic, duplicate-input debug-assert. Task 3381 done.
- **Blocks:** N/A
- **Note:** Sort key for `RealizationNodeId` is local — upstream intentionally doesn't derive `Ord`. Exclusion of thread-count / determinism mode is delegated to upstream `options_hash` producer (e.g. planned `ElasticOptions::cacheable_hash` on task 3383's P3.4 surface, which itself doesn't exist yet — see M-014).

### M-005: Dependency edges (Edge #6 VC→Compute, Edge #10 Real→Compute, Edge #12 Compute→VC)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/deps.rs:178-200` (reverse-index registration for VC and Real → Compute), `:368-435` + `:437-490` (per-edge regression tests). Task 3382 done. Edge #12 fan-out lives in the freshness walk (M-006) and dirty walk (M-007).
- **Blocks:** N/A
- **Note:** Reverse-index registered, so freshness/dirty propagation can find downstream ComputeNodes. With no production producer of ComputeNodes (M-014), exercise is test-only today.

### M-006: Freshness-walk integration through ComputeNodes (Edge #12 fan-out)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/freshness_walk.rs:50-165` (module doc + walk), particularly `:132-160` (P3.3 step-16 edge #12 Compute→output_value_cells fan-out gated on `cutoffs_passed`). Test `propagate_freshness_only_propagates_through_compute_node_to_output_value_cells` at `:1331-1410`.
- **Blocks:** N/A
- **Note:** Three-cutoff structure (Failed-skip, freshness-early, Pending-idempotency) preserved when walking through Compute. Output VCs use `push_value_on_all_branches=true` to mirror `dirty.rs:49-56` conservatism.

### M-007: Dirty-cone propagation through ComputeNodes (Edge #12)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/dirty.rs:20-156` (per-VC and per-Real seed paths both inline-realize Edge #12 fan-out via `graph.compute_nodes.get(cn_id).output_value_cells`), `:401-450` regression test `compute_dirty_cone_propagates_through_compute_node_to_output_value_cells`.
- **Blocks:** N/A
- **Note:** Test-only exercise (consistent with M-014 absence). Also: docstring at `:471-473` explicitly affirms there is NO direct ComputeNode→ConstraintNode edge, matching arch §5 line 199.

### M-008: Demand-walk surfaces ComputeNode realization_inputs

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/demand.rs:110-130` (P3.3 amendment: demanded ComputeNode surfaces its `realization_inputs` so Realizations driving a demanded ComputeNode get demanded too). Test `:494-600+`. Task 3382 (P3.3) done.
- **Blocks:** N/A
- **Note:** A subtle correctness invariant — without this, a ComputeNode would be demand-reachable but its mesh inputs would not be, breaking the freshness-driven evaluation order.

### M-009: `dependent_still_present_in_graph` Compute arm

- **State:** WIRED (test-only exercise)
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/engine_edit.rs:5088-5142` — regression test `dependent_still_present_in_graph_filters_removed_compute_node` plus the explicit comment "ComputeNodes are not yet wired through source syntax, so the round-trip on the helper itself is the only available coverage until P3.4+".
- **Blocks:** N/A
- **Note:** Cross-confirms M-014.

### M-010: Output significance filter (P3.6)

- **State:** PARTIAL
- **Failure mode:** F2 (module wired, opt-in mechanism is hardcoded; integration with freshness walk pending; no production caller)
- **Evidence:** `crates/reify-eval/src/significance_filter.rs` — `FilterOutcome` enum at `:51-68`, `is_opted_in("solver::elastic_static")` hardcoded at `:75-77`, full `significance_filter(...)` pure function at `:144-...`. Task 3385 done. **But:** no call site invokes it — grepping `crates/reify-eval/src` outside the module finds zero references. The freshness-walk hook (3382/P3.3) and the `Engine::active_tolerance_for(subject_entity_ref)` resolution it documents are deferred.
- **Blocks:** 2924 (FEA #16 will be the first caller)
- **Note:** Per-field policy hardcoded to ElasticResult shape: displacement uses Pressure-less tolerance, other 4 fields exact-equal. Opt-in mechanism is hardcoded match — PRD §"Open design questions" leaves marker-trait vs annotation-driven vs hardcoded list to a later cleanup. Conservative-fallback policy correct: any malformation → `Different`.

### M-011: Significance-filter call-site / freshness-walk hook

- **State:** TODO
- **Failure mode:** F4 (the consumer wiring is named in P3.6/3385 docstrings but the integration call is not present)
- **Evidence:** Comment at `significance_filter.rs:11-13` references "P3.3 task 3382, the freshness-walk hook" and `:46-48` references `Engine::active_tolerance_for(subject_entity_ref)` resolution. Neither exists in `freshness_walk.rs` (no FilterOutcome import, no significance_filter call). Engine grep also finds no `active_tolerance_for` fn.
- **Blocks:** 2924 (FEA results invalidation depends on this), 3382 follow-up
- **Note:** This is the seam P3.3 was supposed to close. Likely deferred during 3382 implementation because no ComputeNode producer existed to test against.

### M-012: OpaqueState slot on `ComputeNodeData`

- **State:** PARTIAL
- **Failure mode:** F3 (slot present, Clone drops it deliberately, no transfer path)
- **Evidence:** `graph.rs:89` (field), `:107-119` (Clone sets to `None`), `:96-99` (rationale comment). PRD task description in 3380 explicitly says "P3.1 leaves Option; P3.5 wires". `CacheStore::donate_warm_state`/`get_warm_state` exist (cache.rs:810-828) and the cache is `NodeId`-keyed (so a `NodeId::Compute` warm-state entry is possible), but the move-out-at-dispatch-begin / write-back-at-completion path lives entirely in P3.5 (3384).
- **Blocks:** 3384 (P3.5), 3379 (P4 trampoline that receives warm state)
- **Note:** Tests already pin "OpaqueState slot drops to None on clone" (graph.rs:982-1011), so the contract is unambiguously test-anchored — the gap is execution, not specification.

### M-013: Pending-sentinel propagation

- **State:** TODO
- **Failure mode:** F4 (mechanism named in PRD, decision deferred to P3.5)
- **Evidence:** PRD §"Lifecycle: pending + cancellation"; task 3384 lists 3 options (new `Value::Pending` variant / reuse existing freshness-flag mechanism / sentinel-by-convention). Reify already has `Freshness::Pending` with `pending_cause` chain (`engine_admin.rs:1087-1180`) which is the leading reuse candidate but not formally bound to the ComputeNode running state yet.
- **Blocks:** 3384 (P3.5), 2924 (FEA #16 long-running consumer)
- **Note:** This is the highest-risk "design question" in this PRD because it touches the `Value` enum — adding a variant ripples through every Value match site in the codebase. The `Freshness::Pending` reuse path looks attractive on paper but couples ComputeNode lifetime to freshness-walk timing in a way the audit didn't have budget to verify.

### M-014: `@optimized("target")` lowering on function context to ComputeNode insertion (P3.4)

- **State:** FICTION
- **Failure mode:** F6 (load-bearing dispatch infrastructure leaned on by FEA + multi-load-case PRDs, absent from production code)
- **Evidence:** `insert_compute_node` (graph.rs:522) has callers ONLY in tests (deps.rs, dirty.rs, freshness_walk.rs, engine_edit.rs, demand.rs — all `#[cfg(test)]` or `#[test]`-adjacent). `CompiledFunction::optimized_target` field exists (compiler/src/types.rs:839, populated in `functions.rs:106-122`) and `@optimized` is accepted on function context (annotations.rs:64-130, task 3377), but `crates/reify-eval/src/engine_eval.rs` does NOT inspect `optimized_target` at function-call evaluation time. No `crates/reify-eval/src/engine_compute.rs` or `crates/reify-types/src/compute.rs` files exist (both named in task 3383's `metadata.files`). Task 3383 pending; prior attempt (`run-da7582f16a91`, 2026-05-11) reaped after merge-gate failure — work unrecoverable per carry-forward note.
- **Blocks:** 2924 (FEA #16 — the first ComputeNode consumer; this PRD's stated `Consumer`), 3378 (deferred — `fn solve_elastic_static` stdlib decl), 3444 (curator-filed follow-up)
- **Note:** This is the single biggest gap in the PRD. Every other "WIRED" mechanism is verified by tests that themselves construct `ComputeNodeData` directly; in production, the universe of ComputeNodes is the empty set. This is structurally identical to GR-001 (struct ctors): the runtime entry point is absent despite all surrounding scaffolding being correct.

### M-015: Dispatch registry (string-keyed `(target, ComputeFn)` lookup)

- **State:** FICTION
- **Failure mode:** F6 (named in PRD §"Dispatch registry", no implementation)
- **Evidence:** Grep across `crates/` for `ComputeFn|ComputeNodeRegistry|register_compute*` finds zero hits in production code. Constraint-side precedent `Engine::register_optimized_impl(target, Box<dyn OptimizedImpl>)` exists at `engine_admin.rs:415-422` and is suggested in task 3383 as the model. Task 3383 lists "global OnceLock vs per-Engine" as an unresolved design question.
- **Blocks:** 3383 (P3.4), 3379 (P4 trampoline registers a target here), 2924
- **Note:** Closely coupled to M-014 — one task ships both. The trampoline signature in 3383 names `&CancellationHandle` and `Option<&OpaqueState>` as parameters, which transitively requires M-003 and M-012 to firm up.

### M-016: ComputeNode execution path (`Engine::evaluate` invokes dispatch + caches result)

- **State:** FICTION
- **Failure mode:** F6 (the actual "given a ComputeNode, run it and store the result" loop is unimplemented)
- **Evidence:** No code under `crates/reify-eval/src/` reads `ComputeNodeData.cache_key` to short-circuit or `ComputeNodeData.cached_result` to populate. The output VC propagation walks (M-006/M-007) operate on a ComputeNode that has *already* been evaluated by some other (non-existent) mechanism. Architecturally this is part of P3.4 task 3383.
- **Blocks:** Same as M-014/M-015
- **Note:** This is the seam between "dispatch lookup found a trampoline" and "ComputeNode has produced a result". P3.5 (lifecycle) and P3.6 (significance filter at result boundary) both attach at this seam.

### M-017: `topology_fingerprint` includes a ComputeNode bucket

- **State:** TODO
- **Failure mode:** F4 (P3.1 docstring promises "P3.2 adds the fingerprint bucket"; P3.2 implemented a different mechanism and the bucket was not added)
- **Evidence:** `graph.rs:516-519` ("`topology_fingerprint` does NOT yet include a ComputeNode bucket. P3.2 composes `cache_key` and adds the fingerprint bucket"). Actual `topology_fingerprint()` at `:600-779` combines 7 buckets: value_cells, constraints, realizations, resolutions, guarded_groups, connections, auto_type_substitution — no compute_nodes bucket. P3.2 (3381) shipped only the per-node `compute_cache_key()` function (separate, not fingerprint integration).
- **Blocks:** None in the immediate term (no production ComputeNodes), but **logically blocks correctness once M-014 lands**: a graph that differs only in its ComputeNode set would produce the same `topology_fingerprint` today, masking template/structure deltas.
- **Note:** Documented drift between P3.1's stated handoff and P3.2's actual scope. No follow-up task tracks this — easy to miss when M-014 lands.

### M-018: `NodeArchKind::ComputeNode` documentation drift

- **State:** DRIFT
- **Failure mode:** F5 (taxonomy enum comment says "No corresponding Rust struct in the codebase yet"; struct now exists)
- **Evidence:** `crates/reify-types/src/node_traits.rs:148-153` mentions `(SchemaNode, SourceNode, ComputeNode)` as kinds whose Rust struct counterparts do not yet exist, and `:187` repeats this on the `ComputeNode` variant specifically. P3.1 (task 3380, done) created `ComputeNodeData` in `reify-eval/src/graph.rs:75-93`.
- **Blocks:** None
- **Note:** Cosmetic but exactly the kind of stale-doc footnote that future audits keep tripping over. Schema/Source still legitimately have no struct counterparts; only the ComputeNode line is stale.

## Cross-PRD breadcrumbs

- **`structural-analysis-fea.md`** task #16 (2924) is this PRD's stated consumer; FEA tasks #4 (ElasticOptions / cacheable_hash) and #1 (Material starter lib) feed M-004 (cache key composition) and the input/options serialization that M-014/M-015 will need. The Material starter lib is GR-001 — runtime struct-ctor evaluation; M-014 also depends on GR-001 because the lowered dispatch needs to feed structure-instance values into the trampoline.
- **`multi-load-case-fea.md`** assumes `LoadCase(...)` / `MultiCaseResult(...)` runtime ctors (GR-001) AND that `solve_load_cases(...)` is `@optimized`-dispatched (this PRD).
- **`persistent-fea-cache.md`** consumes `ComputeNodeData.cache_key` (M-004) as the persistent-cache key and `PersistentlyCacheable` trait (already in `persistent_cache.rs`, shipped) — that PRD presupposes M-014/M-015 land for results to flow into its on-disk store.
- **`mesh-morph` PRD** (named in §"Consumer") and modal/thermal solver future consumers — all blocked on the same M-014/M-015/M-016 chain.
