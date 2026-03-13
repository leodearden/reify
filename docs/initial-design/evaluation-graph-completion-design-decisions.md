# Evaluation Graph Completion: Design Decisions

**Status:** Design resolved for all four remaining architectural concerns  
**Version:** 0.2 — Completes the evaluation graph specification begun in v0.1  
**Builds on:** `evaluation-graph-design-decisions.md` v0.1, `ontology-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1, `geometry-engine-design-decisions.md` v0.1

---

## 1. Design approach

The v0.1 evaluation graph specification established the core architecture — immutable snapshots, five node types, pull-based evaluation, two-cone scheduling, warm starting, and cooperative cancellation — but left four concerns as open questions (§12.1–12.4). These were resolved in dependency order:

1. **Provisional/pending state model** — the foundational data model question
2. **Error, diagnostic, and provenance flow** — builds on the state model
3. **Long-running and async tasks** — primarily policy, given the infrastructure from 1 and 2
4. **Purpose-driven activation** — a mapping/scheduling concern consuming all of the above

The central insight driving resolution is: **states are states are trees.** Final results, intermediate results, errors, and pending placeholders are all cached results within the same immutable snapshot infrastructure. No new evaluation machinery is required. Operational concerns (diagnostics, progress, errors) are separated from state as **realisation events** — a parallel event system that references nodes and states but does not live in the evaluation graph.

---

## 2. Provisional and pending state model

### 2.1 The problem

The v0.1 design has a binary model: a node's result is either cached (valid) or needs recomputation. Three concerns require something in between: long-running tasks producing improving results over time, the constraint system's proposing mode generating progressively better solutions, and nodes whose inputs are still being refined upstream.

### 2.2 Result currency

Every cached result carries a **currency** marker:

```
Currency:
    | Final                                    // committed result, fully evaluated
    | Intermediate { generation: u64 }         // node is still refining; generation monotonically increases
    | Pending { last_substantive: ResultRef }   // gated — not recalculated, showing previous best
```

The `NodeCache` entry becomes:

```
NodeCache:
    result: CachedResult              // immutable, content-hashable
    currency: Currency                // distinguishes final from in-progress from gated
    dependency_trace: DependencyTrace // immutable
    warm_state: Option<OpaqueState>   // opaque, mutable, not content-addressed
    basis_version: VersionId          // snapshot this result was computed against
```

Final, intermediate, and pending results all live in the same cache infrastructure. Content-hash keying, warm starting, dependency traces, and early cutoff all work on intermediate results without modification.

### 2.3 Intermediate flag propagation

A node's output is intermediate if any of its inputs are intermediate OR if the node itself is still refining:

```
output.currency = if self.still_refining:
    Intermediate { generation }
elif any(input.currency != Final for input in inputs):
    Intermediate { generation }
else:
    Final
```

When the last upstream intermediate becomes final, downstream nodes re-evaluate (their input hash changes — the final result likely differs from the last intermediate). If the downstream node is not itself progressive, its output becomes Final. This is normal cache invalidation — no special machinery.

### 2.4 Eager evaluation of intermediates with cost-aware gating

Downstream nodes eagerly consume intermediate upstream results, but at **lower priority** than otherwise-equal-priority tasks based on final inputs. A cost-aware gating heuristic determines whether to actually re-evaluate or emit a Pending result:

**Gating policy:** The runtime balances the estimated cost of re-evaluation (both computational cost and opportunity cost of consuming resources) against the value of having an updated intermediate. When idle local resources are available, intermediate-driven evaluations proceed — the system might as well use the resources speculatively. When resource-constrained, the runtime emits a Pending result instead of recalculating.

**Pending as propagation gate:** A Pending result retains the node's most recent substantive result (for UI display) but does not trigger downstream re-evaluation. This naturally quiets the entire downstream subtree without any explicit "pause propagation" mechanism. When the upstream eventually emits a substantive new result, input hashes change and downstream nodes re-enter the scheduling queue.

**Content-hash significance filter:** The system already has a free significance filter on intermediate propagation via early cutoff. If an FEA solver emits iteration N and iteration N+1 with near-identical stress fields, the content hashes match and downstream nodes are not re-evaluated. The gating heuristic only needs to handle the case where intermediates are genuinely different but evaluation cost isn't worth it yet.

### 2.5 Interaction with ResolutionNodes

A ResolutionNode exploring trial values can emit its best-so-far as an intermediate result, making it visible to the UI ("here's the best configuration found so far, still searching"). This is powerful for the proposing mode — the designer sees progressively improving solutions — and follows the same intermediate propagation rules as any other progressive node.

---

## 3. Realisation events: diagnostics, errors, and progress

### 3.1 Separation of state and history

The design separates **what is** (cached results in the evaluation graph) from **what happened** (realisation events). Diagnostics, errors, progress reports, convergence metrics, and lifecycle transitions are all realisation events. They reference nodes and states but do not live in the graph.

### 3.2 Event model

```
RealisationEvent:
    timestamp: Instant
    node_id: NodeId
    snapshot_version: VersionId
    kind: EventKind
    payload: EventPayload            // structured, kind-specific
    references: Vec<NodeId | StateRef>  // related nodes and states
