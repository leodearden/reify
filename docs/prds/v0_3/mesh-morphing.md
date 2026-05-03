# PRD: Mesh Morphing for Topology-Preserving Parameter Changes

Status: stub — deferred, candidate v0.3.x or v0.4. Cross-cuts geometry kernel + FEA + persistent cache. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Avoid mesh-from-scratch on parameter changes that preserve topology (dimensional changes only — no add/remove of features). Detect such changes, morph existing mesh nodes to fit the updated boundary, reuse element connectivity. Big lever for any mesh-consuming workflow (FEA, CFD, toolpath, lattice generation), highest leverage for slider-driven and auto-resolve interactions.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships a Gmsh-based volume mesher that runs from scratch on every cache miss. For a typical parametric design — `param thickness : Length = auto` driving an auto-resolve loop, or a user dragging a dimension slider — every parameter tick changes the geometry, blowing the mesh cache on every step. At 100K elements, that's ~3s serial / ~0.3s parallel per tick of mesh time; auto-resolve loops with 50 evaluations pay this 50 times.

Yet for typical dimensional parameter changes — fillet radius, wall thickness, hole diameter — the mesh *topology* (element connectivity, surface-element correspondence) doesn't have to change. Only node positions need to update. Conventional mesh morphing (RBF morphing, Laplacian smoothing with boundary projection) handles this in milliseconds.

This is the single biggest lever for sub-second slider response in FEA workflows. It also benefits any future mesh-consuming op (CFD, EM, CAM toolpaths, lattice infill, voxel-octree builders) — not just FEA.

## Why deferred (and why a separate PRD)

- Needs a robust **topology-preservation classifier** on parameter changes — non-trivial: dimensional changes preserve topology, but pattern-count changes, boolean-mode changes, and feature-suppression changes don't.
- Needs a morph algorithm with **quality safeguards** — naive node lerp produces tangled / inverted elements when geometry changes are too large. Need a fallback to remesh.
- Depends on FEA kernel landing first (otherwise no concrete consumer to validate against).
- Affects geometry-kernel layer, not just FEA — needs careful API design so that morph results integrate with the existing `RealizationNode` / `ReprKind::VolumeMesh` cache.

## Sketch of approach

Pipeline on parameter change:
1. **Diff classifier** — compare old and new geometry's RealizationNode dependency graphs. Classify: (a) topology-preserving (dimensional only) vs. (b) topology-changing (feature add/remove, pattern count change, boolean mode change). Only (a) is morph-eligible.
2. **Morph step** — for an eligible diff, project old mesh's surface nodes onto the new boundary (closest-point projection on the new B-rep). Smooth interior nodes via Laplacian smoothing or RBF interpolation to preserve element quality.
3. **Quality check** — compute element Jacobians; if any element inverts or quality drops below a threshold, reject the morph and fall back to remesh from scratch.
4. **Cache integration** — morphed mesh is stored alongside the from-scratch mesh in the realization cache, keyed by (input geometry hash, mesh options).

User-visible API: morphing is automatic; user does not opt in. A diagnostic counter ("13/15 mesh updates morphed, 2 remeshed") could surface in verbose output for power users.

## Pre-conditions for activating

- v0.3 FEA kernel shipped — concrete consumer to validate morphing benefit.
- Per-purpose tolerance machinery live — needed for morph quality budget.
- Topology selectors mature enough to express geometry diff classification.

## Open design questions

- **Topology-preservation classifier** — how to detect? Probably reuses the auto-resolve incremental-binding machinery (some classifier exists in the constraint solver). Worth checking.
- **Morph algorithm choice** — Laplacian smoothing is simple but can produce poor quality on large changes; RBF morphing is more robust but slower and more complex. Lean: Laplacian first, RBF if it proves insufficient.
- **Quality threshold for fallback** — Jacobian sign change is a hard fail; quality degradation needs a tunable threshold.
- **Integration with persistent cache** — does a morphed mesh count as a cache entry, or only the from-scratch source? (Lean: only source, morph is per-call cheap.)
- **Cross-PRD impact** — should the morphing layer be a generic geometry-kernel feature exposed to all mesh consumers, or scoped to FEA initially? Lean: generic from day one; the abstraction is small.
- **Failure-mode visibility** — how do we tell the user "morph failed, remeshed from scratch"? Probably just a quiet log line; only matters when they ask why a slider tick was slow.

## Out of scope for this PRD

- Full from-scratch remeshing (already in `structural-analysis-fea.md` task #17).
- Adaptive mesh refinement driven by error indicators (separate PRD `a-posteriori-error-estimation.md`).
- Surface remeshing (this PRD is volume-mesh morphing only — surface mesh comes from elsewhere and is an input).
- Topology-changing parameter responses (those genuinely need a remesh; this PRD only addresses the topology-preserving case).

## Relationship to other PRDs and tasks

- **Speeds up `structural-analysis-fea.md`** — cuts wallclock for slider/auto-resolve workflows by 10–100×; the single biggest interactive-smoothness lever.
- **Benefits future CFD / EM / CAM PRDs** — any mesh-consuming computation reuses the same morphing layer.
- **Composes with `persistent-fea-cache.md`** — morphed meshes can be cached the same way from-scratch meshes are.
- **Independent of `structural-analysis-shells.md`** — shell elements are a separate mesh kind with their own morphing concerns.
