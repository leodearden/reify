# Critical Review: Reify Implementation Architecture (v0.1)

## Executive Summary

This is a notably well-crafted architecture document. It synthesises 16 design documents into a coherent whole, maintains a consistent vocabulary, and makes principled design choices. The immutable-snapshot-based evaluation graph with content-hash caching is a sound foundation. The document is at its strongest when describing the evaluation model, the snapshot data structures, and the warm-start protocol.

However, there are substantive issues -- some self-consistency problems, several completeness gaps that would block implementation, a few technical feasibility concerns, and a handful of places where the document's own principles are not followed through. What follows is a thorough accounting.

---

## 1. Self-Consistency

### 1.1 ResolutionNode "writes" vs. immutable snapshots (Sections 2.1, 2.3, 2.4)

**Issue:** Section 2.1 states ResolutionNode is "the only node type that writes to ValueCells it does not own" and immediately qualifies that in the immutable model this manifests as "producing a new snapshot with resolved values." But this creates a tension with the rest of the evaluation model. Every other node evaluation is a pure function `(snapshot, node_id) -> Result`. ResolutionNode evaluation is `(snapshot, scope_id, auto_params) -> Snapshot` -- it returns a *new snapshot*, not a result that gets cached in the existing snapshot. The `evaluate()` pseudocode in Section 3.1 does not account for this: it shows `result = compute(node_id, inputs); cache_store(...)`, but the ResolutionNode's "result" is an entirely new snapshot that must become the *basis* for all subsequent evaluation.

**Severity:** Significant. An implementer would not know how to integrate ResolutionNode evaluation into the generic `evaluate()` pipeline. Does the scheduler detect that a ResolutionNode produced a new snapshot and restart evaluation from that snapshot? Is there a snapshot-chaining protocol? The document never says.

**Recommendation:** Add a subsection or extend 2.4 to explicitly describe the snapshot-transition protocol when a ResolutionNode converges. Specifically: (a) does the converged trial snapshot become the new "current" snapshot? (b) how does this interact with concurrent evaluations that started against the pre-resolution snapshot? (c) does the Orchestrator create a new snapshot with `Resolution` provenance and broadcast it the same way an `Edit` provenance snapshot is broadcast?

### 1.2 Edge type #6 direction vs. DAG claim (Sections 2.2, 2.1)

**Issue:** Edge type #6 is `ResolutionNode -> ValueCell` ("solver writes resolved values"). Edge type #5 is `ConstraintNode -> ResolutionNode`. Edge type #2 is `ValueCell -> ConstraintNode`. This gives a path `ValueCell -> ConstraintNode -> ResolutionNode -> ValueCell`, which is a cycle if taken at face value. Section 2.1 acknowledges this and declares it a "convergence loop within the ResolutionNode's computation, not a graph cycle." But the edge table in 2.2 lists edge #6 as a real graph edge, not a pseudo-edge. If the graph is truly a DAG at the macro level, then edge #6 should not be a first-class edge in the schema -- it should be an artefact of snapshot production.

**Severity:** Significant. The edge taxonomy needs to distinguish between edges that participate in dependency tracking and scheduling (edges 1-5, 7-10) and edges that represent snapshot mutation semantics (edge 6). Currently they are mixed together.

**Recommendation:** Either (a) remove edge #6 from the edge table and describe the ResolutionNode -> ValueCell relationship purely in terms of snapshot production, or (b) add a column to the edge table indicating whether each edge is a "dependency edge" (used for dirty/demand cone computation) or a "production edge" (describes data flow but not scheduling dependency). The current presentation conflates these.

### 1.3 Reverse dependency index type signature (Sections 5.1, 15)

**Issue:** The `ReverseDependencyIndex` is typed as `Map<ValueCellId, Set<NodeId>>` in both Section 5.1 and the Appendix. But edges 7, 8, and 10 have `RealizationNode` as the source, and edge 5 has `ConstraintNode` as the source. These are not `ValueCellId`s. The reverse index as specified cannot track "which nodes read this RealizationNode" -- it only tracks ValueCell consumers. Yet the dirty cone computation in 5.1 shows `dirty |= reverse_index[node]`, implying the index is keyed by `NodeId` (any node), not just `ValueCellId`.

**Severity:** Significant. The type signature and the algorithm are inconsistent.

**Recommendation:** Change the type to `Map<NodeId, Set<NodeId>>` or add a second index for non-ValueCell nodes. The pseudocode in 5.1 already implies the broader type.

### 1.4 P2 priority level missing (Section 3.3)

