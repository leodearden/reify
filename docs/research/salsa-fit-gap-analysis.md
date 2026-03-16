# Salsa Fit/Gap Analysis for Reify Evaluation Engine

**Date:** 2026-03-16
**Status:** Concluded
**Decision:** Build custom incremental engine; do not adopt or fork Salsa
**Blocking milestone:** M2 (Incremental Evaluation)

---

## Context

The evaluation engine at the heart of Reify is an incremental computation system. Before building it, we assessed whether Salsa (the incremental computation framework used by rust-analyzer, currently at v0.26) is a good fit, or whether we need a custom implementation.

Salsa's architecture (Salsa 2022 / salsa 0.17+) was analyzed against the 9 key requirements of the Reify evaluation engine as described in the implementation architecture document.

---

## 1. Data Model Mapping

**Salsa primitives:** `#[input]` (external ground truth), `#[tracked]` (derived computed values), `#[interned]` (canonical deduplication), `#[accumulator]` (side-channel output like diagnostics).

| Reify Node | Natural Salsa Mapping | Fit |
|---|---|---|
| **SourceNode** | `#[salsa::input]` — source text as input, parsed AST as tracked struct | Good |
| **ValueCell** | `#[salsa::tracked]` struct with value + determinacy | Partial — ValueCells are mutable targets of resolution; Salsa tracked structs are immutable after creation |
| **ConstraintNode** | Tracked function `(constraint_id) -> (Satisfaction, Diagnostics)` | Good |
| **ResolutionNode** | No natural mapping | Poor — cross-snapshot state transition with convergence loop and trial snapshots |
| **RealizationNode** | Tracked function in theory, but needs warm-start state | Poor — opaque non-hashable result, mutable warm state |
| **ComputeNode** | Tracked function `(computation_id) -> ComputationResults` | Partial — long-running, may need warm state, needs cancellation |
| **SchemaNode** | Tracked function that creates tracked structs | Partial — Salsa supports dynamic struct creation/GC, but SchemaNode outputs entire evaluation subgraphs |

**Severity: Significant.** Three of seven node types (ResolutionNode, RealizationNode, SchemaNode) have fundamental modeling friction. The poorly-mapping nodes are the architecturally distinctive ones.

---

## 2. Warm-Start State

**Salsa:** Tracked functions are pure. Memoized results are the only cached state. No mechanism to attach opaque mutable metadata to a cached computation.

**Reify requirement (architecture section 4):** `WarmStartable` trait with `compute_cold(inputs) -> (Result, State)` and `compute_warm(inputs, previous_state, input_diff) -> (Result, State)`. Warm state is explicitly not content-addressed — it's a performance hint. Examples: OCCT `TopoDS_Shape`, solver Jacobian matrices.

**Workaround:** Side-table `HashMap<NodeId, OpaqueState>` external to Salsa, accessed via `report_untracked_read()`. This forces re-execution every revision, defeating Salsa's incrementality for warm-started nodes.

**Severity: Significant.** Warm starting is a first-class architectural feature affecting the hot path. The workaround undermines the core value of using Salsa.

---

## 3. Trial Snapshots

**Salsa:** Single linear revision sequence. `fork_db()` creates read-only frozen views. No copy-on-write forking, no speculative writes, no transaction rollback.

**Reify requirement (architecture sections 2.4–2.5):** ResolutionNode creates trial snapshots branching from current state, evaluates constraints against trial values, iterates toward convergence. Trial snapshots are internal, support recursive nesting, and enable cross-iteration cache hits.

**Workarounds considered:**
- **A. Mutate-evaluate-restore:** Every trial iteration increments the global revision, pollutes revision history, leaks trial states to concurrent queries. Nested resolution is unworkable.
- **B. Separate database instance per trial:** No structural sharing, no cache promotion from trial to main, significant memory overhead.
- **C. Resolution outside Salsa:** Constraint evaluation within trials doesn't benefit from Salsa's incrementality — maintaining a parallel evaluation infrastructure.

**Severity: Blocking.** Trial snapshots are central to the resolution architecture. No workaround preserves both Salsa's incrementality benefits and trial snapshot semantics. You'd end up with two evaluation engines.

---

## 4. Freshness Model

**Salsa:** Binary — a memo is either valid (verified at current revision) or potentially stale.

