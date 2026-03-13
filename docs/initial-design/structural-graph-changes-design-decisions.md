# Structural Graph Changes: Design Decisions

**Status:** Design resolved — ready for implementation prototyping  
**Version:** 0.1 — First crystallization from structural graph changes design sessions  
**Builds on:** `evaluation-graph-design-decisions.md` v0.1, `evaluation-graph-completion-design-decisions.md` v0.2, `name-resolution-and-scoping-design-decisions.md` v0.1, `geometry-engine-design-decisions.md` v0.1

---

## 1. Design approach

The evaluation graph architecture (v0.1, v0.2) established immutable snapshots, five node types, pull-based evaluation, and content-hash caching — but assumed a relatively stable graph topology. Parametric changes (value edits) are the common case and are well-handled. Structural changes — where the set of nodes and edges itself changes — were deferred as an open question (v0.1 §12.5, v0.2 §8.1).

The design resolves structural changes by unifying the entire pipeline from source text to evaluated state in a single dataflow graph, where the graph's topology is itself a computed, cached value flowing through the graph. No separate phases or orchestration machinery are introduced. The existing infrastructure — immutable snapshots, content-hash caching, demand-driven evaluation, warm-state pools — handles structural changes without modification.

**Central insight:** The distinction between "parametric change" and "structural change" is not a distinction in mechanism. Both are dataflow: a parameter changes, its dependents re-evaluate, and if one of those dependents is a schema-producing node, the re-evaluation may yield a different topology. The existing evaluation machinery handles this uniformly.

---

## 2. Sources of structural change

Five sources of structural change are identified, all handled by the same mechanism:

| Source | Trigger | Example |
|---|---|---|
| **Guard flip** | A `where` clause's boolean expression changes truth value | `occurrence fan_mount where needs_cooling` — `needs_cooling` changes from false to true |
| **Recursive depth change** | A recursion-controlling parameter changes | `TreeBracket { depth = 5 }` changed to `depth = 3` |
| **Collection size change** | A collection count parameter changes | `Vent[vent_count]` — `vent_count` changes from 4 to 3 |
| **Purpose activation/deactivation** | A purpose is toggled, adding or removing scoped constraints and associated nodes | `manufacturing_ready(bracket)` activated or deactivated |
| **Auto resolution** | An `auto` parameter that feeds a structure-controlling input is resolved by the constraint solver | `vent_count = auto` resolved to `vent_count = 4` |

All five are instances of the same underlying event: a structure-controlling value changed, causing a schema-producing node to re-evaluate and emit a different topology.

---

## 3. The unified dataflow graph

### 3.1 Two concerns, not three graph types

The evaluation graph manages two orthogonal concerns:

- **Schema** — what nodes and edges exist (topology)
- **State** — what values populate those nodes

These are not separate graph types. They are different kinds of nodes in a single dataflow graph. The pipeline is:

```
Source AST + structure-controlling values → Schema nodes → Value/Constraint/Realization nodes → evaluated state
```

A "Design Graph" is the source AST plus relevant external values. A "Dependency Graph" is the schema with partial value population. A "State Graph" is a fully populated schema. These are stages of evaluation within one graph, not distinct data structures.

### 3.2 Schema nodes

A **Schema node** is a new node type in the evaluation graph. Its computation takes source definitions and structure-controlling parameter values as inputs, and produces a SchemaFragment as output — the concrete set of nodes, edges, and child schemas for a given scope.

```
SchemaNode(scope_id) → SchemaFragment

SchemaFragment:
    scope_id: ScopeId
    nodes: Set<NodeDeclaration>
    edges: Set<(NodeId, NodeId, EdgeKind)>
    child_schemas: Map<OccurrenceId, SchemaFragmentRef>
    structure_version: ContentHash
```

Each `NodeDeclaration` carries enough information to create a node (type, computation definition, static metadata) but no values. Values are computed by the nodes themselves during evaluation.

Schema nodes compose via the containment tree. An assembly's schema is composed of its own nodes plus the schemas of its child occurrences, each of which is itself a schema node's output. Changing a guard deep in the hierarchy only re-elaborates that scope and its ancestors' composition — sibling subtrees are cache hits because their structural inputs haven't changed.

