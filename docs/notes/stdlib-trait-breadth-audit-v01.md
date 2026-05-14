# Stdlib Trait Breadth Audit — §4 `std.structural` and §6 `std.materials`

**Status:** Refreshed 2026-05-14 — most §4/§6 gaps closed; remaining items are flagged as v0.1 DRIFT-by-design or pending downstream work.
**Original audit date:** 2026-04-26 (task 2347, commit `09cf49b894`)
**Refresh date:** 2026-05-14 (task 3529)
**Filename note:** This file is the deliverable named `stdlib-trait-breadth-audit-v01.md` in `docs/prds/stdlib-trait-breadth.md`. It was renamed from `stdlib-trait-audit.md` on 2026-05-14 to match the PRD-named path; `git log --follow` preserves the prior history.
**Spec source:** `docs/reify-stdlib-reference.md` §4 (lines 436–464) and §6 (lines 624–736)
**Implementations:** `crates/reify-compiler/stdlib/structural_physical.ri`, `crates/reify-compiler/stdlib/materials_mechanical.ri`, and the four §6.3–§6.6 modules added by task 2354 (`materials_thermal.ri`, `materials_electrical.ri`, `materials_optical.ri`, `materials_chemical.ri`)
**Gap-resolution provenance (at time of writing, 2026-05-14):** task 2349 / commit `bc5c2d69aa` (§4 inheritance edges: `ElasticallyDeformable : Flexible`, `Plastic : Flexible`); task 2352 / commit `b3429254e3` (§6.2 `MaterialSpec` parents for `FatigueRated`, `FractureTough`, `ImpactResistant`, `Damping`); task 2354 (§6.3–§6.6 module landings — `materials_thermal.ri`, `materials_electrical.ri`, `materials_optical.ri`, `materials_chemical.ri`).
**Companion text:** the `Trait Resolution Policy` header at `crates/reify-compiler/stdlib/materials_mechanical.ri:32-38` explains why `Elastic`, `Strong`, `Hard`, `Ductile` remain free-standing — a deliberate partial-deviation for v0.1 (see §"Summary" gap (d) below).

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
| `Physical` | §4, line 439 | `structural_physical.ri:20` | — | `MaterialSpec` | `geometry: Solid`, `material: Material` | `volume: Real`, `centroid_x: Real`, `centroid_y: Real`, `centroid_z: Real` | **Parent gap (DRIFT-by-design, v0.1):** spec has no parent; current inherits `MaterialSpec`, absorbing `density`/`name` directly. The watered-down shape is gated on `std.geometry.Solid` becoming usable as a param type (see `findings/stdlib-trait-breadth.md` M-007). **Param-shape gap:** spec uses `geometry: Solid` and `material: Material` trait-object; current uses flat scalar reals. **Computed-let gap:** spec declares `let mass = volume(geometry) * material.density` and `let centroid = centroid(geometry)` via geometry query functions; current declares `let mass = volume * density` (flat multiplication) and drops the `centroid` computed-let entirely, replacing it with three separate `centroid_x/y/z` params. |
| `Rigid` | §4, line 446 | `structural_physical.ri:33` | `Physical` | `Physical` | *(no extra params)* — `let moment_of_inertia = moment_of_inertia(geometry, material.density)` | `moment_of_inertia: Real` | **Param vs let gap:** spec computes `moment_of_inertia` as a `let` from geometry; current declares it as a free `param moment_of_inertia: Real` with an added `constraint moment_of_inertia > 0` (not in spec). |
| `Flexible` | §4, line 450 | `structural_physical.ri:46` | `Physical` | — | `stiffness_model: Field<Point3<Length>, Tensor<2,3,Pressure>>` | `stiffness: Real`, `max_deflection: Real` | **Parent gap (OPEN):** spec parent `Physical` still absent; `Flexible` deliberately stays free-standing because adopting `: Physical` is entangled with gap (e)'s `geometry: Solid` migration. **Param-type gap:** spec uses a rich `stiffness_model: Field<…>` (spatially-varying stiffness tensor); current uses scalar `stiffness: Real`. **Extra param:** `max_deflection: Real` has no counterpart in spec. |
| `ElasticallyDeformable` | §4, line 454 | `structural_physical.ri:70` | `Flexible` | `Flexible` | *(no extra params)* | `max_elastic_strain: Real` | **Parent gap CLOSED — task 2349, commit `bc5c2d69aa`:** parent was previously `Elastic` (from `materials_mechanical.ri`, semantically different ancestry); now refines `Flexible` per spec. The comment block at `structural_physical.ri:62-67` documents why the previous `: Elastic` edge was redundant (material elastic moduli flow through the `material : MaterialSpec` slot, not via inheritance). **Extra param:** `max_elastic_strain: Real` has no counterpart in spec. |
| `Plastic` | §4, line 455 | `structural_physical.ri:87` | `Flexible` | `Flexible` | `yield_point: Pressure` | `plastic_strain: Real`, `hardening_modulus: Real` | **Parent gap CLOSED — task 2349, commit `bc5c2d69aa`:** previously free-standing; now refines `Flexible` per spec. **Param mismatch (OPEN):** spec has `yield_point: Pressure`; current replaces it with `plastic_strain: Real` + `hardening_modulus: Real`. |
| `ThermallyConductive` | §4, line 459 | `structural_physical.ri:99` | `Physical` | `Physical` | *(no extra params — pure refinement of Physical)* | `thermal_conductivity: Real`, `max_service_temp: Real` | **Parent gap CLOSED:** now refines `Physical` per spec (see current source). **Param overlap (OPEN):** `thermal_conductivity` and `max_service_temp` overlap with §6.3 `ThermallyCharacterized`; the audit-recommended cross-section migration (move these params *into* §6.3) was NOT done, and §4 + §6.3 now both carry overlapping `thermal_conductivity` fields (see `findings/stdlib-trait-breadth.md` lines 56–58). |
| `ElectricallyConductive` | §4, line 460 | `structural_physical.ri:110` | `Physical` | `Physical` | *(no extra params — pure refinement of Physical)* | `electrical_conductivity: Real`, `resistivity: Real` | **Parent gap CLOSED:** now refines `Physical` per spec. **Param overlap (OPEN):** `electrical_conductivity` and `resistivity` overlap with §6.4 `ElectricallyCharacterized` for the same reasons as `ThermallyConductive`. |
| `Sealed` | §4, line 461 | `structural_physical.ri:121` | — | — | `seal_rating: Pressure` | `seal_pressure_rating: Real` | **Param rename:** spec `seal_rating`; current `seal_pressure_rating`. **Type gap:** spec `Pressure`; current `Real` (pending task 3115 named-dim aliases). |