**Reify requirement (architecture section 7):** Four-variant freshness:
- `Final` — committed, fully evaluated
- `Intermediate { generation }` — still refining
- `Pending { last_substantive }` — gated, showing previous best result
- `Failed { error }` — computation failure

Freshness propagates: if any input is non-Final, output is Intermediate. Pending gates downstream. Freshness-only changes propagate in a lightweight mode without value recomputation.

**Workaround:** Encode freshness in return value as `(Value, Freshness)`. Catch-22: either Salsa's early cutoff fires (ignoring freshness changes, breaking propagation) or it doesn't (re-executing downstream on every freshness transition, wasting work).

**Severity: Significant.** Freshness is essential for interactive responsiveness. Layering it on Salsa's binary model is possible but lossy.

---

## 5. Content-Hash Caching

**Salsa:** Revision-based change tracking with early cutoff (backdating — if re-execution produces the same result, `changed_at` is not updated). Field-level backdating on tracked structs.

**Reify:** Merkle-tree content hashing with version fast path. Non-monotonic cache recovery (reverting a change finds the old cached result by hash match).

**Analysis:** Both achieve the same goal via different mechanisms. Salsa's backdating is functionally equivalent to content-hash early cutoff for within-session incrementality. Differences: content hashing provides non-monotonic cache recovery (minor — undo/redo is fast enough to re-execute) and cross-session persistence (solvable separately). Salsa's field-level backdating is finer-grained in some cases.

**Severity: Minor.** Salsa's revision-based model achieves the same practical effect within a session.

---

## 6. Demand-Driven with Concurrent Fan-Out

**Salsa:** Demand-driven (pull). Parallel execution via Rayon (`par_map`, `join`). Single-writer / multiple-reader. Cancellation via panic unwinding.

**Reify:** Demand-driven with async fan-out (Tokio). Priority levels (P0/P1-fast/P1-slow/P3). Cooperative cancellation with tokens. Priority promotion. Fine-grained cancellation (only stale work, not everything).

**Gaps:**
- Rayon (thread-based) vs Tokio (async) — mismatch but not fundamental
- No priority system in Salsa — all queries are equal
- Salsa cancels everything on input change; Reify cancels selectively
- Single-writer model blocks concurrent readers during resolution commits

**Severity: Significant.** Salsa provides basic parallelism but lacks priority scheduling, fine-grained cancellation, and async integration.

---

## 7. Two-Phase Elaboration/Evaluation

**Salsa:** Tracked functions can create tracked structs dynamically. Structs not recreated on re-execution are garbage collected. Dynamic dependencies are fully supported.

**Reify:** SchemaNode.compute() builds evaluation graph (phase 1). Demand-driven evaluation within that graph (phase 2). Phases iterate until topology stabilizes.

**Analysis:** Salsa's dynamic struct creation maps to SchemaNode creating ValueCells. Conditional creation (guards) and variable counts (collections) are supported. Salsa's GC handles removal. The iteration between phases maps to Salsa's normal re-execution with invalidation. The main friction is philosophical — Reify wants explicit graph manipulation; Salsa wants functions and structs.

**Severity: Minor to Moderate.** The two-phase model maps to Salsa's dynamic struct creation + re-execution. Semantics are compatible.

---

## 8. Performance on the Hot Path

**Salsa:** No official per-query benchmarks. Ruff saw ~10% incremental regression from fine-grained tracking. rust-analyzer operates at interactive speeds for heavyweight queries (type inference, name resolution) where framework overhead is noise.

**Reify:** P0/P1-fast evaluations (scalar arithmetic, constraint checking) must be sub-millisecond. A bracket with 5 parameters and 3 constraints has ~15 nodes; changing one parameter triggers ~5-8 node evaluations.

**Analysis:** For trivial computations (multiply two floats), Salsa's per-query overhead (hash table lookup, revision stamp check, dependency edge walk) could dominate. Coarsening query granularity trades incrementality precision for performance.

**Severity: Moderate.** Acceptable for v0.1 small designs. Could compound at scale (1000+ parameters).

---

## 9. Persistent Data Structures

**Salsa:** Interior mutability throughout (parking_lot locks, atomic integers, hashbrown hash maps). Single mutable store. Snapshots share underlying storage via read-locks.

