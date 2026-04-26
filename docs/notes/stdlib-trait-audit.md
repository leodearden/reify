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

---

## §6.1 `std.materials` base

Source file: `crates/reify-compiler/stdlib/materials_mechanical.ri` (125 lines)
Spec location: `docs/reify-stdlib-reference.md` lines 628–636

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `Material` *(trait)* | §6.1, line 629 | `materials_mechanical.ri:38` as `MaterialSpec` | — | — | `density: Density`, `name: String` | `density: Real`, `name: String` | **Rename (task 1876):** spec trait `Material` was renamed to `MaterialSpec`; the identifier `Material` now names a first-class struct (see BREAKING CHANGE block at `materials_mechanical.ri:6–25`).  Migration guide in that block covers four consumer patterns.  **Type gap:** spec `density: Density`; current `density: Real` (dedicated Density type not yet in type system). |
| `TemperatureDependent` | §6.1, line 634 | **MISSING** | — | — | `reference_temperature: Temperature = 293.15K` | *(not implemented)* | Not found in any `.ri` file.  Despite task 328 being marked done, this trait was never produced.  Candidate follow-up: add to `materials_mechanical.ri` or a new `materials_base.ri`. |

---

## §6.2 `std.materials.mechanical`

Source file: `crates/reify-compiler/stdlib/materials_mechanical.ri` (125 lines)
Spec location: `docs/reify-stdlib-reference.md` lines 639–678

The dominant gap across all eight §6.2 traits: **spec declares each as `: Material` (now
`MaterialSpec`); current implementations have no parent** — all eight are free-standing
traits.  This is noted individually but is a systemic inheritance gap.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `Elastic` | §6.2, line 642 | `materials_mechanical.ri:60` | `Material` | — | `youngs_modulus: Pressure`, `poissons_ratio: Real`, `shear_modulus: Pressure = undef` | `youngs_modulus: Real`, `poissons_ratio: Real`, `shear_modulus: Real` | **Parent gap** (systemic — see section note). **Type gap:** spec `Pressure` for `youngs_modulus` and `shear_modulus`; current `Real`. **Default gap:** spec `shear_modulus = undef` (optional); current is required. **Constraint gap:** spec has `constraint 0 < poissons_ratio < 0.5`; current omits this constraint. |
| `Strong` | §6.2, line 648 | `materials_mechanical.ri:70` | `Material` | — | `yield_strength: Pressure`, `ultimate_tensile_strength: Pressure`, `compressive_strength: Pressure = undef` | `yield_strength: Real`, `uts: Real`, `compressive_strength: Real` | **Parent gap** (systemic). **Param rename:** spec `ultimate_tensile_strength`; current abbreviated to `uts`. **Type gap:** all Pressure → Real. **Default gap:** spec `compressive_strength = undef` (optional); current is required. |
| `Hard` | §6.2, line 654 | `materials_mechanical.ri:83` | `Material` | — | `hardness_value: Real`, `hardness_scale: HardnessScale` | `hardness_value: Real`, `hardness_scale: HardnessScale` | **Parent gap** (systemic). Params otherwise match spec exactly. `HardnessScale` enum declared in spec at line 658 and in implementation at line 80 — both list identical seven variants. |
| `FatigueRated` | §6.2, line 659 | `materials_mechanical.ri:91` | `Material` | — | `fatigue_limit: Pressure = undef`, `fatigue_strength_at: Pressure = undef`, `fatigue_cycles: Int = undef` | `endurance_limit: Real` | **Parent gap** (systemic). **Param collapse:** spec has three params; current collapses to one. `endurance_limit` maps approximately to `fatigue_limit` but is renamed. `fatigue_strength_at` and `fatigue_cycles` are entirely absent. **Type gap:** Pressure → Real. |
| `FractureTough` | §6.2, line 664 | `materials_mechanical.ri:98` | `Material` | — | `fracture_toughness: Scalar<Pressure * Length^(1/2)>` | `fracture_toughness: Real` | **Parent gap** (systemic). **Type gap:** spec composite type `Scalar<Pressure * Length^(1/2)>` (K_Ic units); current `Real`. |
| `Ductile` | §6.2, line 667 | `materials_mechanical.ri:106` | `Material` | — | `elongation_at_break: Real`, `reduction_of_area: Real = undef` | `elongation: Real`, `reduction_of_area: Real` | **Parent gap** (systemic). **Param rename:** spec `elongation_at_break`; current `elongation`. **Default gap:** spec `reduction_of_area = undef` (optional); current is required. |
| `ImpactResistant` | §6.2, line 671 | `materials_mechanical.ri:113` | `Material` | — | `charpy_impact: Energy = undef`, `izod_impact: Energy = undef` | `impact_energy: Real` | **Parent gap** (systemic). **Param collapse:** spec has two distinct test-method params (`charpy_impact`, `izod_impact`); current collapses to one `impact_energy: Real`. Both spec params are optional (undef); current param is required. **Type gap:** Energy → Real. |
| `Damping` | §6.2, line 675 | `materials_mechanical.ri:122` | `Material` | — | `loss_factor: Real` | `damping_ratio: Real`, `loss_factor: Real` | **Parent gap** (systemic). **Extra param:** `damping_ratio: Real` present in current; absent from spec. |