---

## §6.1 `std.materials` base

Source file: `crates/reify-compiler/stdlib/materials_mechanical.ri` (125 lines)
Spec location: `docs/reify-stdlib-reference.md` lines 628–636

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `Material` *(trait)* | §6.1, line 629 | `materials_mechanical.ri:51` as `MaterialSpec` | — | — | `density: Density`, `name: String` | `density: Real`, `name: String` | **Rename (task 1876, CLOSED):** spec trait `Material` was renamed to `MaterialSpec`; the identifier `Material` now names a first-class struct (see BREAKING CHANGE block at `materials_mechanical.ri:11–31`).  Migration guide in that block covers four consumer patterns.  **Type gap (OPEN):** spec `density: Density`; current `density: Real` (dedicated Density type not yet in type system). |
| `TemperatureDependent` | §6.1, line 634 | **MISSING (OPEN)** | — | — | `reference_temperature: Temperature = 293.15K` | *(not implemented)* | Not found in any `.ri` file (verified 2026-05-14 via `grep -rn 'TemperatureDependent' crates/reify-compiler/stdlib/`).  Despite task 328 being marked done, this trait was never produced and no follow-up task has been filed.  Candidate follow-up: add to `materials_mechanical.ri` or a new `materials_base.ri`. |

---

## §6.2 `std.materials.mechanical`

Source file: `crates/reify-compiler/stdlib/materials_mechanical.ri` (125 lines)
Spec location: `docs/reify-stdlib-reference.md` lines 639–678