**Issue:** The priority table lists P0, P1-fast, P1-slow, and P3. There is no P2. This appears intentional (the gap leaves room for future levels) but is never explained and creates ambiguity about whether P2 exists but was omitted.

**Severity:** Minor. An implementer would wonder if P2 was accidentally dropped.

**Recommendation:** Add a one-line note: "P2 is reserved for future use" or renumber P3 to P2 if no gap is intended.

### 1.5 SchemaNode as node type #6 vs. open question #6 (Sections 2.1, 16)

**Issue:** The body of the document treats SchemaNode as a first-class 6th node type with a full signature, edge connections, and caching semantics. Open question #6 in Section 16 asks "Whether SchemaNode is a 6th node type or a specialised ComputeNode." The body and the appendix have already answered this question by treating it as a distinct type throughout. The open question is stale.

**Severity:** Minor. Misleading rather than blocking.

**Recommendation:** Close open question #6 with a note that the document treats SchemaNode as a distinct type, and any implementer should do the same unless a concrete reason emerges during implementation to demote it.

### 1.6 `Currency` vs. early cutoff for RealizationNodes (Sections 3.5, 7.1)

**Issue:** Section 3.5 says RealizationNodes skip output equality checking ("B-rep/mesh comparison is expensive and ill-defined. Content-hash cache keying on inputs provides cutoff instead"). But Section 7.2 says intermediate flag propagation checks `if self.still_refining` and then checks all input currencies. If a RealizationNode consumes an intermediate input, its output becomes `Intermediate`. When that input later becomes `Final` but the *value* has not changed (early cutoff at the upstream node), the RealizationNode will still be re-triggered because its `basis_version` is stale. Since the RealizationNode does not do output equality checking, it cannot cut off -- it will recompute and propagate dirtiness downstream, even though nothing material changed.

**Severity:** Significant. This is a performance bug in the design. In a progressive pipeline (e.g., FEA producing intermediate stress fields feeding a RealizationNode for visualization), the RealizationNode will recompute on every intermediate -> final transition of its inputs, even when the actual values are unchanged.

**Recommendation:** For RealizationNodes, use input-hash matching as the early cutoff (as stated), but ensure the input-hash includes content hashes of upstream results, not just version ids. If the upstream early cutoff fires (result unchanged), the RealizationNode's input hash should also be unchanged, preventing recomputation. This needs to be stated explicitly.

---

## 2. Completeness

### 2.1 SchemaNode re-evaluation during in-flight ResolutionNode (Section 6, 7)

**Issue:** The document does not specify what happens when a SchemaNode needs to re-evaluate while a ResolutionNode (which was set up by the previous SchemaFragment) is mid-computation. Scenario: user edits a structure-controlling parameter. The SchemaNode is dirty. The old SchemaFragment set up a ResolutionNode that is currently solving. The new SchemaFragment may have a completely different ResolutionNode (different auto params, different constraints). Must the in-flight ResolutionNode be cancelled? Does this fall under the cancellation rules of Section 7.5? The ResolutionNode is in the dirty cone, but is it committed?

**Severity:** Significant. This is a common scenario during interactive editing of structural parameters.

**Recommendation:** Add explicit protocol: when a SchemaNode produces a new SchemaFragment that removes or restructures a ResolutionNode, any in-flight evaluation of the old ResolutionNode is cancelled (it is in the dirty cone and its node identity has changed). The cancellation rules of 7.5 apply: if committed, it runs to completion but its result is cached with stale `basis_version`; warm state is saved and potentially donated to the new ResolutionNode.

### 2.2 No specification of how SchemaFragments compose into the full schema (Sections 6.2, 6.3)

**Issue:** Section 6.6 says "Schema nodes compose bottom-up via the containment tree: child schemas are dependencies of parent schemas." The SchemaFragment struct has `child_schemas: Map<OccurrenceId, SchemaFragmentRef>`. But there is no specification of how the runtime assembles a complete, flat evaluation graph from a tree of SchemaFragments. Does the runtime flatten the tree into a single graph? Does each SchemaFragment maintain its own namespace? How are cross-scope edges (e.g., a constraint in a parent scope reading a ValueCell in a child scope) represented?

**Severity:** Significant. An implementer cannot build the graph construction layer without this.

**Recommendation:** Add a subsection to Section 6 describing the schema composition algorithm: how the tree of SchemaFragments is flattened into the runtime evaluation graph, how cross-scope edges are resolved, and how the composed schema is stored in `SchemaRef`.

### 2.3 Warm-start state lifetime and eviction (Sections 4.3, 2.3)