---

## §6.3 `std.materials.thermal`

Spec location: `docs/reify-stdlib-reference.md` lines 680–694
Expected source file: `materials_thermal.ri` — **does not exist**

Despite task 328 being marked `done` in Taskmaster, no `materials_thermal.ri` (or
equivalent) file was produced.  All traits in this section are **MISSING**.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `ThermallyCharacterized` | §6.3, line 683 | **MISSING** | `Material` | — | `thermal_conductivity: ThermalConductivity`, `specific_heat: SpecificHeat`, `thermal_expansion: Real / Temperature`, `melting_point: Temperature = undef`, `max_service_temperature: Temperature = undef`, `glass_transition: Temperature = undef` | *(not implemented)* | No `.ri` file. Candidate follow-up: create `crates/reify-compiler/stdlib/materials_thermal.ri`. |
| `Refractory` | §6.3, line 691 | **MISSING** | `ThermallyCharacterized` | — | *(no extra params — adds constraint only)* | *(not implemented)* | No `.ri` file. Depends on `ThermallyCharacterized`. Spec adds `constraint max_service_temperature >= 1500degC`. |

---

## §6.4 `std.materials.electrical`

Spec location: `docs/reify-stdlib-reference.md` lines 696–711
Expected source file: `materials_electrical.ri` — **does not exist**

All traits in this section are **MISSING**.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `ElectricallyCharacterized` | §6.4, line 699 | **MISSING** | `Material` | — | `resistivity: Scalar<Voltage * Length / Current>`, `dielectric_constant: Real = undef`, `dielectric_strength: Scalar<Voltage / Length> = undef`, `magnetic_permeability: Real = undef` | *(not implemented)* | No `.ri` file. Candidate follow-up: create `materials_electrical.ri`. Note: `std.structural.ElectricallyConductive` currently carries `electrical_conductivity` and `resistivity` (Real) as free params; those should migrate to reference this trait once it exists. |
| `Conductive` | §6.4, line 705 | **MISSING** | `ElectricallyCharacterized` | — | *(no extra params — adds constraint only)* | *(not implemented)* | Spec constraint: `resistivity < 1e-4ohm*m`. |
| `Insulating` | §6.4, line 708 | **MISSING** | `ElectricallyCharacterized` | — | *(no extra params — adds constraints only)* | *(not implemented)* | Spec constraints: `resistivity > 1e6ohm*m` and `determined(dielectric_strength)`. |

---

## §6.5 `std.materials.optical`

Spec location: `docs/reify-stdlib-reference.md` lines 713–722
Expected source file: `materials_optical.ri` — **does not exist**

All traits in this section are **MISSING**.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `OpticallyCharacterized` | §6.5, line 717 | **MISSING** | `Material` | — | `refractive_index: Real`, `absorption_coefficient: Real = undef`, `transmittance: Real = undef`, `reference_thickness: Length = undef` | *(not implemented)* | No `.ri` file. Candidate follow-up: create `materials_optical.ri`. |

---

## §6.6 `std.materials.chemical`

Spec location: `docs/reify-stdlib-reference.md` lines 724–736
Expected source file: `materials_chemical.ri` — **does not exist**

All traits and enums in this section are **MISSING**.

| Trait / Enum | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `CorrosionResistant` | §6.6, line 728 | **MISSING** | `Material` | — | `corrosion_class: CorrosionClass` | *(not implemented)* | No `.ri` file. Candidate follow-up: create `materials_chemical.ri`. |
| `CorrosionClass` *(enum)* | §6.6, line 731 | **MISSING** | — | — | Variants: `C1, C2, C3, C4, C5` | *(not implemented)* | Required by `CorrosionResistant`. |
| `Biocompatible` | §6.6, line 732 | **MISSING** | `Material` | — | `biocompatibility_class: BiocompatibilityClass` | *(not implemented)* | No `.ri` file. |
| `BiocompatibilityClass` *(enum)* | §6.6, line 735 | **MISSING** | — | — | Variants: `USP_Class_I, USP_Class_VI, ISO_10993` | *(not implemented)* | Required by `Biocompatible`. |

---

## Summary of Gaps and Recommended Follow-ups

### (a) `Material` trait → `MaterialSpec` rename (closed via task 1876)