**State (2026-05-14):** four of the eight §6.2 traits now refine `MaterialSpec` per spec —
`FatigueRated`, `FractureTough`, `ImpactResistant`, `Damping` — fixed by task 2352
(commit `b3429254e3`).  The remaining four (`Elastic`, `Strong`, `Hard`, `Ductile`) are
**intentionally free-standing** per the Trait Resolution Policy header at
`crates/reify-compiler/stdlib/materials_mechanical.ri:32-38`: consumer structures carry one
`material : MaterialSpec` slot rather than transitively inheriting `density`/`name` from
every refining trait.  See §"Summary" gap (d) below for the design rationale.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `Elastic` | §6.2, line 642 | `materials_mechanical.ri:73` | `Material` | — | `youngs_modulus: Pressure`, `poissons_ratio: Real`, `shear_modulus: Pressure = undef` | `youngs_modulus: Real`, `poissons_ratio: Real`, `shear_modulus: Real` | **Parent gap (DRIFT-by-design):** intentionally free-standing per Trait Resolution Policy — `density`/`name` flow through the `material : MaterialSpec` slot on consumers. **Type gap:** spec `Pressure` for `youngs_modulus` and `shear_modulus`; current `Real`. **Default gap:** spec `shear_modulus = undef` (optional); current is required. **Constraint gap:** spec has `constraint 0 < poissons_ratio < 0.5`; current omits this constraint. |
| `Strong` | §6.2, line 648 | `materials_mechanical.ri:83` | `Material` | — | `yield_strength: Pressure`, `ultimate_tensile_strength: Pressure`, `compressive_strength: Pressure = undef` | `yield_strength: Real`, `uts: Real`, `compressive_strength: Real` | **Parent gap (DRIFT-by-design):** free-standing per Trait Resolution Policy. **Param rename:** spec `ultimate_tensile_strength`; current abbreviated to `uts`. **Type gap:** all Pressure → Real. **Default gap:** spec `compressive_strength = undef` (optional); current is required. |
| `Hard` | §6.2, line 654 | `materials_mechanical.ri:96` | `Material` | — | `hardness_value: Real`, `hardness_scale: HardnessScale` | `hardness_value: Real`, `hardness_scale: HardnessScale` | **Parent gap (DRIFT-by-design):** free-standing per Trait Resolution Policy. Params otherwise match spec exactly. `HardnessScale` enum declared in spec at line 658 and in implementation at line 93 — both list identical seven variants. |
| `FatigueRated` | §6.2, line 659 | `materials_mechanical.ri:105` | `Material` | `MaterialSpec` | `fatigue_limit: Pressure = undef`, `fatigue_strength_at: Pressure = undef`, `fatigue_cycles: Int = undef` | `endurance_limit: Real` | **Parent gap CLOSED — task 2352, commit `b3429254e3`:** now refines `MaterialSpec` per spec. **Param collapse (OPEN):** spec has three params; current collapses to one. `endurance_limit` maps approximately to `fatigue_limit` but is renamed. `fatigue_strength_at` and `fatigue_cycles` are entirely absent. **Type gap:** Pressure → Real. |
| `FractureTough` | §6.2, line 664 | `materials_mechanical.ri:112` | `Material` | `MaterialSpec` | `fracture_toughness: Scalar<Pressure * Length^(1/2)>` | `fracture_toughness: Real` | **Parent gap CLOSED — task 2352, commit `b3429254e3`:** now refines `MaterialSpec`. **Type gap (OPEN):** spec composite type `Scalar<Pressure * Length^(1/2)>` (K_Ic units); current `Real` — pending fractional dimension exponents (task 3115 description notes this site is blocked even after named-dim aliases land). |
| `Ductile` | §6.2, line 667 | `materials_mechanical.ri:119` | `Material` | — | `elongation_at_break: Real`, `reduction_of_area: Real = undef` | `elongation: Real`, `reduction_of_area: Real` | **Parent gap (DRIFT-by-design):** free-standing per Trait Resolution Policy. **Param rename:** spec `elongation_at_break`; current `elongation`. **Default gap:** spec `reduction_of_area = undef` (optional); current is required. |
| `ImpactResistant` | §6.2, line 671 | `materials_mechanical.ri:129` | `Material` | `MaterialSpec` | `charpy_impact: Energy = undef`, `izod_impact: Energy = undef` | `impact_energy: Real` | **Parent gap CLOSED — task 2352, commit `b3429254e3`:** now refines `MaterialSpec`. **Param collapse (OPEN):** spec has two distinct test-method params (`charpy_impact`, `izod_impact`); current collapses to one `impact_energy: Real`. Both spec params are optional (undef); current param is required. **Type gap:** Energy → Real. |
| `Damping` | §6.2, line 675 | `materials_mechanical.ri:138` | `Material` | `MaterialSpec` | `loss_factor: Real` | `damping_ratio: Real`, `loss_factor: Real` | **Parent gap CLOSED — task 2352, commit `b3429254e3`:** now refines `MaterialSpec`. **Extra param (OPEN):** `damping_ratio: Real` present in current; absent from spec. |