**Issue:** Section 2.3 mentions "LRU weighted by recomputation cost" for cache eviction. Section 4.3 describes warm-state pools. But there is no specification of warm-state eviction. Warm state (e.g., an in-memory OCCT model) can be very large (hundreds of MB per entity). If the design has 100 structures, each with a RealizationNode holding an OCCT warm state, memory pressure becomes critical. The document says warm state is "opaque" and "not content-addressed," but does not specify when it is evicted, whether eviction is LRU or cost-weighted, or how the pool size is managed.

**Severity:** Significant. On real hardware with 16-64 GB RAM, unmanaged warm-state retention for a large design will cause OOM.

**Recommendation:** Add a warm-state eviction policy. At minimum: (a) total warm-state memory budget (configurable), (b) eviction order (LRU weighted by recomputation cost, same as cache eviction), (c) notification to the node that warm state was evicted (so it can fall back to cold compute). The pool interface already supports this (checkout returns `None` if evicted), but the eviction trigger needs specification.

### 2.4 No specification of how `always_demanded` is populated (Section 3.2)

**Issue:** The DemandRegistry lists what is "always-demanded" but does not specify who registers nodes or when. Is this driven by the UI layer? Is it driven by purpose activation? When a purpose is activated, do its constraints and output occurrences automatically become always-demanded?

**Severity:** Minor-to-significant. The demand cone drives scheduling priority. If the population protocol is wrong, interactive performance suffers silently.

**Recommendation:** Specify that (a) the UI layer registers visible RealizationNodes, active constraints, and property-editor ValueCells; (b) purpose activation adds the purpose's constraints and outputs to the always-demanded set; (c) demand registration is ephemeral (deactivating a purpose or switching viewport removes registrations).

### 2.5 Missing: how the evaluation graph boots from a cold start (first load of a `.ri` file)

**Issue:** The document covers warm-starting, incremental change, and structural changes. It does not describe the initial construction of the evaluation graph from a parsed `.ri` source. Is there a "root SchemaNode" that elaborates the entire design? Is the initial snapshot empty? How does the initial demand cone get populated before the UI knows what nodes exist?

**Severity:** Minor. An experienced implementer can infer this, but it should not require inference.

**Recommendation:** Add a brief subsection (could be in Section 2 or 6) describing the cold-start protocol: (a) parse source, (b) elaborate root SchemaNode, producing the initial SchemaFragment tree, (c) construct initial snapshot with all ValueCells at their default/undef values, (d) UI registers initial demands, (e) demand-driven evaluation begins.

### 2.6 Missing: ComputeNode -> ConstraintNode edge (Section 2.2)

**Issue:** The edge table has 10 edge types. None of them is `ComputeNode -> ConstraintNode`. But this is a natural and common pattern: a constraint that checks a computed result (e.g., "safety factor from FEA must be > 2.0"). The only path from ComputeNode results to constraints would be `ComputeNode -> ValueCell (edge 9) -> ConstraintNode (edge 2)`, requiring an intermediate ValueCell. This is viable but should be stated as the canonical pattern.

**Severity:** Minor. The indirection through a ValueCell works but is not documented as the intended pattern.

**Recommendation:** Add a note to Section 2.2 that `ComputeNode -> ConstraintNode` is deliberately absent; the pattern is to route through an intermediate ValueCell, which provides an early-cutoff opportunity.

### 2.7 Missing: SchemaNode edge connections (Section 2.2)

**Issue:** The SchemaNode is the 6th node type but has no entries in the edge table. What are its input edges? (Presumably: ValueCell -> SchemaNode for structure-controlling parameters, and possibly some form of source-AST dependency.) What are its output edges? (It produces nodes and edges, but how is the runtime *notified* that the schema changed?) There is no `SchemaNode -> *` or `* -> SchemaNode` edge type in the table.

**Severity:** Significant. The SchemaNode is architecturally critical but has no specified connectivity in the edge taxonomy.

**Recommendation:** Add at least two edge types: (a) `ValueCell -> SchemaNode` (structure-controlling parameter feeds schema elaboration), (b) `SchemaNode -> [runtime notification]` (schema change triggers reconciliation). The second may not be a "dependency edge" in the traditional sense but needs to be specified.

### 2.8 Missing: how the "version fast path" interacts with resolution snapshots (Sections 3.1, 2.4)

**Issue:** The cache has a `basis_version` fast path: "if the current snapshot version matches `basis_version`, return immediately." But ResolutionNodes produce new snapshots with new version ids on every trial iteration. Do these trial snapshots get new globally unique version ids? If so, the fast path never fires during resolution (every iteration is a new version). If not, how are trial snapshots distinguished?

