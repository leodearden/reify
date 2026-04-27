# Reify v0.1 Implementation Plan

**Version:** 0.1
**Date:** 2026-03-16
**Status:** Draft
**Companion to:** Reify Implementation Architecture, Reify Language Specification v0.1

---

## 1. Confirmed Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Implementation language | **Rust** | HAMT/Arc/async alignment with architecture; FFI to C/C++ kernels; WASM target; ascendant in CAD tooling (Truck, Fidget, Fornjot) |
| Parser | **Tree-sitter** | Incremental reparsing, error-tolerant, editor integration via tree-sitter queries; LL(k) grammar fits well |
| UI strategy | **Headless-first** | Engine as library with CLI + LSP. GUI before v0.1 release for alpha testing, not the initial focus |
| Incremental engine | **Deferred** | Deeper dive needed comparing Salsa vs custom. Initial milestones use a simple sequential evaluator behind the correct interface |
| Implementation philosophy | **Minimal real implementations** | Architecturally correct from day one. No throwaway hacks. E2E pipeline ASAP, then breadth |

---

## 2. Crate Workspace Structure

```
reify/
  Cargo.toml                    # workspace root
  crates/
    reify-syntax/               # Tree-sitter grammar, CST→AST, SourceNode content-addressing
    reify-types/                # Dimensional type system, trait checking, type inference
    reify-compiler/             # Name resolution, elaboration, topology templates
    reify-eval/                 # Evaluation graph, snapshots, caching, scheduling
    reify-constraints/          # Constraint orchestrator, solver dispatch, resolution driver
    reify-geometry/             # Multi-kernel dispatch, realization management
    reify-kernel-occt/          # OCCT FFI binding (C++ via cxx)
    reify-kernel-manifold/      # Manifold FFI binding
    reify-runtime/              # Task scheduler, warm-start pools, event journal, cancellation
    reify-stdlib/               # Standard library .ri files + compiler intrinsic implementations
    reify-lsp/                  # Language server protocol implementation
    reify-cli/                  # CLI binary
  tree-sitter-reify/            # Tree-sitter grammar definition (JavaScript DSL → generated C)
  stdlib/                       # Standard library .ri source files
  tests/                        # Integration tests (.ri files with expected outputs)
```

### Crate dependency graph

```
reify-cli ──┬── reify-lsp
             │
             ├── reify-compiler ──┬── reify-syntax
             │                    └── reify-types
             │
             ├── reify-eval ──────┬── reify-constraints ──┬── (NLopt, libslvs, OR-Tools FFI)
             │                    │                        └── reify-types
             │                    └── reify-types
             │
             ├── reify-geometry ──┬── reify-kernel-occt
             │                    ├── reify-kernel-manifold
             │                    └── reify-types
             │
             └── reify-runtime
```

All crates depend on a shared `reify-types` crate that defines the common type vocabulary: `Value`, `DeterminacyState`, `NodeId`, `ValueCellId`, `Satisfaction`, `Freshness`, `SnapshotProvenance`, dimensional exponent vectors, and the geometric type hierarchy.

---

## 3. Module Specifications

### 3.1 `reify-syntax` — Parser

**Implementation:** Tree-sitter grammar (JavaScript DSL) generating a C parser, called from Rust via `tree-sitter` crate. A Rust CST→AST lowering pass converts the concrete syntax tree into a typed AST.

**Tree-sitter rationale:** Tree-sitter produces an incremental, error-tolerant parser from a grammar definition. On each source edit, it reparses only the changed region (typically O(edit size), not O(file size)). It tolerates syntax errors gracefully, producing a partial tree — essential for editor integration where files are frequently in an invalid state mid-edit. The grammar is written once; the same parser serves both the compiler pipeline and the LSP server.

**Key types (out):**
```rust
// Content-addressable AST subtree — becomes a SourceNode in the eval graph
struct AstFragment {
    hash: ContentHash,        // computed from structure, not identity
    kind: AstFragmentKind,    // ParamDefault, LetBody, ConstraintPredicate, GuardExpr, ...
    span: SourceSpan,         // for diagnostics
    children: Vec<AstFragment>,
}

// Module-level parse result
struct ParsedModule {
    module_path: ModulePath,
    declarations: Vec<Declaration>,  // top-level declarations with AstFragments
    errors: Vec<ParseError>,         // recoverable parse errors
}
```

