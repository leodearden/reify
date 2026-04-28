# PRD: Multi-Kernel Geometry Dispatch

Status: deferred to v0.2 per 2026-04-26 decision.
Design resolved 2026-04-28 — see "Resolved design decisions" below.

## Goal

Implement the multi-kernel implicit dispatch architecture described in `docs/reify-implementation-architecture.md` §10.3, §10.5–§10.8. The runtime should select among OCCT, Manifold, Truck, Fidget, and OpenVDB on a per-operation basis, drive conversions through the evaluation graph, and support the geometry-as-orchestrator pattern with stack and patchwork representation arrangements.

## Background

v0.1 ships an OCCT-only geometry backend. This is sufficient to deliver B-rep modeling, STEP/IGES/STL/glTF I/O, and most parametric workflows, but it has well-documented weaknesses:

- Boolean failures are common on complex geometry (the most-cited OCCT pain point).
- OCCT is largely single-threaded and ~2 GB resident.
- It has no native SDF/voxel path; field-defined geometry is forced through B-rep meshing.

The architecture has always been multi-kernel: §10.1 establishes that no realization is privileged. §10.3 defines implicit dispatch. §10.6 establishes geometry-field bidirectionality (SDF ↔ B-rep). §10.8 enumerates kernel candidates. v0.1 simply collapses dispatch to "always OCCT" and hard-codes the meshing path. v0.2 lifts that constraint.

## Why deferred

- A single robust kernel (OCCT) is enough to reach v0.1 alpha and exercise the rest of the language end-to-end (constraints, purposes, determinacy, GUI, LSP).
- Adding even one extra kernel pulls in: kernel registration mechanism (open question §16 #8), conversion-chain tolerance budgeting (§16 #1, see also `per-purpose-tolerance.md`), per-rep cache keying, and OS-level packaging.
- Mesh-Boolean failures are real but partially mitigated by feature-tag naming and OCCT's own healing pipeline. Users blocked on OCCT Booleans can fall back to STL export for v0.1.
- The architecture already accommodates the change without breaking compatibility — RealizationNode keys are designed to admit `repr_kind` and tolerance dimensions.

## Sketch of approach

The dispatch layer is a thin orchestrator above the existing per-kernel adapters. Each kernel registers a capability descriptor (supported `(operation, repr_kind, tolerance_class)` tuples and approximate cost). When an operation needs a realization, the orchestrator consults: which realizations already exist for the entity, which kernels can produce the demanded `repr_kind` from those, and which downstream operations follow (to minimize conversions). Selection is deterministic given pinned runtime configuration; users can override via `#kernel(...)` pragma.

The first integration is Manifold for mesh Booleans because it directly addresses the highest-frequency OCCT failure mode. Manifold consumes triangle meshes and guarantees manifold output, which slots cleanly into a "B-rep → mesh → boolean → mesh → optional B-rep reconstruction" stack pattern. Fidget arrives next, as the SDF kernel powering geometry-field bidirectionality (§10.6) — `field def` values with SDF semantics realize directly via Fidget rather than being meshed through OCCT. Truck is a B-rep alternative; OpenVDB unlocks the voxel-octree leg of the stack pattern (§10.5) and the `imported` field source for OpenVDB grids (see `imported-field-source.md`).

The patchwork pattern (§10.5 — heterogeneous reps in one assembly) requires that spatial composition operations stay representation-agnostic; spanning operations (visualization, interference checks) materialize compatible realizations on demand. This is mostly already true — RealizationNode keying just needs to admit `repr_kind`.

## Pre-conditions for activating

- v0.1 alpha has shipped and OCCT-only operation is stable in production use.
- A concrete user need for one of: Manifold mesh Booleans, Fidget SDFs, OpenVDB voxels is documented (don't add kernels speculatively).

## Resolved design decisions (2026-04-28)

**ReprKind enum.** `BRep | Mesh | Sdf | Voxel` — four entries, semantic only, no kernel-discriminator suffix. Extensible in a non-breaking minor.

**Cache key.** `(entity_id, repr_kind, tol: f64)`. Kernel identity is **not** in the key — the cache is in-memory only and lives for the life of one `Engine` (one loaded module, one process), so kernel selection per `ReprKind` is fixed for the life of the cache by compile-time features + project pin. See `per-purpose-tolerance.md` for tolerance lookup semantics (partial-order `<=`, not equality).

**Capability descriptor.** Minimal for v0.2 — feasibility table only:
```
CapabilityDescriptor {
    supports: Vec<(Operation, ReprKind)>,
}
```
No `cost_hint`, no `error_factor` — both were speculation without telemetry. The dispatcher ranks candidate chains by conversion-stage count alone. Cost and error-weight extensions can be added later as non-breaking descriptor fields once telemetry shows where they would help.

**Kernel registration mechanism (resolves arch §16 open Q #8).** Compile-time only. Each kernel adapter lives in a separate crate (`reify-kernel-occt`, `reify-kernel-manifold`, `reify-kernel-fidget`, `reify-kernel-openvdb`) gated by Cargo features. All implemented kernels default-on; opt-out via feature flag. Adapters register through a static linker-collection mechanism (`inventory` or equivalent) read once at engine startup. No dylib loading, no runtime plugin discovery for v0.2 — those are v0.3+ concerns if they materialise.

**Project pin.** `reify.toml` declares which kernels the project requires and pins versions. Determinism follows from the pin; the cache does not need to know about kernel versions because a version change forces a process restart.

**Long-chain diagnostic.** When the dispatcher selects a chain longer than 2 conversion stages **and** elapsed realization time for the chain exceeds 500 ms wall (configurable), emit a diagnostic naming the chain so users can see budget pressure. Short-chain pain is not worth nagging about.

**Integration sequence.** Manifold → Fidget → OpenVDB. Manifold first (highest-frequency OCCT pain at smallest scope). Fidget unblocks `field def`-as-geometry (§10.6). OpenVDB unblocks the `imported` field source (`imported-field-source.md`). The sequence is best-first-attention order, not a serial constraint — the three are parallelisable and have no integration-order dependency on each other.

**Truck dropped from v0.2.** Truck's pitches are Rust-native + Apache-2.0 + WASM target; its weaknesses (Booleans less robust than OCCT, fillets at prototyping stage per §10.8) overlap exactly with the gaps OCCT covers. No v0.2 use case is bottlenecked on Truck. WASM-Reify or LGPL-aversion is a v0.3+ motivator.

## Out of scope for this PRD

- CGAL integration (research-grade, not production CAD).
- SolveSpace `libslvs` constraint solver — that's the v0.1 constraint kernel, separate concern.
- GPU offload of geometry kernels (post-v0.2).
- `@optimized` user-source registration (live in v0.1 for stdlib, exposed at user level later).
- Truck B-rep alternative (deferred past v0.2; revisit if WASM or licence story becomes load-bearing).
- Dylib / runtime plugin loading for kernels (v0.3+).