**Severity:** Minor. This is a performance concern, not a correctness one.

**Recommendation:** Clarify that trial snapshots within a ResolutionNode get new version ids, but the content-hash fallback ensures correctness. The version fast path primarily benefits the common case (user edit, single version bump, most nodes unaffected). During resolution, the content-hash path dominates.

---

## 3. Clarity and Unambiguity

### 3.1 "Scope" is overloaded (Throughout)

**Issue:** "Scope" is used to mean (a) the containment scope in the language (a structure body, an occurrence body), (b) the domain of a ResolutionNode, (c) the domain of a SchemaNode, (d) the scope of an optimisation objective (Section 11.5). These are related but not identical. A structure with no `auto` parameters has a containment scope but no ResolutionNode scope.

**Severity:** Minor. An implementer familiar with the design can disambiguate from context, but the document would benefit from explicit disambiguation.

**Recommendation:** Define "containment scope" (language-level), "resolution scope" (set of coupled auto parameters within a containment scope), and "schema scope" (the containment scope whose SchemaNode elaborates its topology). Note which are always 1:1 and which can diverge.

### 3.2 `let` members as ValueCells -- implications unclear (Section 2.1)

**Issue:** Section 2.1 says "`let` members are ValueCells (not transparent inlined expressions) to provide early-cutoff opportunities." But the document does not specify how `let` ValueCells differ from `param` ValueCells in terms of mutability, determinacy, or resolution. Can a `let` member be `auto`? Can it be `undef`? Is it always `determined` (since it is computed from its definition)? The determinacy states (`undef | constrained | auto | determined`) are listed for ValueCells generally but not differentiated by subtype.

**Severity:** Minor. Likely `let` members are always `determined` (their value is computed from the expression), but this should be stated.

**Recommendation:** Add a note: "`let` ValueCells are always `determined` -- their value is the result of evaluating their defining expression. They participate in early cutoff but not in resolution."

### 3.3 "Content hash" is underspecified (Sections 3.1, 5.2, 6.2)

**Issue:** Content hashing is load-bearing throughout the architecture (cache keying, early cutoff, schema change detection). But the document never specifies: (a) what hash function is used, (b) how floating-point values are hashed (NaN handling? negative zero? denormals?), (c) how geometric values (opaque handles) are hashed, (d) how hash collisions are handled, (e) whether the hash is cryptographic or non-cryptographic.

**Severity:** Significant. Floating-point hashing is a well-known pitfall. If `hash(0.1 + 0.2) != hash(0.3)` due to IEEE 754 representation, content-hash caching produces incorrect misses. If `hash(NaN) != hash(NaN)`, caches never hit for NaN-producing computations. Opaque geometry handles pose a deeper problem: how do you content-hash an OCCT `TopoDS_Shape` without serializing it?

**Recommendation:** Add a subsection on content hashing that specifies: (a) non-cryptographic hash (e.g., xxHash, FxHash) is sufficient since collision resistance is not a security requirement; (b) floating-point values are hashed by their bitwise representation (consistent with the bitwise equality decision in 3.5); (c) NaN is normalized before hashing; (d) opaque geometry handles are hashed by hashing their *input specification* (the operation tree that produced them), not the result; (e) hash collisions are handled by full equality comparison on collision (standard hash-map semantics).

### 3.4 The "Orchestrator" concept is asserted but not specified (Sections 1.1, 11.1)

**Issue:** The "orchestrator pattern" is listed as a core design principle and is central to the constraint engine architecture. But the document never defines the orchestrator's interface, state, or decision algorithm. What does the orchestrator's dispatch loop look like? How does it decide which sub-solver to invoke? How does it handle disagreement between sub-solvers? How does it manage partial results from one sub-solver feeding into another?

**Severity:** Significant for the constraint engine (Section 11), minor for the geometry engine (Section 10, where kernel dispatch is better specified).

**Recommendation:** For v0.1, the constraint orchestrator can be simple (classify constraint domain, dispatch to the appropriate solver, fail if no solver handles it). Specify this simple dispatch protocol. Defer the complex cross-domain decomposition to the open questions. But do provide the orchestrator's interface: `fn resolve(constraints: Set<Constraint>, auto_params: Set<ValueCellId>, warm_state: ...) -> Map<ValueCellId, Value>`.

### 3.5 Absence of diagrams

