# PRD: Composite / Laminated Shell Elements

Status: stub — deferred, candidate v0.5+. Sibling to v0.4 `structural-analysis-shells.md`. Filed 2026-05-05 from shells PRD spillover.

## Goal

Extend Reify's shell elements to support layered (laminated) composite materials, where each ply has its own material, thickness, and orientation. Composite laminates are dominant in aerospace, marine, sporting goods, and increasingly in consumer products (carbon-fibre frames, glass-reinforced panels).

## Background

The v0.4 shells PRD (`structural-analysis-shells.md`) ships isotropic-material shells only — single material, single thickness, no through-thickness layering. Composite laminates need:

- Per-ply material (orthotropic, not isotropic).
- Per-ply thickness and orientation (fibre angle in the local shell frame).
- Through-thickness integration that respects the layer stack.
- Failure criteria specific to composites (Tsai-Wu, Hashin, max-strain) rather than von Mises.
- Inter-laminar shear stress recovery (delamination is the dominant failure mode, and it's driven by inter-laminar shear, not in-plane stress).

This is a substantial domain-specific extension — the constitutive matrix becomes a stack of per-ply matrices, the through-thickness integration becomes a sum over plies with discontinuous derivatives at ply boundaries, and the result interpretation needs ply-level breakouts (a "max stress" question is meaningless without specifying which ply).

## Why deferred to v0.5+

- v0.4 shells PRD has not shipped. Foundation first.
- Composite-specific user demand has not been documented. The v0.4 shells work serves a much broader audience (any thin-body designer); composites are a domain niche worth waiting for concrete demand.
- Constitutive surface design is non-trivial — orthotropic material spec, layup definition syntax, failure-criterion choice all need user-grounded design surface decisions.

## Sketch of approach

- **`OrthotropicMaterial` stdlib type** carrying E1, E2, G12, ν12, density, ply allowables (X_T, X_C, Y_T, Y_C, S — the five ply strengths). Starter library: T300/5208 carbon-epoxy, S2/SP-381 glass-epoxy, etc.
- **`Laminate` stdlib type** carrying an ordered list of `Ply { material, thickness, orientation }`. Helpers for symmetric, balanced, and quasi-isotropic layups.
- **Element kernel extension** in `reify-solver-elastic`: through-thickness integration sums over plies; constitutive D matrix is computed per Gauss point as a layered stack rather than a single isotropic relation.
- **Ply-level result fields** in `ElasticResult`: stress and strain per ply (top, mid, bottom of each ply), plus failure-index field per failure criterion.
- **Failure criteria stdlib functions:** `tsai_wu(...)`, `hashin(...)`, `max_strain(...)` taking ply-level stress and material allowables.

## Pre-conditions for activating

- v0.4 `structural-analysis-shells.md` shipped (kernel, mid-surface extraction, BC/material/result framework).
- Concrete composite-design user demand documented.
- Stdlib material-trait infrastructure mature enough for orthotropic specification.

## Open design questions

- **Layup syntax** — list of plies vs. dedicated `Laminate` constructor vs. external file format (e.g., simple JSON / TOML for layup tables that designers maintain in spreadsheets). Lean: stdlib constructor for inline cases + import helper for tabular cases.
- **Failure criterion default** — Tsai-Wu is the textbook default but not always the right one. Lean: no default; user must specify which criterion they're using (failure analysis is a designer judgment, not a default).
- **Inter-laminar shear stress recovery** — extracted from equilibrium post-processing rather than directly from the shell formulation. Standard but not free in implementation.
- **Per-ply stress reporting cardinality** — top/mid/bottom of each ply × three components × two-or-three result moments → result data structures get large fast for many-ply laminates. UX design needed.
- **Sandwich panels** — soft core + stiff face sheets. Same formulation as a 3-ply laminate but the use case (energy absorption, buckling) is distinct enough to maybe warrant its own surface.

## Out of scope for this PRD

- Progressive damage / failure simulation (ply-by-ply failure under increasing load) — separate non-linear analysis PRD.
- Fabric / weave-level constitutive modelling — research-grade, not v0.5.
- Manufacturing-process effects (cure shrinkage, fibre-volume-fraction variation) — separate domain.
- Adhesive / co-bonded joint modelling — overlaps with contact PRD if filed.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-shells.md`** — same kernel, same mid-surface extraction, same BC/material framework with orthotropic constitutive law swapped in.
- **Composes with `multi-load-case-fea.md`** — composite-specific failure indices need per-load-case envelopes the same way isotropic stresses do.
- **Composes with `fea-gui-rendering-shells.md`** — per-ply stress visualization and ply-failure highlighting need GUI surface.
- **May seed a `structural-analysis-progressive-damage.md`** — non-linear ply failure progression is a natural follow-on.
