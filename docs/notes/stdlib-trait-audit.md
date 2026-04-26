# Stdlib Trait Breadth Audit — §4 `std.structural` and §6 `std.materials`

**Status:** Open — gaps identified; no code changes in this task (follow-up tasks recommended)
**Date:** 2026-04-26
**Source:** Task 2347; spec source `docs/reify-stdlib-reference.md` §4 (lines 436–464) and §6
(lines 624–736); implementations `crates/reify-compiler/stdlib/structural_physical.ri` and
`crates/reify-compiler/stdlib/materials_mechanical.ri`

---

## Context

This document audits how closely the current `.ri` stdlib implementations match the
canonical specification in `docs/reify-stdlib-reference.md`.  Scope is limited to:

- **§4 `std.structural`** — Physical, Rigid, Flexible, ElasticallyDeformable, Plastic,
  ThermallyConductive, ElectricallyConductive, Sealed
- **§6 `std.materials`** — §6.1 base (Material / MaterialSpec, TemperatureDependent),
  §6.2 mechanical (Elastic, Strong, Hard, FatigueRated, FractureTough, Ductile,
  ImpactResistant, Damping), §6.3 thermal, §6.4 electrical, §6.5 optical, §6.6 chemical

Sections 1–3, 5, and 7–12 are **out of scope** for this audit.

The audit is informational: no `.ri` files are modified here.  Each identified gap is
summarised in the final section with a recommended follow-up task scope.

---

## Audit Schema

Each table uses eight columns:

| Column | Meaning |
|--------|---------|
| **Trait** | Name of the trait (or enum) being audited |
| **Spec § (line)** | Section heading and approximate line number in `reify-stdlib-reference.md` |
| **Declared In Source** | File and approximate line where the current implementation lives, or `MISSING` |
| **Spec Parents** | Parent traits as written in the spec (`—` = none / top-level) |
| **Current Parents** | Parent traits as written in the current `.ri` file (`—` = none) |
| **Spec Params (summary)** | Parameters declared in the spec (name: type, comma-separated) |
| **Current Params (summary)** | Parameters declared in the implementation |
| **Gaps / Notes** | Discrepancies between spec and current, or notable observations |

Computed `let` bindings are noted in the Gaps column rather than the Params columns to
keep the table readable.  Constraints are noted only when they differ between spec and
current.

---

## §4 `std.structural`

Source file: `crates/reify-compiler/stdlib/structural_physical.ri` (95 lines)
Spec location: `docs/reify-stdlib-reference.md` lines 436–464

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `Physical` | §4, line 439 | `structural_physical.ri:15` | — | `MaterialSpec` | `geometry: Solid`, `material: Material` | `volume: Real`, `centroid_x: Real`, `centroid_y: Real`, `centroid_z: Real` | **Parent gap:** spec has no parent; current inherits `MaterialSpec` (absorbs density/name into Physical directly). **Param-shape gap:** spec uses a `geometry: Solid` object and `material: Material` trait-object; current uses flat scalar reals instead. **Computed-let gap:** spec declares `let mass = volume(geometry) * material.density` and `let centroid = centroid(geometry)` via geometry query functions; current declares `let mass = volume * density` (flat multiplication) and drops the `centroid` computed-let entirely, replacing it with three separate `centroid_x/y/z` params. |
| `Rigid` | §4, line 446 | `structural_physical.ri:28` | `Physical` | `Physical` | *(no extra params)* — `let moment_of_inertia = moment_of_inertia(geometry, material.density)` | `moment_of_inertia: Real` | **Param vs let gap:** spec computes `moment_of_inertia` as a `let` from geometry; current declares it as a free `param moment_of_inertia: Real` with an added `constraint moment_of_inertia > 0` (not in spec). |
| `Flexible` | §4, line 450 | `structural_physical.ri:37` | `Physical` | — | `stiffness_model: Field<Point3<Length>, Tensor<2,3,Pressure>>` | `stiffness: Real`, `max_deflection: Real` | **Parent gap:** spec parent `Physical` is absent; current is free-standing. **Param-type gap:** spec uses a rich `stiffness_model: Field<…>` (spatially-varying stiffness tensor); current uses scalar `stiffness: Real`. **Extra param:** `max_deflection: Real` has no counterpart in spec. |
| `ElasticallyDeformable` | §4, line 454 | `structural_physical.ri:48` | `Flexible` | `Elastic` | *(no extra params)* | `max_elastic_strain: Real` | **Parent mismatch:** spec parent `Flexible`; current parent `Elastic` (from `materials_mechanical.ri`) — semantically different ancestry. **Extra param:** `max_elastic_strain: Real` has no counterpart in spec. |
| `Plastic` | §4, line 455 | `structural_physical.ri:61` | `Flexible` | — | `yield_point: Pressure` | `plastic_strain: Real`, `hardening_modulus: Real` | **Parent gap:** spec parent `Flexible` is absent; current is free-standing. **Param mismatch:** spec has `yield_point: Pressure`; current replaces it with `plastic_strain: Real` + `hardening_modulus: Real`. |
| `ThermallyConductive` | §4, line 459 | `structural_physical.ri:72` | `Physical` | — | *(no extra params — pure refinement of Physical)* | `thermal_conductivity: Real`, `max_service_temp: Real` | **Parent gap:** spec parent `Physical` is absent; current is free-standing. **Extra params:** `thermal_conductivity` and `max_service_temp` have no counterpart in spec (spec §4 trait is a bare refinement; thermal params belong in §6.3 `ThermallyCharacterized`). |
| `ElectricallyConductive` | §4, line 460 | `structural_physical.ri:82` | `Physical` | — | *(no extra params — pure refinement of Physical)* | `electrical_conductivity: Real`, `resistivity: Real` | **Parent gap:** spec parent `Physical` is absent; current is free-standing. **Extra params:** `electrical_conductivity` and `resistivity` have no counterpart in spec (spec §4 trait is a bare refinement; electrical params belong in §6.4 `ElectricallyCharacterized`). |
| `Sealed` | §4, line 461 | `structural_physical.ri:92` | — | — | `seal_rating: Pressure` | `seal_pressure_rating: Real` | **Param rename:** spec `seal_rating`; current `seal_pressure_rating`. **Type gap:** spec `Pressure`; current `Real`. |
