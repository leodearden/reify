# PRD: FDM Print Model — Rung 2 (numerical RVE periodic-BC homogenisation)

Status: **stub — deferred.** Refinement rung of `fdm-as-printed-fea.md`, above R1. Filed 2026-05-26 from the umbrella's fidelity ladder. Activates after R1 ships.

## Goal

Replace R1's closed-form / orientation-averaged effective stiffness with **numerical Representative-Volume-Element (RVE) homogenisation**: build a periodic unit cell of the bead/void mesostructure (and infill cell), apply periodic boundary conditions, run the 6 unit-strain load cases on a small offline FE solve, and volume-average to extract the full effective anisotropic stiffness tensor. This is the literature-standard mesostructure→tensor bridge and gives the best stiffness fidelity short of full-part transient simulation. Registers as the R2 rung of the umbrella's `as_printed_material` progressive node.

## Background

Per the 2026-05-26 research, numerical RVE periodic-BC homogenisation is the unambiguous standard for mesostructure→effective-tensor (Rodríguez 2003 closed-form is the R1 rung below it). The RVE solve is small and **offline**: per `(pattern, density, bead geometry)` the resulting material card is cached and reused — far cheaper than full-part simulation. No mature Rust FE homogeniser exists; the periodic-BC unit-cell solver is built on the existing solver primitives (`reify-solver-elastic` / `faer-rs` / `nalgebra`). EasyPBC (ABAQUS) and COMBS (RVE generation) are algorithm references only.

## Sketch of approach

- Unit-cell geometry generator: bead/void periodic cell (from R1's geometry) + infill unit cell per `InfillPattern`/density.
- Periodic-BC small FE solve (6 unit-strain cases) on the unit cell → volume-averaged 6×6 effective tensor.
- Per-`(pattern, density, bead-geometry)` cache of effective tensors (the Digimat-AM "material card" analog), keyed and reused across the part.
- Emit the umbrella's `AnisotropicMaterial` field from the cached tensors + per-region orientation frame.

## Pre-conditions for activating

- `fdm-print-mesostructure-r1.md` shipped (bead/void geometry + per-bead lamina stiffness feed the unit cell).
- `anisotropic-heterogeneous-elastostatics.md` shipped (the 6×6 anisotropic constitutive surface + field consumer; the RVE result is a general anisotropic tensor, the most demanding consumer of the foundation's `ConstitutiveLaw`).
- A periodic-BC capability on the solver primitives (this PRD ships it for the small unit-cell solve).

## Relationship to other PRDs

- **Refinement rung of `fdm-as-printed-fea.md`** — registers as R2; umbrella owns the progressive mechanism, this PRD owns the homogenisation.
- **Above `fdm-print-mesostructure-r1.md`** — consumes R1's bead/void geometry + lamina stiffness; replaces R1's analytical effective stiffness with the numerical tensor.
- **Reusable by `composite-laminated-shells.md`** — RVE homogenisation of a ply unit cell is the same machinery; consider sharing the periodic-BC solver.

## Out of scope

- Transient thermal / residual stress / bridge-sag — `fdm-print-thermal-bondsag-r3.md`.
- Full-part explicit mesostructure FE (millions of elements) — never; RVE is the tractable substitute.
- Nonlinear / damage homogenisation — elastic stiffness tensor only here.

## Open design questions

- RVE size / periodicity: how many beads per cell to capture the periodic void lattice without over-meshing. Lean: smallest periodic cell that tiles the raster + layer pattern.
- Cache granularity: per-`(pattern, density)` vs also per-bead-geometry. Lean: include bead geometry in the key (it changes the void fraction).
- Whether the periodic-BC solver lives in `reify-solver-elastic` or `reify-fdm`. Lean: solver crate (general FE capability), consumed by `reify-fdm`.