**Reify:** HAMT-backed immutable snapshots with structural sharing. Multiple snapshots coexist. In-flight evaluations pin their snapshot. Trial snapshots branch off.

**Analysis:** Fundamentally different approaches. Adopting Salsa means abandoning HAMT snapshots. Salsa's single-writer model prevents concurrent snapshot isolation needed for trial snapshots and in-flight evaluation pinning.

**Severity: Significant.** The snapshot model is load-bearing for trial snapshots and concurrent evaluation isolation.

---

## Summary

| Dimension | Severity | Workaround viable? |
|---|---|---|
| 1. Data model mapping | Significant | Partial — model misfit nodes outside Salsa |
| 2. Warm-start state | Significant | Fragile — defeats Salsa's incrementality |
| 3. Trial snapshots | **Blocking** | No — requires parallel evaluation engine |
| 4. Freshness model | Significant | Lossy — either breaks early cutoff or propagation |
| 5. Content-hash caching | Minor | N/A — Salsa's native mechanism suffices |
| 6. Concurrent fan-out | Significant | Layering priority scheduler reimplements runtime |
| 7. Two-phase elaboration | Minor–Moderate | Yes — dynamic tracked structs work |
| 8. Hot path performance | Moderate | Coarsening trades precision for speed |
| 9. Persistent data structures | Significant | Requires abandoning HAMT model |

---

## Decision: Build Custom

Salsa is an excellent fit for compilers — static source-to-output pipelines where queries are pure, heavyweight, and revision-linear. Reify's evaluation engine is a live constraint resolution system with mutable solver state, speculative branching, priority-based scheduling, and an interactive feedback loop.

The blocking gap (trial snapshots) alone is sufficient to reject adoption. The accumulation of significant gaps (warm-start state, freshness model, concurrency model, persistent data structures) means that even if trial snapshots could be worked around, more effort would be spent fighting Salsa than building the needed features.

**Forking Salsa is also not recommended.** The gaps reflect a fundamentally different computation model, not missing features. Salsa assumes pure functions over a linearly-evolving database. Reify needs impure computations (warm state), branching evolution (trial snapshots), and rich lifecycle metadata (freshness). Modifying Salsa to support these would be a rewrite, not a fork.

---

## Ideas to Steal from Salsa

1. **Revision-based validation with version fast path.** Salsa's "verified_at / changed_at" revision stamps are simple and effective. Use as the first check before content-hash validation. (Already in architecture as `basis_version`.)

2. **Backdating (early cutoff).** When re-execution produces the same result, don't update `changed_at`. The single most impactful optimization in incremental computation. (Already in architecture.)

3. **Durability stratification.** Per-input durability levels (Low/Medium/High) with a version vector that skips validation of stable subgraphs. Standard library definitions are High durability; user parameters are Low. A parameter edit skips validating all stdlib-derived computations.

4. **Field-level change tracking.** Per-field revision tracking on tracked structs is finer-grained than whole-node comparison. Consider tracking changes at the member level within ValueCells rather than at the ValueCell level.

5. **Dynamic dependency recording via trace.** The Adapton model (thread-local trace recording during evaluation, replay for verification) is proven in both Salsa and the academic literature. (Already in architecture.)

6. **Cancellation via token + unwind.** Cooperative cancellation tokens with a panic-unwind fallback for unresponsive FFI.

7. **GC of unreachable tracked structs.** When topology changes and nodes disappear, match by identity across re-executions, GC unmatched. Good pattern for SchemaNode re-elaboration.

---

## Custom Engine Architecture Implications

The custom engine should implement:

- **Version + content-hash dual validation** (Salsa-style revision fast path + Merkle content hash fallback)
- **Immutable snapshots with HAMT structural sharing** (as designed — `im-rs` initially)
- **Durability stratification** (stolen from Salsa — skip validation of stable subgraphs)
- **Dynamic dependency traces** (Adapton model, as designed)
- **Warm-state side-table** (keyed by NodeId, separate from content-addressed cache, LRU eviction)
- **Trial snapshot branching** (fork snapshot, apply trial values, evaluate, commit or discard)
- **4-variant freshness** as first-class cache metadata
- **Async evaluation with priority scheduling** (Tokio, as designed)

The M1 sequential evaluator is the right starting point — get the interfaces right, then replace internals in M2.