The spec still refers to the trait as `Material`; the implementation renamed it to
`MaterialSpec` in task 1876 to free the identifier for the new first-class struct.  The
rename is a deliberate, documented breaking change (see `materials_mechanical.ri:6–25`).
**Consumer migration is required** for any external `.ri` file that references the old
trait name under one of the four patterns documented in the BREAKING CHANGE block.
*Recommendation:* update the spec at §6.1 line 629 to reflect `MaterialSpec` as the
canonical trait name, or add a spec note that `Material` denotes the struct and
`MaterialSpec` denotes the trait.

### (b) `TemperatureDependent` missing

The base material trait `TemperatureDependent` (spec §6.1, line 634) is not implemented
in any `.ri` file.  *Candidate task:* add `TemperatureDependent` to
`crates/reify-compiler/stdlib/materials_mechanical.ri` (or a new `materials_base.ri`).

### (c) Entire §6.3–6.6 missing (thermal / electrical / optical / chemical)

Eight traits (`ThermallyCharacterized`, `Refractory`, `ElectricallyCharacterized`,
`Conductive`, `Insulating`, `OpticallyCharacterized`, `CorrosionResistant`,
`Biocompatible`) and two enums (`CorrosionClass`, `BiocompatibilityClass`) have no `.ri`
implementation, despite task 328 being marked done.  *Candidate tasks (one per
subsection):*
- **§6.3:** create `stdlib/materials_thermal.ri` with `ThermallyCharacterized` + `Refractory`.
- **§6.4:** create `stdlib/materials_electrical.ri` with `ElectricallyCharacterized`, `Conductive`, `Insulating`.
- **§6.5:** create `stdlib/materials_optical.ri` with `OpticallyCharacterized`.
- **§6.6:** create `stdlib/materials_chemical.ri` with `CorrosionResistant`, `CorrosionClass`, `Biocompatible`, `BiocompatibilityClass`.

### (d) §6.2 mechanical traits lack `MaterialSpec` parent

All eight §6.2 traits (`Elastic`, `Strong`, `Hard`, `FatigueRated`, `FractureTough`,
`Ductile`, `ImpactResistant`, `Damping`) are declared free-standing; spec mandates each
refines `Material` (now `MaterialSpec`).  Adding `: MaterialSpec` to each would also
require their consumers (e.g. `Physical : MaterialSpec`) to be reconsidered for potential
redundancy.  *Candidate task:* update `materials_mechanical.ri` to add `: MaterialSpec`
to all eight traits, then verify that `structural_physical.ri` still compiles cleanly.

### (e) §4 parameter-shape gaps (`geometry`/`material` vs flat `Real`)

`Physical` and its subtypes use flat scalar `Real` params (`volume`, `centroid_x/y/z`)
where the spec expects a `geometry: Solid` object and `material: Material` trait-object
with computed lets driven by geometry query functions (`volume(geometry)`,
`centroid(geometry)`, `moment_of_inertia(geometry, material.density)`).  Closing this gap
requires the `Solid` geometry type and its query functions to be available in the stdlib.
*Candidate task:* once `std.geometry.Solid` lands, migrate `Physical` and `Rigid` to the
spec's geometry-driven form and replace the three `centroid_x/y/z` params with a single
computed `centroid` let.

### (f) §4 inheritance gaps (`Flexible`, `ElasticallyDeformable`, `Plastic`)

- `Flexible` should inherit `Physical` (spec); currently free-standing.
- `ElasticallyDeformable` should inherit `Flexible` (spec); currently inherits `Elastic`
  (a material trait — semantically different).
- `Plastic` should inherit `Flexible` (spec); currently free-standing.
- `ThermallyConductive` and `ElectricallyConductive` should inherit `Physical` (spec);
  currently free-standing and carry extra params that arguably belong in §6.3/§6.4.

*Candidate task:* reconcile §4 structural trait hierarchy once the geometry/material type
system (gap (e)) is resolved, as the parent changes are entangled with the param-shape
migration.

### (g) §6.2 parameter-name discrepancies

| Trait | Spec param name | Current param name | Note |
|-------|-----------------|--------------------|------|
| `Strong` | `ultimate_tensile_strength` | `uts` | Abbreviated |
| `FatigueRated` | `fatigue_limit` | `endurance_limit` | Renamed; `fatigue_strength_at` and `fatigue_cycles` absent |
| `Ductile` | `elongation_at_break` | `elongation` | Truncated |
| `ImpactResistant` | `charpy_impact` + `izod_impact` | `impact_energy` | Collapsed to single param |
| `Damping` | *(only `loss_factor`)* | + `damping_ratio` | Extra param in current |

*Candidate task:* rename params in `materials_mechanical.ri` to match spec, restore
missing params, and remove or document the extra `damping_ratio` param.  Coordinate with
any consumer code that references `uts`, `endurance_limit`, `elongation`, or
`impact_energy`.