---

## §6.3 `std.materials.thermal`

Spec location: `docs/reify-stdlib-reference.md` lines 680–694
Source file: `crates/reify-compiler/stdlib/materials_thermal.ri` — **landed via task 2354**

**State CLOSED — task 2354:** all §6.3 traits now exist on main.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `ThermallyCharacterized` | §6.3, line 683 | `materials_thermal.ri:38` | `Material` | `MaterialSpec` | `thermal_conductivity: ThermalConductivity`, `specific_heat: SpecificHeat`, `thermal_expansion: Real / Temperature`, `melting_point: Temperature = undef`, `max_service_temperature: Temperature = undef`, `glass_transition: Temperature = undef` | `thermal_conductivity: Real`, `specific_heat: Real`, `thermal_expansion: Real`, `melting_point: Real`, `max_service_temperature: Real`, `glass_transition: Real` | **Parent CLOSED:** parent points at `MaterialSpec` per the §6.1 rename (see `materials_thermal.ri:14-19` for the parent-trait-rename note). **Type gap (OPEN):** all dimensional types degraded to `Real` pending task 3115 named-dim aliases. **Default gap (OPEN):** spec marks last three as `undef`; current is required (`glass_transition = 0.0` sentinel convention noted in source comments). |
| `Refractory` | §6.3, line 691 | `materials_thermal.ri:53` | `ThermallyCharacterized` | `ThermallyCharacterized` | *(no extra params — adds constraint only)* | *(constraint `max_service_temperature >= 1500.0`)* | **CLOSED — task 2354:** structure matches spec. Constraint uses `1500.0` (K-equivalent Real) pending Temperature type. |

---

## §6.4 `std.materials.electrical`

Spec location: `docs/reify-stdlib-reference.md` lines 696–711
Source file: `crates/reify-compiler/stdlib/materials_electrical.ri` — **landed via task 2354**

**State CLOSED — task 2354:** all §6.4 traits now exist on main.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `ElectricallyCharacterized` | §6.4, line 699 | `materials_electrical.ri:47` | `Material` | `MaterialSpec` | `resistivity: Scalar<Voltage * Length / Current>`, `dielectric_constant: Real = undef`, `dielectric_strength: Scalar<Voltage / Length> = undef`, `magnetic_permeability: Real = undef` | *(see source — all Real)* | **Parent CLOSED.** **Type gap (OPEN):** all dimensional types degraded to `Real` pending task 3115 named-dim aliases (`ElectricResistivity`, `DielectricStrength`). **Param overlap:** `std.structural.ElectricallyConductive` (§4) still carries `electrical_conductivity` and `resistivity` as overlapping free params — the audit-recommended cross-section migration was NOT done; both layers carry the fields. |
| `Conductive` | §6.4, line 705 | `materials_electrical.ri:58` | `ElectricallyCharacterized` | `ElectricallyCharacterized` | *(no extra params — adds constraint only)* | *(constraint `resistivity < 1e-4`)* | **CLOSED — task 2354:** structure matches spec. Constraint uses `Real` literal pending Resistivity type. |
| `Insulating` | §6.4, line 708 | `materials_electrical.ri:70` | `ElectricallyCharacterized` | `ElectricallyCharacterized` | *(no extra params — adds constraints only)* | *(see source)* | **CLOSED — task 2354:** structure matches spec; spec constraint `determined(dielectric_strength)` is approximated with available primitives. |

---

## §6.5 `std.materials.optical`

Spec location: `docs/reify-stdlib-reference.md` lines 713–722
Source file: `crates/reify-compiler/stdlib/materials_optical.ri` — **landed via task 2354**

**State CLOSED — task 2354:** the §6.5 trait now exists on main.

| Trait | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `OpticallyCharacterized` | §6.5, line 717 | `materials_optical.ri:27` | `Material` | `MaterialSpec` | `refractive_index: Real`, `absorption_coefficient: Real = undef`, `transmittance: Real = undef`, `reference_thickness: Length = undef` | *(see source)* | **Parent CLOSED.** **Type gap (OPEN):** `absorption_coefficient` and `reference_thickness` use `Real` pending task 3115 `AbsorptionCoeff` and `Length` aliases. |