### 3.3 Schema node inputs

A Schema node's inputs are:

- The source AST for the scope being elaborated. Content-addressable — if the source hasn't changed, same hash.
- The resolved values of all structure-controlling parameters within that scope: guard booleans, collection sizes, recursion depths, variant discriminants.
- The elaborated schemas of any trait definitions applied to this scope.

Critically, non-structure-controlling parameters are NOT inputs to elaboration. Changing `bracket.thickness` from 5mm to 6mm does not dirty the Schema node. This keeps elaboration cheap: it only re-runs when the small set of structural controls changes.

### 3.4 Caching key and early cutoff

The natural caching key is `(source_ast_hash, structure_controlling_values_hash)`. In practice, the source AST is usually stable, so elaboration cache misses are almost always caused by structure-controlling value changes — typically a small set of booleans and counts per scope.

**Early cutoff on schema output:** If elaboration re-runs but produces the same SchemaFragment (e.g., a guard expression depends on `x > 5`, x changed from 10 to 12, guard was true and remains true), the `structure_version` hash matches the previous output. Nothing downstream of the Schema node re-evaluates. The structural change was a non-event.

This is the same early-cutoff mechanism that exists for value nodes, applied to the schema itself. No new machinery.

---

## 4. Schema change propagation

### 4.1 Propagation semantics

When a Schema node re-evaluates and produces a genuinely different SchemaFragment (early cutoff does not fire), the downstream propagation includes graph mutation — not just value propagation. This is the one additional propagation mode introduced by this design.

At a Value node, propagation means: re-evaluate the computation, compare output hash, propagate if changed.

At a Schema node, propagation means: re-evaluate the elaboration, diff the output SchemaFragment against the previous one, create/remove/update downstream nodes accordingly, then propagate dirtiness to surviving nodes whose edges changed.

### 4.2 Schema diff and reconciliation

The runtime diffs the old SchemaFragment against the new to determine:

**New nodes:** Create NodeCache entries. Check the warm-state pool for applicable warm state (keyed by node type + relevant input signature). These nodes enter the dirty set — they have no cached results.

**Removed nodes:** Their cached results become orphaned in the content-addressed cache (still present, age out via LRU). Their warm state is donated to the warm-state pool. Demand registrations referencing removed nodes are cleaned up.

**Surviving nodes with changed edges:** If a surviving node gained or lost a dependency (due to a sibling appearing or disappearing), its input hash changes and it enters the dirty set for normal re-evaluation.

**Surviving nodes with unchanged edges:** Unaffected — cache hit.

**Reverse dependency index:** Incrementally updated. Removed edges are deleted; new edges are inserted. Cost is O(changed edges), not O(total edges).

### 4.3 Node identity across schema versions

Nodes are identified by their path in the containment tree plus their role: `assembly.bracket.thickness` is the same ValueCell regardless of schema version.

If a guard flips an occurrence off and back on, the "new" nodes have the same identity as the old ones and can find their old cached results and warm state. Cache reuse across structural toggles is trivial — same identity, same cache key, cache hit.

The warm-state pool serves as a fallback for cases where identity doesn't match (different definition version, different structural context) rather than the primary recovery mechanism.

For collections, v0.1 uses positional indexing: `vents[0]`, `vents[1]`, etc. Shrinking a collection removes from the end. See §8.1 for the upgrade path to keyed collection identity.

---

## 5. Stratification of structure-controlling values

### 5.1 The constraint

Structure-controlling values — guard booleans, collection sizes, recursion depths — must be resolvable without reference to nodes whose existence they control. This is a natural constraint: the shape of a structure cannot depend on nodes that only exist if the shape is already known.

This constraint is not new. The name-resolution design (§8.3) already establishes it for recursive structures: "the recursion-controlling parameter must be resolvable from constraints that don't depend on the recursive structure's internal topology." The structural graph changes design generalises this to all structure-controlling values uniformly.

### 5.2 Stratification falls out of the DAG property

The stratification constraint does not need to be enforced as a separate rule. It is a consequence of the dataflow graph being a DAG. If a structure-controlling value depended on a node whose existence it controls, the dependency graph would contain a cycle, which is statically detectable and reported as an error.

### 5.3 Example: the designer's natural fix

