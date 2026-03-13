# Geometry Engine: Design Decisions

**Status:** Representation model and kernel integration architecture established — ready for evaluation graph design and geometry ontology detailing  
**Version:** 0.1 — First crystallization from geometry engine design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1

---

## 1. Design approach

The geometry engine design was approached from the language semantics downward: what does the language's existing ontology and type system imply about how geometry should work, and how do implementation-level geometry kernels plug in without breaking the abstraction?

The central finding is that **the language's existing design principles — immutable structures, fields as first-class entities, geometry as a parameter, and the `@optimised` hook — already determine the geometry engine architecture**. The geometry of a structure is defined by the composition of declarations in the source text. Geometric representations (B-rep, mesh, SDF, voxels) are implementation-level realizations, not language-level concepts. Kernel selection is a runtime inference problem, not a language-level annotation. No new core language features are required.

---

## 2. The canonical geometry is the source text

### 2.1 Geometry is specification, not representation

The geometry of a structure is defined by its declaration in the language: the composition of primitives, operations, sub-structures, constraints, and fields that together specify a mathematical object. A B-rep model, a triangle mesh, an SDF grid, a voxel octree — these are all *realizations* produced by the implementation to serve specific downstream operations. None is "the geometry." The geometry is the specification.

This is consistent with how the language already treats structures. A structure is an immutable snapshot of a physical configuration (ontology §2.1, §8.3). The source text is the design; the runtime produces computational artifacts from it. Geometry is one such artifact — the most important one, but not ontologically different from a constraint solution or a simulation result.

**Design decision:** There is no "canonical geometric model" in the implementation. There are only realizations, each created because some downstream operation needs one, each managed by the evaluation infrastructure (§4), each cached and invalidated through a dependency graph. No realization is privileged. All are contingent on the operations they serve.

### 2.2 What this dissolves

This framing eliminates several questions that would otherwise require complex design:

- **"What is the primary representation?"** — There is no primary. Different operations demand different realizations. The implementation produces whichever are needed.
- **"How do representations stay in sync?"** — They don't need to stay "in sync" because they are independent derivations from the source specification. If the source changes, dependent realizations are invalidated and recomputed. No convergent modeling problem exists at the language level.
- **"When does representation conversion happen?"** — When an operation requires a realization type that doesn't yet exist or has been invalidated. Conversion is demand-driven, managed by the evaluation infrastructure.

### 2.3 Relationship to existing design principles

This is a direct consequence of two prior decisions:

1. **Structures are immutable within the design system** (ontology §2.1). The source text defines a fixed mathematical specification. The runtime evaluates it. Geometry follows the same pattern.
2. **Geometry is a parameter** (ontology §2.1). Like any parameter, it can be `undef`, `auto`, or determined. Its value is a mathematical object, not a data structure in a specific format.

---

## 3. Geometric types in the type system

### 3.1 Mathematical types, not representational types

The language's geometric type hierarchy describes *what something is* mathematically and topologically. It does not describe *how it is stored or computed*. There are no `BRepSolid` or `MeshSurface` types at the language level.

**Core geometric entity types:**

| Type | What it represents |
|---|---|
| `Solid` | A closed bounded region of 3D space (a volume) |
| `Shell` | A connected set of faces bounding a region (the skin of a solid) |
| `Surface` | A 2D manifold embedded in 3D space |
| `Curve` | A 1D manifold embedded in 2D or 3D space |
| `Point` | A 0-dimensional position in space |
| `PointCloud` | An unordered collection of points |

These types may be parameterised by spatial dimensionality where it makes sense (a `Curve` in 2D vs. 3D), following the existing pattern of geometric type parameterisation (type system §2.3).

### 3.2 Geometric property traits

Properties of geometric objects are expressed through traits, consistent with the language's sole abstraction mechanism (ontology §5.1):

| Trait | Meaning |
|---|---|
| `Closed` | The object has no boundary (a closed curve, a closed surface) |
| `Manifold` | Every point has a neighbourhood homeomorphic to ℝⁿ |
| `Orientable` | A consistent normal/orientation can be defined everywhere |
| `Convex` | Every line segment between interior points lies in the interior |
| `Connected` | Cannot be separated into disjoint non-empty open subsets |
| `Bounded` | Fits within a finite bounding box |
| `Watertight` | Closed + Manifold (common engineering shorthand) |