**Interface contract:**
- **In:** Source text (UTF-8 string) or incremental edit (range + replacement text).
- **Out:** `ParsedModule` with content-addressed `AstFragment`s. On incremental edit, only changed fragments get new hashes; unchanged fragments retain their previous hash (this is Tree-sitter's incremental guarantee, propagated through content addressing).

**Key dependency:** `tree-sitter` crate (Rust bindings to tree-sitter runtime), generated C parser from `tree-sitter-reify/`.

---

### 3.2 `reify-types` — Type System

**Responsibility:** Dimensional type checking, trait conformance, type parameter resolution, common type vocabulary.

**Key types:**
```rust
// 10-element rational exponent vector
struct DimensionVector([Rational; 10]);
// Indices: Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle, SolidAngle, Money

// The core value type — what ValueCells hold
enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(Arc<str>),
    Scalar { value: f64, dimension: DimensionVector },
    Vector { components: Vec<f64>, n: usize, dimension: DimensionVector },
    // ... Matrix, Tensor, Point, Orientation, Frame, Transform
    Enum { type_id: TypeId, variant: u32 },
    List(Arc<[Value]>),
    Set(Arc<BTreeSet<Value>>),       // or im::OrdSet
    Map(Arc<BTreeMap<Value, Value>>),
    Option(Option<Box<Value>>),
    GeometryHandle(GeometryHandleId), // opaque — no user inspection
    FieldHandle(FieldHandleId),       // opaque
    Undef,                            // determinacy: not yet decided
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeterminacyState { Undef, Constrained, Auto, Determined }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Satisfaction { Satisfied, Violated, Indeterminate, Inapplicable }

enum Freshness {
    Final,
    Intermediate { generation: u64 },
    Pending { last_substantive: CachedResultRef },
    Failed { error: ErrorRef },
}

// Snapshot provenance
enum SnapshotProvenance {
    Edit { changed: HashSet<ValueCellId>, parent: SnapshotId },
    Elaboration { changed_scopes: HashSet<ScopeId>, parent: SnapshotId },
    Merge { sources: Vec<SnapshotId>, resolution: ConflictResolution },
    Import { source: ExternalSource },
    Resolution { scope: ScopeId, resolved: HashSet<ValueCellId>, parent: SnapshotId },
}

// Node and cell identifiers — path-based for stable identity across topology changes
struct ValueCellId { entity_path: EntityPath, member: MemberName }
struct NodeId { /* variants for each of 7 node types */ }
```

**Interface contract:**
- **In:** AST declarations with type annotations.
- **Out:** Resolved types, dimensional checks (errors on mismatch), trait conformance results, type parameter bindings.

---

### 3.3 `reify-compiler` — Name Resolution + Elaboration

**Responsibility:** Module DAG construction, name resolution, elaboration of topology templates, production of SourceNodes.

**Key types (out):**
```rust
// The topology template for a scope — produced at compile time
struct TopologyTemplate {
    scope_id: ScopeId,
    // Static declarations: always present
    static_value_cells: Vec<ValueCellDecl>,
    static_constraints: Vec<ConstraintDecl>,
    static_realizations: Vec<RealizationDecl>,
    static_sub_scopes: Vec<ScopeId>,
    // Conditional declarations: present when guard is true
    guarded_groups: Vec<GuardedGroup>,
    // Collection declarations: replicated per element
    collection_decls: Vec<CollectionDecl>,
    // SourceNodes for each declaration unit
    source_nodes: Vec<(SourceNodeId, AstFragment)>,
    // Which ValueCells are structure-controlling (feed SchemaNode via edge #7)
    structure_controlling: HashSet<ValueCellId>,
}

struct GuardedGroup {
    guard_source: SourceNodeId,      // guard expression (edge #1 to SchemaNode)
    guard_value_cell: ValueCellId,   // resolved boolean (edge #7 to SchemaNode)
    members: Vec<Declaration>,       // declarations present when guard is true
}
```

**Interface contract:**
- **In:** `ParsedModule` from syntax crate.
- **Out:** `TopologyTemplate` per scope, resolved `SourceNode`s, module dependency graph, diagnostics.

---

### 3.4 `reify-eval` — Evaluation Engine Core

**Responsibility:** The central runtime. Evaluation graph, snapshots, caching, dependency tracking, scheduling, the two-phase elaboration/evaluation cycle.

**Key types:**
```rust
struct Snapshot {
    version: VersionId,
    graph: EvaluationGraph,
    values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    topology_fingerprint: ContentHash,
    provenance: SnapshotProvenance,
}

// The evaluation graph — persistent data structure with embedded forward edges
struct EvaluationGraph {
    source_nodes: PersistentMap<SourceNodeId, SourceNodeData>,
    value_cells: PersistentMap<ValueCellId, ValueCellNode>,
    constraint_nodes: PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    resolution_nodes: PersistentMap<ScopeId, ResolutionNodeData>,
    realization_nodes: PersistentMap<RealizationNodeId, RealizationNodeData>,
    compute_nodes: PersistentMap<ComputeNodeId, ComputeNodeData>,
    schema_nodes: PersistentMap<ScopeId, SchemaNodeData>,
}

// Cache entry per node
struct NodeCache {
    result: CachedResult,
    freshness: Freshness,
    dependency_trace: DependencyTrace,
    warm_state: Option<OpaqueState>,
    basis_version: VersionId,
}

// Demand registry
struct DemandRegistry {
    always_demanded: HashSet<NodeId>,
    demand_cone: HashSet<NodeId>,  // backward transitive closure, cached
}

// Reverse dependency index (mutable, derived, reconstructible)
struct ReverseDependencyIndex {
    index: HashMap<NodeId, HashSet<NodeId>>,
}
```

**Interface contract:**
- **In (from compiler):** `TopologyTemplate`s and `SourceNode`s for elaboration.
- **In (from constraints):** `SolveResult` from resolution, committed as new snapshots.
- **In (from geometry):** `Representation` results from realization.
- **In (from UI/CLI):** Demand registrations, parameter edits.
- **Out (to constraints):** ConstraintNode evaluation requests with input values.
- **Out (to geometry):** RealizationNode evaluation requests with geometry operation sequences.
- **Out (to UI/CLI):** Current snapshot state, event journal, diagnostics.

**`PersistentMap` implementation:** Initially `im::HashMap` from the `im-rs` crate. The interface is `PersistentMap<K, V>` (a trait alias or newtype), so swapping the backing implementation later does not propagate.

**Content hashing:** Non-cryptographic, 128-bit. Candidate algorithms: xxHash (XXH3-128), wyhash. NaN canonicalization and -0/+0 distinction per architecture section 3.6. Merkle-tree structured — a node's input hash is computed from its dependencies' content hashes.

---

### 3.5 `reify-constraints` — Constraint Orchestrator

**Responsibility:** Classify constraints, dispatch to sub-solvers, manage cross-domain decomposition, drive the resolution process.

**Key types:**
```rust
// Uniform solver interface — all sub-solvers implement this
trait ConstraintSolver: Send + Sync {
    fn capabilities(&self) -> SolverCapabilities;

    fn check(
        &self,
        constraints: &[ConstraintInput],
        values: &ValueMap,
    ) -> Vec<(ConstraintNodeId, Satisfaction, ConstraintDiagnostics)>;

    fn solve(
        &mut self,
        constraints: &[ConstraintInput],
        auto_params: &HashSet<ValueCellId>,
        values: &ValueMap,
        warm_state: Option<&OpaqueState>,
    ) -> SolveResult;
}

enum SolveResult {
    Solved { values: HashMap<ValueCellId, Value>, warm_state: OpaqueState },
    Infeasible { core: HashSet<ConstraintNodeId> },
    NoProgress { reason: DiagnosticRef },
    DidNotConverge { best_so_far: HashMap<ValueCellId, Value>, warm_state: OpaqueState },
}

enum ConstraintDomain { Dimensional, Geometric, Logical, CrossDomain }

// v0.1 static solver mapping
struct SolverRegistry {
    dimensional: Box<dyn ConstraintSolver>,   // NLopt (AUGLAG)
    geometric: Box<dyn ConstraintSolver>,     // SolveSpace libslvs
    logical: Box<dyn ConstraintSolver>,       // OR-Tools CP-SAT
    fallback: Box<dyn ConstraintSolver>,      // NLopt
}
```

**Interface contract:**
- **In:** Set of ConstraintNodes with classified domains, auto parameter set, current ValueCell states, warm state.
- **Out:** `SolveResult` — resolved values or failure diagnostics. The eval engine commits results as new snapshots.

**FFI dependencies:**
| Solver | Library | Language | Binding approach |
|---|---|---|---|
| NLopt | `nlopt` | C | Direct FFI via `nlopt-rust` crate or raw bindings |
| SolveSpace | `libslvs` | C | Direct FFI — clean C API |
| OR-Tools CP-SAT | `or-tools` | C++ | `cxx` bridge or protobuf-based interface |
| Ipopt | `ipopt` | C | Direct FFI via `ipopt-rs` or raw bindings |
| Ceres | `ceres-solver` | C++ | `cxx` bridge |

---

### 3.6 `reify-geometry` — Geometry Engine

**Responsibility:** Multi-kernel dispatch, realization management, tolerance budget tracking, `@optimized` hook registration.

**Key types:**
```rust
// Kernel capability registration
trait GeometryKernel: Send + Sync {
    fn name(&self) -> &str;
    fn supported_operations(&self) -> &[GeometryOp];
    fn supported_representations(&self) -> &[ReprKind];

    fn execute(
        &mut self,
        ops: &[GeometryOp],
        inputs: &[GeometryHandle],
        warm_state: Option<&OpaqueState>,
    ) -> Result<(GeometryHandle, OpaqueState), GeometryError>;

    fn export(&self, handle: &GeometryHandle, format: ExportFormat) -> Result<Vec<u8>, ExportError>;
    fn query(&self, handle: &GeometryHandle, query: GeometryQuery) -> Result<Value, QueryError>;
    fn tessellate(&self, handle: &GeometryHandle, tolerance: f64) -> Result<Mesh, TessError>;
}

enum ReprKind { BRep, Mesh, SDF, Voxel }
enum GeometryOp {
    // Primitives
    Box { width: f64, depth: f64, height: f64 },
    Cylinder { radius: f64, height: f64 },
    Sphere { radius: f64 },
    // ... other primitives
    // Booleans
    Union, Intersection, Difference,
    // Modifications
    Fillet { radius: f64 }, Chamfer { distance: f64 },
    // Sweeps
    Extrude { distance: f64 }, Revolve { angle: f64 },
    // Transforms
    Translate { displacement: [f64; 3] },
    Rotate { axis: [f64; 3], angle: f64 },
    // Queries
    Volume, Area, Centroid, BoundingBox,
}

enum ExportFormat { STEP(STEPVersion), STL { resolution: f64 }, ThreeMF }

// Dispatch planner — selects kernel for each operation
struct DispatchPlanner {
    kernels: Vec<Box<dyn GeometryKernel>>,
    preference_order: Vec<String>,  // kernel names, project-level config
}
```

**Interface contract:**
- **In:** Operation sequence from RealizationNode, input geometry handles, tolerance requirement.
- **Out:** New geometry handle, measurement results, tessellation for viewport.

---

### 3.7 `reify-kernel-occt` — OCCT Binding

**Responsibility:** FFI wrapper implementing `GeometryKernel` for OpenCASCADE.

**Binding approach:** Use `cxx` to bridge to a thin C++ wrapper layer around OCCT. The C++ layer handles OCCT's exception model (`Standard_Failure`) and translates to Rust `Result`. The wrapper exposes a focused API surface — only the OCCT operations Reify needs, not the full 2M-line API.

**Key OCCT types wrapped:**
- `TopoDS_Shape` → opaque `GeometryHandle`
- `BRepPrimAPI_Make*` → primitive construction
- `BRepAlgoAPI_Fuse/Cut/Common` → Booleans
- `BRepFilletAPI_MakeFillet` → fillets
- `STEPControl_Writer` → STEP export
- `BRepMesh_IncrementalMesh` → tessellation

**Warm-start state:** The `TopoDS_Shape` from the previous evaluation. Operation replay seeds from this shape.

---

### 3.8 `reify-runtime` — Runtime Services

**Responsibility:** Task scheduling, warm-start pool management, event journal, cancellation protocol.

**Key types:**
```rust
struct Task {
    node_id: NodeId,
    snapshot: Arc<Snapshot>,
    priority: Priority,
    warm_state: Option<OpaqueState>,
    cancellation_token: CancellationToken,
}

enum Priority { P0Interactive, P1Fast, P1Slow, P3Speculative }

// Event journal — append-only, dual-indexed
struct EventJournal {
    events: Vec<RealizationEvent>,
    by_time: BTreeMap<Instant, Vec<usize>>,     // index into events
    by_node: HashMap<NodeId, Vec<usize>>,       // index into events
}

// Warm-start pool with memory budget
struct WarmStatePool {
    states: HashMap<NodeId, PoolEntry>,
    donated: HashMap<(NodeType, EntityPath), PoolEntry>,
    budget_bytes: usize,
    used_bytes: usize,
}
```

**Async runtime:** Tokio. Compute-bound tasks on `spawn_blocking` or a dedicated thread pool. The eval engine's recursive `evaluate()` is async, with concurrent fan-out at dependency boundaries.

---

### 3.9 `reify-lsp` — Language Server

**Responsibility:** LSP protocol implementation providing diagnostics, completions, hover, go-to-definition.

Built on top of `tower-lsp` crate. Shares the Tree-sitter parser with the compiler pipeline (same grammar, same parse trees). Reads evaluation state from the engine for live diagnostics.

---

### 3.10 `reify-cli` — CLI Binary

**Responsibility:** The user-facing entry point for v0.1.

```
reify check <file.ri>          # parse, type-check, elaborate, evaluate, report constraint status
reify build <file.ri> -o out/  # full pipeline → STEP/STL/3MF export
reify eval <file.ri>           # evaluate and print parameter values + determinacy states
reify lsp                      # start LSP server
```

---

## 4. Implementation Milestones

### Milestone 1: "Hello Bracket" — End-to-End Pipeline

**Goal:** A `.ri` file describing a simple bracket with constraints evaluates to a STEP file.

**Scope:**
- `reify-syntax`: Tree-sitter grammar for the subset: `module`, `import`, `structure def`, `param`, `let`, `constraint` (inline), `sub` (non-collection), basic expressions (arithmetic, comparisons, function calls), unit literals.
- `reify-types`: Dimensional type checking for the subset. 9-element exponent vectors. Basic type inference for `let` bindings.
- `reify-compiler`: Name resolution (single module + prelude). Basic elaboration (no guards, no collections, no auto types). Produce `TopologyTemplate` with static declarations only.
- `reify-eval`: Minimal sequential evaluator. `Snapshot` as a plain `HashMap` (real HAMT comes in M2). `evaluate()` is synchronous recursive descent, no caching, no scheduling. ValueCells, ConstraintNodes, RealizationNodes only.
- `reify-constraints`: Constraint *checking* only (no solving). Evaluate predicates against determined values, report `Satisfied`/`Violated`.
- `reify-geometry` + `reify-kernel-occt`: OCCT binding for primitives (`box`, `cylinder`, `sphere`), Booleans (`union`, `difference`), `fillet`, `translate`, `rotate`. STEP export.
- `reify-cli`: `reify check` and `reify build` commands.
- `reify-stdlib`: Minimal prelude — SI units, basic math functions, `point3`/`vec3` constructors.

**Example input:**
```
module examples.bracket

structure def Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    param thickness : Length = 5mm
    param hole_diameter : Length = 10mm
    param fillet_radius : Length = 3mm

    constraint thickness > 2mm
    constraint thickness < width / 4
    constraint hole_diameter < height / 2

    let volume = width * height * thickness
}
```

**Exit criterion:** `reify build examples/bracket.ri -o bracket.step` produces a valid STEP file. `reify check` reports constraint satisfaction status.

---

### Milestone 2: "Real Engine" — Incremental Evaluation

**Goal:** The evaluation engine implements the architecture's core incrementality model.

**Scope:**
- `reify-eval`: HAMT-backed `PersistentMap` (via `im-rs` initially). Content-hash caching with version fast path. Dependency tracking (Adapton-style dynamic traces). Dirty/demand cone computation via reverse dependency index. Early cutoff per node type. Snapshot provenance tracking.
- `reify-runtime`: Basic task scheduler (sequential with priority ordering — not yet concurrent). Freshness tracking (Final/Intermediate/Pending/Failed).
- **Incremental engine deep dive:** By this milestone, we need the Salsa vs custom decision resolved. The M1 evaluator was intentionally simple; M2 replaces it with the real thing.

**Exit criterion:** Changing a parameter in a loaded design re-evaluates only the dirty-demand intersection. Content-hash matches on unchanged subtrees prevent recomputation. Freshness propagates correctly.

---

### Milestone 3: "Auto Resolution" — Constraint Solving

**Goal:** `auto` parameters are resolved by the constraint orchestrator.

**Scope:**
- `reify-eval`: ResolutionNode implementation. Trial snapshot mechanism. Resolution as state progression (helix model). Bottom-up resolution tree traversal.
- `reify-constraints`: NLopt integration for dimensional `auto` resolution. Constraint classification. Basic optimization (`minimize`/`maximize` as sugar for optimization constraints). `SolveResult` types and failure mode handling.
- `reify-compiler`: Support for `auto` keyword in param declarations. Strict vs free auto distinction.
- Updated stdlib: `auto` support in constraint predicates.

**Exit criterion:** A structure with `param thickness : Length = auto` and constraints resolves `thickness` to a feasible value. `minimize mass` finds the optimal value within constraints.

---

### Milestone 4: "Living Design" — Interactive Loop

**Goal:** Concurrent evaluation, warm starting, cancellation — the interactive editing experience.

**Scope:**
- `reify-runtime`: Tokio-based work-stealing thread pool. Priority levels (P0/P1-fast/P1-slow/P3). Cooperative cancellation with tokens. Commitment policy (dual-threshold). Priority promotion.
- `reify-eval`: Concurrent `evaluate()` with async fan-out. Warm-start protocol (`WarmStartable` trait). Warm-state pools with memory-budgeted LRU eviction. Event journal (append-only, dual-indexed).
- `reify-kernel-occt`: Warm-start support — operation replay seeding from previous `TopoDS_Shape`.
- `reify-lsp`: LSP server with live diagnostics, hover info, go-to-definition, basic completions.

**Exit criterion:** Editing a parameter in an editor with LSP produces updated diagnostics within a few hundred milliseconds. Long-running realization tasks are cancellable. Warm start shows measurable speedup over cold recomputation.

---

### Milestone 5: "Language Breadth" — Full Language Coverage

**Goal:** The remaining language constructs are implemented.

**Scope (incremental, in rough priority order):**
1. **Traits and trait bounds** — compile-time trait conformance, multiple inheritance, diamond resolution.
2. **Sub-structures with collections** — `sub vents : List<Vent>`, count constraints, positional indexing.
3. **Guards** — `where` clauses, conditional declarations, structural presence/absence.
4. **Connect/chain** — port compatibility, frame alignment constraints, connector instantiation.
5. **Occurrences** — process entities with in/out ports.
6. **Enums and match** — C-style enums, exhaustive matching, desugaring to guarded declarations.
7. **Fields** — `Field<D, C>` type, analytical/sampled/composed, field composition, differential operators.
8. **Purposes** — activation/deactivation, scoped constraint injection, output occurrences.
9. **Functions** — `fn` with type parameters, recursion, overloading, `@optimized` hook.
10. **More geometry** — sweeps (extrude, revolve, loft, sweep), patterns (linear, circular, mirror), queries (distance, area, volume, centroid, bounding_box, edge/face selectors).
11. **More constraint domains** — geometric constraints via SolveSpace `libslvs`, logical constraints via OR-Tools CP-SAT.
12. **Multi-module** — `import`, module DAG, re-exports, prelude suppression.

---

### Milestone 6: "Visual" — GUI for Alpha Testing

**Goal:** A minimal 3D viewport and property editor for alpha testing.

**Technology decision deferred.** Options:
- **wgpu + egui** — pure Rust, native performance, but significant UI work.
- **Tauri + WebGL** — web technologies in a desktop shell, faster UI iteration.
- **Web-native** — engine compiled to WASM, browser UI. Enables cloud deployment.

**Scope:**
- 3D viewport rendering tessellated geometry from RealizationNodes.
- Property editor reading/writing ValueCells.
- Constraint panel showing ConstraintNode status.
- Demand registry integration — viewport visibility drives demand.
- Snapshot-versioned UI updates (only apply updates from current snapshot).

---

## 5. Interface Contract Summary

```
                    ┌──────────────┐
                    │  .ri source  │
                    └──────┬───────┘
                           │ UTF-8 text / incremental edits
                           ▼
                   ┌───────────────┐
                   │ reify-syntax  │
                   │  (Tree-sitter)│
                   └───────┬───────┘
                           │ ParsedModule { declarations, AstFragments (content-hashed) }
                           ▼
                  ┌─────────────────┐
                  │ reify-compiler  │
                  │ (name res +     │
                  │  elaboration)   │
                  └────────┬────────┘
                           │ TopologyTemplates, SourceNodes, TypeInfo, diagnostics
                           ▼
              ┌────────────────────────┐
              │      reify-eval        │
              │  (evaluation engine)   │◄──── parameter edits, demand registrations
              │                        │       (from CLI / LSP / GUI)
              │  Snapshots (HAMT)      │
              │  Eval graph (7 nodes)  │
              │  Cache (content-hash)  │
              │  Two-cone scheduling   │
              └───┬──────────┬─────┬───┘
                  │          │     │
    ConstraintNode│  Realiz- │     │ ValueCell reads,
    eval requests │  ation   │     │ diagnostics, events
                  │  requests│     │
                  ▼          ▼     ▼
        ┌──────────┐  ┌──────────┐  ┌──────────────┐
        │reify-    │  │reify-    │  │ reify-lsp /  │
        │constraint│  │geometry  │  │ reify-cli    │
        │s         │  │          │  └──────────────┘
        └────┬─────┘  └────┬─────┘
             │              │
    ┌────────┼──────┐  ┌────┼────────────┐
    │        │      │  │    │            │
    ▼        ▼      ▼  ▼    ▼            ▼
  NLopt   libslvs  OR  OCCT  Manifold  libfive
          Tools
```

### Key boundary contracts

| Boundary | Direction | Data | Format |
|---|---|---|---|
| syntax → compiler | push | `ParsedModule` | Rust types (in-process) |
| compiler → eval | push | `TopologyTemplate`, `SourceNode` | Rust types (in-process) |
| eval → constraints | pull | `ConstraintInput` (node + typed values) | Rust trait call |
| constraints → eval | push | `SolveResult` (resolved values) | Rust types |
| eval → geometry | pull | `RealizationRequest` (op sequence + handles) | Rust trait call |
| geometry → eval | push | `GeometryHandle` + measurement `Value`s | Rust types |
| eval → runtime | push | `Task` (node + snapshot + priority) | Async channel |
| runtime → eval | push | Task completion notification | Async channel |
| eval → LSP/CLI | pull | Snapshot state, diagnostics, events | Rust API (in-process) |
| LSP/CLI → eval | push | Parameter edits, demand changes | Rust API (in-process) |

All in-process boundaries use Rust types and trait calls. No serialization within the process. The distribution-readiness noted in the architecture (section 12.6) means these interfaces could later be backed by serialization, but v0.1 is single-process.

---

## 6. Key Dependencies (External Crates and Libraries)

| Crate/Library | Purpose | License |
|---|---|---|
| `tree-sitter` | Parser runtime | MIT |
| `im` | Persistent data structures (HAMT) | MPL-2.0 |
| `tokio` | Async runtime + work-stealing | MIT |
| `xxhash-rust` | Content hashing (XXH3-128) | MIT/Apache-2.0 |
| `cxx` | C++/Rust FFI bridge | MIT/Apache-2.0 |
| `tower-lsp` | LSP protocol implementation | MIT |
| OpenCASCADE | B-rep geometry kernel | LGPL-2.1 |
| Manifold | Mesh Booleans | Apache-2.0 |
| NLopt | Nonlinear optimization | LGPL-2.1 |
| SolveSpace `libslvs` | Geometric constraint solver | GPL-3.0 (*) |
| OR-Tools | Constraint programming | Apache-2.0 |

Reify is AGPL-3.0, which is compatible with `libslvs` GPL-3.0 (GPL code can be included in AGPL projects). No isolation or replacement needed.

---

## 7. Remaining Deep Dives

| # | Topic | When | Blocking |
|---|---|---|---|
| 1 | **Salsa vs custom incremental engine** | Before M2 implementation | M2 |
| 2 | **OCCT binding approach** — evaluate `opencascade-rs` maturity vs custom `cxx` wrapper | Before M1 implementation | M1 |
| 3 | **SolveSpace GPL licensing** — accept GPL, isolate via IPC, or replace with Ceres | Before M3 | M3 |
| 4 | **GUI technology** — wgpu+egui vs Tauri vs web-native | Before M6 | M6 |
| 5 | **Tree-sitter grammar authoring** — prototype the Reify grammar in tree-sitter's JS DSL | M1 first task | M1 |
| 6 | **Content hash algorithm selection** — benchmark xxHash vs wyhash vs others for our workload | M2 | M2 |
| 7 | **Tolerance budget allocation** — error budgets across conversion chains | Post-M5 | Not blocking |
| 8 | **Persistent naming problem** — geometric selectors across topology changes | M5 (geometry breadth) | M5 partial |