---

## §6.6 `std.materials.chemical`

Spec location: `docs/reify-stdlib-reference.md` lines 724–736
Source file: `crates/reify-compiler/stdlib/materials_chemical.ri` — **landed via task 2354**

**State CLOSED — task 2354:** all §6.6 traits and enums now exist on main.

| Trait / Enum | Spec § (line) | Declared In Source | Spec Parents | Current Parents | Spec Params (summary) | Current Params (summary) | Gaps / Notes |
|-------------|--------------|-------------------|--------------|-----------------|----------------------|--------------------------|--------------|
| `CorrosionResistant` | §6.6, line 728 | `materials_chemical.ri:38` | `Material` | `MaterialSpec` | `corrosion_class: CorrosionClass` | `corrosion_class: CorrosionClass` | **CLOSED — task 2354.** |
| `CorrosionClass` *(enum)* | §6.6, line 731 | `materials_chemical.ri:23` | — | — | Variants: `C1, C2, C3, C4, C5` | Variants: `C1, C2, C3, C4, C5` | **CLOSED — task 2354:** variants match spec exactly. |
| `Biocompatible` | §6.6, line 732 | `materials_chemical.ri:47` | `Material` | `MaterialSpec` | `biocompatibility_class: BiocompatibilityClass` | `biocompatibility_class: BiocompatibilityClass` | **CLOSED — task 2354.** |
| `BiocompatibilityClass` *(enum)* | §6.6, line 735 | `materials_chemical.ri:31` | — | — | Variants: `USP_Class_I, USP_Class_VI, ISO_10993` | Variants: `USP_Class_I, USP_Class_VI, ISO_10993` | **CLOSED — task 2354:** variants match spec exactly. |

---

## Summary of Gaps and Recommended Follow-ups

State labels used below:
- **CLOSED** — gap resolved on main; provenance cited.
- **DRIFT-by-design** — deliberate v0.1 deviation from spec; downstream work needed before reconsidering.
- **OPEN** — still a gap; either a candidate follow-up task is filed (cited) or none has been filed yet.

### (a) `Material` trait → `MaterialSpec` rename — **CLOSED (task 1876)**

The spec still refers to the trait as `Material`; the implementation renamed it to
`MaterialSpec` in task 1876 to free the identifier for the new first-class struct.  The
rename is a deliberate, documented breaking change (see BREAKING CHANGE block at
`crates/reify-compiler/stdlib/materials_mechanical.ri:11-31`).  Consumer migration is
required for any external `.ri` file that references the old trait name under one of the
four patterns documented in that block.  *Spec follow-up (not blocking):* update the spec
at §6.1 line 629 to reflect `MaterialSpec` as the canonical trait name, or add a spec
note that `Material` denotes the struct and `MaterialSpec` denotes the trait.

### (b) `TemperatureDependent` missing — **OPEN (no follow-up filed)**

The base material trait `TemperatureDependent` (spec §6.1, line 634) is not implemented
in any `.ri` file (re-verified 2026-05-14).  No follow-up task has been filed.  *Candidate
follow-up:* add `TemperatureDependent` to `crates/reify-compiler/stdlib/materials_mechanical.ri`
(or a new `materials_base.ri`).

### (c) §6.3–6.6 modules — **CLOSED (task 2354)**

The four modules `materials_thermal.ri`, `materials_electrical.ri`, `materials_optical.ri`,
and `materials_chemical.ri` (and the enums `CorrosionClass` / `BiocompatibilityClass`) all
landed on main via task 2354.  All ten declarations now exist with correct names and
parent edges (refining `MaterialSpec` per the §6.1 rename).  The remaining gaps in these
modules are dimensional-type degradations to `Real` and overlapping fields with §4
`ThermallyConductive` / `ElectricallyConductive` — see gaps (g) and (e) below.

### (d) §6.2 mechanical traits' `MaterialSpec` parent — **PARTIAL: CLOSED + DRIFT-by-design (task 2352, commit `b3429254e3`)**

Four of the eight §6.2 traits now refine `MaterialSpec` per spec:

- `FatigueRated : MaterialSpec` (`materials_mechanical.ri:105`)
- `FractureTough : MaterialSpec` (`materials_mechanical.ri:112`)
- `ImpactResistant : MaterialSpec` (`materials_mechanical.ri:129`)
- `Damping : MaterialSpec` (`materials_mechanical.ri:138`)