Traits compose naturally: `Solid` implies `Closed + Bounded + 3D`. `Manifold` can be required by operations that need it (e.g., certain Boolean algorithms). The type system checks these at the level that determinacy allows — a fully determined solid is verified watertight; an `undef` geometry trivially satisfies all trait requirements (consistent with conformance checking, type system §4.6).

### 3.3 Geometry as opaque handles

A geometric value in the language is an opaque handle. The designer cannot inspect "the vertices" or "the control points" — those are representation-specific concepts that don't exist at the language level. The designer works with the mathematical object through operations: `union(a, b)`, `fillet(solid, edge, radius)`, `distance(p, surface)`.

This opacity is essential for representation independence. If the language exposed B-rep topology (faces, edges, vertices), it would be impossible to back the same type with an SDF kernel. The mathematical operations are the interface; the kernel provides the implementation.

**Access to representation-specific details** is needed for interop (exporting a mesh, inspecting B-rep topology for debugging). This is provided through explicit representation-access operations that are understood to be implementation-level, not language-level:

```
// Language level — mathematical
param body : Solid = union(cylinder(r = 5mm, h = 20mm), box(10mm, 10mm, 5mm))

// Representation access — implementation level, explicit, not the normal workflow
// (syntax illustrative, not final)
#inspect body as BRep  // Tooling/debug command, not a language expression
```

The exact mechanism for representation access is deferred to implementation design. The principle is: the language works with mathematical objects; representation details are available through explicit tooling interfaces when needed.

### 3.4 Geometry and the field system

Geometry can be represented as a field: an implicit/SDF representation is a `Field<Point3<Length>, Scalar<Length>>` where the zero-level-set defines the boundary (ontology §2.4). This is already a first-class concept in the language. The geometry engine must support this bidirectionally:

- A field can define geometry (SDF → implicit surface → operations that work on implicit geometry).
- Geometry can be sampled as a field (solid → distance field → used in field-driven design such as lattice density mapping).

This bidirectional relationship is a realization concern, not a type system concern. A `Solid` is a `Solid` regardless of whether it originated from CSG operations or from a field's zero-level-set. The evaluation infrastructure handles the conversion.

---

## 4. The evaluation graph: generalised implementation infrastructure

### 4.1 The key insight

The implementation needs to produce concrete computational artifacts from the abstract design specification. This is true for geometry (producing B-rep models, meshes, SDF grids), for constraints (producing solved parameter values), for simulation (producing stress fields, thermal distributions), and for manufacturing (producing toolpaths, build layouts). Every one of these has the same computational structure: inputs from the design (or from other evaluations), a computation, outputs that are cached and depend on the inputs.

**Design decision:** A single **evaluation graph** infrastructure handles all implementation-level computation uniformly. Geometric realization, constraint solving, simulation, and manufacturing output generation are all evaluation tasks managed by the same infrastructure. They differ in what they compute, not in how they're managed.

### 4.2 What the evaluation graph provides

| Capability | Description |
|---|---|
| **Demand-driven evaluation** | Lazy: only compute what's needed. A realization is produced when an operation or export demands it, not when the geometry is defined. |
| **Dependency tracking** | Every evaluation task knows what inputs it depends on — source declarations, parameter values, results of other evaluations. |
| **Invalidation** | When an input changes (designer edits source, parameter becomes determined), dependent tasks are marked stale. |
| **Incremental recomputation** | Only stale tasks are re-evaluated. The scope of recomputation is bounded by the dependency graph. |
| **Concurrency** | Independent subgraphs evaluate in parallel. A mesh generation and a constraint solve that don't depend on each other run concurrently. |
| **Caching** | Evaluation results are retained until invalidated. Repeated queries for the same realization don't recompute. |

