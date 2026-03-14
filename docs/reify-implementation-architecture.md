# Reify Implementation Architecture

**Version:** 0.1
**Date:** 2026-03-13
**Status:** Synthesized from 16 design decision documents and design review resolutions.
**Scope:** Complete implementation-level architecture reference. All design review resolutions applied.

---

## 1. Introduction

Reify is a text-based DSL for engineering design. Source files (`.ri`) describe structures, occurrences, constraints, and fields. The runtime progressively evaluates these declarations into fully realized, manufacturable artifacts. This document describes the implementation architecture that makes that evaluation possible.

### 1.1 Design Principles

Four principles permeate every architectural decision:

**Immutable snapshots.** Every design state is an immutable snapshot. No mutation. New states share structure with old states via persistent data structures (HAMT). Mutable state that necessarily exists (warm-start caches, scheduling infrastructure, UI state) is encapsulated behind pure interfaces or explicitly outside the evaluation model.

**Demand-driven evaluation.** Nothing is computed until needed. Node results are computed when requested, checked against a content-hash cache, and recomputed only on miss. The two-cone scheduling model determines what to compute and when.

**Orchestrator pattern.** No monolithic solver or kernel. The runtime dispatches to specialized sub-engines (geometry kernels, constraint solvers, analysis tools) and manages their interaction. This pattern appears at every layer: constraint resolution, geometry realization, kernel dispatch.

**Source-as-canonical.** There is no privileged geometric representation. Source text is the canonical design specification. B-rep, mesh, SDF, and voxel representations are all realizations produced on demand. If the source changes, dependent realizations are invalidated and recomputed.

### 1.2 Declaration Kinds (Post-Review)

The language has four entity types and three non-entity declaration kinds:

| Declaration kind | Entity? | Has identity? | Has determinacy? | Eval graph presence |
|---|---|---|---|---|
| `structure` | Yes | Yes | Yes | ValueCells, RealizationNodes, etc. |
| `occurrence` | Yes | Yes | Yes | ValueCells, RealizationNodes, etc. |
| `constraint` | Yes | Yes | Yes | ConstraintNodes |
| `field` | Yes | Yes | Yes | ValueCells, ComputeNodes |
| `fn` | No | No | No | Inlined into dependent nodes |
| `trait` | No | No | No | None -- compile-time only |
| `purpose` | No (but AST identity) | Yes | No | Contributes constraints/outputs when activated |

Key corrections from design review:
- `trait` has **no evaluation graph presence** -- traits are named, composable bundles of requirements resolved at compile time.
- `purpose` is a named declaration with AST identity. When activated, its constraints and outputs are present in the evaluation graph; when deactivated, they are absent.
- `fn` is pure computation with no graph presence (inlined into dependent nodes).

---

## 2. Evaluation Graph

The evaluation graph is the central runtime data structure. It is a directed acyclic graph (DAG) of typed nodes connected by dependency edges, built on immutable snapshots with persistent data structures.

### 2.1 Node Taxonomy (7 Types)

#### ValueCell -- the atomic data layer

```
ValueCell(entity_id, member_name) -> (Value, DeterminacyState)
```

Where `DeterminacyState` is `undef | constrained | auto | determined`.

Granularity: per parameter. Every `param` and `let` member of every entity instance gets a ValueCell. A design with 10,000 parameters produces 10,000 ValueCells. `let` members are ValueCells (not transparent inlined expressions) to provide early-cutoff opportunities. For example, `clamp(thickness, 2mm, 10mm)` stays at 10mm when thickness changes from 11mm to 12mm.

Collection-derived values are single ValueCells containing the whole collection. Per-element decomposition creates node-per-element scaling problems; whole-collection recomputation is typically cheap.

#### ConstraintNode -- predicate evaluation

```
ConstraintNode(constraint_instance_id) -> (Satisfaction, Diagnostics)
```

Where `Satisfaction` is `satisfied | violated | indeterminate | inapplicable`. The last two handle `undef` inputs.

Granularity: per constraint application. A multi-line constraint body is one node. Quantifiers over collections (`forall hole in bolt_holes: predicate(hole)`) produce one node with edges to all collection elements' relevant ValueCells. Collection membership changes are structural graph modifications handled by the immutable snapshot model.

Structured diagnostics output:

```
ConstraintDiagnostics:
    status: Satisfaction
    predicate_results: Map<PredicateId, (Satisfaction, Detail)>
```

Top-level `Satisfaction` is used for early cutoff; per-predicate results are available for UI rendering.

#### ResolutionNode -- `auto` solving

```
ResolutionNode(scope_id, auto_params: Set<ValueCellId>) -> Map<ValueCellId, Value>
```

Granularity: per scope. Reflects the constraint system's bottom-up resolution strategy. Coupled optimization over a scope's `auto` parameters cannot generally be decomposed per-parameter.

The ResolutionNode's output is a map of resolved values. This output is committed to a new snapshot with `Resolution` provenance (section 2.5), advancing the design from a less-resolved state to a more-resolved state. This is a state transition, not a within-state dependency -- the ResolutionNode does not appear as a dependency of the ValueCells it resolves within any single graph state.

Internal decomposition: the ResolutionNode analyzes the constraint graph to identify connected components; uncoupled subsets become independent sub-problems solved concurrently. The graph layer sees one ResolutionNode per scope.

What might appear as a dependency cycle (`ValueCell -> ConstraintNode -> ResolutionNode -> ValueCell`) is actually a helix across states: the unresolved ValueCells at the input are in state S_n; the resolved ValueCells at the output are in state S_{n+1}. The dependency graph within any single state is a strict DAG. The additional dimension is monotonic state progression -- determinacy only increases. See section 2.5.

#### RealizationNode -- producing representations

```
RealizationNode(entity_id, repr_kind, tolerance) -> Representation
```

Granularity: per (entity, representation_kind, tolerance_level). The same entity might need both a B-rep and a mesh, or a coarse mesh for visualisation and a fine mesh for FEA.

Containment-tree decomposition: sub-structures have their own RealizationNodes. A parent RealizationNode composes sub-realizations (union, positioning) rather than rebuilding from primitives. Changing a sub-structure parameter invalidates only that sub-structure's RealizationNode and the parent's composition node; other sub-structures are cache hits.

Operations within a single entity are evaluated monolithically within that entity's RealizationNode. Warm starting handles internal incrementality. If monolithic evaluation bottlenecks, decomposition into sub-structures is the correct response.

#### ComputeNode -- expensive computations

```
ComputeNode(computation_id) -> ComputationResults
```

Covers: FEA, CFD, toolpath generation, lattice infill, surrogate model training, format export, etc. Granularity: per computation invocation. One FEA run = one node. Fine-grained incrementality belongs inside the node's warm-start implementation. Results may include fields (stress tensor), scalars (safety factor), collections (critical points), or any typed value.

#### SchemaNode -- topology production

```
SchemaNode(scope_id) -> SchemaFragment
```

Introduced by the structural graph changes design. A SchemaNode produces a `SchemaFragment` describing the topology of its scope:

```
SchemaFragment:
    scope_id: ScopeId
    nodes: Set<NodeDeclaration>
    edges: Set<(NodeId, NodeId, EdgeKind)>
    child_schemas: Map<OccurrenceId, SchemaFragmentRef>
    structure_version: ContentHash
```

Each `NodeDeclaration` carries the information needed to create a node: type, node identity, edge connectivity, and references to the SourceNodes that define its computation. The computation definition itself lives in SourceNodes, not in the NodeDeclaration -- this is what allows non-structural source edits (changing a constraint body, adjusting a default value) to bypass SchemaNode re-elaboration entirely.

Schema nodes compose via the containment tree: an assembly's schema = own nodes + child occurrence schemas. Changing a guard deep in the hierarchy only re-elaborates that scope and ancestors' composition; sibling subtrees are cache hits.