**Issue:** The document is prose and pseudocode only. There are no architecture diagrams, no data flow diagrams, no sequence diagrams. For a document of this complexity, visual representations would significantly aid comprehension. The evaluation pipeline (Section 6.2: `Source AST + structure-controlling values -> SchemaNodes -> Value/Constraint/Realization nodes -> evaluated state`) is described in one line of ASCII but deserves a proper diagram. The two-cone scheduling model deserves a Venn diagram. The node type interaction matrix deserves a graph diagram.

**Severity:** Minor in terms of correctness, significant in terms of usability for an implementation team.

**Recommendation:** Add at minimum: (1) an evaluation graph node type diagram showing all 6 types and their 10+ edge types, (2) a two-cone scheduling diagram, (3) a sequence diagram for the "bracket thickness change" worked example.

---

## 4. Technical Feasibility and Quality Assessment

### 4.1 HAMT performance at scale (50K+ parameters)

**Assessment:** Sound. HAMTs with a branching factor of 32 (standard) give O(log32(n)) ~= O(4) for 50K entries. Structural sharing means creating a new snapshot with one changed value copies ~4 internal nodes. Memory overhead is ~32 pointers per internal node. For 50K ValueCells, the HAMT itself is small (<10 MB). Rust's `im` or `rpds` crate implements this efficiently.

**Concern:** The real performance question is not the HAMT itself but the *dependency tracking and dirty cone computation* over 50K nodes. With a dense dependency graph (many constraints, each reading many parameters), the dirty cone can be large. The reverse index walk is O(cone size), which could be thousands of nodes for a parameter that feeds into a widely-used constraint.

**Verdict:** Feasible for v0.1. Likely bottleneck is not the snapshot data structure but the evaluation scheduling overhead for large dirty cones. The document's "work outside both cones is not computed" rule is the correct mitigation.

### 4.2 Two-cone scheduling model soundness

**Assessment:** The model is sound. The dirty cone is a conservative superset (may include nodes that turn out to be cache hits after content-hash verification). The demand cone is exact (backward transitive closure from always-demanded nodes). The intersection is the correct set.

**Concern -- potential starvation of P3 work:** If the user is continuously editing (every 50-100ms), every keystroke creates a new snapshot. P3 (speculative) work is "cancelled immediately when new snapshot arrives" (Section 7.5). If edits are fast enough, P3 work never completes. This is acceptable for interactive editing (P3 is speculative), but the document should acknowledge that P3 work (like STEP export, FEA) may never complete during active editing sessions.

**Concern -- priority inversion:** Section 12.2 mentions priority promotion ("if a P1-slow task depends on an in-flight P3 task, the P3 task is promoted to P1-slow"). This is correct. But: what if a P1-slow task depends on a ResolutionNode that is itself P1-slow and mid-iteration? The P1-slow task blocks on the ResolutionNode. If a new snapshot arrives and the ResolutionNode is in the dirty cone, the ResolutionNode may be cancelled, and the P1-slow task that was waiting on it must also be cancelled or restarted. This cancellation cascade is not explicitly described.

**Severity:** The starvation concern is minor (correct behavior). The cancellation cascade is significant and should be documented.

**Recommendation:** Add a note on cascading cancellation: when a node is cancelled, all nodes currently awaiting its result are also cancelled. This falls out of the async task model (awaiting a cancelled future returns a cancellation error), but should be stated explicitly.

### 4.3 Warm-start protocol practicality with OCCT

**Assessment:** Partially feasible but the document is too optimistic.

OCCT's `TopoDS_Shape` is a reference-counted handle into a shared data structure (the `BRep_Builder` graph). "Incremental rebuild" of a fillet radius change requires: (a) identifying the fillet feature in the OCCT history, (b) removing the old fillet, (c) re-applying the fillet with the new radius, (d) rebuilding all downstream features. OCCT does support this via `BRepAlgoAPI_Defeaturing` and `BRepFilletAPI_MakeFillet`, but only if the feature history is maintained -- which OCCT does not do natively in all cases. The document's example ("OCCT locates the fillet feature, updates the radius, and incrementally rebuilds") assumes a parametric history that OCCT itself does not maintain. The warm-start layer would need to maintain its own feature history outside OCCT.

**Severity:** Significant. The warm-start protocol is correct in its abstraction, but the worked example in Section 4.2 implies a level of OCCT incrementality that requires significant implementation effort.

**Recommendation:** Acknowledge that OCCT warm starting requires a feature history layer built on top of OCCT's API. For v0.1, warm starting for OCCT may be limited to "reuse the previous shape as a starting point for re-evaluation of the full operation sequence" rather than "surgically update a single feature." The protocol supports both -- the recommendation is to set expectations correctly.

