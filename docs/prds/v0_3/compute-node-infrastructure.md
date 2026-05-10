# PRD: ComputeNode Infrastructure

Status: deferred — v0.3 foundational. Blocks `structural-analysis-fea.md` task #16 (2924), which is the first ComputeNode consumer.

This PRD consolidates the ComputeNode-shaped material already resolved in `docs/reify-implementation-architecture.md` (§2.1 node taxonomy, §6 dependency-edge taxonomy, §7.6 node traits, §10 multi-rep stack, §13 trait combinations) into a single buildable spec. The architecture doc settles design; this PRD turns that design into code.

## Goal

Land the `ComputeNode` graph type plus the dispatch / cache-key / lifecycle / significance-filter machinery required for `@optimized("target::name")` functions to materialize as ComputeNodes in `EvaluationGraph`. After this PRD lands, a stdlib `fn solve_elastic_static(...)` annotated `@optimized("solver::elastic_static")` resolves to a Rust trampoline call, with warm-state attached to node lifetime, dependency edges declared, cache key composed correctly, in-flight cancellation on input change, and significance-filter comparison at the result boundary.

## Background

`docs/reify-implementation-architecture.md`:

- **§2.1 Node Taxonomy:** `ComputeNode` is one of 7 node types. `(computation_id) -> ComputationResults`. Default traits `WARM_STARTABLE | COMMITTABLE` (§7.6).
- **§5 Dependency edges:** Edge #6 `ValueCell → ComputeNode` ("Computation reads parameter values"), #10 `RealizationNode → ComputeNode` ("Computation needs representation"), #12 `ComputeNode → ValueCell` ("Computation result populates value"). `ComputeNode → ConstraintNode` deliberately absent — routes through intermediate ValueCell for early-cutoff (line 199).
- **§6.1 / §6.2:** "One FEA run = one node"; cache-key = "Hashes of all dependency values/input-hashes" (line 468); content hash = "Domain-specific, typically result hash" (line 425).
- **§13:** "an FEA solver node might be `warm_startable + progressive + committable`" — the trait combinations PRD-task-16 (2924) exercises.

The taxonomy enum (`NodeArchKind::ComputeNode` in `reify-types/src/node_traits.rs:188`) exists. The struct, graph integration, cache wiring, and dispatch path do not — the enum variant has an explicit "(No corresponding Rust struct in the codebase yet.)" annotation.

## Why now

The FEA PRD has stalled task 2924 at architect planning: it cannot wire `solve_elastic_static` to `@optimized` because (a) `@optimized` is not legal on `function` context (`crates/reify-compiler/src/annotations.rs:99` restricts to `structure | occurrence | constraint_def`), (b) `CompiledFunction` has no `optimized_target` field, and (c) there is no `ComputeNode` to dispatch to. The first ComputeNode consumer needs the ComputeNode itself.

## Sketch of approach

### Struct shape

`ComputeNodeData` parallel to existing `ResolutionNodeData` / `RealizationNodeData` in `crates/reify-eval/src/graph.rs`:

```rust
pub struct ComputeNodeData {
    // Identity
    pub computation_id: ComputeNodeId,    // (target string + input scope)
    pub target: String,                    // e.g. "solver::elastic_static"

    // Inputs (drive cache key)
    pub value_inputs: Vec<ValueCellId>,    // edge #6 sources
    pub realization_inputs: Vec<RealizationNodeId>, // edge #10 sources
    pub options_hash: ContentHash,         // serialized options struct hash

    // Cache
    pub cache_key: ContentHash,            // composition of inputs
    pub cached_result: Option<Value>,      // last successful output
    pub result_content_hash: Option<ContentHash>,

    // Lifecycle
    pub opaque_state: Option<OpaqueState>, // warm-state slot (P3.1 leaves Option; P3.5 wires)
    pub running: Option<CancellationHandle>, // populated while in-flight

    // Output side
    pub output_value_cells: Vec<ValueCellId>, // edge #12 sinks
}
```

Insertion API on `EvaluationGraph`; lookup by `ComputeNodeId`.

### Cache key

Composition over: `(target, hash(value_inputs in canonical order), hash(realization_input content-hashes in canonical order), options_hash)`. Explicitly excluded: thread count, determinism mode, mesh-builder thread count — these change bit pattern but not engineering value, and live on a separate "execution profile" key.

### Dispatch registry

`@optimized("target::name")` on a stdlib `fn` lowers to a `ComputeNode` whose `target` string is the lookup key. The Rust side registers `(target_string, ComputeFn)` pairs at crate-init; runtime invokes the registered fn with `(value_inputs, realization_inputs, options)` and stores the result.

### Significance filter

Two ComputeNode results compare via tolerance-equivalence at the result-type boundary, not via content hash. This is a deliberate scoped relaxation of bit-determinism — only ComputeNodes whose result type opts in (mechanism deferred to P3.6) participate. Pure-value ComputeNodes use content hash like everything else.

### Lifecycle: pending + cancellation

While a ComputeNode is running, downstream consumers see a `pending` sentinel value (existing `Value::Pending`-style or new variant — P3.5 decides). Input change triggers cancellation: cooperative — the in-flight Rust fn periodically polls a `CancellationHandle` and bails cleanly. Hard cancellation (thread kill) is not viable in safe Rust.

