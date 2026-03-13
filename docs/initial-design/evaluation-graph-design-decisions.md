# Evaluation Graph: Design Decisions

**Status:** Core architecture established — ready for implementation prototyping and remaining design questions (long-running tasks, partial results, diagnostics, purpose-driven activation)  
**Version:** 0.1 — First crystallization from evaluation graph design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1, `geometry-engine-design-decisions.md` v0.1

---

## 1. Design approach

The evaluation graph was identified across multiple prior design phases as the critical shared infrastructure — the runtime backbone that unifies geometry processing, constraint solving, simulation, and manufacturing operations. Its design was approached by identifying the requirements scattered across the existing design documents, then resolving five foundational questions in dependency order: node and edge model, evaluation strategy, invalidation and change detection, incrementality granularity, and concurrency model.

The central architectural decision is that **the evaluation graph is built on immutable snapshots with persistent data structures**, rather than mutable state with invalidation propagation. This choice provides structural guarantees for concurrency, reproducibility, and correctness that would otherwise require careful engineering of lock protocols and invalidation ordering. The mutable state that necessarily exists (warm-start caches, scheduling infrastructure, UI state) is either encapsulated behind pure interfaces or is explicitly outside the evaluation model as acceleration structures.

**Prior art drawn upon:** Adapton (incremental memoised computation), Salsa/Rust's query system, build-systems-à-la-carte (Mokhov et al.'s taxonomy of schedulers and trace types), reactive signals/dataflow, Clojure's persistent data structures, and content-addressable storage systems.

---

## 2. Node taxonomy

The evaluation graph has five node types, each at a different natural granularity determined by the trade-off between tracking overhead and recomputation cost.

### 2.1 ValueCell — the atomic data layer

A ValueCell holds a single typed value with a determinacy state.

```
ValueCell(entity_id, member_name) → (Value, DeterminacyState)
```

Where `DeterminacyState` is one of `undef | constrained | auto | determined` (from ontology §3.2).

Every `param` and `derived` member of every entity instance gets a ValueCell. For `param` members, the ValueCell is an input (set by the designer or by a solver). For `derived` members, the ValueCell is a computation that reads other ValueCells.

**Granularity:** Per parameter. A structure with 20 parameters produces 20 ValueCells. A design with 10,000 parameters produces 10,000 ValueCells. This is the finest grain in the system. The overhead is acceptable because ValueCells are small (a typed value + determinacy tag + dependency set) and individual scalar evaluations are trivial.

**Derived members are ValueCells, not transparent inlined expressions.** This provides early-cutoff opportunities: if a derived value recomputes to the same result (e.g., `clamp(thickness, 2mm, 10mm)` when thickness changes from 11mm to 12mm — clamped value stays at 10mm), downstream dependents are unaffected. The overhead of the intermediate node is small; the cutoff benefit is valuable whenever it fires and unpredictable at graph-construction time.

### 2.2 ConstraintNode — predicate evaluation

Each constraint application (inline or from a named constraint def) becomes a ConstraintNode.

```
ConstraintNode(constraint_instance_id) → (Satisfaction, Diagnostics)
```

Where `Satisfaction` is `satisfied | violated | indeterminate | inapplicable`. The last two handle `undef` inputs — the constraint cannot be fully evaluated but is not violated.

**Granularity:** Per constraint application. A multi-line constraint body is one node. Individual predicate lines within the body contribute to diagnostics but are not independently useful as graph nodes.

**Quantifiers over collections:** `forall hole in bolt_holes: predicate(hole)` is one node, with edges to all collection elements' relevant ValueCells. When the collection changes membership (a bolt hole added or removed), the node's dependency set changes — this is a structural graph modification handled naturally by the immutable snapshot model (the new snapshot has different ValueCells).

**Structured diagnostics:** The diagnostic output is a structured record, not a flat string. The top-level `Satisfaction` status is used for early cutoff and dependency propagation. Detailed per-predicate results are available for UI rendering (highlighting which specific feature is in violation) but are a node-internal concern invisible to the graph layer.

```
ConstraintDiagnostics:
    status: Satisfaction
    predicate_results: Map<PredicateId, (Satisfaction, Detail)>
```

### 2.3 ResolutionNode — `auto` solving

When ValueCells are in `auto` state, a ResolutionNode resolves them.

```
ResolutionNode(scope_id, auto_params: Set<ValueCellId>) → Map<ValueCellId, Value>
```

**Granularity:** Per scope. This is a deliberate choice reflecting the constraint system's bottom-up resolution strategy (constraint system §6.2). An optimization problem like "minimise mass subject to all constraints" over a scope's `auto` parameters is a single coupled problem that cannot in general be decomposed into independent per-parameter resolutions.

**This is the only node type that writes to ValueCells it does not own.** Every other node writes only to its own output. ResolutionNodes write determined values to the ValueCells of the `auto` parameters they resolve. In the immutable snapshot model, this "write" manifests as producing a new snapshot with the resolved values.

**Internal decomposition:** The ResolutionNode first analyses the constraint graph over its `auto` parameters to identify connected components. Uncoupled subsets become independent sub-problems solved concurrently within the node. This is internal to the node — the graph layer sees one ResolutionNode per scope.

