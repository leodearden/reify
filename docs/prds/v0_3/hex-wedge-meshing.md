# PRD: Hex and Wedge Meshing for Swept Geometries

Status: stub — deferred, candidate v0.3.x. Partial answer to thin-body FEA before shells (`structural-analysis-shells.md`) ship in v0.4. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Add hexahedral (8-node) and wedge / triangular-prism (6-node) elements for swept geometries — extrudes, lofts with constant cross-section, revolves. These element types handle thin features dramatically better than tets without requiring 2D shell formulation.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships tet-only FEA. Tets are universal but inefficient for swept geometries:

- A simple extruded thin plate (say 100×100×2 mm) needs hundreds of thousands of tets to mesh well, but only a few thousand prisms or hexes — 50–100× element-count reduction.
- Tets in swept geometry suffer the same shear-locking issues as tets anywhere; hexes / prisms have better bending behavior even at P1 order.
- Mesh quality is naturally controlled when sweeping a 2D base mesh along an axis — no aspect-ratio surprises.

Most CAD-FEA tools have specialized swept-feature recognition that triggers prismatic meshing. Reify can do the same: detect swept features, generate base mesh on the cross-section, sweep along the axis to produce wedge or hex elements.

This is a smaller scope than shell elements — no new formulation surface, no mid-surface extraction problem — but addresses a meaningful subset of the thin-body pain. Shells remain the proper general fix.

## Why deferred (and why separate from FEA PRD)

- v0.3 FEA PRD is already 22 tasks; pulling hex/wedge in would expand it materially.
- Detection heuristic for "swept" features is a non-trivial geometry-kernel addition.
- Element kernel work is small but distinct from tet kernel — better to land tet-only first, validate, then add hex/wedge as a focused addition.
- Some user pain may be addressable by P2 tets + thin-body diagnostic in v0.3; need to see how much before committing to hex/wedge work.

## Sketch of approach

1. **Sweep detection** — geometry-kernel pass identifies bodies whose construction history is dominated by `extrude`, `loft` (with constant cross-section), or `revolve`. Tag the body with `swept_kind = Extrude { axis, length }` or similar.
2. **Sweep meshing** — for a tagged swept body, generate 2D mesh on cross-section, sweep along axis to produce wedge (from triangle base) or hex (from quad base) elements. Element count = base_mesh_count × sweep_subdivisions.
3. **Element kernel** — implement P1 hex (8-node) and P1 wedge (6-node) reference elements + stiffness assembly in `reify-solver-elastic`. P2 variants (20-node hex, 15-node wedge) deferred unless demand surfaces.
4. **Mixed-element assembly** — global assembly path needs to handle mixed tet + hex + wedge elements (assemblies often combine swept thin features with tet-meshed blocky parts).
5. **Coupling at boundaries** — where a swept feature meets non-swept geometry, hex/wedge surface tris need to mate with tet surface tris cleanly.

User-visible: opt-out only via `ElasticOptions.force_tet = true`. Default behavior is to recognize and use hex/wedge when applicable; user does not need to think about it.

## Pre-conditions for activating

- v0.3 FEA kernel (tet path) shipped and validated.
- v0.2 multi-kernel mesher landing (so the meshing pipeline is in a place to extend).
- Some user signal that thin-feature pain is biting hard enough to justify the work before shells are ready.

## Open design questions

- **Sweep detection heuristics** — exact criteria? Lean: extrude/revolve from 2D cross-section, loft with single non-twisted profile. Anything more complex is "not swept."
- **Meshing implementation** — Gmsh has transfinite meshing for swept geometries; usable directly, or write our own sweep on top of a 2D Gmsh base mesh? Lean: Gmsh transfinite where applicable, fall back to base-then-sweep for cases Gmsh doesn't handle.
- **P1 vs. P2 hex/wedge** — P1 first, P2 only if needed. P2 hex (20-node) is a noticeable formulation jump.
- **Coupling at non-swept boundaries** — for v0.3.x scope, may simply require that a swept feature is a *complete* body; multi-body coupling via constraints. Real coupling deferred to shells PRD timeframe.
- **When to refuse** — sweep detection must not hide pathological cases (twisted lofts, near-degenerate sweeps). Need clear failure mode when sweep meshing isn't applicable.

## Out of scope for this PRD

- Hex meshing of arbitrary geometry (not swept) — much harder problem; deferred indefinitely.
- Pyramid elements (5-node transition between tet and hex) — useful for mixed meshes; defer until concrete need.
- Hex meshing of swept-with-twist features (lofts with rotation) — sweep can't handle cleanly; remains tet.
- Shell elements — sibling PRD (`structural-analysis-shells.md`), proper general fix for thin bodies.
- P2 hex/wedge elements — defer until P1 is shipped and demand surfaces.

## Relationship to other PRDs and tasks

- **Companion to `structural-analysis-fea.md`** — extends the same kernel and pipeline; only adds new element types and a sweep-detection geometry pass. Reuses BCs, materials, options, validation framework.
- **Partial alternative to `structural-analysis-shells.md`** — addresses a subset of thin-body cases (those backed by swept geometry) with much smaller engineering scope. Shells still required for non-swept thin features (general sheet metal, casings).
- **May benefit from `mesh-morphing.md`** — swept meshes morph particularly well (clean topology, regular structure).
- **Touches v0.2 multi-kernel** — extends the mesher capability descriptor (`multi-kernel.md`) with new operation/repr_kind tuples.