## Resolved design decisions

- **Struct in `reify-eval/src/graph.rs`**, not a new crate. Parallel to existing `Resolution`/`Realization`NodeData.
- **Cache key excludes thread-count / determinism mode.** Per `docs/prds/v0_3/structural-analysis-fea.md` task #4 (ElasticOptions PRD-mapping): "threads is NOT in the FEA cache key — same answer to tolerance regardless of thread count".
- **Dependency edges are declared, not inferred.** ComputeNode construction names its `value_inputs` / `realization_inputs` lists; freshness-walk uses those, doesn't infer from body.
- **Significance filter is opt-in per result type**, not universal. Default is content-hash comparison; FEA opts in.
- **Cancellation is cooperative**, not preemptive.
- **`@optimized` migrates from constraint-only to constraint+function.** Existing structure/occurrence allow-list slots stay (per `annotations.rs:93-98` comment) for back-compat; function becomes the canonical context.

## Open design questions (defer to implementation)

- **Dispatch registry shape:** global `OnceLock<HashMap<&'static str, ComputeFn>>` registered at crate-init, OR per-Engine registry built at engine construction? Tradeoff: global is simpler but sticky across tests; per-engine is more testable but requires plumbing. **Owner:** P3.4.
- **Significance-filter opt-in mechanism:** marker trait on result type / stdlib annotation / hardcoded result-type list. **Owner:** P3.6.
- **Pending sentinel:** new `Value::Pending` variant vs reuse of existing freshness-flag mechanism. **Owner:** P3.5.

## Decomposition plan

Six tasks. Wire dependencies: P3.1 first; P3.2 then P3.3 then P3.6; P3.4 then P3.5; all six block FEA task 2924.

- **P3.1 — `ComputeNodeData` struct + `EvaluationGraph` integration.** Add the struct to `graph.rs` parallel to `RealizationNodeData`/`ResolutionNodeData`. Insert/lookup APIs. `ComputeNodeId` type. `OpaqueState` slot wired but left `None` (P3.5 populates). Tests: round-trip insert+lookup, multiple ComputeNodes coexist, struct fields match this PRD's spec. No new crate-level deps.

- **P3.2 — Cache-key composition.** Compose `cache_key` from `(target, value-input hashes, realization-input content-hashes, options_hash)` in canonical order. Exclude thread count + determinism mode. Mesh content-hash spec on `RealizationNode` (may already exist — verify and reuse). Tests: identical inputs → identical key; thread-count-only delta → identical key; option delta → different key. Dep: P3.1.

- **P3.3 — Dependency edges + freshness-walk integration.** Declare `RealizationNode→ComputeNode` (#10) and `ValueCell→ComputeNode` (#6) edges; wire `ComputeNode→ValueCell` (#12) output side. `freshness_walk.rs` walks ComputeNodes correctly: input change marks ComputeNode dirty, ComputeNode dirty marks output ValueCells dirty. Tests: input-cell write invalidates downstream ComputeNode + its consumers. Dep: P3.1, P3.2.

- **P3.4 — Dispatch registry + `@optimized("target")` lowering.** Decide registry shape (open question above); implement registration mechanism; wire `@optimized("solver::foo")` on a stdlib `fn` to produce a ComputeNode whose `target` string drives lookup. Stub a minimal "identity" compute fn for tests (e.g., `@optimized("test::identity")` returns its input). Tests: registered target dispatches; unknown target produces clean diagnostic; unregistered target during execution surfaces a runtime error not a crash. Dep: P3.1.

- **P3.5 — Lifecycle: pending propagation + cancellation contract.** While ComputeNode is running, downstream sees `pending` (mechanism deferred above). Cooperative cancellation: `CancellationHandle` (`Arc<AtomicBool>` or similar) passed into the dispatched fn; input change sets the flag; in-flight fn polls and bails. Drop on cancellation reaps the OpaqueState. Tests: rapid input changes don't leak OpaqueState; downstream consumer sees `pending` then final value; cancellation observed within reasonable poll budget. Dep: P3.1, P3.4.

- **P3.6 — Output significance filter (per-purpose tolerance).** Two ComputeNode results compare via tolerance-equivalence at the result-type boundary, not content hash. Opt-in mechanism (open question above). Integrate with existing per-purpose tolerance scope (`crates/reify-eval/src/tolerance_scope.rs`). Tests: bit-different but tolerance-equivalent results trigger no downstream invalidation; bit-different and tolerance-different results do. Dep: P3.1, P3.2.

## Gates

- **Per-purpose tolerance machinery live** (already shipped — `tolerance_scope.rs` present).
- **`OpaqueState` plumbing exists** (already shipped — `reify-types/src/warm.rs`, `reify-eval/src/warm_pool.rs`).
- **No external dependency** — pure-Rust workspace-internal change.

## Consumer

`docs/prds/v0_3/structural-analysis-fea.md` task #16 (2924) is the first consumer and will exercise every piece. Future consumers: modal solver, thermal solver, mesh-morph (PRD pending), implicit-lattice density-field nodes (§10.5 multi-rep stack).