**Cycle handling:** The potential cycle ValueCell → RealizationNode → ConstraintNode → ResolutionNode → ValueCell is not a cycle in the graph — it is a convergence loop *within* the ResolutionNode's computation. The ResolutionNode internally iterates: propose parameter values → realise geometry → check constraints → adjust. This iteration uses the same graph infrastructure via evaluation contexts (§3), not a separate evaluation mechanism. The graph itself remains a DAG at the macro level.

### 2.4 RealizationNode — producing representations

When a concrete geometric representation (or mesh, SDF, etc.) is needed, a RealizationNode produces it.

```
RealizationNode(entity_id, repr_kind, tolerance) → Representation
```

**Granularity:** Per (entity, representation_kind, tolerance_level). The same entity might need both a B-rep and a mesh, or a coarse mesh for visualisation and a fine mesh for FEA. Each is a separate RealizationNode. No representation is privileged (geometry engine §2.1).

**Containment-tree decomposition:** Sub-structures have their own RealizationNodes. A parent structure's RealizationNode *composes* sub-realisations (union, positioning) rather than rebuilding from primitives. Changing a sub-structure's parameter invalidates only that sub-structure's RealizationNode and the parent's composition node. The other sub-structures' RealizationNodes are cache hits. This gives operation-level incrementality through the natural structure of the containment tree, without the graph explosion of per-CSG-operation nodes.

**Operations within a single entity** (a complex CSG expression with no sub-structures) are evaluated monolithically within that entity's RealizationNode. Warm starting (§5) handles the incrementality. If a single entity's geometry is so complex that monolithic evaluation is a bottleneck, decomposition into sub-structures is the correct response (and better design practice).

### 2.5 ComputeNode — expensive computations

FEA, CFD, toolpath generation, lattice infill, surrogate model training, format export, and any other heavy derived computation.

```
ComputeNode(computation_id) → ComputationResults
```

**Granularity:** Per computation invocation. One FEA run = one node. These are coarse because the computations are monolithic — fine-grained incrementality belongs inside the node's warm-start implementation, not at the graph level.

ComputeNode results may include fields (stress tensor field), scalars (safety factor), collections (critical points), or any other typed value that downstream nodes can consume.

---

## 3. Immutable snapshots and evaluation contexts

### 3.1 Core model

Every state of the evaluation graph is an immutable **snapshot**. No mutation occurs. When something changes — a designer edit, a solver trial, a resolved value — the system produces a new snapshot that shares structure with the old one via persistent data structures (HAMT — hash array mapped tries). Evaluation is a pure function from snapshot to results.

```
Snapshot:
    version: VersionId                          // monotonic, globally unique
    values: PersistentMap<ValueCellId, (Value, DeterminacyState)>
    edges: PersistentMap<NodeId, Set<NodeId>>   // dependency edges
    provenance: SnapshotProvenance              // how this snapshot was created
```

Structural sharing means creating a new snapshot with one changed parameter copies O(log n) trie nodes, not the entire map. A design with 50,000 ValueCells produces a new snapshot on each edit that shares >99.9% of its memory with the previous one.

### 3.2 Snapshot provenance

Snapshots carry a lightweight provenance annotation recording how they were created.

```
SnapshotProvenance:
    | Edit { changed: Set<ValueCellId>, parent: SnapshotId }
    | Merge { sources: List<SnapshotId>, resolution: ConflictResolution }
    | Import { source: ExternalSource }
    | Resolution { scope: ScopeId, resolved: Set<ValueCellId>, parent: SnapshotId }
```

Provenance enables efficient change detection: `Edit` provenance gives the changed cells at O(1) cost, avoiding the need for a structural diff. When provenance is unavailable (imported or externally produced snapshots), structural HAMT diffing provides the fallback at O(k log n) where k is the number of changes.

### 3.3 Evaluation contexts for resolution

ResolutionNodes need to explore trial values without polluting the committed design state. Rather than implementing a separate evaluation mechanism, resolution uses the same graph infrastructure through **evaluation contexts** — child snapshots that branch from the current state.

```
resolve(base_snapshot, scope_id, auto_params) -> Snapshot:
    trial_snapshot = base_snapshot
        .with(param_a, (trial_value_a, Determined))
        .with(param_b, (trial_value_b, Determined))

    constraint_results = evaluate_constraints(trial_snapshot, scope_id)

    if converged:
        return trial_snapshot   // the resolved values are in this snapshot
    else:
        next_trial = adjust(trial_snapshot, constraint_results)
        return resolve(next_trial, scope_id, auto_params)
```

Each solver iteration creates a new snapshot. No context management, no commit/rollback protocol. Trial snapshots that the solver abandons are garbage collected. Trial snapshots that converge have their resolved values merged into the current snapshot, producing a new snapshot with `Resolution` provenance.

**Benefits of this approach:**