```

Event kinds include: `diagnostic`, `progress`, `error`, `intermediate_emitted`, `completed`, `cancelled`, `commitment_acquired`, `staleness_detected`.

Events are append-only and indexed by both timestamp and node_id. This provides two natural query patterns:

- **Journal (temporal view):** query by time range — "what happened in the last 30 seconds?" Useful for activity feeds, notification streams, debugging.
- **Structural view:** query by node_id — "what diagnostics has this constraint node emitted?" Useful for the constraint panel, per-node inspection, determinacy traces.

### 3.3 Constraint diagnostics

A ConstraintNode evaluation produces a cached result (the `Satisfaction` status and structured per-predicate diagnostics, as specified in v0.1 §2.2) AND emits realisation events for violations, `indeterminate` results, tolerance warnings, and other noteworthy outcomes. The result is the state; the events are the history.

The UI's constraint panel reads the current cached result for display and subscribes to the event journal for notifications. This gives both a live snapshot of constraint status and a history of how it changed.

### 3.4 Diagnostic aggregation

Aggregation uses the event journal directly. Querying all violation events against the current snapshot version, grouped by entity, severity, or constraint type, provides the summary view. No explicit aggregation node is needed in the graph — the journal's index by snapshot version gives "all diagnostics for the current state" efficiently.

This is consistent with the principle that the evaluation graph contains computation, not operational bookkeeping. Aggregation is an operational/UI concern.

### 3.5 Determinacy stack traces

The ontology's concept of determinacy as a "stack trace" (§8.2) — showing not just how complete something is, but why it's incomplete — maps to a backward walk through dependency edges from an `undef` or `indeterminate` result to the root cause.

**Computed on-demand, not precomputed.** Users trigger this relatively rarely (clicking "why is this incomplete?" in the UI). The dependency traces stored alongside cached results provide all the information needed. The traversal is a straightforward graph walk and does not require caching or special infrastructure.

### 3.6 Infrastructure-level diagnostics

Tolerance warnings from the geometry engine (representation tolerance budget exhaustion, conversion fidelity degradation), solver convergence diagnostics, and resource-limit warnings are all realisation events with appropriate `EventKind` values. They follow the same routing — journal for history, node-indexed queries for structural inspection. No separate warning system is needed.

---

## 4. Long-running and async tasks

### 4.1 Overview

With the provisional state model and event infrastructure in place, long-running task integration is primarily a policy concern. The existing task model (v0.1 §8.1), priority levels (§8.2), and cancellation protocol (§8.4) provide the foundation. This section specifies commitment policy, staleness handling, and progress reporting.

### 4.2 Task commitment

A committed task runs to completion against its original snapshot regardless of subsequent edits. Commitment protects expensive work from being discarded by routine editing.

**Commitment policy is project configuration, not source code logic.** Two configurable thresholds with sensible defaults:

| Threshold | Default | Semantics |
|---|---|---|
| `always_commit_after` | 120 seconds | Any task running longer than this is committed unconditionally |
| `commit_when_proportion_done` | 0.5 | A task estimated to be past this proportion of completion is committed |

**Progress estimation:** If the node supports progress reporting (via realisation events), reported progress is used directly. If not, progress is estimated as `elapsed_time / previous_runtime_for_this_node`. The estimate is treated as optimistic — a node that took 60s last time might take 300s this time due to changed problem size or stiffness. The 0.5 default is conservative: even if the estimate is 2x optimistic, the task is still at ≥0.25 actual progress, which is nontrivial work to discard.

**Per-node policy overrides** are available through a dedicated UI widget showing committed tasks with progress bars, resource metrics, priority, and staleness status. Override options include:

- **Commit if slow** (default): the dual-threshold policy above
- **Always cancel when stale**: never commit; always restart on dirty-cone intersection
- **Only run on final inputs**: don't evaluate on intermediate upstream results

These overrides can be set per node instance or per node type.

### 4.3 Staleness detection

A committed task's result is computed against a specific snapshot. Staleness is detected via the persistent data structure's structural sharing: if the subtree of the snapshot that provides the node's input dependencies is the same structure (shared trie nodes) in the basis snapshot and the current snapshot, the result is not stale — it is valid for the current state by construction. If the subtrees differ, the result is stale.

No explicit diffing against dependency traces is needed. The immutable snapshot infrastructure provides this check for free.

### 4.4 Cancellation refinement

The v0.1 cancellation policy (§8.4) is refined with commitment:

| Condition | Behaviour |
|---|---|
| Task is in dirty cone, **not committed** | Cancelled (existing policy) |
| Task is in dirty cone, **committed** | Runs to completion; result cached with stale `basis_version` |
| User explicitly requests re-evaluation | Force-cancels even committed tasks; restarts at P1-slow |
| User explicitly cancels | Force-cancels; warm state saved for future use |

**No parallel evaluations of the same node.** When a committed task is running stale, the fresh re-evaluation is queued to start when the committed task completes. The re-evaluation inherits the warm state from the completed task, enabling fast convergence. This avoids priority inversion entirely — there are never two instances of the same node competing for resources.

**Sequence on committed stale completion:** committed task finishes → result cached with stale `basis_version` and `Intermediate` currency → warm state saved → re-evaluation queued at appropriate priority with warm start → re-evaluation converges quickly using warm state → result becomes Final at current snapshot.

### 4.5 Progress reporting

Progressive nodes emit intermediate results and realisation events as specified in §2 and §3. For long-running tasks that aren't meaningfully progressive (a monolithic external solver call), the event model handles progress: emit `progress` events with structured payload (iteration count, convergence metric, estimated time remaining) without intermediate results. The UI subscribes to events for the node. No new machinery beyond the event system.

### 4.6 Committed task UI

Committed tasks surface in a dedicated UI widget displaying:

- Task identity (node, entity, computation type)
- Progress (reported or estimated)
- Resource consumption (wall time, CPU/GPU utilisation)
- Staleness status (current, stale with list of changed inputs, or valid despite newer snapshot)
- Priority
- Policy controls (override commitment behaviour for this node or type)
- Manual cancel/restart actions

---

## 5. Purpose-driven activation

### 5.1 Purpose as syntactic sugar for scoped constraints

A purpose is a named scope that applies constraints to specific entities. Activating a purpose is equivalent to applying those constraints. The constraint system's normal evaluation semantics handle everything else: constraint nodes enter the graph, resolution nodes fire for `auto` parameters, demand follows from UI elements watching the results.

No special scheduling mechanism, demand injection, or purpose-to-nodes mapping is needed. Purpose composes entirely through existing constraint and evaluation infrastructure.

```
purpose manufacturing_ready(bracket: RigidMechanical) {
    constraint all_geometric_params_determined(bracket)
    constraint representation_tolerance(bracket) <= 1um
    minimize cost(bracket)
    export bracket as STEP
    export bracket as Drawing
}
```

Activating `manufacturing_ready(my_bracket)` applies these constraints to `my_bracket`. The constraint system evaluates them, resolution nodes resolve any `auto` parameters under the `minimize cost` objective, and export occurrences produce deliverables. Deactivating the purpose removes the constraints.

### 5.2 Checking, solving, and proposing fall out naturally

A purpose does not need to know which mode it's operating in. The determinacy state of the inputs determines the behaviour:

- All inputs determined → constraint checking runs
- Some inputs `auto` → resolution runs (solving mode)
- Many inputs `undef` → constraint nodes report `indeterminate`, determinacy stack traces are available (proposing mode)

This is just constraint evaluation against the current state — no mode selection logic.

### 5.3 Multiple simultaneous purposes

Multiple purposes can be active on different (or overlapping) parts of the design. Each contributes its constraints independently. Conflicting tolerance requirements on the same entity result in separate RealizationNodes (keyed by `(entity, repr_kind, tolerance)` as established in v0.1). The tighter realization might satisfy the looser one too — an optimisation opportunity but not a correctness concern.

### 5.4 Resource scheduling for heavyweight purposes

A heavyweight purpose (manufacturing readiness implying FEA, toolpath generation, tolerance stack-up analysis) injects many constraints and export demands simultaneously. The existing priority system handles this without explicit staging: the two-cone intersection prioritises what's both dirty and demanded, and the cost-aware gating from §2.4 naturally throttles expensive work. If the user activates a heavyweight purpose while actively editing, interactive-priority work dominates and analysis runs at P3 until editing pauses.

### 5.5 Export and Import as occurrence traits

Export and Import are degenerate occurrences — boundary nodes where the design meets the outside world. An export consumes a structure without producing one in the design domain (it produces a file artifact externally). An import produces a structure without consuming one (it introduces external geometry into the design).

They inherit all occurrence semantics: parameterisation, constraints, composition into processes, and participation in the evaluation graph as normal nodes.

```
occurrence def STEPExport : Export {
    param subject : Structure
    param format_version : STEPVersion = AP214
    constraint representation_tolerance(subject) <= 1um
    // trait-provided: produces a file artifact
}
```

Export occurrences are placed inside purpose definitions to specify the deliverables that the purpose implies. An import occurrence carries provenance (source, tolerance guarantees, import timestamp) and provides the boundary conditions for the tolerance contract system when working with external geometry.

**Design decision:** Export and Import are traits on occurrences, not separate entity types. This preserves the ontology's four-primitive model (Structure, Occurrence, Constraint, Field) while giving import/export full participation in the occurrence system.

---

## 6. Node traits

### 6.1 Overview

Nodes carry declarative traits that inform the scheduler and UI. These compose orthogonally with the existing priority system and are not exclusive.

| Trait | Semantics |
|---|---|
| `immediate` | Not cancellable; expected sub-frame completion. May be evaluated inline rather than scheduled as a separate task. Corresponds to P0/P1-fast priority. |
| `warm_startable` | Implements the `WarmStartable` interface (v0.1 §5.2). Scheduler preserves warm state on cancellation and completion. |
| `progressive` | Emits intermediate results over time. Scheduler expects multiple cache updates for a single evaluation. |
| `committable` | Subject to the commitment policy (§4.2). Scheduler applies commitment thresholds. Absent this trait, the node is always cancellable. |

A node can carry multiple traits. An FEA solver node might be `warm_startable + progressive + committable`: warm-starts from the previous solution, emits improving results as iterations converge, and shouldn't be killed after significant progress.

### 6.2 Relationship to priority

Node traits inform priority assignment but do not replace it. An `immediate` node is always P0/P1-fast. A `progressive + committable` node starts at whatever priority the two-cone model assigns and may become non-cancellable via the commitment policy. Traits are static declarations on the node type; priority is a dynamic scheduling-time assignment.

---

## 7. Summary of decisions

| Decision | Choice | Rationale |
|---|---|---|
| Intermediate results | Same cache infrastructure as final results, distinguished by `Currency` marker | No new evaluation machinery; content-hash caching, warm starting, and early cutoff work on intermediates for free |
| Intermediate propagation | Eager at lower priority with cost-aware gating; Pending as propagation gate | Idle resources used speculatively; loaded systems naturally throttle; Pending quiets downstream without explicit pause |
| Diagnostics and events | Realisation events — append-only log, indexed by timestamp and node_id, referencing nodes and states | Separates what happened from what is; journal provides temporal view, node index provides structural view |
| Diagnostic aggregation | Via journal queries, not graph nodes | Aggregation is operational/UI concern, not computation |
| Determinacy stack traces | On-demand backward graph walk | Rarely triggered; dependency traces provide all needed information |
| Commitment policy | Dual-threshold project config: `always_commit_after` (120s default), `commit_when_proportion_done` (0.5 default) | Self-calibrating; captures both absolute cost and near-completion value; per-node/type overrides via UI |
| Staleness detection | Structural sharing in persistent data structures | Free — shared trie nodes prove input identity without diffing |
| Committed stale tasks | Run to completion at current priority; no parallel evaluations; re-evaluation queued with warm state | Avoids priority inversion; warm state enables fast convergence on restart |
| Purpose | Syntactic sugar for scoped constraints on specific entities | No special scheduling machinery; composes through existing constraint and evaluation infrastructure |
| Checking/solving/proposing | Falls out of input determinacy state, not explicit mode selection | Purpose doesn't need to know its mode; constraint evaluation handles it |
| Export/Import | Occurrence traits (degenerate occurrences) | Preserves four-primitive ontology; inherits all occurrence semantics |
| Node traits | `immediate`, `warm_startable`, `progressive`, `committable` — orthogonal, composable | Declarative scheduler hints; inform priority and lifecycle without replacing dynamic scheduling |

---

## 8. Open questions for subsequent phases

### 8.1 Graph structural changes

Adding or removing sub-structures, changing collection membership, conditional sub-structure presence — these change the graph's topology. How are structural changes detected, represented, and propagated in the immutable snapshot model? (Carried forward from v0.1 §12.5.)

### 8.2 Tolerance budget allocation

The representation tolerance contract requires allocating error budgets across chains of conversions. How does the evaluation graph manage tolerance flow? (Carried forward from v0.1 §12.6.)

### 8.3 Implementation technology choices

Persistent data structure library, async runtime, content hashing algorithm, cache storage backend, event journal storage. (Carried forward from v0.1 §12.7.)

### 8.4 JIT optimisation of node graphs

The evaluation graph topology may be amenable to significant runtime optimisation — node fusion, scheduling pattern learning, adaptive granularity. Deferred to post-v0.1.

### 8.5 Sophisticated cost estimation for gating heuristics

The v0.1 gating heuristic uses simple cost proxies (wall time, estimated proportion complete). Sophisticated versions (estimating marginal value of intermediate evaluation based on convergence rate and downstream fan-out, monetary cost of compute resources) are optimisation-later candidates.

---

*Document generated from evaluation graph completion design sessions. Intended to be read alongside `evaluation-graph-design-decisions.md` v0.1, which specifies the core architecture this document completes.*