The remaining four (`Elastic`, `Strong`, `Hard`, `Ductile`) **deliberately remain
free-standing**.  The design rationale is the Trait Resolution Policy at
[`crates/reify-compiler/stdlib/materials_mechanical.ri:32-38`](../../crates/reify-compiler/stdlib/materials_mechanical.ri):
consumer structures carry one `material : MaterialSpec` slot rather than transitively
inheriting `density`/`name` via every refining trait.  v0.2 conformance machinery will
revisit whether the asymmetry should be removed.

### (e) §4 parameter-shape gaps (`geometry`/`material` vs flat `Real`) — **DRIFT-by-design (v0.1; M-007 in gap register)**

`Physical` and its subtypes use flat scalar `Real` params (`volume`, `centroid_x/y/z`)
where the spec expects a `geometry: Solid` object and `material: Material` trait-object
with computed lets driven by geometry query functions (`volume(geometry)`,
`centroid(geometry)`, `moment_of_inertia(geometry, material.density)`).  Closing this gap
requires the `Solid` geometry type and its query functions to be available as `.ri` param
types — currently tracked under M-013 in
`docs/architecture-audit/findings/stdlib-trait-breadth.md`.  Until then, the watered-down
shape is the accepted v0.1 compromise (`Physical : MaterialSpec` directly absorbs
`density`/`name`).

### (f) §4 inheritance gaps (`Flexible`, `ElasticallyDeformable`, `Plastic`, `ThermallyConductive`, `ElectricallyConductive`) — **PARTIAL: most CLOSED (tasks 2349 + 2354 sibling work)**

| Edge | Spec | Current | State |
|------|------|---------|-------|
| `ElasticallyDeformable : Flexible` | yes | yes | **CLOSED (task 2349, commit `bc5c2d69aa`)** — previously refined `Elastic`; the new comment block at `structural_physical.ri:62-67` documents why that edge was redundant given the `material : MaterialSpec` slot pattern. |
| `Plastic : Flexible` | yes | yes | **CLOSED (task 2349, commit `bc5c2d69aa`)** — previously free-standing. |
| `ThermallyConductive : Physical` | yes | yes | **CLOSED** — current source has the parent edge (see `structural_physical.ri:99`).  Param overlap with §6.3 remains (see "Param overlap" notes in the §4 table). |
| `ElectricallyConductive : Physical` | yes | yes | **CLOSED** — current source has the parent edge (see `structural_physical.ri:110`).  Param overlap with §6.4 remains. |
| `Flexible : Physical` | yes | **no** | **OPEN (entangled with gap (e))** — `Flexible` deliberately stays free-standing because adopting `: Physical` is gated on the `geometry: Solid` migration.  Reconsider once M-007/M-013 lifts. |

### (g) §6.2 parameter-name discrepancies — **OPEN (gated on task 3115)**

| Trait | Spec param name | Current param name | Note |
|-------|-----------------|--------------------|------|
| `Strong` | `ultimate_tensile_strength` | `uts` | Abbreviated |
| `FatigueRated` | `fatigue_limit` | `endurance_limit` | Renamed; `fatigue_strength_at` and `fatigue_cycles` absent |
| `Ductile` | `elongation_at_break` | `elongation` | Truncated |
| `ImpactResistant` | `charpy_impact` + `izod_impact` | `impact_energy` | Collapsed to single param |
| `Damping` | *(only `loss_factor`)* | + `damping_ratio` | Extra param in current |

The renames are entangled with the dimensional-type degradation (every spec-cited
`Pressure` / `Energy` / `Temperature` / `Length` degrades to `Real` here per M-012) —
task **3115** (`pending`) tracks the named-dim aliases that unblock 15 blocked-composite
sites across these five `.ri` modules; a coordinated rename + retype sweep is the natural
unit of work once 3115 lands.

---

## Acceptance-criteria smoke check (task 3529)

The PRD `docs/prds/stdlib-trait-breadth.md` names this deliverable
`docs/notes/stdlib-trait-breadth-audit-v01.md`.  The smoke check is:

```sh
test -f docs/notes/stdlib-trait-breadth-audit-v01.md
```

This file (you are reading it) satisfies the PRD-named path requirement.