- **Same infrastructure:** Dependency tracking, caching, and scheduling are provided by the graph engine. No reimplementation inside the resolver.
- **Natural transactionality:** Tentative values are isolated in trial snapshots. Multiple resolvers for different scopes can run concurrently without interference.
- **Cross-iteration incrementality:** The content-hash cache automatically provides incrementality across solver iterations. If iterations 3 and 7 produce the same geometry for a DFM check (because the parameter under exploration doesn't affect that geometry), the cache hit is automatic.
- **Cache promotion:** When a trial snapshot converges, cached results computed during solving (realisations, constraint checks) are valid at the final values. They're naturally available in the cache without an explicit promotion step — the content-hash key matches.
- **Recursive nesting:** Resolving scope X may require first resolving child scope Y (bottom-up strategy). Y's resolution creates trial snapshots branching from X's trial snapshot. The layering composes naturally through the persistent data structure's structural sharing.

### 3.4 Concurrency from immutability

Immutable snapshots provide concurrency without coordination:

- **Multiple readers, zero coordination.** Any number of evaluations can read the same snapshot simultaneously. Each has a consistent point-in-time view by construction.
- **Concurrent solvers.** Independent resolution scopes explore trial snapshots from the same base without interference — neither mutates anything.
- **Speculative evaluation.** Work can proceed against trial snapshots speculatively. If the speculation is wrong, the work is simply unused — no rollback needed.
- **Distribution readiness.** Content-addressed immutable snapshots are naturally networkable. A remote worker receives a snapshot (or relevant slice), computes, returns a result. No distributed locking or cache coherence protocol required.

### 3.5 Garbage collection

A snapshot stays alive as long as something references it — an active evaluation, the "current" pointer, a pinned historical state (for undo or a long-running export job). When the last reference drops, the snapshot's unique trie nodes (not shared with any other live snapshot) are freed.

The working set is small in practice: the current snapshot, one or two previous ones held by in-flight evaluations, and the current solver's trial snapshot. Structural sharing means these together use only slightly more memory than a single snapshot.

**Cache eviction** is managed separately. An LRU eviction policy weighted by recomputation cost (expensive results like B-rep Booleans get higher priority weights) bounds cache memory. Cache capacity is a configurable resource knob.

---

## 4. Evaluation strategy

### 4.1 Pull-based with content-hash verification

The core strategy is **demand-driven (pull-based)**. A node's result is computed when something asks for it, checked against the content-hash cache, and recomputed only if the cache misses. This is the natural fit for immutable snapshots.

```
evaluate(snapshot, node_id) -> Result:
    input_hash = hash(read dependencies from snapshot)
    if cache contains (node_id, input_hash):
        return cached result

    inputs = read dependencies from snapshot
    result = compute(node_id, inputs)
    store (node_id, input_hash) -> result
    return result
```

**Cache keying: content-based with version fast path.** Each cached result stores both the content hash and the snapshot version at which it was computed. The fast path: if the current snapshot version matches, return immediately (no hashing). On version mismatch, compute the content hash and check. This gives O(1) hits for repeated reads within the same snapshot and content-based cross-version reuse (the "change and change back" scenario hits the cache automatically).

Content hashes are computed **lazily** — only on version-based cache miss. For the tight interactive loop (same snapshot, many reads), no hashing overhead.

### 4.2 Always-demanded nodes and the demand registry

Nodes feeding live UI elements are registered as **always-demanded** in a lightweight mutable registry:

```
DemandRegistry:
    always_demanded: Set<NodeId>
    demand_cone: Set<NodeId>          // backward transitive closure, cached
```

What gets registered as always-demanded: nodes feeding the current 3D viewport (RealizationNodes for visible structures), ConstraintNodes displayed in the constraint panel, ValueCells shown in the property editor, and any node feeding a live diagnostic indicator.

The **demand cone** is the backward transitive closure from always-demanded nodes through dependency edges — everything that would need to be evaluated to satisfy those demands. It is maintained incrementally: additions trigger a backward walk to add transitive dependencies; removals clean up dependencies not reachable from other always-demanded nodes.

### 4.3 The two-cone scheduling model

When a new snapshot is created (designer edit), two sets are computed:

**The dirty cone** (forward from changed cells): walk dependency edges forward from the changed ValueCells through the reverse dependency index. This is a conservative superset of what's actually stale — it doesn't account for early cutoff.

**The demand cone** (backward from always-demanded nodes): the pre-computed set of everything the live UI needs.

**The intersection** — nodes that are both potentially dirty and transitively needed by something the user is actively looking at — is the set of nodes that should be recomputed immediately, with highest priority.

**Priority scheduling:**

| Priority | Criterion | Scheduling |
|---|---|---|
| **P0 — Interactive** | ValueCell reads for property editor, keystroke echo | Synchronous or near-synchronous |
| **P1-fast** | In dirty∩demand intersection, cheap (scalars, constraints) | Dispatched immediately, expected sub-frame completion |
| **P1-slow** | In dirty∩demand intersection, expensive (realisations, resolutions) | Dispatched immediately, async completion with UI progress indication |
| **P3 — Speculative** | In dirty cone but not demand cone | Background, preemptible by higher-priority work |

Work outside both cones is not needed for this edit. The system does not compute it.

### 4.4 Dependency tracking: dynamic with static optimisation

Dependencies are discovered during evaluation via **dynamic trace recording**. Every ValueCell read during a node's evaluation is recorded in the dependency trace. This is the Adapton model — dependencies are traces of actual execution.

This is necessary because the language has data-dependent reads: conditional expressions, quantifiers over variable-size collections, field compositions with conditional sampling. Static dependency analysis cannot capture these.

**Verification of cached results** replays the dependency trace: for each recorded read, check if the current snapshot has the same value. If all match, the result is valid (same execution path would occur). If any differs, re-evaluate from scratch and record a new trace.

**Static optimisation:** For the many cases where dependencies are statically known (simple arithmetic, constraints over explicit parameters), a static analysis pass pre-computes the dependency set. This avoids trace-recording overhead for the common case. Dynamic tracking kicks in only for expressions with conditionals, quantifiers, or other data-dependent reads.

### 4.5 Evaluation ordering: recursive descent with concurrent fan-out

The evaluator walks the dependency graph recursively. When it encounters a node with multiple uncached dependencies, it spawns those dependencies as concurrent tasks and awaits all of them before computing the current node.

```
evaluate(snapshot, node_id):
    if cache_valid(snapshot, node_id):
        return cached_result

    deps = get_dependencies(snapshot, node_id)
    dep_results = parallel_map(deps, |dep| evaluate(snapshot, dep))
    result = compute(node_id, dep_results)
    cache_store(snapshot, node_id, result)
    return result
```

This gives depth-first simplicity for linear dependency chains and breadth-first parallelism at fan-out points. The async runtime's work-stealing scheduler handles thread utilisation naturally.

---

## 5. Warm starting

### 5.1 The insight

Cached results are not just outputs to reuse or discard — they are **inputs to the next computation of the same node**. Many expensive computations converge dramatically faster when given a good starting point: Newton's method with a warm start from the previous solution converges in 1–3 iterations instead of 20+. B-rep kernels can incrementally update a model when one parameter changes rather than rebuilding from scratch. Meshers can locally remesh only the affected region. FEA solvers converge faster with the previous solution as initial condition.

The binary "cache hit or recompute from scratch" model leaves enormous performance on the table. For B-rep kernels specifically, the difference can be minutes (cold rebuild of a complex model) versus frames-per-second (incremental update of a few faces).

### 5.2 The warm-start protocol

Each node type can optionally implement a warm-start interface:

```
trait WarmStartable:
    type State      // opaque internal state (OCCT model, mesh, solver state, etc.)
    
    fn compute_cold(inputs) -> (Result, State)
    fn compute_warm(inputs, previous_state, input_diff) -> (Result, State)
```

The evaluation graph stores not just the cached result but the **opaque internal state** alongside it. When a node needs recomputation, the evaluator checks whether previous state exists and what changed in the inputs (computed from the dependency trace). If previous state exists, it calls `compute_warm`. Otherwise, it falls back to `compute_cold`.

```
NodeCache:
    result: CachedResult              // immutable, content-hashable
    dependency_trace: DependencyTrace  // immutable
    warm_state: Option<OpaqueState>   // opaque, mutable, not content-addressed
```

The warm state is explicitly *not* part of the content-addressed cache. It is not hashed, not compared, not used for cache validity checks. It is a performance hint that travels alongside the cached result. If it is present, use it for faster recomputation. If not (evicted, first evaluation, remote worker), fall back to cold computation with identical semantic results.

### 5.3 Input diffs for warm starting

The dependency trace records which inputs were read and what their values were. When recomputation is needed (cache miss), the system computes the **input diff** — which traced values differ between the cached trace and the current snapshot. This diff is passed to `compute_warm` so the node can scope its incremental update.

For a RealizationNode wrapping OCCT, the input diff might be "fillet radius changed from 2mm to 3mm, everything else identical." The node locates the fillet feature in the OCCT model, updates its radius, and triggers OCCT's incremental rebuild — recomputing affected faces and edges while leaving the rest of the model untouched.

### 5.4 Warm-state encapsulation

The mutable state inside a warm-start-capable node (the OCCT `TopoDS_Shape`, the solver's internal data structures, the mesher's spatial index) is entirely encapsulated. From the graph's perspective, evaluation is a pure function: given inputs X, produce result R. The internal mutation is an optimisation invisible to the graph layer.

This is the same relationship as the `@optimised` hook from the ontology (§2.3) — a semantically equivalent fast path. The language-level definition is pure; the implementation can be as stateful as it needs to be internally.

### 5.5 Warm-start tiers

Three tiers of warm starting, in order of implementation priority:

**Tier 1 — Same node, previous result (v0.1).** The evaluation graph already has the cached state for node N from the previous snapshot. On recomputation, pass the previous warm state to `compute_warm`. This is the common case and covers the critical interactive editing loop.

**Tier 2 — Same node, closest parameter set seen (future).** Keep multiple cached states per node, indexed by input values. On recomputation, pick the state whose inputs are closest to the current inputs (in a domain-appropriate metric). Valuable for parameter-space exploration (slider interactions) where the designer moves back and forth.

**Tier 3 — Any node of the same type, closest parameters (future).** A type-level cache: all instances of `HexBolt` share a pool of warm states indexed by parameter values. Creating a new bolt instance warm-starts from the closest existing bolt.

**Design decision:** Implement tier 1 for v0.1. The warm-start protocol (opaque state type, input-diff mechanism) supports tiers 2 and 3 without modification. The multi-state cache and type-level indexing are clear upgrade paths.

### 5.6 Warm-state pools

For nodes that support it, warm states are managed in a small pool (initially size 1, expandable). The pool supports:

- **Checkout:** Acquire a warm state for use by an evaluation. If the pool is empty, fall back to cold computation.
- **Return:** Return an updated warm state after evaluation completes.
- **Clone-then-modify:** When the underlying kernel supports it (OCCT supports shape copying), a warm state can be cloned to populate the pool. This enables concurrent evaluations of the same node from different warm-start points, and naturally populates tier-2 caches from actual usage patterns.

---

## 6. Change detection and the reverse dependency index

### 6.1 Change detection mechanism

In the immutable snapshot model, there is no invalidation in the traditional sense — no dirty bits, no propagation signals. The functional equivalent of "what's stale" is determined by two mechanisms:

1. **Snapshot diff:** What ValueCells changed between the previous and current snapshot. Obtained from provenance annotation (O(1) for single edits) or structural HAMT diff (O(k log n) fallback).

2. **Content-hash cache verification:** When evaluating a node, compute the content hash of its current inputs and check the cache. If the hash matches a cached entry, the result is valid regardless of which snapshot version produced it.

The dirty cone (forward walk from changed cells through the reverse index) is a **conservative pre-computation** of what might be stale. It over-approximates because it doesn't account for early cutoff, but it's cheap to compute and valuable for scheduling (§4.3).

### 6.2 The reverse dependency index

Every cached result stores its dependency trace. From these traces, a reverse index is maintained:

```
ReverseDependencyIndex:
    Map<ValueCellId, Set<NodeId>>   // "which nodes read this cell?"
```

The index is maintained incrementally: entries are added when a node is evaluated and its result cached, removed when a cached result is evicted. It is a mutable acceleration structure derived from the cache — reconstructible at any time, and a slightly stale index merely makes the dirty cone slightly more conservative (harmless).

**Dirty cone computation:**

```
dirty = reverse_index[changed_cells]       // direct dependents
for node in dirty:
    dirty ∪= reverse_index[node]           // transitive dependents
```

Cost is proportional to the cone size, not the full graph size.

### 6.3 Dynamic dependencies and the dirty cone

The reverse index reflects the dependency structure of *cached* evaluations. When a change causes a conditional branch to flip, the actual dependencies may differ from the cached trace. The dirty cone is sound but potentially incomplete in this case:

- Nodes that *were* dependent are in the dirty cone — correct, they might be stale.
- Nodes that *become* dependent (due to a new branch) are not in the dirty cone — but they are discovered during re-evaluation when their upstream dependency is recomputed and produces a changed result, triggering a fresh demand-driven pull.

For always-demanded nodes, the demand cone backstops the dirty cone — even if the dirty cone misses a dependency path, the demand-driven evaluation discovers it.

### 6.4 Early cutoff

After computing a node's result, it is compared to the previously cached result. If equal, downstream dependents are unaffected. This is the immutable model's equivalent of "clean" propagation stopping dirty-bit cascades.

**Equality determination by node type:**

| Node type | Equality check | Cost |
|---|---|---|
| ValueCell (scalar) | Bitwise value equality | Trivial |
| ValueCell (geometric spec) | Content hash of the expression tree | Cheap |
| ConstraintNode | Satisfaction status equality | Cheap |
| ResolutionNode | Resolved value set equality | Cheap |
| RealizationNode | Not checked — input-hash match is used instead | N/A |
| ComputeNode | Domain-specific — typically result hash | Varies |

**Bitwise equality for scalar values.** Tolerance-based comparison risks hiding genuine small changes that matter to tight constraints. If floating-point non-determinism produces different bits for the same mathematical result, the downstream recomputation triggered by the "false positive" is cheap and correct. Correctness over premature optimisation.

**RealizationNodes skip output equality checking.** Comparing two B-rep models or meshes for equivalence is expensive and ill-defined. Instead, the content-hash cache keying on *inputs* provides the early cutoff: if the inputs haven't changed, the output hasn't changed — no output comparison needed. If the inputs *have* changed, the output is assumed changed and dependents re-evaluate.

### 6.5 Non-monotonic edits

The content-hash cache naturally handles "change and change back" scenarios. If `thickness` changes from 5mm to 6mm (snapshot N→N+1) and then back to 5mm (snapshot N+1→N+2), evaluating against snapshot N+2 finds the original cached results from snapshot N — the content hashes match. No recomputation needed, no special-case logic.

---

## 7. Incrementality granularity summary

The evaluation graph provides coarse-grained incrementality through dependency tracking and cache reuse. Fine-grained incrementality *within* nodes is achieved through warm starting and internal decomposition. This keeps the graph manageable in size while enabling high-performance interactive updates.

| Node type | Granularity | Primary incrementality mechanism | Rationale |
|---|---|---|---|
| **ValueCell** | Per parameter/derived member | Early cutoff via value comparison | Finest grain; trivial recomputation cost justifies tracking overhead |
| **ConstraintNode** | Per constraint application | Full re-evaluation with structured diagnostic diff | Semantic unit; internal predicates not worth separate nodes |
| **ResolutionNode** | Per scope, with internal decomposition of uncoupled parameter subsets | Warm starting (previous solution as initial guess) | Coupled optimisation defines the natural problem boundary |
| **RealizationNode** | Per entity per (repr_kind, tolerance), composing sub-entity realisations | Warm starting (kernel incremental update) + sub-entity cache reuse via containment tree | Containment tree provides natural decomposition without graph explosion |
| **ComputeNode** | Per computation invocation | Warm starting (previous results as initial conditions) | Monolithic computations; internal incrementality is solver-specific |

**Collection-derived values** are single ValueCells containing the whole collection. Per-element decomposition creates node-per-element scaling problems for large collections. Whole-collection recomputation is typically cheap (map a simple function), and early cutoff at the collection level catches unchanged cases. Per-element decomposition can be introduced as a targeted optimisation if profiling identifies specific bottlenecks.

---

## 8. Concurrency model

### 8.1 Task model

Each node evaluation is a **task** submitted to a shared async runtime (Tokio-style work-stealing thread pool).

```
Task:
    node_id: NodeId
    snapshot: SnapshotRef
    priority: Priority
    warm_state: Option<OpaqueState>
    cancellation_token: CancellationToken
```

CPU-bound tasks (constraint evaluation, scalar computation) run on the compute pool. Blocking tasks (disk I/O, network calls) run on a separate blocking pool. GPU-dispatched work runs on a dedicated dispatch thread with async completion.

### 8.2 Priority levels

| Priority | Criterion | Behaviour |
|---|---|---|
| **P0 — Interactive** | ValueCell reads for property editor, keystroke echo | Synchronous or near-synchronous; never queued behind heavy work |
| **P1-fast** | In dirty∩demand intersection, cheap nodes | Dispatched immediately, expected sub-frame completion |
| **P1-slow** | In dirty∩demand intersection, expensive nodes | Dispatched immediately, async completion with UI progress indication |
| **P3 — Speculative** | In dirty cone but not demand cone | Background, preemptible by higher-priority work |

**Priority promotion:** If a P1-slow task depends on a P3 task already in-flight, the P3 task is promoted to P1-slow. This falls out naturally from the recursive evaluation model — a P1-slow evaluation recurses into its dependencies, and any in-flight lower-priority dependency gets its priority boosted.

### 8.3 Warm-state concurrency

Warm-start state is mutable and node-specific. Concurrent access is managed through a **state pool** (initially size 1, expandable):

- **Exclusive access (v0.1):** Each node's warm state has a mutex. One evaluation at a time. On conflict, the second evaluation waits or falls back to cold computation.
- **Clone-then-modify (expansion path):** When the underlying kernel supports cloning (OCCT supports shape copying), the warm state can be cloned to enable concurrent evaluations from different starting points. Cloned states populate the pool organically from actual usage patterns.
- **Pool of size N (future):** Naturally supports tier-2 warm starting (multiple states at different parameter values). Check out a state, compute, return the updated state.

The interface is designed so the calling pattern (`checkout → compute → return`) generalises from mutex (pool of size 1) to pool (size N) without changing the evaluation code.

### 8.4 Cancellation

**Cooperative cancellation via tokens.** Each task holds a `CancellationToken`. Long-running computations check the token at natural breakpoints (between solver iterations, between geometric operations). On cancellation, the task returns early, optionally saving its current warm state for reuse by a restarted task.

**Cancellation policy by priority:**

| Priority | Cancellation behaviour |
|---|---|
| P0, P1-fast | Never cancelled — completes in sub-frame time |
| P1-slow | Cancelled if the new edit's dirty cone includes this node; otherwise, allowed to complete (still valid) |
| P3 (speculative) | Cancelled immediately when a new snapshot arrives |

**Resolution and cancellation:** If the base snapshot is superseded by a new edit, a resolver mid-iteration is cancelled only if the edit changes a parameter within the resolver's scope (its constraint landscape has changed). Edits outside the scope leave the resolver's work valid.

### 8.5 Determinism

**Semantic determinism: guaranteed.** The same node evaluated against the same snapshot always produces the same result, regardless of scheduling order. This follows from the immutable snapshot model and pure evaluation.

**Temporal non-determinism: accepted.** The order in which results become available to the UI is non-deterministic (whichever task finishes first updates the display first). This is the expected behaviour of a responsive UI with async updates.

**Cross-snapshot UI consistency:** UI updates are tagged with their snapshot version. The display layer only applies updates from the current snapshot. When a new snapshot arrives, incomplete P1 work against the old snapshot that's in the new edit's dirty cone is cancelled and restarted. The UI never shows a 3D preview from snapshot N alongside constraint results from snapshot N+1.

### 8.6 Cycle handling under concurrency

The constraint↔geometry cycle (pushed inside ResolutionNodes) manifests as nested async/await: a resolution task awaits sub-evaluations (realisations, constraint checks), which run on the same thread pool. The resolution task suspends at await points, freeing its thread for other work (including the sub-tasks it's waiting on). The async runtime's work-stealing scheduler prevents thread starvation.

For the expected nesting depth (2–4 levels of scope nesting), runtime resource usage (suspended futures, stack frames) is not a concern. If pathological nesting is observed, the resolution strategy can be changed to sequential bottom-up (resolve deepest scopes first, fully, before starting parent scopes) at the cost of some parallelism.

### 8.7 Distribution readiness

The task interface is distribution-agnostic. A task is "evaluate node X against snapshot S, optionally with warm state W." Whether this runs on a local thread, a GPU, or a remote machine is a scheduling decision. The interface is the same.

For remote evaluation: snapshots are serializable (persistent data structures are trees of values). Warm-start state is not assumed to be available remotely — remote evaluation falls back to cold computation. Results are serializable and integrate into the local cache on return. No distributed locking or cache coherence protocol is required.

---

## 9. Mutability audit

The system's state is partitioned by mutability with clear rationale for each choice.

### 9.1 Immutable (load-bearing for correctness and concurrency)

| Component | Why immutable |
|---|---|
| **Snapshots** (value maps, edge maps) | Core correctness invariant: concurrency safety, reproducibility, time-travel debugging |
| **Cached results** | Content-addressed, never modified after creation; enables cross-version reuse |
| **Dependency traces** | Attached to cached results; used for verification and reverse-index construction |

### 9.2 Mutable, encapsulated behind pure interface

| Component | Why mutable | Encapsulation |
|---|---|---|
| **Warm-start state** | Internal kernel/solver state must be mutable for incremental updates | Behind `compute_cold`/`compute_warm` interface; invisible to graph layer |

### 9.3 Mutable acceleration structures (derived, reconstructible)

| Component | Why mutable | Failure mode if stale |
|---|---|---|
| **Reverse dependency index** | Derived from cache; incremental maintenance is cheaper than persistent-map overhead | Dirty cone is slightly more conservative → wasted recomputation, not incorrect results |
| **Cache storage** (the map itself) | Insertion/eviction is inherently stateful; no need for point-in-time consistency | Standard concurrent data structure (lock-free reads) |

### 9.4 Mutable, outside evaluation model

| Component | Why mutable | Scope |
|---|---|---|
| **Demand registry / demand cone** | Ephemeral UI state; changes on viewport pan, panel switch | Scheduling hints, not evaluation inputs |
| **Thread pool / task scheduler** | Inherently stateful infrastructure | Standard async runtime |

**The invariant:** The evaluation model — "given this snapshot, what is the result of evaluating this node?" — is entirely pure and deterministic. Everything mutable is either below the abstraction (warm state), beside the abstraction (scheduling, UI state), or a cache that merely accelerates the pure computation.

---

## 10. Edge types

The five node types are connected by dependency edges recording data flow.

| From → To | Meaning | Example |
|---|---|---|
| ValueCell → ValueCell | Derived value reads other values | `volume` reads `thickness`, `width`, `height` |
| ValueCell → ConstraintNode | Constraint reads a parameter | `thickness > 2mm` reads `thickness` |
| ValueCell → RealizationNode | Geometry parameterised by a value | Box dimensions, fillet radius |
| ValueCell → ComputeNode | Computation reads parameters | FEA reads `material.youngs_modulus` |
| ConstraintNode → ResolutionNode | Solver reads constraint landscape | `auto` resolution reads constraint set |
| ResolutionNode → ValueCell | Solver writes resolved values | `auto thickness` gets determined |
| RealizationNode → ConstraintNode | DFM constraint reads geometry | Wall thickness check needs a realised solid |
| RealizationNode → ComputeNode | Computation needs a representation | Stress analysis reads mesh realisation |
| ComputeNode → ValueCell | Result feeds back | Stress field → density field → lattice params |
| RealizationNode → RealizationNode | Parent composes sub-realisations | Assembly realisation reads sub-structure realisations |

The graph is a DAG at the macro level. The apparent cycle ValueCell → RealizationNode → ConstraintNode → ResolutionNode → ValueCell is resolved by pushing the convergence loop inside ResolutionNodes (§2.3), which internally iterate using trial snapshots.

---

## 11. Lifecycle of a change: end-to-end trace

The designer changes `bracket.thickness` from `5mm` to `6mm`:

1. **Snapshot creation.** `snapshot_v2 = snapshot_v1.with(bracket.thickness, (6mm, Determined))`. Provenance: `Edit { changed: {bracket.thickness}, parent: v1 }`.

2. **Dirty cone computation.** Forward walk from `bracket.thickness` through the reverse dependency index. Finds: `bracket.volume` (derived), `constraint: thickness > 2mm`, `constraint: thickness < width / 2`, `bracket body realisation` (reads thickness for box dimensions), and transitively anything depending on those.

3. **Intersection with demand cone.** The 3D viewport has `bracket body realisation` as always-demanded. The constraint panel has both constraints as always-demanded. Intersection identifies priority-1 work.

4. **P1-fast evaluation.** `bracket.volume`: cached result was computed with `thickness = 5mm`; content hash mismatch → recompute → new volume. Early cutoff: new volume differs from old → dependents of `volume` are genuinely dirty.

5. **P1-fast evaluation.** `constraint: thickness > 2mm`: input is `thickness = 6mm`. `6mm > 2mm` → satisfied. Previous result was also satisfied → early cutoff: status unchanged, UI indicator needs no update.

6. **P1-slow evaluation.** `bracket body realisation`: input hash changed → recompute B-rep. Warm state exists from previous evaluation. Input diff: `thickness: 5mm → 6mm`. The OCCT wrapper calls `compute_warm` — locates the box feature, updates its dimension, incrementally rebuilds affected geometry. Dispatched to thread pool; UI shows "computing" indicator. On completion, new B-rep is cached with updated warm state, 3D preview updates.

7. **P3 speculation.** Other nodes in the dirty cone but not the demand cone (STEP export node, FEA mesh) are speculatively re-evaluated in the background.

8. **Index update.** Reverse dependency index is updated for any nodes whose dependency sets changed during re-evaluation.

---

## 12. Open questions for subsequent design phases

### 12.1 Long-running and async tasks

FEA, optimisation, and ML surrogate evaluation can take seconds to hours. How do they integrate without blocking the graph? How does a long-running ComputeNode report progress? What happens when the snapshot it's evaluating against becomes stale mid-computation? Under what conditions should it be cancelled versus allowed to complete?

### 12.2 Partial results and streaming

The "proposing" mode (constraint system §4.3) and UI responsiveness targets imply partial/progressive results. How does a node publish intermediate state? How do downstream consumers handle an upstream result that's "in progress"? What is the UI contract for displaying provisional versus committed results?

### 12.3 Error, diagnostic, and provenance flow

Constraint violations, `undef` traces, tolerance warnings, solver convergence diagnostics. How do diagnostics propagate through the graph? How are they aggregated across nodes? How does the determinacy "stack trace" (ontology §8.2 — showing not just how complete something is, but why it's incomplete) map to the evaluation graph's dependency structure?

### 12.4 Purpose-driven activation

How does a purpose ("evaluate for manufacturing readiness") map to a subgraph? How does the system determine which nodes need evaluation to satisfy a purpose's determinacy requirements? How does this interact with the demand registry and the two-cone scheduling model?

### 12.5 Graph structural changes

Adding or removing sub-structures, changing collection membership, conditional sub-structure presence — these change the graph's topology. How are structural changes detected, represented, and propagated in the immutable snapshot model? What is the cost of structural diffing versus value diffing?

### 12.6 Tolerance budget allocation

The representation tolerance contract (geometry engine §6) requires allocating error budgets across chains of conversions (B-rep → mesh → SDF → voxel). How does the evaluation graph manage tolerance flow? How do tolerance requirements interact with realisation node caching (different tolerance → different cache key → potentially different warm-start state)?

### 12.7 Implementation technology choices

Persistent data structure library selection (Rust `im` crate, custom HAMT, etc.). Async runtime selection and configuration. Content hashing algorithm. Cache storage backend (in-memory, memory-mapped, distributed). These are implementation decisions that should be informed by prototyping and benchmarking.

---

## 13. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Core data model | Immutable snapshots with persistent data structures | Structural guarantees for concurrency, reproducibility, correctness |
| Node taxonomy | 5 types: ValueCell, ConstraintNode, ResolutionNode, RealizationNode, ComputeNode | Each at its natural granularity; covers all evaluation concerns |
| Cycle resolution | DAG at macro level; cycles pushed inside ResolutionNodes using trial snapshots | Clean graph semantics; resolution uses same infrastructure recursively |
| Evaluation strategy | Pull-based / demand-driven with content-hash verification | Natural fit for immutable snapshots; compute only what's needed |
| Scheduling | Two-cone model (dirty cone ∩ demand cone) with four priority levels | Deterministic identification of critical-path work; no heuristics for P1 |
| Dependency tracking | Dynamic (trace-based) with static optimisation | Dynamic required for conditionals/quantifiers; static cheaper for common case |
| Change detection | Snapshot provenance + content-hash cache + conservative dirty cone | No invalidation signals needed; correctness from immutability |
| Early cutoff | Per-node value comparison (bitwise for scalars, input-hash for expensive nodes) | Prevents unnecessary propagation without expensive output comparison |
| Warm starting | Opaque state protocol with input diffs; tier 1 for v0.1 | Bridges immutable model with mutable kernel internals; dramatic perf for incremental edits |
| Warm-state pools | Clone-then-modify population; checkout/return interface | Organic growth from usage; supports concurrent access and tier-2 caching |
| Concurrency | Shared async work-stealing runtime; cooperative cancellation | Standard, efficient; snapshot immutability eliminates most coordination needs |
| Mutability boundary | Immutable for evaluation model; mutable for encapsulated internals and acceleration structures | Clear invariant: evaluation is pure; everything mutable is below or beside the abstraction |

---

*Document generated from evaluation graph design sessions. Intended as a living specification to be refined through subsequent design phases.*