### 4.4 Content-hash caching and floating-point determinism

**Assessment:** The document makes the right call (Section 3.5): "Bitwise equality for scalar values. Tolerance-based comparison risks hiding genuine small changes." This is correct for *equality checking*. But the deeper issue is *determinism*: does the same sequence of floating-point operations always produce the same result?

On a single machine with a single compiler, yes (IEEE 754 is deterministic for a given instruction sequence). But: (a) compiler optimisations can reorder floating-point operations, changing results; (b) different OCCT versions or kernel patches may produce different results for the same input; (c) parallel execution with non-deterministic reduction order (e.g., summing an array on multiple threads) can produce different results.

The document states "Semantic determinism: guaranteed" (Section 12.5) and cites "the assumption that kernel operations are deterministic given identical inputs." This assumption is fragile.

**Severity:** Minor for v0.1 (single machine, single thread pool, consistent binary). Becomes significant for distribution (Section 12.6) or kernel upgrades.

**Recommendation:** Add a note that floating-point determinism is guaranteed *for a fixed runtime binary on a fixed platform*. Cache entries from different runtime versions or platforms should not be assumed compatible. Kernel version pinning (already mentioned in 10.3) is the correct mitigation.

### 4.5 Interactive editing performance (sub-100ms)

**Assessment:** Feasible for the critical path (P0 and P1-fast), which covers parameter echo in the property editor and simple constraint checking. The concern is the P1-slow path (realization), which includes OCCT B-rep operations. A single fillet operation in OCCT can take 50-500ms. A full B-rep rebuild of a moderately complex part can take 2-10 seconds. The warm-start protocol helps but does not guarantee sub-100ms for geometry updates.

The architecture correctly handles this with progressive rendering: the P1-slow RealizationNode runs asynchronously, the UI shows the previous realization until the new one is ready, and intermediate results are possible for progressive kernels. This is the right approach.

**Verdict:** The architecture supports interactive *parameter editing* at sub-100ms. Geometry *realization updates* will lag behind parameter changes by 100ms to several seconds, which is acceptable if the UI provides feedback (progress indicators, stale-state visualization). The document does not specify the UI feedback model, but that is arguably outside the scope of a runtime architecture document.

### 4.6 Concurrency model practicality

**Assessment:** The work-stealing thread pool (Tokio-style) is well-suited to the task model. The concern is warm-state exclusivity: with a mutex per node's warm state (v0.1), only one evaluation of a given node can proceed at a time. This is fine for normal operation (a node is re-evaluated at most once per snapshot), but during resolution, the ResolutionNode internally evaluates sub-problems that may contend on the same warm states.

A specific concern: the document says "Expected nesting depth: 2-4 levels" for async/await within ResolutionNodes (Section 12.4). With a work-stealing scheduler and 2-4 levels of nesting, thread utilization depends on the number of available threads and the fan-out at each level. For a typical 8-16 core machine, this is fine. But if a ResolutionNode spawns many concurrent sub-evaluations that each block on warm-state mutexes, thread starvation is possible.

**Severity:** Minor. The sequential bottom-up resolution fallback (Section 12.4) is the correct safety net.

**Recommendation:** Document the expected thread count and warm-state contention characteristics. For v0.1, a thread pool size of `num_cpus` with a separate blocking pool is standard. Warm-state contention should be monitored and pool sizes increased if contention is observed.

### 4.7 Multi-kernel dispatch feasibility

