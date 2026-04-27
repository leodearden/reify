# PRD: Multi-Kernel Geometry Dispatch

Status: deferred to v0.2 per 2026-04-26 decision.

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
- Per-purpose tolerance contract (`per-purpose-tolerance.md`) is at least at design-spec stage; without per-purpose tolerance, conversion budgets across kernels cannot be allocated.
- Kernel registration mechanism (open question §16 #8) is resolved.
- A concrete user need for one of: Manifold mesh Booleans, Fidget SDFs, OpenVDB voxels, or Truck B-rep is documented (don't add kernels speculatively).

## Out of scope for this PRD

- CGAL integration (research-grade, not production CAD).
- SolveSpace `libslvs` constraint solver — that's the v0.1 constraint kernel, separate concern.
- GPU offload of geometry kernels (post-v0.2).
- `@optimized` user-source registration (live in v0.1 for stdlib, exposed at user level later).