A naive design might write:

```
constraint total_airflow(vents) >= required_airflow(fan_mount) where needs_cooling
```

where `vents` is `Vent[vent_count]` and `vent_count = auto`. This creates a cycle: `vent_count` determines which vent instances exist, but `total_airflow(vents)` requires the instances to exist to compute the total airflow, and the airflow constraint is needed to resolve `vent_count`.

The fix is to express the constraint in terms that don't require the instances:

```
constraint per_vent_airflow(Vent) * vent_count >= required_airflow(fan_mount) where needs_cooling
```

Now the constraint depends on a type-level property (`per_vent_airflow(Vent)`) and the count parameter itself — not on the instances. The cycle is broken and the stratification constraint is satisfied.

---

## 6. Partial elaboration and progressive schemas

### 6.1 The problem

When a structure-controlling parameter is `auto` and not yet resolved, the Schema node cannot fully elaborate. It knows that a guarded entity is structurally present (the guard's inputs are determined and true) but doesn't know the collection size or recursion depth (those depend on a not-yet-resolved `auto`).

### 6.2 Intermediate SchemaFragments

The Schema node emits an Intermediate result (using the Currency model from evaluation-graph-completion §2.2). The partial SchemaFragment contains enough information to set up the resolution problem — the ResolutionNode, the bounding constraints, and the non-structure-dependent nodes — but leaves the structure-dependent portion (e.g., collection instances) unelaborated.

When the ResolutionNode resolves the `auto` parameter, the structure-controlling value becomes determined. The Schema node's inputs change, it re-evaluates, and it produces a Final SchemaFragment with the full topology.

This is a two-pass schema elaboration: partial → resolution → full. It maps directly onto the existing intermediate/final currency distinction. No new machinery is required.

### 6.3 Evaluation order

Schema nodes compose bottom-up via the containment tree: child schemas are dependencies of parent schemas. Pull-based evaluation naturally evaluates children before parents. The walk order is not prescribed — it falls out of the dependency edges and handles the concurrent case correctly. Independent sibling schemas elaborate in parallel.

---

## 7. Constraint violation and continued evaluation

### 7.1 Policy

Constraint violations produce a cached result (`violated`), not a halt. Evaluation continues past violations so that the designer can see the full consequences of a constraint-violating state — they may choose to relax the constraint rather than revert the change.

### 7.2 Priority reduction

Evaluation of subgraphs downstream of a violated constraint proceeds at lower priority than evaluation of conforming subgraphs. This is a scheduling hint within the existing priority system: when the system is resource-constrained, conforming work takes precedence. When idle, violation-consequence evaluation proceeds.

This does not require new priority levels. The existing four-level priority system accommodates it: conforming P1 work is scheduled before violation-downstream P1 work, implemented as a secondary sort key within a priority level.

---

## 8. Open questions

### 8.1 Keyed collection identity

v0.1 uses positional indexing for collection elements. When a collection shrinks, elements are removed from the end. This is correct but may not match the designer's intent — a bolt pattern with angular positions, for example, might more naturally remove a specific bolt than always the last one.

**Upgrade path:** Collection element identity could be managed via traits. An indexing trait on the collection element type would define how elements are identified and which elements are added or removed when the count changes. Positional indexing would be the default trait; angular, spatial, or user-defined indexing would be alternatives. This is deferred to post-v0.1 — positional indexing is simple, correct, and sufficient for initial use.

### 8.2 Schema node as sixth node type

The Schema node is functionally a new node type in the evaluation graph, bringing the total to six (ValueCell, ConstraintNode, ResolutionNode, RealizationNode, ComputeNode, SchemaNode). Alternatively, it could be modelled as a specialised ComputeNode whose output type is SchemaFragment and whose propagation semantics include graph mutation. This is an implementation-level decision that does not affect the design's semantics.

### 8.3 Structural diff cost

The schema diff (old SchemaFragment vs. new) is O(|old| + |new|) in the worst case but can be optimised via structural sharing — if child schemas are content-addressed, unchanged subtrees can be compared by hash in O(1). For typical structural changes (a single guard flip or small count change), the diff is dominated by the local change and its immediate neighbourhood.

---

## 9. Snapshot model refinement

### 9.1 Schema separation

The Snapshot model is refined to separate topology from state:

```
Snapshot:
    version: VersionId
    schema: SchemaRef                                              // topology
    values: PersistentMap<ValueCellId, (Value, DeterminacyState)>  // state
    provenance: SnapshotProvenance
```

The `edges` field from the original Snapshot model (evaluation-graph-design-decisions §3.1) is subsumed by the schema. Dependency edges are part of the SchemaFragment, not the value map.

### 9.2 Provenance extension

A new provenance variant captures structural changes:

```
SnapshotProvenance:
    | Edit { changed: Set<ValueCellId>, parent: SnapshotId }
    | Restructure { new_schema: SchemaRef, parent: SnapshotId }
    | Merge { sources: List<SnapshotId>, resolution: ConflictResolution }
    | Import { source: ExternalSource }
    | Resolution { scope: ScopeId, resolved: Set<ValueCellId>, parent: SnapshotId }
```

The `Restructure` variant makes the two kinds of change (parametric vs. structural) explicit in provenance, which is useful for both scheduling and debugging.

### 9.3 Graph representation

Forward dependency edges (depends-on) are represented as embedded references within nodes. A node IS a handle for its dependency subgraph — passing a node carries its context, and structural sharing of subgraphs is pointer sharing of nodes. When a node's dependency changes, a new node is created pointing to the updated dependency, sharing all others. The cascade is O(depth) along the spine; siblings are shared.

Reverse dependency edges (depended-on-by) are maintained as a mutable side-index, as established in the evaluation graph design (v0.1 §9.3). This is a derived, reconstructible acceleration structure. Its failure mode (stale index → conservative dirty cone → wasted recomputation, not incorrect results) is safe.

**Upgrade path:** If profiling or distributed computation requirements indicate, the reverse index can be replaced with a second immutable persistent data structure mirroring the forward graph with reversed edges, updated atomically with the forward graph.

---

## 10. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Unified dataflow graph | Schema, values, constraints, realisations, and computations are all nodes in one graph | No separate phases or orchestration; structural changes are just dataflow |
| Schema as computed value | SchemaFragments are cached, content-addressed outputs of Schema nodes | Early cutoff catches no-op structural changes; same caching infrastructure as values |
| Schema propagation | Schema node re-evaluation includes graph mutation (create/remove nodes) | One additional propagation mode, not a separate system |
| Stratification | Structure-controlling values cannot depend on structure they control | Falls out of DAG property; cycle detection catches violations statically |
| Partial elaboration | Schema nodes emit Intermediate results when structure-controlling `auto` values are unresolved | Reuses Currency model from evaluation-graph-completion; no new machinery |
| Node identity | Path-based (containment tree + role) | Cache reuse across structural toggles; matches designer's mental model |
| Collection identity | Positional indexing for v0.1 | Simple, correct; upgrade path to keyed identity via traits |
| Constraint violation | Continued evaluation at lower priority | Designer sees consequences; scheduling hint within existing priority system |
| Snapshot model | `schema: SchemaRef` + `values: PersistentMap` — topology and state separated | Clean separation of concerns; edges belong to schema, not value map |
| Provenance | `Restructure` variant added for structural changes | Explicit in provenance; useful for scheduling and debugging |
| Forward edges | Embedded references in nodes | Node is subgraph handle; structural sharing is pointer sharing |
| Reverse edges | Mutable side-index for v0.1 | Reconstructible acceleration structure; safe failure mode; upgrade path to immutable exists |

---

## 11. Worked example: guard flip with auto-resolved collection

### 11.1 Design

```
structure def Housing {
    param width: Length = 100mm
    param height: Length = 60mm
    param needs_cooling: Bool = false
    param vent_count: Int = auto

    occurrence body: Box { width = self.width, height = self.height }

    occurrence fan_mount: FanMount where needs_cooling {
        size = 40mm
        constraint fan_mount.size < self.width / 2
    }

    occurrence vents: Vent[vent_count] where needs_cooling {
        spacing = self.width / (vent_count + 1)
    }

    constraint vent_count >= 2 where needs_cooling
    constraint vent_count <= 6 where needs_cooling
    constraint per_vent_airflow(Vent) * vent_count >= required_airflow(fan_mount)
        where needs_cooling
    constraint total_mass(self) < 500g
}
```

### 11.2 Initial state: `needs_cooling = false`

The unified graph contains:

- `Schema(Housing)` with inputs `AST(Housing)`, `needs_cooling=false`. Output SchemaFragment contains: ValueCells for `width`, `height`, `needs_cooling`; body-related nodes; `Constraint(total_mass)`. No fan_mount or vent nodes — guarded out.
- Value, constraint, and realisation nodes for the body, all with Final cached results.

### 11.3 Designer sets `needs_cooling = true`

**Schema re-evaluation (first pass).** `Schema(Housing)` is dirty — `needs_cooling` is a structure-controlling input. Re-evaluates with `needs_cooling=true`. Guards for `fan_mount`, `vents`, and cooling constraints are now true. But `vent_count = auto` — not yet determined.

Schema emits an Intermediate SchemaFragment containing: fan_mount nodes, cooling constraint nodes, `ResolutionNode(resolve_vent_count)`, but no vent instance nodes (collection size unknown).

**Diff and reconciliation.** New nodes appear: `Value(fan_mount.size)`, `Constraint(fan_mount_size_limit)`, `Realization(fan_mount_geom)`, `ResolutionNode(resolve_vent_count)`, bounding constraints on vent_count, airflow constraint. All enter dirty set.

**Resolution.** `resolve_vent_count` evaluates. Constraints: `vent_count >= 2`, `vent_count <= 6`, `5 * vent_count >= 18`. Feasible: `vent_count >= 4`. Minimum: `vent_count = 4`.

`Value(vent_count) = 4, Final`. This is a structure-controlling value that just became determined. `Schema(Housing)` is dirty again.

**Schema re-evaluation (second pass).** Re-evaluates with `needs_cooling=true`, `vent_count=4`. Produces Final SchemaFragment with 4 vent ValueCells and 4 vent RealizationNodes.

**Diff and reconciliation.** 4 vent nodes appear. `Constraint(total_mass)` gains dependencies on vent geometries — edge set changed, enters dirty set.

**Normal evaluation.** Vent spacings evaluate: `100mm / 5 = 20mm`. Vent geometries produced. `total_mass` re-evaluates including vents. Steady state.

### 11.4 Designer changes `width` from 100mm to 80mm

Parametric change — `width` is not structure-controlling. `Schema(Housing)` is NOT dirty. No structural change.

Dirty set: `Value(body.width)`, vent spacings, `Constraint(fan_mount_size_limit)`, `Realization(body_geom)`.

`fan_mount_size_limit`: `40mm < 80mm / 2 = 40mm` → violated. Constraint panel lights up. Evaluation continues at lower priority for downstream nodes.

Vent spacings: `80mm / 5 = 16mm`. Vent geometries recompute. `total_mass` re-evaluates.

### 11.5 Designer changes `fan_mount.size` from 40mm to 30mm

Parametric change — but flows through resolution to cause structural change.

`required_airflow(fan_mount)` recalculates: 30mm fan → 12 CFM. Airflow constraint: `5 * vent_count >= 12` → `vent_count >= 2.4` → minimum 3. `resolve_vent_count` re-evaluates: `vent_count = 3`.

Structure-controlling value changed. `Schema(Housing)` re-evaluates with `vent_count=3`. Diff: `vents[3]` nodes disappear. Warm state donated to pool. Surviving vent spacings recalculate: `80mm / 4 = 20mm`. `total_mass` edge set changes (one fewer vent), re-evaluates. Steady state.

### 11.6 Designer sets `needs_cooling` back to `false`, then `true` again

Toggle off: Schema re-evaluates, all cooling nodes disappear. Cached results orphaned in content-addressed cache. Warm state donated to pool.

Toggle on: Schema re-evaluates, cooling nodes reappear. Node identity (path-based) matches the previous incarnation. Content-addressed cache still holds results from the first activation — cache hits across the board. The toggle costs almost nothing the second time.

---

*Document generated from structural graph changes design sessions. Intended to be read alongside `evaluation-graph-design-decisions.md` v0.1 and `evaluation-graph-completion-design-decisions.md` v0.2, which specify the core architecture this document extends.*