**Assessment:** Feasible for the initial combination (OCCT + Manifold + SolveSpace). The kernel registration mechanism is deferred (open question #8), which is acceptable for v0.1 if the kernel set is hardcoded.

**Concern:** The document says dispatch is "deterministic given fixed runtime configuration" (Section 10.3). But the dispatch decision requires knowing "what downstream operations follow (minimise conversions)." This requires look-ahead into the demand cone, which is a non-trivial scheduling problem. For v0.1, a simpler dispatch rule (prefer the kernel that already has a warm state, fall back to OCCT for B-rep, Manifold for mesh Booleans) is more practical.

**Severity:** Minor. The look-ahead dispatch is an optimization that can be deferred.

**Recommendation:** For v0.1, specify a simple dispatch rule: (a) if a warm state exists for a kernel, prefer that kernel; (b) otherwise, prefer the kernel with the best capability match; (c) ties broken by a static preference ordering in the project configuration.

### 4.8 Over-design vs. under-design for v0.1

**Over-designed:**
- The warm-start tier system (Section 4.4). Tiers 2 and 3 are explicitly deferred. The protocol to support them adds no v0.1 complexity. This is well-handled.
- The distribution readiness section (12.6). Good to have in the architecture doc, but no v0.1 implementation work should go into distribution. Correctly scoped.
- The multi-objective optimization discussion (Section 11.4). Weighted sum is sufficient for v0.1. Lexicographic and Pareto are correctly deferred.

**Under-designed:**
- The constraint orchestrator (Section 11.1-11.4). The architecture says "dispatch to specialised sub-solvers" but provides no dispatch protocol, no solver interface, and no failure mode specification. For v0.1, the constraint system needs at minimum: a solver interface trait, a dispatcher that classifies constraints by domain, and a fallback behavior when no solver handles a constraint.
- Error recovery and partial results (Section 9). The `Failed` currency variant is specified, but there is no specification of *retry* behavior. If a B-rep Boolean fails (a common OCCT event), does the node stay `Failed` forever? Can the user trigger a retry? Is there automatic retry with different parameters (e.g., a finer tolerance)?
- The tolerance budget allocation (open question #1). The document repeatedly mentions tolerance budgets across conversion chains but explicitly defers the allocation algorithm. For v0.1, a simple policy (e.g., equal budget per step, or "use the final tolerance for every step") should be specified as a starting point.

### 4.9 Scalability bottleneck: demand cone maintenance

**Issue:** The demand cone is "the backward transitive closure" of always-demanded nodes. Maintaining this incrementally as the schema changes requires updating a potentially large set. If the demand cone covers 80% of the graph (common when the user has many constraints and a full viewport), the intersection with the dirty cone is essentially the dirty cone. The "intersection" optimization becomes vacuous.

**Severity:** Minor. The two-cone model is still correct; it just degrades to "evaluate everything dirty" when demand is broad. The optimization matters most for large designs where the user is zoomed into a subassembly (narrow demand cone).

**Recommendation:** Note that the demand cone's effectiveness depends on viewpoint and active constraints. For a full-design view with all constraints visible, the demand cone approaches the full graph and the scheduling benefit is primarily from priority ordering, not from cone intersection filtering.

### 4.10 Snapshot garbage collection under rapid editing

**Issue:** Section 2.3 says "Snapshots stay alive as long as referenced. Working set is small: current snapshot, 1-2 previous ones held by in-flight evaluations, current solver's trial snapshot." But during rapid editing (e.g., dragging a slider), dozens of snapshots may be created per second. Each snapshot holds a reference to the HAMT. If in-flight evaluations hold references to old snapshots (which they must, since they were started against those snapshots), the working set can grow. With HAMT structural sharing, the memory overhead per snapshot is small (O(log n) for k changes), but the reference count maintenance and GC of unreferenced snapshots needs specification.

**Severity:** Minor. Rust's reference counting (Arc) handles this naturally. But the document should confirm that snapshots are Arc-wrapped and that in-flight evaluations pin their snapshot for the duration of evaluation.

**Recommendation:** Add a brief note: snapshots are reference-counted (Arc); an in-flight evaluation pins its snapshot; when the evaluation completes, the reference is dropped; unreferenced snapshots are deallocated.

---

## 5. Specific Recommendations

| # | Section | Issue | Severity | Recommendation |
|---|---------|-------|----------|----------------|
| 1 | 2.1, 2.4, 3.1 | ResolutionNode snapshot production protocol unspecified | Critical | Add explicit protocol for how resolution results become new snapshots and how concurrent evaluations transition |
| 2 | 2.2 | Edge #6 (ResolutionNode -> ValueCell) conflated with dependency edges | Significant | Distinguish production edges from dependency edges in the edge table |
| 3 | 5.1, 15 | Reverse dependency index typed as `Map<ValueCellId, ...>` but used as `Map<NodeId, ...>` | Significant | Fix type to `Map<NodeId, Set<NodeId>>` |
| 4 | 2.2 | SchemaNode has no edge type entries | Significant | Add `ValueCell -> SchemaNode` and specify schema change notification mechanism |
| 5 | 6 | SchemaFragment composition into a flat evaluation graph unspecified | Significant | Add schema composition algorithm subsection |
| 6 | 4.3 | Warm-state eviction policy absent | Significant | Add memory budget and eviction order specification |
| 7 | 6, 7 | SchemaNode re-evaluation vs. in-flight ResolutionNode interaction unspecified | Significant | Add cancellation cascade protocol for structural changes |
| 8 | 3.5, 7.2 | RealizationNode intermediate -> final transition may bypass early cutoff | Significant | Clarify that input-hash uses content hashes, not version ids, for cutoff |
| 9 | 3.1 | Content hashing mechanics unspecified (FP hashing, collision handling, opaque handles) | Significant | Add content hashing specification subsection |
| 10 | 11.1 | Constraint orchestrator dispatch protocol unspecified | Significant | Add solver interface trait and simple dispatch protocol for v0.1 |
| 11 | 4.2 | OCCT warm-start example overstates incremental capability | Significant | Clarify that feature-level incrementality requires a history layer; set realistic v0.1 expectations |
| 12 | 9 | No retry or recovery protocol for Failed nodes | Significant | Specify user-triggered retry and optional automatic retry with parameter variation |
| 13 | 3.2 | DemandRegistry population protocol unspecified | Minor-Significant | Specify who registers demands and when |
| 14 | 3.3 | P2 priority level gap unexplained | Minor | Add note about P2 reservation |
| 15 | 16 | Open question #6 is stale (SchemaNode is treated as type #6 throughout) | Minor | Close the open question |
| 16 | 2.1 | `let` ValueCell determinacy behavior unspecified | Minor | Note that `let` members are always `determined` |
| 17 | - | No architecture diagrams | Minor | Add node-type graph, two-cone diagram, and lifecycle sequence diagram |
| 18 | - | "Scope" terminology overloaded | Minor | Define containment scope, resolution scope, and schema scope |
| 19 | 2.3 | Snapshot lifetime management (Arc, pinning) unspecified | Minor | Add note on reference-counting and in-flight evaluation pinning |
| 20 | 10.3 | Look-ahead kernel dispatch impractical for v0.1 | Minor | Specify a simple v0.1 dispatch rule (warm preference, capability match, static ordering) |
| 21 | 2.2 | Missing ComputeNode -> ConstraintNode edge; indirection through ValueCell not documented as canonical pattern | Minor | Add a note explaining the intended pattern |
| 22 | 10.4 | Tolerance budget allocation deferred but v0.1-blocking | Minor | Specify a simple default policy (equal per-step or use final tolerance everywhere) |

---

## 6. What Deserves Praise

Several aspects of this document are exceptionally well done:

- **The immutable snapshot model with HAMT structural sharing** is the correct architectural choice. It provides natural transactionality, concurrent evaluation without locks, cache-friendly access patterns, and simple garbage collection. The document correctly identifies all the benefits and correctly scopes the mutable state that must exist outside the model.

- **The content-hash caching with version fast path** is a pragmatic hybrid that avoids the cost of hashing on every access while maintaining correctness. The "change and change back" property (Section 5.2) is an elegant consequence that eliminates the need for undo-specific logic.

- **The two-cone scheduling model** is a clean, well-motivated design that directly connects what needs to be computed (dirty cone) with what the user cares about (demand cone). The priority levels are well-chosen.

- **The warm-start protocol** (Section 4) is well-abstracted. The separation of warm state from cached results, the explicit fallback to cold computation, and the tiered future extension are all correct design choices. The trait interface is clean and implementable.

- **The Currency enum** (Final/Intermediate/Pending/Failed) is a well-designed state machine for result lifecycle. The "Pending as propagation gate" insight is particularly good -- it naturally quiets downstream computation without explicit pause logic.

- **The mutability audit** (Section 13, final section) is a valuable self-check that I wish more architecture documents included. It makes the invariants explicit and provides a concise summary of where mutable state lives and why it is safe.

- **The worked examples** (Section 16 appendix) are concrete, realistic, and trace through the full architecture. They demonstrate that the author has thought through end-to-end scenarios, not just individual components.

- **The explicit acknowledgment of open questions** (Section 16) with priority ordering is mature engineering practice. The document does not pretend to have solved everything.

---

## Summary Verdict

This is a strong architecture document that provides a solid foundation for implementation. The core evaluation model (immutable snapshots, content-hash caching, two-cone scheduling, warm starting) is sound and well-motivated. The most critical gap is the ResolutionNode snapshot-production protocol (issue #1 above), which is the one place where an implementer would truly be stuck. The remaining significant issues are completeness gaps that can be filled during implementation but would benefit from specification upfront. The document's main risk is that it is *almost* complete enough to implement from -- the gaps are subtle enough that an implementer might not notice them until they hit a blocking question mid-implementation.

I would recommend addressing the critical issue (#1) and the most impactful significant issues (#2-6, #9-10) before handing this to an implementation team. The remaining issues can be resolved during implementation as they are encountered.