This is the same pattern as incremental build systems (Bazel), incremental compilers (Rust's query system, Salsa), and reactive dataflow systems. The engineering-specific observation is that constraint solving, geometric modeling, and simulation are all instances of this pattern, and treating them uniformly avoids building separate infrastructures that each reinvent the same mechanisms.

### 4.3 Geometry in the evaluation graph

Geometric realizations are nodes in the evaluation graph. A structure's geometry specification (its declaration in the source text) is the root input. Downstream nodes produce realizations as needed:

```
[Source: bracket body = union(plate, rib)]
         │
         ├──→ [B-rep realization] ──→ [STEP export]
         │                        ──→ [GD&T evaluation]
         │
         ├──→ [Mesh realization] ──→ [FEA stress analysis]
         │                       ──→ [STL export]
         │
         └──→ [SDF realization] ──→ [Lattice infill generation]
                                ──→ [Field-driven density mapping]
```

Each realization node tracks its dependencies (the source specification, any parameters that affect the geometry, the tolerance requirements). When the source changes, all realizations are invalidated. When a tolerance requirement changes (e.g., switching from early-design exploration to manufacturing output), affected realizations may need recomputation at higher fidelity.

### 4.4 Relationship to the constraint orchestrator

The constraint system's orchestrator (constraint system §2) is a specialisation of the evaluation graph for constraint solving. The constraint engine is a set of evaluation tasks — dispatching to sub-solvers, managing feedback between them, resolving `auto` parameters. These tasks live in the same graph as geometric realizations.

This means constraint results and geometric realizations can depend on each other naturally:

- A geometric realization depends on parameter values that are resolved by the constraint solver (a fillet radius determined by `auto`).
- A constraint evaluation depends on geometric queries (a DFM constraint checking minimum wall thickness requires a geometric realization to measure against).

The evaluation graph handles these cross-domain dependencies through the same dependency tracking mechanism, without special-casing the geometry-constraint interaction.

### 4.5 The evaluation graph is an implementation architecture, not a language concept

The evaluation graph does not appear in the language. The language has structures, parameters, constraints, fields, and purposes. The evaluation graph is how the runtime *implements* the evaluation of these language-level concepts. The designer interacts with it indirectly:

- **Purposes** (ontology §8.2) trigger evaluation of relevant subgraphs ("evaluate this design for manufacturing readiness").
- **`auto` resolution** triggers constraint evaluation tasks.
- **Export operations** trigger geometric realization tasks.
- **Diagnostics** from the evaluation graph surface through the language's existing diagnostic mechanisms (constraint violations, `undef` propagation traces, determinacy reports).

---

## 5. Kernel integration: implicit dispatch

### 5.1 The dispatch problem

Given an evaluation task that requires a geometric operation (Boolean union, fillet, meshing, SDF evaluation), the runtime must choose a kernel to perform it. Multiple kernels may be available, each supporting different operations on different representations with different performance and robustness characteristics.

### 5.2 Implicit dispatch as the default

**Design decision:** The runtime infers which kernel(s) to invoke based on the operations required, the realizations available, and the registered kernel capabilities. No language-level annotation directs kernel selection in the normal case. The designer doesn't need to know or care which kernel is doing the work.

The runtime's dispatch logic considers:

1. **What operation is needed** — Boolean union, fillet, mesh generation, etc.
2. **What realizations are currently available** — if a B-rep already exists and the operation works on B-rep, prefer that path to avoid conversion.
3. **What kernels support this operation** — from the runtime's registry of kernel capabilities.
4. **What downstream operations will follow** — planning ahead to minimise representation conversions across a chain of operations.
5. **Tolerance requirements** (§6) — some kernels guarantee tighter tolerances than others.

### 5.3 Determinism

Dispatch must be deterministic given a fixed runtime configuration. The same design, evaluated with the same kernel configuration, must produce the same results. This means:

- The runtime configuration (which kernels are available, which versions, preference ordering) is project-level metadata — pinned, versioned, reproducible.
- Within a given configuration, the dispatch algorithm is deterministic. No randomness, no race-condition-dependent choices.
- Different configurations may produce different results (a mesh from OCCT's mesher vs. a mesh from CGAL's mesher), but the specification-level geometry is the same. The differences are within the tolerance bounds managed by §6.

### 5.4 Inspectability and override

The dispatch plan is inspectable: the designer can query what kernel was chosen for any evaluation task and why. This is a tooling/debugging interface, not a language construct.

Explicit override is available via pragma when the designer needs control:

```
#kernel(occt)  // Force OCCT for geometric operations in this scope
structure def PrecisionBracket {
    // ...
}
```

Pragmas are toolchain directives that do not change the meaning of the program (syntax §9.2). A design that works with one kernel must work (within tolerance) with another. The pragma controls *which* implementation is used, not *what* the geometry is.

### 5.5 Kernel registration

Kernels register their capabilities with the runtime. This registration is a runtime configuration concern, not a language-level construct:

- **What representations the kernel works with** — B-rep, mesh, SDF, voxel, etc.
- **What operations the kernel supports** — Boolean operations, filleting, meshing, SDF evaluation, format import/export, etc.
- **Quality and performance characteristics** — tolerance guarantees, scalability limits, operation cost estimates.

The registration mechanism and format are implementation details to be designed in the implementation architecture phase.

### 5.6 `@optimised` and kernel bindings

The `@optimised` hook (ontology §2.3) serves as the semantic equivalence bridge between language-level definitions and kernel implementations. Every geometric operation has a language-level semantic definition (e.g., Boolean union defined as point-set union). Kernel implementations are semantically equivalent fast paths.

For standard library operations (union, intersection, fillet, extrude, etc.), the `@optimised` annotations live in the **kernel binding layer** — the code that bridges a specific kernel to the runtime. They do not appear in the language-level standard library definitions that users read and write.

For user-authored geometric operations, `@optimised` is available in the source text to register custom fast-path implementations. This is the exception, not the normal workflow.

### 5.7 What this means in practice

The designer writes:

```
structure def Bracket : RigidMechanical {
    param body : Solid = union(
        box(50mm, 30mm, 5mm),
        translate(vec(0mm, 0mm, 5mm), box(5mm, 30mm, 40mm))
    )
}
```

The runtime:

1. Recognises that `body` requires evaluation of `union`, `box`, and `translate` operations.
2. Consults the kernel registry — OCCT supports all three on B-rep; Manifold supports union and box on mesh; libfive supports all three on SDF.
3. Considers what downstream operations will need `body` — if a STEP export is pending, B-rep is required; if only visualisation, mesh is sufficient.
4. Selects the kernel and representation that best serves the current demand with minimum conversion.
5. Produces the realization, caches it, and tracks its dependencies.

The designer is unaware of steps 2–5 unless they choose to inspect the evaluation graph.

---

## 6. Representation tolerance

### 6.1 The problem

Representations are approximations. A mesh approximates a mathematical surface. An SDF grid samples a continuous function at discrete points. Even B-rep involves floating-point arithmetic with finite precision. The gap between the mathematical specification and any realization is unavoidable.

This gap matters in practice. A part designed to ±2.5μm tolerance can be out of spec already in the STEP file if the B-rep kernel's internal tolerance is too loose. Downstream operations inherit and potentially amplify representation error.

### 6.2 Representation tolerance as a bidirectional contract

**Design decision:** A **representation tolerance** specifies the maximum acceptable geometric deviation between the mathematical specification and any realization of it. It is a bidirectional contract:

- **Upstream (requirement):** the evaluation graph must produce realizations accurate to this bound. This drives kernel selection, mesh density, SDF resolution, and error budgeting across intermediate conversion steps.
- **Downstream (guarantee):** consumers of the realization may rely on it being accurate to this bound. Downstream operations can use this guarantee in their own error analysis.

The same language construct binds both directions. A tolerance annotation at any node in the evaluation graph simultaneously constrains the realizations that feed it and promises a fidelity level to everything that consumes it.

### 6.3 Where tolerance lives

**Primary: at the purpose level.** A manufacturing purpose carries a tolerance requirement derived from the manufacturing process's capabilities and the part's dimensional specifications. An early-design exploration purpose carries a looser tolerance for speed. The purpose mechanism (ontology §8.2) is the natural and recommended place to express tolerance requirements:

```
purpose def manufacturing_ready {
    require all geometric_params determined
    representation_tolerance = 1um    // Sub-micron for precision machining
}

purpose def early_exploration {
    representation_tolerance = 200um  // Coarse for speed during conceptual design
}
```

**Escape hatch: at the entity level.** For cases where one region of the design needs tighter control than the rest — a precision datum surface on an otherwise coarsely-toleranced casting, for example — tolerance can be specified directly on a structure or sub-structure. This is the exception, not the normal workflow.

### 6.4 Representation tolerance is distinct from design tolerance

Representation tolerance (accuracy of the computational model relative to the mathematical specification) and design tolerance (acceptable variation in the physical artifact, expressed through GD&T and dimensional tolerances) are orthogonal:

- A part can have tight GD&T and loose representation tolerance (early-stage design exploration of a precision component).
- A part can have loose GD&T and tight representation tolerance (generating precise toolpaths for a part with generous specs).

The language and runtime must not conflate these. Design tolerances are constraints on the design (expressed through the constraint system). Representation tolerances are requirements on the implementation (expressed through the tolerance mechanism described here).

### 6.5 Tolerance and the evaluation graph

Tolerance flows through the evaluation graph as a property of realization nodes. When multiple downstream operations require different tolerances, the tightest requirement governs (or separate realizations are produced for different tolerance levels, if the cost difference justifies it).

The runtime manages a **tolerance budget** across chains of operations. A sequence of conversions (B-rep → mesh → SDF → voxel) accumulates error at each step. The runtime allocates per-step error budgets such that the total stays within the end-to-end tolerance requirement. This is a runtime heuristic, not a language-level concern — the designer expresses the end-to-end requirement; the runtime figures out how to meet it.

### 6.6 Imported geometry and declared tolerance

Geometry imported from external sources (STEP files, STL meshes, downloaded models) arrives with unknown representation fidelity. The runtime can estimate bounds (from mesh resolution, from STEP file tolerance headers), but cannot guarantee anything about the source.

The designer can declare a tolerance on imported geometry:

```
sub bracket_mesh : Solid = import("bracket.stl") {
    representation_tolerance = 50um  // Designer's assertion about import fidelity
}
```

This declaration is both an assertion ("I trust this mesh to ±50μm") and a promise to downstream consumers ("you may rely on this being accurate to ±50μm"). The designer takes responsibility for the claim — the runtime cannot verify it for external geometry. This is consistent with how the language handles external data generally: the language provides the declaration mechanism; correctness of external inputs is the designer's responsibility.

---

## 7. Multi-representation coexistence in practice

### 7.1 The stack pattern

A non-trivial design often requires a conceptual stack of representations that can at most only incompletely represent each other:

1. A B-rep of a part volume (authored geometry)
2. A mesh of that B-rep for FEA
3. A stress/strain field from FEA
4. A density field derived from the stress field
5. An implicit lattice structure driven by the density field
6. A voxel octree representing the SLA-printable result

Each step in this stack is a node in the evaluation graph. Each depends on the previous. The evaluation graph manages the chain: if the B-rep changes, the mesh is invalidated, which invalidates the FEA, which invalidates the density field, and so on. Recomputation propagates through the chain as needed.

The language expresses this chain naturally using its existing constructs — structures, fields, and the purpose/determinacy system:

```
structure def OptimisedBracket : RigidMechanical {
    param body : Solid = union(plate, rib)

    field stress : Point3<Length> -> Tensor<2, 3, Pressure> = fem_analysis(body, loads)

    field density : Point3<Length> -> Scalar<Dimensionless> = composed {
        |p| clamp(von_mises(stress(p)) / yield_strength, 0.15, 1.0)
    }

    sub infill : Solid = lattice_infill(body, density, cell = Gyroid, min_wall = 0.4mm)
}
```

The designer writes this as a coherent specification. The runtime decomposes it into an evaluation graph with nodes for B-rep generation, meshing, FEA, field computation, lattice generation, and voxelisation — each using the appropriate kernel, each cached and dependency-tracked.

### 7.2 The patchwork pattern

An assembly may contain sub-structures with geometry from heterogeneous sources:

- An SLA-printed part with implicit/voxel geometry
- Standard fasteners represented as B-rep models from a library
- A downloaded STL mesh of a third-party component

Each sub-structure's geometry arrives in (or is best suited to) a different representation. The assembly's spatial composition (containment tree, transforms, connections) is representation-agnostic — it operates on mathematical objects via frames and transforms, regardless of how each sub-structure is realised.

Operations that span the assembly (interference checking, assembly-level visualisation, system-level simulation) require compatible realizations. The evaluation graph handles this by producing the required realization for each sub-structure as demanded by the spanning operation — meshing the B-rep fasteners and converting the SLA part's voxels to mesh, for example, to enable mesh-based interference checking.

The designer does not need to manage these conversions. The evaluation graph infers them from the operation's requirements.

---

## 8. Alternatives considered

### 8.1 Geometry as representation-parameterised type

`Solid<BRep>`, `Solid<Mesh>`, `Solid<SDF>` — making representation a type parameter. Rejected: this forces representation awareness into the language level, breaking abstraction. Operations that don't care about representation would need to be generic over it, adding syntactic noise. And it implies that a solid stored as B-rep is a different *type* from the same solid stored as mesh, which is mathematically wrong.

### 8.2 Primary representation with derived others

One "authored" representation as the source of truth, with other representations derived on demand. Rejected as a language-level concept: it privileges one realization over others. However, the source text *specification* is the true "primary" — this insight dissolved the question entirely (§2.1). The evaluation graph may internally track derivation chains (mesh was derived from B-rep), but this is implementation bookkeeping, not a language concept.

### 8.3 Convergent modeling at the language level

Peer representations with bidirectional sync (the Parasolid convergent modeling approach). Rejected: this is an unsolved research problem even commercially. The constraint synchronization between representations is enormously complex. The language's approach — source specification as canonical, all realizations as derived artifacts — avoids the problem entirely.

### 8.4 Explicit kernel dispatch via annotations

`@kernel("occt")` or `@use(Manifold)` on operations or structures. Rejected as the default mechanism: it couples the design to specific implementations, violates the principle that the runtime infers implementation details, and creates maintenance burden when kernels are updated or swapped. Available as a pragma override for the rare cases where explicit control is needed (§5.4).

---

## 9. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Canonical geometry | Source text specification | Consistent with immutable structures; no representation privileged |
| Geometric type hierarchy | Mathematical (Solid, Surface, Curve, etc.) | Representation-independent; matches type system philosophy |
| Geometric properties | Traits (Closed, Manifold, Convex, etc.) | Consistent with language's sole abstraction mechanism |
| Geometric values | Opaque handles | Enables representation independence; prevents leaky abstraction |
| Implementation infrastructure | Generalised evaluation graph | Unifies geometry, constraint solving, simulation, and manufacturing |
| Kernel selection | Implicit runtime dispatch | Keeps implementation details out of the language; deterministic given config |
| `@optimised` role | Semantic equivalence bridge in kernel binding layer | Not in user-facing source for standard operations |
| Representation tolerance | Bidirectional contract (requirement + guarantee) | Manages unavoidable approximation; placed primarily at purpose level |
| Imported geometry tolerance | Designer-declared | Honest about external data; consistent with tolerance contract model |

---

## 10. Open questions for subsequent design phases

### 10.1 Evaluation graph design (next major piece)

The evaluation graph is identified as the critical shared infrastructure. Its detailed design — task scheduling, dependency representation, invalidation strategies, concurrency model, incremental recomputation algorithms, diagnostic surfacing — is a foundational implementation architecture decision. Likely warrants its own dedicated design session.

### 10.2 Geometry ontology

The full set of geometric primitives (box, cylinder, sphere, cone, torus, extrusion, revolution, sweep, loft, etc.), operations (union, intersection, subtraction, fillet, chamfer, shell, offset, etc.), and their type signatures needs to be specified. Traits for geometric properties need enumeration. This is a substantial design task downstream of the decisions made here.

### 10.3 Tolerance management details

The tolerance budget allocation across conversion chains, the runtime heuristics for balancing fidelity against performance, the interaction between representation tolerance and kernel-specific tolerance parameters (e.g., OCCT's per-operation tolerance settings), and the diagnostics for tolerance violations all need detailed specification.

### 10.4 Geometric queries and selectors

The ad-hoc port syntax (`@face`, `@region`, `@edge` — syntax §6.4) requires a geometric query engine that works across representations. How are named features identified and tracked across realizations? How do topological queries (adjacent faces, connected edges) work through opaque handles? This connects to the persistent naming problem in parametric CAD.

### 10.5 External geometry import

The mechanisms for importing geometry from external formats (STEP, STL, OBJ, 3MF, etc.), the metadata preserved through import, the tolerance declaration workflow, and the integration of imported geometry into the evaluation graph need specification.

### 10.6 Field-to-geometry bridge details

The bidirectional relationship between geometry and fields (§3.4) — SDF fields defining geometry, geometry sampled as distance fields — needs detailed specification of conversion triggers, accuracy guarantees, and interaction with the evaluation graph.

---

*Document generated from geometry engine design sessions. Intended as a living specification to be refined through subsequent design phases.*
