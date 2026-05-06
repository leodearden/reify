# PRD: Varying-Thickness Shell Elements

Status: stub — deferred, candidate v0.5+. Sibling to v0.4 `structural-analysis-shells.md`. Filed 2026-05-05 from shells PRD spillover.

## Goal

Support shell elements where thickness varies across the mid-surface — tapered flexures, draft-angle sheet metal, pressure-vessel walls with local thickening, blade-like profiles. The v0.4 shells PRD ships constant-thickness only; this PRD lifts that restriction.

## Background

Real designs often vary thickness for strength, weight, or manufacturing reasons:

- **Tapered flexures:** thicker at the root (high stress) and thinner at the tip (where compliance matters).
- **Draft-angle sheet metal:** stamping or casting introduces inherent thickness gradients.
- **Pressure-vessel reinforcement:** local thickening near nozzles or supports.
- **Blade profiles:** turbine blades, propeller blades, hydrofoils — thickness varies smoothly along chord and span.

The v0.4 shells PRD assumes a single thickness per body (or per `@shell(thickness=...)` annotation). Voxel-medial mid-surface extraction *already* produces a per-vertex thickness field as a byproduct (twice the SDF at each medial voxel) — the data is there. What's missing is plumbing it through element assembly and result interpretation.

## Why deferred to v0.5+

- v0.4 shells PRD has not shipped. Constant-thickness path needs to land first as the foundation.
- Element-level integration with varying thickness is a kernel modification (Gauss quadrature samples thickness at each quad point), not just a data plumbing change. Worth doing once on top of a stable formulation rather than during the formulation work itself.
- User-explicit thickness specification (versus extracted-from-medial) needs syntax design — annotation grammar like `@shell(thickness = thickness_field(...))` where thickness_field is a stdlib field producer.

## Sketch of approach

- **Data path:** voxel-medial extraction already produces per-vertex thickness. v0.4 collapses it to a per-body scalar; v0.5 preserves the field.
- **Element kernel extension:** through-thickness integration in `reify-solver-elastic` reads thickness at the Gauss point (interpolated from element nodes) instead of from a per-element constant.
- **User specification surface:** three modes:
  - **Auto (default):** thickness comes from medial extraction (no annotation needed).
  - **Annotated scalar:** `@shell(thickness = 2 mm)` — constant override (existing v0.4 behavior).
  - **Annotated field:** `@shell(thickness = linear_taper(root = 5 mm, tip = 1 mm, axis = ...))` — stdlib field producer, evaluated at mid-surface points.
- **Result interpretation:** stress varies through thickness more strongly when thickness varies in-plane (because the bending-stiffness gradient introduces extra in-plane bending modes). Stress recovery samples the local thickness at each query point.
- **Mesher coupling:** the shell mesher needs to refine where thickness changes rapidly relative to element size — same logic as for stress concentrations but driven by thickness gradient.

## Pre-conditions for activating

- v0.4 `structural-analysis-shells.md` shipped (constant-thickness path stable, validation suite passing).
- Stdlib field-producer infrastructure for thickness specification (`linear_taper`, `radial_thickening`, `imported_thickness_map`, etc.) — small additions, can be defined alongside this PRD.
- A concrete user need (tapered flexure, pressure vessel with local thickening, blade design) documented.

## Open design questions

- **Continuous-vs-discontinuous thickness fields** — stepped thickness changes (e.g., a flange transitioning to a thin web) are common but break smooth-field assumptions. Either model the transition with mesh refinement + linear interpolation across the step, or treat it as two separate shell regions tied with MPCs (same mechanism as shell/tet coupling).
- **Manufacturing-driven thickness defaults** — for stamped/cast bodies the thickness gradient is determined by the tool, not the designer. Future composition with a CAM-aware tool could populate thickness automatically.
- **Per-element vs. per-Gauss-point thickness** — per-element (each element has one thickness) is simpler; per-Gauss-point (interpolated from nodes) is more accurate but couples shape functions to thickness. Lean per-Gauss-point for accuracy.
- **Validation benchmarks for varying thickness** — the standard shell benchmarks (pinched cylinder, Scordelis-Lo) assume constant thickness. Need to identify or construct varying-thickness reference solutions.

## Out of scope for this PRD

- Through-thickness material variation (functionally graded materials) — composite-shells territory; composes with `composite-laminated-shells.md`.
- Adaptive thickness optimization (let auto-resolve drive thickness as a field, not just a scalar) — natural follow-on but adds optimization-surface scope.
- Anisotropic thickness gradients in composites — wait until both this PRD and composite-shells PRD have shipped, then revisit composition.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-shells.md`** — same kernel, same mid-surface, same BC framework; only thickness handling changes.
- **Composes with `composite-laminated-shells.md`** — varying total thickness × variable ply count is the union of both; cleanest if both ship before tackling.
- **Composes with `mesh-morphing.md`** — thickness fields morph alongside geometry under parameter changes; warm-start preservation works the same way.
- **Composes with `fea-gui-rendering-shells.md`** — varying thickness needs a thickness-display mode (heat map on mid-surface, or extruded-thickness rendering with varying offset).