SchemaNode inputs:
1. Structure-controlling SourceNodes: guard expressions, sub declarations, collection definitions (via edge #1).
2. Structure-controlling ValueCells: resolved guard booleans, collection sizes, recursion depths, variant discriminants (via edge #7).
3. Child SchemaNodes: elaborated schemas of child occurrences (via edge #13).

**Non-structure-controlling SourceNodes feed evaluation nodes directly** (via edge #2), bypassing the SchemaNode. Changing a constraint body or parameter default does not dirty the SchemaNode. **Non-structure-controlling parameter values are not inputs to elaboration.** Changing `bracket.thickness` does not dirty the SchemaNode.

#### SourceNode -- compiler-to-graph boundary

```
SourceNode(ast_path) -> ASTFragment
```

The interface between the compiler and the evaluation graph. Each SourceNode holds a content-addressable AST subtree representing a single unit of source definition: a parameter default expression, a let binding body, a constraint predicate, a guard expression, a geometry operation sequence, etc.

Granularity: per declaration or finer. Different parts of a declaration may be separate SourceNodes when they feed different evaluation nodes. For example, a `sub` declaration's `where` guard is a structure-controlling SourceNode (feeds the SchemaNode via edge #1), while its parameter assignments are non-structure-controlling SourceNodes (feed ValueCells directly via edge #2). A bare `param` declaration with no default has no SourceNode -> ValueCell edge; its ValueCell exists in the schema but its value comes from constraints, resolution, user edit, or remains `undef`.

SourceNode evaluation is trivial: look up the current AST subtree and compute its content hash. The compiler populates SourceNodes when source text changes; the evaluation graph's normal invalidation machinery handles the rest. Incremental parsing produces incremental SourceNode updates; content-hash early cutoff means whitespace-only or comment-only changes are free.

The SourceNode is what makes source-change invalidation uniform with parameter-change invalidation. Both flow through the same dependency edges, dirty/demand cones, and cache verification. No special-case handling of "source changed" is needed.

### 2.2 Edge Types (13 Kinds)

Thirteen dependency edge types connect the seven node types. Every edge in this table is a dependency edge: it participates in pull-based evaluation, dirty/demand cone computation, and DAG cycle detection.

| # | From -> To | Meaning | Example |
|---|---|---|---|
| 1 | SourceNode -> SchemaNode | Structure-controlling source feeds elaboration | Guard expression `where needs_cooling` feeds Housing's SchemaNode |
| 2 | SourceNode -> evaluation node | Computation definition feeds evaluation | Constraint body feeds its ConstraintNode; default expression feeds ValueCell |
| 3 | ValueCell -> ValueCell | `let` value reads other values | `volume` reads `thickness`, `width`, `height` |
| 4 | ValueCell -> ConstraintNode | Constraint reads parameter value | `thickness > 2mm` reads `thickness` |
| 5 | ValueCell -> RealizationNode | Geometry parameterised by value | Box dimensions, fillet radius |
| 6 | ValueCell -> ComputeNode | Computation reads parameter values | FEA reads `material.youngs_modulus` |
| 7 | ValueCell -> SchemaNode | Structure-controlling value feeds elaboration | `needs_cooling` boolean feeds Housing's SchemaNode |
| 8 | ConstraintNode -> ResolutionNode | Solver reads constraint landscape | `auto` resolution reads constraints |
| 9 | RealizationNode -> ConstraintNode | DFM constraint reads geometry | Wall thickness check needs solid |
| 10 | RealizationNode -> ComputeNode | Computation needs representation | Stress analysis reads mesh |
| 11 | RealizationNode -> RealizationNode | Parent composes sub-realizations | Assembly reads sub-structures |
| 12 | ComputeNode -> ValueCell | Computation result populates value | FEA safety factor feeds field value |
| 13 | SchemaNode -> SchemaNode | Child schema composes into parent | Vent's SchemaNode feeds Housing's SchemaNode |

In edge #2, "evaluation node" means any of ValueCell, ConstraintNode, RealizationNode, or ComputeNode -- whichever node the source definition feeds. The meaning is uniform: the AST subtree defining a node's computation is an explicit dependency of that node.

The dependency graph is a DAG. There is no cycle caveat. The relationship between ResolutionNodes and the ValueCells they resolve is a state transition, not a within-state dependency, and does not appear in this table. See section 2.5.

`ComputeNode -> ConstraintNode` is deliberately absent. The canonical pattern routes through an intermediate ValueCell: the ComputeNode result populates a ValueCell (edge #12), which the ConstraintNode reads (edge #4). The intermediate ValueCell provides an early-cutoff opportunity -- if the ComputeNode recomputes but produces the same result, the ConstraintNode is not re-evaluated.

### 2.3 Immutable Snapshots and Persistent Data Structures

#### Snapshot model (with schema separation)

```
Snapshot:
    version: VersionId              // monotonic, globally unique
    schema: SchemaRef               // topology (subsumes edges)
    values: PersistentMap<ValueCellId, (Value, DeterminacyState)>  // state
    provenance: SnapshotProvenance
```

The `edges` field from the original v0.1 design is subsumed by the schema. Dependency edges are part of `SchemaFragment`, not the value map.

#### HAMT structural sharing

Snapshots share structure via persistent data structures (hash array mapped tries -- HAMT). Creating a new snapshot with one changed parameter copies O(log n) trie nodes. A design with 50,000 ValueCells shares >99.9% of memory between consecutive snapshots.

#### Provenance variants

Snapshots carry lightweight provenance:

```
SnapshotProvenance:
    | Edit { changed: Set<ValueCellId>, parent: SnapshotId }
    | Restructure { new_schema: SchemaRef, parent: SnapshotId }
    | Merge { sources: List<SnapshotId>, resolution: ConflictResolution }
    | Import { source: ExternalSource }
    | Resolution { scope: ScopeId, resolved: Set<ValueCellId>, parent: SnapshotId }
```

`Edit` provenance gives changed cells at O(1). `Restructure` records topology changes. Fallback for computing diffs: structural HAMT diffing at O(k log n) where k is the number of changes.

#### Graph representation

Forward dependency edges: embedded references within nodes. A node IS a handle for its dependency subgraph. When a node's dependency changes, a new node is created pointing to the updated dependency, sharing all others. Cascade is O(depth) along the spine; siblings are shared.

Reverse dependency edges: mutable side-index (derived, reconstructible acceleration structure). Safe failure mode: stale index leads to conservative dirty cone (wasted recomputation, not incorrect results). Upgrade path: replace with second immutable persistent data structure mirroring the forward graph with reversed edges, updated atomically.

#### Garbage collection

Snapshots stay alive as long as referenced. Working set is small: current snapshot, 1-2 previous ones held by in-flight evaluations, current solver's trial snapshot. Cache eviction: LRU weighted by recomputation cost (expensive results like B-rep Booleans get higher weights). Cache capacity is configurable.

### 2.4 Evaluation Contexts for Resolution

ResolutionNodes explore trial values via child snapshots branching from the current state:

```
resolve(base_snapshot, scope_id, auto_params) -> Snapshot:
    trial_snapshot = base_snapshot
        .with(param_a, (trial_value_a, Determined))
        .with(param_b, (trial_value_b, Determined))
    constraint_results = evaluate_constraints(trial_snapshot, scope_id)
    if converged:
        return trial_snapshot
    else:
        next_trial = adjust(trial_snapshot, constraint_results)
        return resolve(next_trial, scope_id, auto_params)
```

Benefits of trial snapshots:
- Same infrastructure (dependency tracking, caching, scheduling) for trial exploration.
- Natural transactionality -- tentative values isolated from the main state.
- Cross-iteration incrementality -- content-hash cache hits across iterations.
- Cache promotion -- converged trial results valid at final values without explicit promotion.
- Recursive nesting -- resolving scope X may first resolve child scope Y.

Concurrency from immutability: multiple readers with zero coordination; concurrent solvers explore trial snapshots without interference; speculative evaluation proceeds against trial snapshots and wrong speculation is simply unused.

The structure of how per-scope resolutions compose across the containment tree -- the resolution tree -- is described in section 2.5.

### 2.5 Resolution as State Progression

The ResolutionNode's relationship to ValueCells is not a dependency within a single graph state but a transition between states. This section describes the structure of that transition.

#### The resolution helix

Within any single snapshot, the dependency graph is a strict DAG. The path `ValueCell -> ConstraintNode -> ResolutionNode` is a dependency chain: the ResolutionNode reads constraints, which read ValueCells. The ResolutionNode's output (resolved values) appears not in the same snapshot but in a *new* snapshot with `Resolution` provenance. What might look like a cycle when projected flat is actually a helix: the input ValueCells are in state S_n (determinacy: `auto`); the output ValueCells are in state S_{n+1} (determinacy: `determined`). The additional dimension is monotonic -- determinacy only increases (`undef -> constrained -> auto -> determined`), guaranteeing well-foundedness.

#### The resolution tree

Multiple scopes may have `auto` parameters requiring resolution. The ordering of resolution transitions follows the containment tree of auto resolution scopes:

- Leaf scopes resolve first (their auto parameters have no structural dependencies on child scopes).
- Parent scopes resolve after all children (child results are inputs to parent constraints).
- Sibling scopes are independent and may resolve concurrently.

The dependency structure of resolution events is a tree isomorphic to the containment tree restricted to scopes with auto parameters. Each node in this tree represents a state transition: one scope's auto parameters advancing from unresolved to determined.

```
S0 --[resolve LeafA]--> S1 --[resolve LeafB]--> S2 --[resolve Parent]--> S3
```

The resolution tree may be lazily discovered. When a parent has a structure-controlling auto parameter (e.g., `vent_count = auto`), child scopes do not exist until that parameter is resolved. The two-pass schema elaboration (section 6.6) handles this: partial elaboration creates the resolution problem for structure-controlling autos, resolution determines their values, full elaboration creates child scopes, and bottom-up resolution proceeds on the now-known subtree.

#### Relationship to evaluation contexts

Each resolution transition uses the trial snapshot mechanism described in section 2.4. The ResolutionNode explores trial values within child snapshots, converges, and the converged snapshot becomes the next state. The sequence of converged snapshots along the resolution tree is the state progression of the design from underdetermined to determined.

---

## 3. Evaluation Strategy

### 3.1 Pull-Based with Content-Hash Verification

Demand-driven: a node's result is computed when requested, checked against the content-hash cache, and recomputed only on miss.

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

**Cache keying: content-based with version fast path.** Fast path: if the current snapshot version matches `basis_version` in the cache entry, return immediately (no hashing). On version mismatch, compute the content hash of inputs. Content hashes are computed lazily (only on version miss).

**Evaluation ordering: recursive descent with concurrent fan-out.**

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

Depth-first for linear chains; breadth-first parallelism at fan-out points. The async runtime's work-stealing scheduler handles thread utilisation.

### 3.2 Demand Registry and Always-Demanded Nodes

```
DemandRegistry:
    always_demanded: Set<NodeId>
    demand_cone: Set<NodeId>    // backward transitive closure, cached
```

Registered as always-demanded:
- RealizationNodes for structures visible in the 3D viewport.
- ConstraintNodes shown in the constraint panel.
- ValueCells feeding the property editor.
- Any node feeding live diagnostic indicators.

The demand cone is maintained incrementally.

### 3.3 Two-Cone Scheduling Model

On creation of a new snapshot:

1. **Dirty cone** (forward from changed cells): walk dependency edges forward via the reverse dependency index. Conservative superset.
2. **Demand cone** (backward from always-demanded): pre-computed set of everything the live UI needs.
3. **Intersection**: nodes both potentially dirty and needed by the UI -- scheduled immediately at highest priority.

Priority scheduling:

| Priority | Criterion | Scheduling |
|---|---|---|
| **P0 -- Interactive** | ValueCell reads for property editor, keystroke echo | Synchronous or near-synchronous |
| **P1-fast** | In dirty-demand intersection, cheap (scalars, constraints) | Dispatched immediately, expected sub-frame |
| **P1-slow** | In dirty-demand intersection, expensive (realizations, resolutions) | Dispatched immediately, async with progress |
| **P3 -- Speculative** | In dirty cone but NOT demand cone | Background, preemptible |

Work outside both cones is not computed.

### 3.4 Dependency Tracking (Dynamic with Static Optimization)

Dependencies are discovered via **dynamic trace recording** (the Adapton model). Every ValueCell read during evaluation is recorded. This is necessary because the language has data-dependent reads (conditionals, quantifiers, field compositions with conditional sampling).

**Verification** replays the dependency trace: check each recorded read against the current snapshot. If all match, the result is valid. If any differs, re-evaluate from scratch.

**Static optimization:** for statically-known dependencies (simple arithmetic, constraints over explicit parameters), static analysis pre-computes the dependency set, avoiding trace overhead. Dynamic tracking is used only for conditionals, quantifiers, and data-dependent reads.

### 3.5 Early Cutoff

After computing a node's result, compare to the previous result. If equal, downstream dependents are unaffected.

Equality determination by node type:

| Node type | Equality check | Cost |
|---|---|---|
| SourceNode | Content hash of AST subtree | Trivial |
| ValueCell (scalar) | Bitwise value equality | Trivial |
| ValueCell (geometric spec) | Content hash of expression tree | Cheap |
| ConstraintNode | Satisfaction status equality | Cheap |
| ResolutionNode | Resolved value set equality | Cheap |
| RealizationNode | Not checked -- input-hash match used instead | N/A |
| ComputeNode | Domain-specific, typically result hash | Varies |
| SchemaNode | `structure_version` content hash | Cheap |

**Bitwise equality for scalar values.** Tolerance-based comparison risks hiding genuine small changes. False positives from floating-point non-determinism trigger cheap downstream recomputation. Correctness over premature optimization.

**RealizationNodes skip output equality checking.** B-rep/mesh comparison is expensive and ill-defined. Content-hash cache keying on inputs provides cutoff instead.

---

## 4. Warm Starting

### 4.1 The WarmStartable Protocol

Cached results are not just outputs but inputs to the next computation of the same node. Many expensive computations converge dramatically faster with a good starting point (Newton's method: 1-3 iterations vs 20+; B-rep incremental update: frames-per-second vs minutes).

```
trait WarmStartable:
    type State      // opaque internal state (OCCT model, mesh, solver state, etc.)
    fn compute_cold(inputs) -> (Result, State)
    fn compute_warm(inputs, previous_state, input_diff) -> (Result, State)
```

Warm state is explicitly **NOT** part of the content-addressed cache. Not hashed, not compared, not used for validity checks. A performance hint only. If absent (evicted, first evaluation, remote worker), fall back to cold computation with identical semantic results.

### 4.2 Input Diffs

The dependency trace records inputs and values. On cache miss, the system computes an input diff (which traced values differ). The diff is passed to `compute_warm` to scope the incremental update.

Example: a RealizationNode wrapping OCCT receives input diff "fillet radius changed from 2mm to 3mm." OCCT locates the fillet feature, updates the radius, and incrementally rebuilds -- dramatically faster than cold recomputation.

### 4.3 Warm-State Encapsulation and Pools

Internal mutable state is entirely encapsulated. From the graph's perspective, evaluation is pure. Same relationship as the `@optimized` hook.

```
NodeCache:
    result: CachedResult              // immutable, content-hashable
    freshness: Freshness              // Final | Intermediate | Pending | Failed
    dependency_trace: DependencyTrace // immutable
    warm_state: Option<OpaqueState>   // opaque, mutable, not content-addressed
    basis_version: VersionId          // snapshot this result was computed against
```

Warm-state pools: small pool (initially size 1, expandable). Supports checkout, return, clone-then-modify (when kernel supports it, e.g. OCCT shape copying). Interface generalizes from mutex (pool size 1) to pool (size N) without changing evaluation code.

### 4.4 Tiers

Three tiers, with v0.1 implementing only the first:

| Tier | Description | Scope |
|---|---|---|
| **Tier 1 (v0.1)** | Same node, previous result | The common case covering the interactive editing loop |
| **Tier 2 (future)** | Same node, closest parameter set seen | Multiple cached states per node, indexed by input values. Valuable for parameter-space exploration |
| **Tier 3 (future)** | Any node of same type, closest parameters | Type-level cache: all HexBolt instances share a pool |

The protocol supports tiers 2 and 3 without modification.

---

## 5. Change Detection and Incrementality

### 5.1 Reverse Dependency Index

```
ReverseDependencyIndex:
    Map<NodeId, Set<NodeId>>   // "which nodes read this node's output?"
```

Maintained incrementally: entries added on evaluation/caching, removed on eviction. Derived from the cache and reconstructible at any time. A stale index merely makes the dirty cone slightly more conservative (harmless). Any node that appears on the left side of a dependency edge (section 2.2) gets entries in the index.

Dirty cone computation:

```
dirty = reverse_index[changed_nodes]
for node in dirty:
    dirty |= reverse_index[node]
```

Cost is proportional to cone size, not full graph size.

Dynamic dependencies and the dirty cone: the reverse index reflects cached evaluations. When a conditional branch flips, actual dependencies may differ from the cached trace. The dirty cone is sound but potentially incomplete. Nodes that *were* dependent are in the dirty cone (correct). Nodes that *become* dependent are discovered during re-evaluation via fresh demand-driven pull. The demand cone backstops the dirty cone.

### 5.2 Non-Monotonic Edits and Content-Hash Cache

The content-hash cache naturally handles "change and change back." If thickness goes 5mm -> 6mm -> 5mm, the original cached results are found by content-hash match. No special undo mechanism required.

### 5.3 Incrementality Granularity Summary Table

| Node type | Granularity | Primary incrementality | Rationale |
|---|---|---|---|
| SourceNode | Per declaration or finer | Early cutoff via content hash | Trivial evaluation; changes only when source text changes |
| ValueCell | Per parameter/`let` | Early cutoff via value comparison | Finest grain; trivial recomputation |
| ConstraintNode | Per constraint application | Full re-eval with diagnostic diff | Semantic unit |
| ResolutionNode | Per scope, internal decomposition | Warm starting (previous solution as initial guess) | Coupled optimization |
| RealizationNode | Per entity per (repr_kind, tolerance) | Warm starting + sub-entity cache reuse | Containment tree provides decomposition |
| ComputeNode | Per computation invocation | Warm starting | Monolithic; internal incrementality solver-specific |
| SchemaNode | Per scope | Early cutoff on `structure_version` hash | Topology rarely changes |

---

## 6. Structural Graph Changes

### 6.1 Sources of Structural Change

Five sources, all handled by the same unified dataflow mechanism:

| Source | Trigger | Example |
|---|---|---|
| Guard flip | `where` clause boolean changes truth value | `sub fan_mount : FanMount where needs_cooling` -- `needs_cooling` becomes `true` |
| Recursive depth change | Recursion-controlling parameter changes | `TreeBracket { depth = 5 }` -> `depth = 3` |
| Collection size change | `count` constraint on `List<Structure>` changes | `sub vents : List<Vent>` with `constraint vents.count == vent_count` -- `vent_count` 4 -> 3 |
| Purpose activation/deactivation | Purpose toggled, adding/removing scoped constraints | `manufacturing_ready(bracket)` activated |
| Auto resolution | `auto` parameter feeding structure-controlling input resolved | `vent_count = auto` resolved to 4 |

All five are instances of the same event: a structure-controlling value changed, causing a SchemaNode to re-evaluate and emit a different topology.

The distinction between "parametric change" and "structural change" is **not a distinction in mechanism**. Both are dataflow: a parameter changes, its dependents re-evaluate, and if one of those dependents is a SchemaNode, the re-evaluation may yield a different topology. The existing evaluation machinery handles this uniformly. No separate phases or orchestration are introduced.

### 6.2 Schema Nodes and SchemaFragment

The evaluation graph manages two orthogonal concerns:
- **Schema** -- what nodes and edges exist (topology).
- **State** -- what values populate those nodes.

These are different kinds of nodes in a single dataflow graph. Pipeline:

```
Source AST + structure-controlling values -> SchemaNodes -> Value/Constraint/Realization nodes -> evaluated state
```

Caching key: `(source_ast_hash, structure_controlling_values_hash)`. Source AST is usually stable, so misses almost always come from structure-controlling value changes (typically a small set of booleans and counts).

Early cutoff on schema output: if elaboration re-runs but produces the same `SchemaFragment` (e.g., guard `x > 5` when x changes from 10 to 12 -- still true), the `structure_version` hash matches and nothing downstream re-evaluates.

### 6.3 Schema Change Propagation and Reconciliation

When a SchemaNode produces a genuinely different `SchemaFragment` (early cutoff does not fire), downstream propagation includes **graph mutation** -- creating/removing nodes. This is the one additional propagation mode:

| Category | Action |
|---|---|
| **New nodes** | Create NodeCache entries. Check warm-state pool for applicable warm state (keyed by node type + relevant input signature). Enter dirty set. |
| **Removed nodes** | Cached results orphaned in content-addressed cache (age out via LRU). Warm state donated to warm-state pool. Demand registrations cleaned up. |
| **Surviving nodes with changed edges** | Input hash changes; enter dirty set for normal re-evaluation. |
| **Surviving nodes with unchanged edges** | Unaffected; cache hit. |
| **Reverse dependency index** | Incrementally updated. O(changed edges), not O(total edges). |

### 6.4 Node Identity

**Path-based identity.** Nodes are identified by their path in the containment tree plus role: `assembly.bracket.thickness` is the same ValueCell regardless of schema version. If a guard flips an occurrence off and back on, "new" nodes have the same identity as old ones and find their old cached results and warm state. Cache reuse across structural toggles is trivial -- same identity, same cache key, cache hit.

**Collections use positional indexing for v0.1** (`vents[0]`, `vents[1]`, ...). Shrinking removes from the end. Keyed collection identity (e.g., angular position for bolt patterns) is deferred to post-v0.1.

### 6.5 Stratification of Structure-Controlling Values

Structure-controlling values must be resolvable without reference to nodes whose existence they control. The shape of a structure cannot depend on nodes that only exist if the shape is already known.

This falls out of the DAG property. If a structure-controlling value depended on a node whose existence it controls, the dependency graph would contain a cycle -- statically detectable, reported as an error. **No separate enforcement rule is needed.**

Example of the designer's natural fix for a stratification cycle:

Naive (cyclic):
```
constraint total_airflow(vents) >= required_airflow(fan_mount) where needs_cooling
```
where `vents` is `sub vents : List<Vent>` and `vent_count = auto`. Cycle: `vent_count` determines instances, but `total_airflow` needs instances, and the airflow constraint resolves `vent_count`.

Fixed (acyclic):
```
constraint per_vent_airflow(Vent) * vent_count >= required_airflow(fan_mount) where needs_cooling
```
Depends on type-level property and count parameter, not instances.

### 6.6 Partial Elaboration and Progressive Schemas

When a structure-controlling parameter is `auto` and unresolved, the SchemaNode cannot fully elaborate.

**Two-pass schema elaboration (partial -> resolution -> full).** The SchemaNode emits an `Intermediate` result. The partial `SchemaFragment` contains enough to set up the resolution problem (ResolutionNode, bounding constraints, non-structure-dependent nodes) but leaves the structure-dependent portion unelaborated. When the ResolutionNode resolves `auto`, the structure-controlling value becomes determined, the SchemaNode re-evaluates, and produces a `Final` SchemaFragment with the full topology.

This maps directly onto the existing intermediate/final freshness distinction. No new machinery is required.

Schema nodes compose bottom-up via the containment tree: child schemas are dependencies of parent schemas. Pull-based evaluation naturally evaluates children before parents. Independent sibling schemas elaborate in parallel.

---

## 7. Provisional State and Long-Running Tasks

### 7.1 Result Freshness (4 Variants)

Every cached result carries a Freshness marker (four variants, per design review resolution 4.1):

```
Freshness:
    | Final                                    // committed, fully evaluated
    | Intermediate { generation: u64 }         // still refining; generation monotonically increases
    | Pending { last_substantive: ResultRef }   // gated; not recalculated, showing previous best
    | Failed { error: ErrorRef }               // computation failure (see section 9)
```

`Final`, `Intermediate`, `Pending`, and `Failed` results all live in the same cache infrastructure. Content-hash keying, warm starting, dependency traces, and early cutoff all work on non-`Final` results without modification.

### 7.2 Intermediate Flag Propagation

A node's output is intermediate if any of its inputs are intermediate OR if the node itself is still refining:

```
output.freshness = if self.still_refining:
    Intermediate { generation }
elif any(input.freshness != Final for input in inputs):
    Intermediate { generation }
else:
    Final
```

When the last upstream intermediate becomes Final, downstream nodes re-evaluate (input hash changes). If the downstream node is not itself progressive, output becomes Final. This is normal cache invalidation.

**Eager evaluation of intermediates with cost-aware gating:**
- Downstream nodes eagerly consume intermediate upstream results, but at **lower priority** than otherwise-equal-priority tasks based on final inputs.
- **Gating policy:** Runtime balances estimated cost of re-evaluation against value of the updated intermediate. When idle local resources are available, intermediate-driven evaluations proceed. When resource-constrained, emit `Pending` result.
- **Pending as propagation gate:** Pending retains the most recent substantive result (for UI display) but does NOT trigger downstream re-evaluation. Naturally quiets the downstream subtree without explicit "pause propagation."
- **Content-hash significance filter:** Early cutoff is a free significance filter. If FEA iterations N and N+1 produce near-identical fields, content hashes match and downstream is not re-evaluated.

### 7.3 Task Commitment Policy

A committed task runs to completion against its original snapshot regardless of subsequent edits. Commitment policy is project configuration, not source code logic. Two configurable thresholds:

| Threshold | Default | Semantics |
|---|---|---|
| `always_commit_after` | 120 seconds | Any task running longer is committed unconditionally |
| `commit_when_proportion_done` | 0.5 | Task estimated past this proportion is committed |

Progress estimation: reported progress used directly if available; else estimated as `elapsed_time / previous_runtime_for_this_node`.

Per-node policy overrides via dedicated UI widget:
- **Commit if slow** (default): dual-threshold policy.
- **Always cancel when stale**: never commit; always restart on dirty-cone intersection.
- **Only run on final inputs**: don't evaluate on intermediate upstream results.

Overrides are settable per node instance or per node type.

### 7.4 Staleness Detection

Uses persistent data structure structural sharing: if the subtree providing a node's input dependencies is the same structure (shared trie nodes) in the basis and current snapshot, the result is not stale. If subtrees differ, the result is stale. **No explicit diffing against dependency traces is needed** -- the immutable snapshot infrastructure provides this check for free.

### 7.5 Cancellation Refinement

| Condition | Behaviour |
|---|---|
| Task in dirty cone, NOT committed | Cancelled |
| Task in dirty cone, committed | Runs to completion; result cached with stale `basis_version` |
| User explicitly requests re-evaluation | Force-cancels even committed; restarts at P1-slow |
| User explicitly cancels | Force-cancels; warm state saved |

**No parallel evaluations of the same node.** When a committed task is running stale, a fresh re-evaluation is queued to start when the committed task completes. The re-evaluation inherits warm state. Avoids priority inversion.

Sequence on committed stale completion:
1. Committed task finishes.
2. Result cached with stale `basis_version` and `Intermediate` freshness.
3. Warm state saved.
4. Re-evaluation queued with warm start.
5. Re-evaluation converges quickly.
6. Result becomes `Final` at the current snapshot.

Cooperative cancellation via tokens. Long-running computations check the token at natural breakpoints (between solver iterations, geometric operations). On cancellation, warm state is optionally saved.

| Priority | Cancellation behavior |
|---|---|
| P0, P1-fast | Never cancelled (sub-frame time) |
| P1-slow | Cancelled if dirty cone includes this node; otherwise completes |
| P3 (speculative) | Cancelled immediately when new snapshot arrives |

Resolution and cancellation: a resolver mid-iteration is cancelled only if the edit changes a parameter within the resolver's scope.

### 7.6 Node Traits

Nodes carry declarative traits informing the scheduler and UI. Compose orthogonally with the existing priority system.

| Trait | Semantics |
|---|---|
| `immediate` | Not cancellable; expected sub-frame. May be evaluated inline. Corresponds to P0/P1-fast. |
| `warm_startable` | Implements the `WarmStartable` interface. Scheduler preserves warm state. |
| `progressive` | Emits intermediate results over time. Scheduler expects multiple cache updates. |
| `committable` | Subject to commitment policy. Absent this trait, always cancellable. |

Traits are composable. Example: an FEA solver node might be `warm_startable + progressive + committable`.

Traits inform priority assignment but do not replace it. Traits are static declarations on the node type; priority is a dynamic scheduling-time assignment.

---

## 8. Realization Events and Diagnostics

### 8.1 Event Model (Separation of State and History)

The design separates **what is** (cached results in the evaluation graph) from **what happened** (realization events). Diagnostics, errors, progress, convergence metrics, and lifecycle transitions are all realization events. They reference nodes and states but do not live in the graph.

### 8.2 Event Kinds and Payloads

```
RealizationEvent:
    timestamp: Instant
    node_id: NodeId
    snapshot_version: VersionId
    kind: EventKind
    payload: EventPayload            // structured, kind-specific
    references: Vec<NodeId | StateRef>
```

Event kinds: `diagnostic`, `progress`, `error`, `intermediate_emitted`, `completed`, `cancelled`, `commitment_acquired`, `staleness_detected`.

Events are append-only, indexed by both timestamp and `node_id`. Two query patterns:
- **Journal** (temporal view): "What happened in the last 30 seconds?"
- **Structural view** (by node_id): "What diagnostics has this constraint emitted?"

### 8.3 Constraint Diagnostics

A ConstraintNode evaluation produces:
1. A cached result (Satisfaction status + structured per-predicate diagnostics).
2. Realization events for violations, indeterminate results, tolerance warnings, etc.

Result = state; events = history. The UI reads the current cached result for display and subscribes to the event journal for notifications.

### 8.4 Determinacy Stack Traces (On-Demand Backward Walk)

The ontology's determinacy "stack trace" maps to a backward walk through dependency edges from an `undef`/`indeterminate` result to its root cause.

**Computed on-demand, not precomputed.** Users trigger this rarely. Dependency traces provide all needed information. Implementation is a straightforward graph walk.

### 8.5 Diagnostic Aggregation via Journal Queries

Aggregation is via event journal queries, not graph nodes. Querying all violation events against the current snapshot version, grouped by entity/severity/constraint type, provides a summary view. No aggregation node is needed. Consistent with the principle that the evaluation graph contains computation, not operational bookkeeping.

Infrastructure-level diagnostics (tolerance warnings from the geometry engine, solver convergence diagnostics, resource-limit warnings) are all realization events with appropriate `EventKind` values. Same routing as other events. No separate warning system needed.

---

## 9. Error Handling Model

### 9.1 Computation Failures as Graph-Level Events

Per design review resolution 4.1: for v0.1, computation failures are evaluation-graph-level events, **NOT** language-level values. There is **no `Result<T, E>` type, no `try`/`catch`, no language-level error propagation.**

When a computation fails:
1. The node's result is marked `Failed` (4th variant in the `Freshness` enum).
2. A realization event with `EventKind::error` is emitted.
3. Downstream nodes become `Pending` with a diagnostic chain.
4. The UI surfaces failures through existing diagnostics.

Out-of-bounds indexing and missing-key lookups are evaluation-graph-level failures under this model.

### 9.2 Failed Freshness Variant

```
Freshness:
    | Final
    | Intermediate { generation: u64 }
    | Pending { last_substantive: ResultRef }
    | Failed { error: ErrorRef }
```

`Failed` is a terminal state for a given evaluation. The node retains its error information. Downstream nodes consuming a `Failed` input propagate the failure as `Pending` with a diagnostic chain pointing to the original failure.

### 9.3 Constraint Violation Continued Evaluation

Constraint violations produce a cached result (`violated`), NOT a halt. Evaluation continues so the designer sees the full consequences. The designer may choose to relax the constraint rather than revert.

Priority reduction: subgraphs downstream of a violated constraint proceed at lower priority. This is a scheduling hint within the existing priority system: conforming P1 work is scheduled before violation-downstream P1 work, implemented as a secondary sort key within the priority level. **No new priority levels are required.**

---

## 10. Geometry Engine Architecture

### 10.1 Source Text as Canonical Geometry

There is no "canonical geometric model" in the implementation. There are only realizations (B-rep, mesh, SDF, voxels), each created because some downstream operation needs one, each cached and invalidated through the evaluation graph. **No realization is privileged. All are contingent on the operations they serve.**

This dissolves several traditional problems:
- "What is the primary representation?" -- No primary; different operations demand different realizations.
- "How do representations stay in sync?" -- They don't need to; they are independent derivations from the source specification. If the source changes, dependent realizations are invalidated and recomputed.
- "When does representation conversion happen?" -- When an operation requires a realization type that doesn't exist or has been invalidated. Demand-driven.

### 10.2 Mathematical Geometric Types (Opaque Handles)

Core geometric entity types are mathematical, not representational. No `BRepSolid` or `MeshSurface` at the language level.

| Type | Definition |
|---|---|
| `Solid` | Closed region of 3D space (not necessarily bounded) |
| `Shell` | Connected set of faces bounding a region |
| `Surface` | 2D manifold in 3D space |
| `Curve` | 1D manifold in 2D/3D space |
| `Point` | 0-dimensional position |
| `PointCloud` | Unordered point collection |

Geometric values are opaque handles. Designers cannot inspect vertices or control points. They work through operations: `union(a, b)`, `fillet(solid, edge, radius)`, `distance(p, surface)`. This is essential for representation independence -- if the language exposed B-rep topology, it would be impossible to back the same type with an SDF kernel.

Geometric property traits: `Closed`, `Manifold`, `Orientable`, `Convex`, `Connected`, `Bounded`, `Watertight` (= `Closed + Manifold`). Note: `Solid` no longer implies `Bounded` (operations like `half_space` produce unbounded solids; operations requiring bounded inputs require the `Bounded` trait explicitly).

### 10.3 Multi-Kernel Implicit Dispatch

The runtime infers which kernel(s) to invoke based on operations required, realizations available, registered kernel capabilities. No language-level annotation in the normal case.

**Dispatch considerations:**
- What operation is needed.
- What realizations are already available.
- What kernels support this operation.
- What downstream operations follow (minimize conversions).
- Tolerance requirements.

**Determinism:** Dispatch is deterministic given fixed runtime configuration. Runtime config (kernel availability, versions, preference ordering) is project-level metadata -- pinned, versioned, reproducible. No randomness, no race-condition choices.

**Inspectability and override:** The dispatch plan is inspectable through the tooling/debugging interface. Override via pragma: `#kernel(occt)`. Pragmas are toolchain directives that don't change program meaning.

**Kernel registration:** Kernels register capabilities with the runtime: supported representations, supported operations, quality/performance characteristics. Registration mechanism deferred to implementation design.

### 10.4 Representation Tolerance (Bidirectional Contract)

Representation tolerance is a bidirectional contract: the maximum acceptable geometric deviation between the mathematical specification and any realization. Upstream: the evaluation graph must produce realizations accurate to this bound. Downstream: consumers may rely on the bound.

Where tolerance lives:
- **Primary:** at the purpose level. A manufacturing purpose carries tight tolerance; an exploration purpose carries loose tolerance.
- **Escape hatch:** at entity level for cases where one region needs tighter control.

Representation tolerance vs. design tolerance: orthogonal. Representation = accuracy of computational model vs. spec. Design = acceptable variation in a physical artifact (GD&T). Must not conflate.

**Tolerance in the evaluation graph:** Tolerance flows as a property of RealizationNodes. The runtime manages the tolerance budget across conversion chains (B-rep -> mesh -> SDF -> voxel accumulates error). The runtime allocates per-step error budgets. This is a runtime heuristic, not language-level. Tolerance budget allocation details are deferred (open question).

**Imported geometry:** The designer declares tolerance on imported geometry as both an assertion and a promise. The runtime cannot verify for external geometry.

### 10.5 Multi-Representation Patterns

**Stack pattern:** A linear chain of representation conversions, each an evaluation graph node:
```
B-rep -> mesh -> FEA stress field -> density field -> implicit lattice -> voxel octree
```
Invalidation propagates through the chain.

**Patchwork pattern:** An assembly with heterogeneous representations -- an SLA part as voxels, fasteners as B-rep, a downloaded STL mesh. Assembly spatial composition is representation-agnostic. Spanning operations (interference check, visualisation) require compatible realizations; the evaluation graph produces them on demand.

### 10.6 Geometry-Field Bidirectionality

Geometry can be represented as a field (SDF = `Field<Point3<Length>, Scalar<Length>>` where the zero-level-set defines the boundary). The relationship is bidirectional:
- A field can define geometry (SDF -> implicit surface).
- Geometry can be sampled as a field (solid -> distance field).

This is a realization concern, not a type system concern. Conversion triggers are demand-driven.

### 10.7 `@optimized` Hook and Kernel Bindings

`@optimized` is a semantic equivalence bridge. A language-level definition exists in terms of language primitives; the `@optimized` annotation registers that a semantically equivalent optimized implementation is available in the runtime.

```
@optimized("geo_kernel::coincidence_solver")
constraint def Coincident(a : Point3<Length>, b : Point3<Length>) {
    distance(a, b) == 0mm
}
```

For standard library operations, `@optimized` annotations live in the kernel binding layer, not in user-facing source. For user-authored geometric operations, `@optimized` is available in source text as an exception.

### 10.8 Geometry Kernel Candidates

The following geometry kernels were evaluated for use within the multi-kernel dispatch architecture:

- **OpenCASCADE (OCCT):** ~2M lines C++, LGPL-2.1. The only production-grade open-source B-rep kernel. Full NURBS, Booleans, fillets, chamfers, healing, STEP/IGES/STL/BREP/OBJ/glTF I/O. Weaknesses: Boolean failures on complex geometry, largely single-threaded, ~2 GB footprint.
- **Truck:** Rust, Apache-2.0. NURBS + B-rep topology + STEP I/O + Booleans. WASM-compilable. Booleans less robust than OCCT; fillets at prototyping stage.
- **CGAL:** C++, GPL/commercial. Gold standard for algorithmic geometry. Exact arithmetic. Triangulations, Voronoi, Nef polyhedra Booleans, mesh generation. Not a CAD kernel (lacks sweep/revolve/NURBS).
- **Manifold:** C++, Apache-2.0. Guaranteed-manifold mesh Booleans, 100--1000x faster than CGAL for Booleans. Default in OpenSCAD.
- **libfive / Fidget:** SDF kernel with feature-preserving meshing. Fidget (Rust successor) achieves 31x speedup via hand-written JIT. Used by nTopology.
- **OpenVDB:** C++, Apache-2.0. Sparse volumetric data structure. Level-set operations. Industry standard for VFX.
- **SolveSpace:** C++, GPL-3.0. Lightweight parametric CAD with constraint solver available as `libslvs`.

**Strategic conclusion:** The most pragmatic near-term strategy is to combine kernels -- OCCT for B-rep, Manifold for mesh Booleans, SolveSpace's solver for constraints. This directly supports the multi-kernel implicit dispatch architecture described in 10.3.

### 10.9 Alternatives Considered

Four alternative approaches to geometry representation dispatch were evaluated and rejected:

1. **Geometry as representation-parameterised type** (`Solid<BRep>`) -- breaks the representation-independence abstraction.
2. **Primary representation with derived others** -- privileges one realization, contradicting the source-as-canonical principle.
3. **Convergent modelling at language level** -- remains an unsolved research problem.
4. **Explicit kernel dispatch via annotations** -- couples design intent to implementation detail. Available as a pragma override (`#kernel(...)`) for exceptional cases, but not the default.

---

## 11. Constraint Engine Architecture

### 11.1 Orchestrator Pattern (Not Monolithic Solver)

The constraint engine dispatches to specialized sub-solvers and manages their interaction. No single solver handles all constraint domains well. The orchestrator pattern is the only tractable architecture.

This orchestrator pattern is expected to be dominant across the entire runtime, not just constraints. The language integrates diverse representations, algorithms, sub-engines, and kernels. Using the right combination requires orchestration at every layer.

### 11.2 Constraint Domains

Four qualitatively different domains:

**Dimensional/parametric constraints** -- numeric relationships between scalar values with units. The majority of constraints in most designs. Standard numeric constraint satisfaction with dimensional analysis type system guaranteeing operand compatibility before the solver sees them.

```
constraint wall_thickness > 2mm
constraint grip_length == plate_a.thickness + plate_b.thickness
```

**Geometric constraints** -- spatial relationships (coincidence, parallelism, tangency) on geometric entities. The `@optimized` hook bridges language-level definitions to kernel-native solvers.

```
constraint Coincident(hole.center, bolt.axis_point)
constraint Parallel(face_a.normal, face_b.normal)
```

**Logical/combinatorial constraints** -- discrete choices, boolean gating, type selection. Involves enumeration, backtracking, or SAT-style reasoning.

```
constraint bolt.head_type == HeadType.Hex or bolt.head_type == HeadType.Socket
constraint load > 10kN implies bolt.grade >= 10.9
```

**Cross-domain constraints** -- span multiple domains in a single predicate. These are first-class and the primary reason the engine must be an orchestrator:

```
constraint def DFM_Milling {
    param part : Structure
    param machine : MillingMachine
    forall feature in part.internal_corners:
        feature.radius >= machine.min_tool_radius
    forall wall in part.walls:
        wall.thickness >= wall.depth / machine.max_wall_aspect_ratio
}
```

The orchestrator's core responsibility: decompose cross-domain constraints into sub-problems, dispatch to sub-solvers, manage feedback between coupled sub-solvers.

### 11.3 Checking -> Solving -> Proposing Spectrum

**Checking:** given a fully determined design, verify all constraints hold. Evaluate every predicate; report violations.

**Solving:** given a partially determined design with `auto` parameters, find values for `auto` parameters satisfying all constraints. Classical constraint satisfaction, potentially nonlinear and mixed continuous-discrete.

**Proposing:** given a highly underdetermined design (early-stage, extensive `undef`), provide useful feedback: what is constrainable, what is in conflict, what would need to be determined to make progress, what are reasonable values.

These form a graceful degradation hierarchy. If the engine cannot optimally solve, it can still check. If it cannot fully propose, it can still solve what is solvable and report what is not.

### 11.4 Optimization as Constraint-Oriented Auto Resolution

Optimization and constraint solving are unified at the language level. Optimization is constraint-oriented resolution of `auto` parameters. `minimize` and `maximize` are syntactic sugar:

```
structure def LightweightBracket : Rigid {
    param thickness : Length = auto
    param material : Material = auto
    constraint thickness >= 2mm
    minimize mass
}
```

The sugar expansion creates an optimization constraint -- a predicate asserting that resolved `auto` values must be such that no feasible alternative achieves a lower/higher value of the merit expression. The `@optimized` hook ensures the implementation dispatches to actual optimization algorithms.

Multi-objective support:
- **Weighted sum** (default): `minimize 0.6 * mass + 0.4 * cost`. Collapses to single-objective.
- **Lexicographic ordering** (explicit extension): "minimize mass; among equal-mass, minimize cost."
- **Pareto exploration** (tooling concern, not language-level).

### 11.5 Scope-Level Objectives and Bottom-Up Resolution

Optimization objectives are scoped to the containing entity. Narrowest scope wins.

Default resolution strategy is **bottom-up**:
1. Resolve `auto` parameters in leaf scopes using local objectives.
2. Treat resolved leaf scopes as fixed (parameters now determined).
3. Resolve `auto` parameters in parent scopes using the parent's objectives, with child results as given.
4. Continue upward to root.

Bottom-up is exact when scopes are uncoupled. It is an approximation when there is coupling (child's locally optimal result is not globally optimal). The implementation detects coupling and surfaces it as a diagnostic. The designer can then broaden the optimization scope.

### 11.6 Strict vs Free Auto

**Strict `auto` (default):** resolution requires the resolved value is well-determined -- either uniquely determined by constraints or uniquely optimal under the applicable objective. If neither holds, strict `auto` is an error. Ties and flat regions use a deterministic tiebreaking rule.

**Free `auto` (`auto(free)`):** explicit opt-in for exploration. Returns a feasible solution and triggers a warning that the result is not uniquely determined. Useful for early-stage exploration.

With the global default objective (centrality/robustness), strict `auto` is well-defined almost everywhere. Fails only with genuine degeneracy.

### 11.7 Solver Technology Landscape

The constraint survey identifies the following solver categories relevant to Reify's orchestrator:

**Geometric constraint solvers:** SolveSpace (`libslvs`) for 3D geometric constraints using Newton-Raphson per group. FreeCAD PlaneGCS for 2D with four solver algorithms (DogLeg, Levenberg-Marquardt, BFGS, SQP) and QR-decomposition Jacobian diagnosis.

**SMT solvers:** Z3 (linear/polynomial arithmetic, NLSAT, optimization module). CVC5 (matches Z3 plus syntax-guided synthesis). dReal (transcendental functions, delta-completeness -- natural tolerance model). MathSAT5/OptiMathSAT (unsatisfiable core extraction).

**Interval constraint propagation:** IBEX (contractor programming, branch-and-prune). Practical for 10-50 variables general, hundreds for structured/sparse problems. Provides guaranteed worst-case bounds for tolerance analysis.

**Numerical optimization:** Ipopt (large-scale NLP, millions of variables when sparse). NLopt (unified interface to dozens of algorithms, AUGLAG meta-algorithm). Ceres Solver (robustified nonlinear least squares, automatic differentiation). CVXPY (disciplined convex programming, polynomial-time guaranteed global minimum).

**Discrete/combinatorial:** Google OR-Tools CP-SAT (CP + SAT clause learning + LP relaxations). Gecode (custom propagator APIs). Chuffed (lazy clause generation). HiGHS (LP/MIP).

**Physics engines:** MuJoCo (interactive fidelity), Drake (mathematical programming physics).

**Equation-based:** ModelingToolkit.jl (acausal composition with Pantelides algorithm for DAE index reduction).

The recommended architecture layers these tools: constraint classification at the language level, dispatch to appropriate solvers per domain, and managed interaction for cross-domain constraints.

---

## 12. Concurrency Model

### 12.1 Task Model and Work-Stealing Thread Pool

Each node evaluation is a task submitted to a shared async runtime (Tokio-style work-stealing thread pool).

```
Task:
    node_id: NodeId
    snapshot: SnapshotRef
    priority: Priority
    warm_state: Option<OpaqueState>
    cancellation_token: CancellationToken
```

CPU-bound tasks run on the compute pool. Blocking tasks (disk I/O, network) run on a separate blocking pool. GPU-dispatched work runs on a dedicated dispatch thread with async completion.

### 12.2 Priority Levels and Promotion

Four priority levels (same as the scheduling model in section 3.3): P0, P1-fast, P1-slow, P3.

Priority promotion: if a P1-slow task depends on an in-flight P3 task, the P3 task is promoted to P1-slow. This falls out naturally from the recursive evaluation model.

### 12.3 Warm-State Concurrency

Managed through the state pool (initially size 1):
- **Exclusive access (v0.1):** mutex per node's warm state.
- **Clone-then-modify (expansion path):** when the kernel supports cloning.
- **Pool of size N (future):** supports tier-2 warm starting.

Interface designed so `checkout -> compute -> return` generalizes without changing evaluation code.

### 12.4 Cancellation Protocol

Cooperative cancellation via tokens. See section 7.5 for the full cancellation refinement table.

Nested async/await within ResolutionNodes: a resolution task awaits sub-evaluations on the same thread pool, suspends at await points. The work-stealing scheduler prevents thread starvation. Expected nesting depth: 2-4 levels. Fallback: sequential bottom-up resolution.

### 12.5 Determinism Guarantees

**Semantic determinism: guaranteed.** Same node + same snapshot = same result. The content-hash cache correctness depends on the assumption that kernel operations are deterministic given identical inputs (design review resolution 4.3).

**Temporal non-determinism: accepted.** The order of UI updates is non-deterministic. Cross-snapshot UI consistency: UI updates are tagged with the snapshot version; the display layer only applies updates from the current snapshot.

**`fn` purity:** `fn` declarations are pure -- no side effects, no state. This is a language-level guarantee. Kernel determinism is an implementation-level assumption.

### 12.6 Distribution Readiness

The task interface is distribution-agnostic:
- Snapshots are serialisable (persistent data structures are trees of values).
- Warm-start state is not assumed available remotely (cold fallback).
- Results are serialisable.
- No distributed locking or cache coherence protocol is required.

Content-addressed immutable snapshots are naturally networkable.

---

## 13. Module Loading and Compilation

### 13.1 File-Module Mapping and DAG Enforcement

Every `.ri` file is exactly one module. A file cannot contain multiple module declarations, and a module cannot span multiple files. The file system is the module system.

Every `.ri` file must begin with a `module` declaration specifying its full path, which must match the file's location in the source tree (enforced by tooling):

```
module std.mechanical.fasteners.bolt
```

This file must be located at `std/mechanical/fasteners/bolt.ri`.

Directories are namespaces, not modules. A directory may contain a `mod.ri` file serving as the directory-level module, curating the directory's public API via re-exports.

The module dependency graph must be a **directed acyclic graph (DAG)**. If module A imports module B (directly or transitively), module B cannot import module A. Circular module dependencies are a compile error and a design smell.

### 13.2 Unit Bootstrap Ordering

The standard library module tree has a strict dependency layering:

```
std.math -> std.units -> std.geometry -> std.ports / std.materials -> std.process -> std.analysis / std.io
```

Lower layers never import from higher layers.

The compiler reads `std.units.si` at a bootstrap stage before parsing user code. Unit literals in user code depend on unit declarations being available during parsing.

The prelude (`std.prelude`) is implicitly imported into every module. Contents include:
- `std.math.numeric` (abs, min, max, clamp, sqrt, etc.)
- `std.math.trig` (sin, cos, tan, etc.)
- `std.math.linalg` (dot, cross, normalize, magnitude)
- `std.units.dimensions` (all named dimension aliases)
- `std.units.si` (all SI units with prefixes)
- `std.units.constants.pi`
- `std.geometry.constructors` (point3, vec3, orient_*, frame3, transform3, project)
- `std.ports.Port`, `std.ports.Directionality`
- `std.determinacy` predicates (`determined()`, `constrained()`, `undetermined()`)

Suppression: `#no_prelude` pragma (needed for defining the prelude itself).

All declarations within a module are mutually visible regardless of textual order (order-independence). Import statements are conventionally placed at the top but are not required to precede declarations that use them.

### 13.3 Prelude Injection

Three properties the prelude must maintain:
1. **Small** -- a designer should be able to memorise its contents.
2. **Stable** -- changes to the prelude break every module. Additions acceptable; removals and semantic changes are not.
3. **Universal** -- everything in the prelude should be useful to a significant majority of modules.

The prelude is the single exception to the no-wildcard-imports rule. The user never writes this import; the compiler inserts it.

---

## 14. Purpose-Driven Activation

### 14.1 Purpose as Scoped Constraint Injection

A purpose is a named, parameterised declaration that is semantically equivalent to a scope containing zero or more `constraint` declarations and/or `Output` occurrence instantiations. Per design review resolution 2.2:

```
purpose manufacturing_ready(bracket: Rigid) {
    constraint AllParamsDetermined(bracket)
    constraint RepresentationWithin(bracket, 1um)
    minimize cost(bracket)
    sub bracket_step : STEPOutput { subject = bracket }
    sub bracket_drawing : DrawingOutput { subject = bracket }
}
```

When activated, its constraints and outputs are present in the evaluation graph. When deactivated, they are absent. This uses the same structural presence/absence mechanism as `where` guards.

**No special scheduling mechanism, demand injection, or purpose-to-nodes mapping is needed.** Purposes compose entirely through existing constraint and evaluation infrastructure.

### 14.2 Activation/Deactivation Mechanics

Activation/deactivation is via implementation-defined UX (GUI toggle, CLI flag, headless always-on). Diagnostics reference the purpose by name. Activating a purpose is equivalent to a structural change (adding constraints and occurrences). Deactivating removes them. Both are handled by the standard schema change propagation mechanism.

### 14.3 Checking/Solving/Proposing Falls Out of Determinacy

A purpose does not need mode selection logic. Input determinacy state determines behavior:

| Input state | Behaviour |
|---|---|
| All inputs determined | Constraint checking |
| Some inputs `auto` | Resolution (solving mode) |
| Many inputs `undef` | Constraints report `indeterminate`; determinacy stack traces available (proposing mode) |

### 14.4 Multiple Simultaneous Purposes

Multiple purposes may be active on different or overlapping parts of the design. Each contributes constraints independently. Conflicting tolerance requirements on the same entity result in separate RealizationNodes (keyed by (entity, repr_kind, tolerance)). A tighter realization might satisfy a looser one -- an optimization opportunity, not a correctness concern.

Heavyweight purposes injecting many constraints and export demands simultaneously are handled by the existing priority system: the two-cone intersection prioritises dirty and demanded work; cost-aware gating throttles expensive work. While actively editing, interactive-priority work dominates; analysis runs at P3 until editing pauses.

### 14.5 Output/Input as Boundary Occurrences

Per design review resolution 1.4, `Export`/`Import` are renamed to `Output`/`Input`.

Output and Input are traits on occurrences -- boundary nodes where the design meets the outside world. Output consumes a structure without producing one in the design domain (produces a file artifact externally). Input produces a structure without consuming one (introduces external geometry).

```
occurrence def STEPOutput : Output {
    param subject : Structure
    param format_version : STEPVersion = AP214
    constraint RepresentationWithin(subject, 1um)
}
```

Output occurrences placed inside purpose definitions specify deliverables. Input occurrences carry provenance (source, tolerance guarantees, import timestamp) providing boundary conditions for the tolerance contract system.

Output and Input are traits on occurrences, not separate entity types. This preserves the four-primitive ontology (Structure, Occurrence, Constraint, Field) while giving import/export full occurrence system participation.

---

## 15. Appendix: Snapshot Data Model

### Snapshot (Final, Post-Structural-Changes)

```
Snapshot:
    version: VersionId              // monotonic, globally unique
    schema: SchemaRef               // topology (subsumes edges)
    values: PersistentMap<ValueCellId, (Value, DeterminacyState)>  // state
    provenance: SnapshotProvenance
```

### SchemaFragment

```
SchemaFragment:
    scope_id: ScopeId
    nodes: Set<NodeDeclaration>
    edges: Set<(NodeId, NodeId, EdgeKind)>
    child_schemas: Map<OccurrenceId, SchemaFragmentRef>
    structure_version: ContentHash
```

### NodeCache (Final, Post-Freshness-Extension)

```
NodeCache:
    result: CachedResult              // immutable, content-hashable
    freshness: Freshness              // Final | Intermediate | Pending | Failed
    dependency_trace: DependencyTrace // immutable
    warm_state: Option<OpaqueState>   // opaque, mutable, not content-addressed
    basis_version: VersionId          // snapshot this result was computed against
```

### Freshness (4 Variants, Per Design Review Resolution 4.1)

```
Freshness:
    | Final
    | Intermediate { generation: u64 }
    | Pending { last_substantive: ResultRef }
    | Failed { error: ErrorRef }
```

### SnapshotProvenance (5 Variants)

```
SnapshotProvenance:
    | Edit { changed: Set<ValueCellId>, parent: SnapshotId }
    | Restructure { new_schema: SchemaRef, parent: SnapshotId }
    | Merge { sources: List<SnapshotId>, resolution: ConflictResolution }
    | Import { source: ExternalSource }
    | Resolution { scope: ScopeId, resolved: Set<ValueCellId>, parent: SnapshotId }
```

### ConstraintDiagnostics

```
ConstraintDiagnostics:
    status: Satisfaction              // satisfied | violated | indeterminate | inapplicable
    predicate_results: Map<PredicateId, (Satisfaction, Detail)>
```

### DemandRegistry

```
DemandRegistry:
    always_demanded: Set<NodeId>
    demand_cone: Set<NodeId>          // backward transitive closure, cached
```

### ReverseDependencyIndex

```
ReverseDependencyIndex:
    Map<NodeId, Set<NodeId>>     // "which nodes read this node's output?"
```

### Task

```
Task:
    node_id: NodeId
    snapshot: SnapshotRef
    priority: Priority                // P0 | P1Fast | P1Slow | P3
    warm_state: Option<OpaqueState>
    cancellation_token: CancellationToken
```

### RealizationEvent

```
RealizationEvent:
    timestamp: Instant
    node_id: NodeId
    snapshot_version: VersionId
    kind: EventKind                   // diagnostic | progress | error | intermediate_emitted
                                      //   | completed | cancelled | commitment_acquired
                                      //   | staleness_detected
    payload: EventPayload             // structured, kind-specific
    references: Vec<NodeId | StateRef>
```

### WarmStartable Protocol

```
trait WarmStartable:
    type State
    fn compute_cold(inputs) -> (Result, State)
    fn compute_warm(inputs, previous_state, input_diff) -> (Result, State)
```

### Node Type Inventory (Consolidated)

| # | Node Type | Signature |
|---|---|---|
| 1 | SourceNode | `(ast_path) -> ASTFragment` |
| 2 | ValueCell | `(entity_id, member_name) -> (Value, DeterminacyState)` |
| 3 | ConstraintNode | `(constraint_instance_id) -> (Satisfaction, Diagnostics)` |
| 4 | ResolutionNode | `(scope_id, auto_params: Set<ValueCellId>) -> Map<ValueCellId, Value>` |
| 5 | RealizationNode | `(entity_id, repr_kind, tolerance) -> Representation` |
| 6 | ComputeNode | `(computation_id) -> ComputationResults` |
| 7 | SchemaNode | `(scope_id) -> SchemaFragment` |

---

## 16. Appendix: Open Questions and Deferred Items

### Open Questions (Carried from All Design Documents)

| # | Item | Status | Priority |
|---|---|---|---|
| 1 | Tolerance budget allocation (error budgets across conversion chains B-rep -> mesh -> SDF -> voxel) | Open | v0.1 |
| 2 | Implementation technology choices (persistent data structure library, async runtime, hashing algorithm, cache backend) | Open | v0.1 |
| 3 | JIT optimization of node graphs (node fusion, scheduling pattern learning, adaptive granularity) | Deferred | Post-v0.1 |
| 4 | Sophisticated cost estimation for gating heuristics (marginal value, convergence rate, monetary cost) | Deferred | Post-v0.1 |
| 5 | Keyed collection identity (upgrade from positional indexing -- angular, spatial, user-defined) | Deferred | v0.2 |
| 6 | ~~Whether SchemaNode is a 6th node type or a specialized ComputeNode~~ | Resolved: SchemaNode is a distinct node type (7th, alongside SourceNode) | -- |
| 7 | Structural diff cost optimization via content-addressed child schemas | Open | Optimization |
| 8 | Kernel registration mechanism and format | Open | v0.1 |
| 9 | `Geometry` supertrait integration with type system | Open | v0.1 |
| 10 | Geometric queries and selectors / persistent naming problem | Open | v0.1 |

### Constraint System Open Questions (C-10.1 through C-10.8)

| # | Item | Description |
|---|---|---|
| C-10.1 | Solver dispatch strategy | How to decompose mixed-domain constraints and select candidate solvers for each sub-problem. |
| C-10.2 | Incrementality | How to handle incremental constraint changes without full re-solving; integration with dependency tracking. |
| C-10.3 | Scalability and partitioning | Exploiting hierarchy for constraint decomposition; scaling to thousands of structures. |
| C-10.4 | Non-convexity and multiple solutions | Handling multiple local optima; confidence in solution quality. |
| C-10.5 | Quantifiers and collection constraints | Supporting `forall`/`exists` over dynamically-sized collections. |
| C-10.6 | Geometric constraint sub-problem | Depth of the orchestrator's geometry understanding; implementing the `@optimized` hook for geometric constraints. |
| C-10.7 | Field-to-geometry bridge | Maintaining SDF/B-rep consistency; constraints that span field and geometric domains. |
| C-10.8 | ML and heuristic integration | Using surrogate models for expensive constraints; validation against formal constraints; trust model for approximate solvers. |

### Implementation Priority Ordering

Four-level priority ordering for constraint system implementation (applies broadly):

1. **Works** -- correct and robust checking and basic solving for small problems.
2. **Good** -- rich diagnostics, progress along the checking → solving → proposing spectrum for small-to-medium problems.
3. **Fast & usable** -- no direct UI lag, minimal response latency, strongly concurrent, useful partial results, ~<5 s complete answers, GPU offload.
4. **Large** -- big problems, robust partitioning, algorithmic cost scaling, graceful degradation, full hardware exploitation.

### Items Deferred to v0.2 or Later

| # | Item | Priority | Notes |
|---|---|---|---|
| 1 | Default robustness objective | v0.1.1 | Mechanism depends on constraint solver internals |
| 2 | Rich structural query/traversal (`children`, `members` pseudo-collection) | v0.2 | |
| 3 | Geometry selector strengthening | v0.2 | |
| 4 | `Result<T>` or `fallback` expressions | v0.2 | |
| 5 | Associated `fn` in traits | v0.2+ | |
| 6 | Data-carrying enums | v0.2+ | v0.1 enums are C-style (no associated data) |
| 7 | Tolerance stack-up analysis (RSS, worst-case, Monte Carlo) | v0.2 | Requires assembly graph + statistical computation |
| 8 | Field-valued material properties | v0.2 | |
| 9 | Warm-start tier 2 (closest parameter set per node) | Future | Protocol supports without modification |
| 10 | Warm-start tier 3 (type-level cache across instances) | Future | Protocol supports without modification |

### Lifecycle Worked Examples

#### Bracket thickness change (5mm -> 6mm)

1. New snapshot created with `Edit` provenance, `changed = {bracket.thickness}`.
2. Dirty cone computed via forward walk from `bracket.thickness` using the reverse dependency index.
3. Intersection with demand cone computed.
4. P1-fast: `bracket.volume` recomputed. Early cutoff check against previous value.
5. P1-fast: `constraint: thickness > 2mm` re-evaluated. `satisfied -> satisfied` = early cutoff; downstream not re-evaluated.
6. P1-slow: `bracket body realization` dispatched with warm start. Input diff: `thickness: 5mm -> 6mm`. OCCT incremental rebuild.
7. P3: speculative re-evaluation of STEP export, FEA mesh.
8. Reverse dependency index updated with fresh traces.

#### Housing guard flip (needs_cooling: false -> true)

1. New snapshot with `Edit` provenance.
2. SchemaNode for Housing is in the dirty cone (inputs include `needs_cooling`).
3. SchemaNode re-evaluates. `needs_cooling` was `false`, now `true`.
4. **First pass (partial):** `Intermediate` SchemaFragment emitted containing fan_mount nodes, cooling constraints, ResolutionNode for `vent_count`, but no vent instances (because `vent_count = auto` is unresolved).
5. ResolutionNode evaluates: analyzes airflow constraints, resolves `vent_count = 4`.
6. **Second pass (full):** SchemaNode re-evaluates with `vent_count = 4` determined. `Final` SchemaFragment includes 4 vent instances (`vents[0]` through `vents[3]`).
7. Schema diff: new nodes for fan_mount and 4 vents created. NodeCache entries initialised. Warm-state pool checked for applicable state.
8. Normal evaluation: vent spacings computed (20mm), geometries realized, `total_mass` updated.

If `needs_cooling` is toggled back to `false` and then to `true` again:
- Off: all cooling nodes disappear; results orphaned in cache; warm state donated to pool.
- On: nodes reappear with same path-based identity. Content-addressed cache still holds results from the first activation -- cache hits across the board. The toggle costs nearly nothing the second time.

---

## Mutability Audit

| Category | Items | Invariant |
|---|---|---|
| **Immutable (load-bearing)** | Snapshots (value maps, schema refs), cached results, dependency traces | Core evaluation correctness |
| **Mutable, encapsulated behind pure interface** | Warm-start state (behind `compute_cold`/`compute_warm`) | Semantic transparency: absent warm state -> cold compute -> identical result |
| **Mutable acceleration structures (derived, reconstructible)** | Reverse dependency index, cache storage (lock-free reads) | Stale = conservative dirty cone = wasted recomputation, not incorrect results |
| **Mutable, outside evaluation model** | Demand registry / demand cone (ephemeral UI state), thread pool / task scheduler | Does not affect evaluation determinism |

**Invariant:** The evaluation model is entirely pure and deterministic. Everything mutable is below the abstraction (warm state), beside the abstraction (scheduling, UI state), or a cache accelerating pure computation.
