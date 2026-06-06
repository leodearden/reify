# Stdlib `param X : Real` Placeholder Audit

**Status:** Open — classification complete; dimensionless annotations applied (step-2); follow-up tasks filed (step-3); composite-dim task-E (#3115) resolved 2026-05-15 → all 11 blocked-composite sites tightened to named-dimension aliases; mechanical task-A (#3111) resolved 2026-06-05 → all 12 materials_mechanical.ri tightenable-now sites tightened
**Date:** 2026-05-07 (audit), 2026-05-15 (task-E close-out)
**Source:** Task 3090 (origin tasks: 2354 stdlib design, 2696 Density type, 2759 tensor literals)
**Audit doc parallel:** `docs/notes/stdlib-trait-breadth-audit-v01.md` (trait-breadth audit, task 2347; refreshed 2026-05-14 via task 3529, formerly named `stdlib-trait-audit.md`)

---

## Purpose and Methodology

Every `param X : Real` site in the stdlib `.ri` modules is classified into one of six
buckets:

| Bucket | Meaning | Action |
|--------|---------|--------|
| `tightenable-now` | The resolver already registers the spec'd type (named dim or parametric); tightening is correct but deferred because it cascades to ~50+ call sites in `examples/` and test fixtures. | File a per-module follow-up task. |
| `genuine-dimensionless` | `Real` IS the correct type — the quantity is a dimensionless ratio or a scale-dependent score. | Annotate `// dimensionless` on the param line. |
| `blocked-composite` | The spec'd type is a composite dimension (e.g. W/(m·K), Ω·m, N/m) that the resolver cannot yet express as a named scalar alias or parametric type. | File a follow-up task for composite-dim named aliases. |
| `blocked-geometry-type` | The spec'd type references a `Geometry` / `DatumRef` type that does not exist in the resolver. | File a follow-up task for the geometry-type capability. |
| `blocked-field-in-param` ✓ (retired) | Historically: the spec'd type is `Field<X, Y>` in a `param` position; an old TODO in solver_elastic.ri claimed the resolver `Field` arm was restricted to `field def`. Task 3088 added the `Field<D, C>` arm to `resolve_parameterized_builtin_type` at `type_resolution.rs:1313` (and its `_with_subst` mirror at `:1509`); task 3117 confirmed it works in `param` positions and tightened `displacement`/`stress`; task 3641 used the same capability to tighten the remaining post-audit slots (`frame`, `ShellStress.{top,mid,bottom}`). Bucket retired — no site classifies here today. | — |
| `structural-contract` | `Real` is intentionally dimension-agnostic — the runtime builtins produce correctly-dimensioned values but the trait itself must not participate in dimension checking. Tightening would BREAK the contract. | Record rationale only; no follow-up task. |

### Inline annotation policy

Only `genuine-dimensionless` sites receive a trailing `// dimensionless` inline
comment directly on their `param` line. **`tightenable-now` sites are intentionally
not annotated inline** — they are tracked in this audit document and in each module's
file-header pointer block (which names the specific follow-up task). Inline markers
on tightenable-now sites would clutter param lines that will be refactored (type
changed and annotation removed) in the follow-up tasks, making the diff harder to
review. Readers scanning a `.ri` file and wondering why an un-annotated `param X :
Real` site in `Material {}` is not annotated can consult the per-module file header
and this document.

### Resolver capability reference

Named scalar types available today (`type_resolution.rs:471-622`,
`crates/reify-types/src/dimension.rs:362-393`):
Length, Mass, Time, Current, Temperature, AmountOfSubstance, LuminousIntensity, Angle,
SolidAngle, Money, Area, Volume, Force, Energy, Power, Pressure, Frequency, Voltage,
Charge, Capacitance, Resistance, Conductance, Inductance, MagneticFlux,
MagneticFluxDensity, LuminousFlux, Illuminance, AbsorbedDose, AngularVelocity,
DynamicViscosity, **MomentOfInertia**, **Density**, Dimensionless,
**ThermalConductivity**, **SpecificHeat**, **ThermalExpansion**,
**ElectricResistivity**, **ElectricalConductivity**, **DielectricStrength**,
**Stiffness**, **AbsorptionCoeff**, **FractureToughness** (last 9 added by task #3115;
FractureToughness uses fractional Length exponent via `from_rational_exps`).

Parametric types available today (`type_resolution.rs:1340-1421`):
`Scalar<Q>`, `Vector3<Q>`, `Point3<Q>`, `Tensor<rank,n,Q>`, `Matrix<m,n,Q>`, `Field<D,C>`.

### Acceptance criteria

1. All 88 `param X : Real` sites in the 10 stdlib modules are classified.
2. All genuine-dimensionless sites carry a trailing `// dimensionless` annotation.
3. File-header disclaimers in each module are updated to point to this document
   and to the specific follow-up task IDs filed in step-3.
4. `cargo test -p reify-compiler` passes — no type changes are made in this task.
5. Follow-up tasks filed for each deferred tightening and each blocked capability.

---

## Audit Tables

### `materials_chemical.ri` — no audit needed

Zero `param X : Real` occurrences. All params are enum-typed
(`CorrosionClass`, `BiocompatibilityClass`). This module is noted for completeness
so readers know it was not overlooked.

---

### `materials_mechanical.ri` — 19 sites (post-β, tightened by #3111)

Source: `crates/reify-compiler/stdlib/materials_mechanical.ri`

Line numbers reflect the post-β surface after task #4240 renamed `uts` →
`ultimate_tensile_strength`, split `endurance_limit` → `fatigue_limit` +
`fatigue_strength_at` (both Pressure = undef), and split `impact_energy` →
`charpy_impact` + `izod_impact` (both Energy = undef). Task #3111 tightened all
10 pre-β tightenable-now sites plus the 2 new post-β sites.

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 62 | `MaterialSpec` trait | `density` | `Density` ✓ | `Density` | tightened-by-#3111 | task-A ✓ |
| 75 | `Material` struct | `density` | `Density` ✓ | `Density` | tightened-by-#3111 | task-A ✓ |
| 76 | `Material` struct | `youngs_modulus` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 88 | `Elastic` trait | `youngs_modulus` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 89 | `Elastic` trait | `poissons_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 90 | `Elastic` trait | `shear_modulus` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 102 | `Strong` trait | `yield_strength` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 103 | `Strong` trait | `ultimate_tensile_strength` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 104 | `Strong` trait | `compressive_strength` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 115 | `Hard` trait | `hardness_value` | `Real` | `Real` | genuine-dimensionless | — |
| 133 | `FatigueRated` trait | `fatigue_limit` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 134 | `FatigueRated` trait | `fatigue_strength_at` | `Pressure` ✓ | `Pressure` | tightened-by-#3111 | task-A ✓ |
| 143 | `FractureTough` trait | `fracture_toughness` | `FractureToughness` ✓ | `Pressure·√Length` (K_Ic) | tightened-by-#3115 | task-E ✓ |
| 153 | `Ductile` trait | `elongation_at_break` | `Real` | `Real` | genuine-dimensionless | — |
| 154 | `Ductile` trait | `reduction_of_area` | `Real` | `Real` | genuine-dimensionless | — |
| 165 | `ImpactResistant` trait | `charpy_impact` | `Energy` ✓ | `Energy` | tightened-by-#3111 | task-A ✓ |
| 166 | `ImpactResistant` trait | `izod_impact` | `Energy` ✓ | `Energy` | tightened-by-#3111 | task-A ✓ |
| 174 | `Damping` trait | `damping_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 175 | `Damping` trait | `loss_factor` | `Real` | `Real` | genuine-dimensionless | — |

**Notes:**
- `poissons_ratio` is a dimensionless elastic ratio; `Real` is correct.
- `hardness_value` is a scale-dependent numeric reading (Rockwell, Brinell, Vickers
  etc. use incommensurable scales); `Real` is correct.
- `elongation_at_break` (renamed from `elongation` by #4240) and `reduction_of_area`
  are fractional percentages; `Real` is correct.
- `damping_ratio` and `loss_factor` are energy ratios; `Real` is correct.
- `fracture_toughness` (K_Ic) has SI units Pa·√m = Pressure × Length^(1/2); tightened
  to the named-dimension alias `FractureToughness` by task #3115. The blocker was the
  const-eval helper `from_exps(...)` (denominator=1 only); #3115 added a sibling
  `from_rational_exps(...)` that admits fractional Length exponents (1/2 here).
- `fatigue_cycles : Int = undef` (FatigueRated) is an integer cycle count — not a Real
  placeholder; it was added by #4240 and is correctly typed as Int.
- All 10 pre-β `tightenable-now` sites plus 2 new post-β sites tightened by #3111
  (2026-06-05).

---

### `materials_thermal.ri` — 6 sites

Source: `crates/reify-compiler/stdlib/materials_thermal.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 36 | `ThermallyCharacterized` trait | `thermal_conductivity` | `ThermalConductivity` ✓ | W/(m·K) | tightened-by-#3115 | task-E ✓ |
| 37 | `ThermallyCharacterized` trait | `specific_heat` | `SpecificHeat` ✓ | J/(kg·K) | tightened-by-#3115 | task-E ✓ |
| 38 | `ThermallyCharacterized` trait | `thermal_expansion` | `ThermalExpansion` ✓ | 1/K | tightened-by-#3115 | task-E ✓ |
| 39 | `ThermallyCharacterized` trait | `melting_point` | `Temperature` ✓ | `Temperature` | tightened-by-#3112 | task-B ✓ |
| 40 | `ThermallyCharacterized` trait | `max_service_temperature` | `Temperature` ✓ | `Temperature` | tightened-by-#3112 | task-B ✓ |
| 41 | `ThermallyCharacterized` trait | `glass_transition` | `Temperature` ✓ | `Temperature` | tightened-by-#3112 | task-B ✓ |

**Notes:**
- `thermal_conductivity` (W/(m·K) = kg·m/s³/K), `specific_heat` (J/(kg·K) = m²/s²/K),
  and `thermal_expansion` (1/K) tightened to named-dimension aliases
  `ThermalConductivity`, `SpecificHeat`, `ThermalExpansion` by task #3115 (task-E).
- `melting_point`, `max_service_temperature`, and `glass_transition` are pure
  temperatures; `Temperature` is registered at `type_resolution.rs:497` → tightenable-now.

---

### `materials_optical.ri` — 4 sites

Source: `crates/reify-compiler/stdlib/materials_optical.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 25 | `OpticallyCharacterized` trait | `refractive_index` | `Real` | `Real` | genuine-dimensionless | — |
| 26 | `OpticallyCharacterized` trait | `absorption_coefficient` | `AbsorptionCoeff` ✓ | 1/m (per-length) | tightened-by-#3115 | task-E ✓ |
| 27 | `OpticallyCharacterized` trait | `transmittance` | `Real` | `Real` | genuine-dimensionless | — |
| 28 | `OpticallyCharacterized` trait | `reference_thickness` | `Length` ✓ | `Length` | tightened-by-#3113 | task-C ✓ |

**Notes:**
- `refractive_index` (c/v_phase) and `transmittance` (optical power ratio) are
  dimensionless; `Real` is correct.
- `absorption_coefficient` (Beer-Lambert α, units m⁻¹ = Length⁻¹) tightened to the
  named-dimension alias `AbsorptionCoeff` by task #3115.
- `reference_thickness` tightened to `Length` by task #3113.

---

### `materials_electrical.ri` — 4 sites

Source: `crates/reify-compiler/stdlib/materials_electrical.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 49 | `ElectricallyCharacterized` trait | `resistivity` | `ElectricResistivity` ✓ | Ω·m | tightened-by-#3115 | task-E ✓ |
| 50 | `ElectricallyCharacterized` trait | `dielectric_constant` | `Real` | `Real` | genuine-dimensionless | — |
| 51 | `ElectricallyCharacterized` trait | `dielectric_strength` | `DielectricStrength` ✓ | V/m | tightened-by-#3115 | task-E ✓ |
| 52 | `ElectricallyCharacterized` trait | `magnetic_permeability` | `Real` | `Real` | genuine-dimensionless | — |

**Notes:**
- `dielectric_constant` (ε_r, relative permittivity) and `magnetic_permeability`
  (μ_r, relative permeability) are dimensionless ratios; `Real` is correct.
- `resistivity` (Ω·m = kg·m³/(A²·s³)) and `dielectric_strength` (V/m = kg·m/(A·s³))
  tightened to named-dimension aliases `ElectricResistivity` and `DielectricStrength`
  by task #3115.

---

### `materials_fea.ri` — 5 sites

Source: `crates/reify-compiler/stdlib/materials_fea.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 90 | `ElasticMaterial` trait | `poisson_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 134 | `Steel_AISI_1045` struct | `poisson_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 172 | `Aluminium_6061_T6` struct | `poisson_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 210 | `Titanium_Ti6Al4V` struct | `poisson_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 251 | `ABS_Plastic` struct | `poisson_ratio` | `Real` | `Real` | genuine-dimensionless | — |

**Notes:**
- All five are Poisson's ratio (ν = −ε_transverse / ε_axial), a dimensionless ratio
  constrained to [0, 0.5). `Real` is the correct type here. The `ElasticMaterial`
  trait already uses `Pressure` for `youngs_modulus` and `Density` for `density`;
  `poisson_ratio` is the one param in this module that is genuinely dimensionless.
- All five carry `constraint poisson_ratio >= 0` and `constraint poisson_ratio < 0.5`
  declared in `ElasticMaterial`, ensuring the physical range is enforced at the
  type-system level.

---

### `structural_physical.ri` — 15 sites

Source: `crates/reify-compiler/stdlib/structural_physical.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 16 | `Physical` trait | `volume` | `Real` | `Volume` | tightenable-now | task-D |
| 17 | `Physical` trait | `centroid_x` | `Real` | `Length` | tightenable-now | task-D |
| 18 | `Physical` trait | `centroid_y` | `Real` | `Length` | tightenable-now | task-D |
| 19 | `Physical` trait | `centroid_z` | `Real` | `Length` | tightenable-now | task-D |
| 29 | `Rigid` trait | `moment_of_inertia` | `Real` | `MomentOfInertia` | tightenable-now | task-D |
| 42 | `Flexible` trait | `stiffness` | `Stiffness` ✓ | N/m | tightened-by-#3115 | task-E ✓ |
| 43 | `Flexible` trait | `max_deflection` | `Real` | `Length` | tightenable-now | task-D |
| 61 | `ElasticallyDeformable` trait | `max_elastic_strain` | `Real` | `Real` | genuine-dimensionless | — |
| 78 | `Plastic` trait | `plastic_strain` | `Real` | `Real` | genuine-dimensionless | — |
| 79 | `Plastic` trait | `hardening_modulus` | `Real` | `Pressure` | tightenable-now | task-D |
| 91 | `ThermallyConductive` trait | `thermal_conductivity` | `ThermalConductivity` ✓ | W/(m·K) | tightened-by-#3115 | task-E ✓ |
| 92 | `ThermallyConductive` trait | `max_service_temp` | `Real` | `Temperature` | tightenable-now | task-D |
| 103 | `ElectricallyConductive` trait | `electrical_conductivity` | `ElectricalConductivity` ✓ | S/m | tightened-by-#3115 | task-E ✓ |
| 104 | `ElectricallyConductive` trait | `resistivity` | `ElectricResistivity` ✓ | Ω·m | tightened-by-#3115 | task-E ✓ |
| 113 | `Sealed` trait | `seal_pressure_rating` | `Real` | `Pressure` | tightenable-now | task-D |

**Notes:**
- `MomentOfInertia` is a registered named dimension (second moment of mass, kg·m²)
  at `dimension.rs:362-393` → tightenable-now.
- `max_elastic_strain` and `plastic_strain` are dimensionless fractions (ΔL/L);
  `Real` is correct.
- `stiffness` (N/m = kg/s²), `thermal_conductivity` (W/(m·K)),
  `electrical_conductivity` (S/m), and `resistivity` (Ω·m) tightened to
  named-dimension aliases `Stiffness`, `ThermalConductivity`,
  `ElectricalConductivity`, and `ElectricResistivity` by task #3115.

---

### `tolerancing.ri` — 24 sites

Source: `crates/reify-compiler/stdlib/tolerancing.ri`

All 24 sites use `Real` as a placeholder for a `Geometry` or `DatumRef` type that does
not yet exist in the resolver.

#### `feature` — 16 sites (blocked-geometry-type)

| Line | Owner | Param | Classification | Follow-up |
|------|-------|-------|----------------|-----------|
| 43 | `GeometricTolerance` trait | `feature` | blocked-geometry-type | task-F |
| 65 | `Flatness` struct | `feature` | blocked-geometry-type | task-F |
| 72 | `Straightness` struct | `feature` | blocked-geometry-type | task-F |
| 78 | `Circularity` struct | `feature` | blocked-geometry-type | task-F |
| 84 | `Cylindricity` struct | `feature` | blocked-geometry-type | task-F |
| 91 | `Parallelism` struct | `feature` | blocked-geometry-type | task-F |
| 98 | `Perpendicularity` struct | `feature` | blocked-geometry-type | task-F |
| 107 | `Angularity` struct | `feature` | blocked-geometry-type | task-F |
| 116 | `Position` struct | `feature` | blocked-geometry-type | task-F |
| 123 | `Concentricity` struct | `feature` | blocked-geometry-type | task-F |
| 130 | `Symmetry` struct | `feature` | blocked-geometry-type | task-F |
| 139 | `CircularRunout` struct | `feature` | blocked-geometry-type | task-F |
| 145 | `TotalRunout` struct | `feature` | blocked-geometry-type | task-F |
| 152 | `ProfileOfSurface` struct | `feature` | blocked-geometry-type | task-F |
| 158 | `ProfileOfLine` struct | `feature` | blocked-geometry-type | task-F |
| 169 | `Datum` struct | `feature` | blocked-geometry-type | task-F |

#### `datum_refs` — 8 sites (blocked-geometry-type)

| Line | Owner | Param | Classification | Follow-up |
|------|-------|-------|----------------|-----------|
| 53 | `OrientationTolerance` trait | `datum_refs` | blocked-geometry-type | task-F |
| 58 | `LocationTolerance` trait | `datum_refs` | blocked-geometry-type | task-F |
| 92 | `Parallelism` struct | `datum_refs` | blocked-geometry-type | task-F |
| 99 | `Perpendicularity` struct | `datum_refs` | blocked-geometry-type | task-F |
| 108 | `Angularity` struct | `datum_refs` | blocked-geometry-type | task-F |
| 117 | `Position` struct | `datum_refs` | blocked-geometry-type | task-F |
| 124 | `Concentricity` struct | `datum_refs` | blocked-geometry-type | task-F |
| 131 | `Symmetry` struct | `datum_refs` | blocked-geometry-type | task-F |

**Notes:** `feature` represents a geometric entity (face, edge, axis, surface) and
`datum_refs` represents a set of datum reference frames. Neither has a type-system
counterpart today. Both require a `Geometry` / `DatumRef` resolver capability that
must be introduced before these params can be tightened.

---

### `io.ri` — 1 site

Source: `crates/reify-compiler/stdlib/io.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 109 | `Costed` trait | `quantity_produced` | `Real` | `Real` | genuine-dimensionless | — |

**Notes:** `quantity_produced` is a count or fractional bulk-unit quantity; the existing
comment at line 104-106 already explains the rationale (`Scalar * Real → Scalar`
preserves the Money dimension through `line_cost`). Annotated `// dimensionless` to
make the classification machine-readable.

---

### `solver_elastic.ri` — 5 original + 4 post-audit sites

Source: `crates/reify-compiler/stdlib/solver_elastic.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 166 | `ElasticOptions` struct | `cg_tolerance` | `Real` | `Real` | genuine-dimensionless | — |
| 168 | `ElasticOptions` struct | `shell_threshold` | `Real` | `Real` | genuine-dimensionless | — |
| 171 | `ElasticOptions` struct | `shell_branch_prune_ratio` | `Real` | `Real` | genuine-dimensionless | — |
| 284 | `ElasticResult` struct | `displacement` | `Field<Point3<Length>, Vector3<Length>>` | `Field<Point3<Length>, Vector3<Length>>` | resolved ✓ task-G #3117 | — |
| 285 | `ElasticResult` struct | `stress` | `Field<Point3<Length>, Tensor<2,3,Pressure>>` | `Field<Point3<Length>, Tensor<2,3,Pressure>>` | resolved ✓ task-G #3117 | — |

Post-audit sites added after the original table was fixed (task #3641 scope):

| Line | Owner | Param | Tightened Type | Classification | Resolved in |
|------|-------|-------|----------------|----------------|-------------|
| 286 | `ElasticResult` struct | `frame` | `Field<Point3<Length>, Matrix<3,3,Real>>` | tightened | task #3641 |
| 343 | `ShellStress` struct | `top` | `Field<Point3<Length>, Tensor<2,3,Pressure>>` | tightened | task #3641 |
| 344 | `ShellStress` struct | `mid` | `Field<Point3<Length>, Tensor<2,3,Pressure>>` | tightened | task #3641 |
| 345 | `ShellStress` struct | `bottom` | `Field<Point3<Length>, Tensor<2,3,Pressure>>` | tightened | task #3641 |

**Notes:**
- `cg_tolerance` (relative residual norm), `shell_threshold` (thickness/extent ratio),
  and `shell_branch_prune_ratio` (branch/thickness ratio) are all dimensionless;
  `Real` is correct.
- `displacement` and `stress`: task 3117 confirmed the TODO at `solver_elastic.ri:243-260`
  was stale — task 3088 had already added the `Field<D, C>` arm to
  `resolve_parameterized_builtin_type` (type_resolution.rs:1313) and its `_with_subst`
  mirror (:1509). Both params are now declared with their precise Field types.
  Regression-locked by `tests/solver_elastic_tests.rs::elastic_result_struct_has_correct_param_shape`
  and `tests/parametric_field_resolution_tests.rs` (covers identical forms with the `Body` fixture).
- `frame` and `ShellStress.top/mid/bottom`: added post-audit; resolver supports Field for
  these forms too (confirmed by task 3117). Tightened from `Real` to their proper
  `Field<…>` types in task #3641 using the same resolver capability.
  Regression-locked by `tests/solver_elastic_tests.rs::{elastic_result_struct_has_correct_param_shape,
  shell_stress_struct_has_top_mid_bottom_field_params}`.

---

### `analysis.ri` — 7 sites

Source: `crates/reify-compiler/stdlib/analysis.ri`

| Line | Owner | Param | Current Type | Spec / Intent Type | Classification | Follow-up |
|------|-------|-------|-------------|-------------------|----------------|-----------|
| 30 | `AnalysisResult` trait | `von_mises_stress` | `Real` | `Stress` (= Pressure) | structural-contract | — |
| 31 | `AnalysisResult` trait | `principal_stress_1` | `Real` | `Stress` | structural-contract | — |
| 32 | `AnalysisResult` trait | `principal_stress_2` | `Real` | `Stress` | structural-contract | — |
| 33 | `AnalysisResult` trait | `principal_stress_3` | `Real` | `Stress` | structural-contract | — |
| 34 | `AnalysisResult` trait | `max_shear_stress` | `Real` | `Stress` | structural-contract | — |
| 35 | `AnalysisResult` trait | `safety_factor_value` | `Real` | `Real` | structural-contract | — |
| 46 | `Analysis` trait | `yield_strength` | `Real` | `Pressure` | structural-contract | — |

**Rationale for structural-contract classification:** The file-header explicitly states
"All params use `Real` as a dimension-agnostic placeholder. The runtime builtins produce
correctly-dimensioned values (e.g. Scalar<PRESSURE> for stresses, dimensionless Real for
safety_factor_value). This trait is intended as a structural contract — it does not
participate in dimension checking and will not reject dimensioned conforming values."
Tightening e.g. `von_mises_stress : Real` to `von_mises_stress : Stress` would BREAK
the contract: Real-typed conforming structures (which the runtime produces) would be
rejected by the dimension checker. **No follow-up task is filed for this module.**

---

## Summary

| Classification | Count | Action |
|----------------|-------|--------|
| `tightenable-now` | 20 | tasks-B/C/A resolved; task-D (#3114) pending |
| `genuine-dimensionless` | 21 | Annotated `// dimensionless` in-place |
| `tightened-by-#3111` | 12 | task-A ✓ resolved 2026-06-05 — 10 pre-β + 2 post-β (#4240) sites in materials_mechanical.ri: density→Density, youngs_modulus/shear_modulus/yield_strength/ultimate_tensile_strength/compressive_strength/fatigue_limit/fatigue_strength_at→Pressure, charpy_impact/izod_impact→Energy |
| `tightened-by-#3115` | 11 | Composite-dim alias task-E ✓ resolved 2026-05-15 — all 11 sites now use named-dimension aliases (ThermalConductivity, SpecificHeat, ThermalExpansion, ElectricResistivity, ElectricalConductivity, DielectricStrength, Stiffness, AbsorptionCoeff, FractureToughness) |
| `blocked-composite` | 0 | All 11 previous blocked-composite sites tightened by #3115 |
| `blocked-geometry-type` | 24 | Filed geometry-type follow-up task (task-F) |
| `blocked-field-in-param` | 0 | Resolved by task 3117; both sites tightened to Field types |
| `structural-contract` | 7 | Rationale recorded; no tightening needed or intended |
| **Total** | **101** | |

> Note: the original audit counted 99 rows across all tables (88 unique `param X : Real`
> source lines, plus 11 extra because some params appear in both a trait declaration and
> conforming structures — e.g. `materials_fea.ri::poisson_ratio` appears 5× across
> ElasticMaterial + 4 concrete structs). Task #4240 (post-β) added 2 new Real sites
> (fatigue_strength_at, izod_impact) that were immediately tightened by #3111, bringing
> the total to 101. The `tightenable-now` count falls from 30 to 20 as tasks A (#3111),
> B (#3112), and C (#3113) resolve; each per-module table shows the resolved rows inline.

---

## Filed Follow-up Tasks

> This section is populated in step-3 after tasks are submitted to Taskmaster.
> Each entry will list: task ID, title, scope, parent = task 3090.

| Label | Title | Scope | Task ID |
|-------|-------|-------|---------|
| task-A ✓ | Tighten `materials_mechanical.ri` dimensioned params | density→Density, youngs_modulus/shear_modulus/yield_strength/ultimate_tensile_strength/compressive_strength/fatigue_limit/fatigue_strength_at→Pressure, charpy_impact/izod_impact→Energy; update conforming structures in examples/ and tests/ (post-β names from #4240) | #3111 (resolved 2026-06-05) |
| task-B ✓ | Tighten `materials_thermal.ri` Temperature params | melting_point / max_service_temperature / glass_transition → Temperature; Refractory constraint updated to `>= 1500.0K`; call sites in examples/ and tests/ updated. | #3112 (resolved) |
| task-C ✓ | Tighten `materials_optical.ri` `reference_thickness` | reference_thickness → Length; update call sites | #3113 (resolved) |
| task-D | Tighten `structural_physical.ri` dimensioned params | volume→Volume, centroid_x/y/z→Length, moment_of_inertia→MomentOfInertia, max_deflection→Length, hardening_modulus→Pressure, max_service_temp→Temperature, seal_pressure_rating→Pressure; update call sites | #3114 |
| task-E ✓ | Add named-dimension aliases for composite quantities | Introduced 9 aliases (ThermalConductivity, SpecificHeat, ThermalExpansion, ElectricResistivity, ElectricalConductivity, DielectricStrength, Stiffness, AbsorptionCoeff, FractureToughness — last needs fractional Length exponent, supported via new `from_rational_exps` helper) to NAMED_DIMENSIONS; resolver table-driven so no resolver changes needed. All 11 audit-identified blocked-composite sites tightened. Trait-level constraints (`stiffness > 0`, `resistivity < 0.0001`, etc.) rewritten to use dimensioned RHS literals (e.g. `> 0.0 * 1N / 1m`) — bare numeric RHS evaluated to Indeterminate at runtime because `eval_cmp` compares dimensions; see esc-3115-112 design note. | #3115 (resolved 2026-05-15) |
| task-F | Introduce `Geometry` / `DatumRef` resolver capability | Add a `Geometry` opaque type and `DatumRef` type to the resolver so `tolerancing.ri::feature` (16 sites) and `datum_refs` (8 sites) can be tightened away from `Real` | #3116 |
| task-G ✓ | Investigate and resolve `Field<X,Y>` in `param` positions | Confirmed: resolver arm at `type_resolution.rs:1313` (added by task 3088) works in `param` positions. TODO was stale. Both `ElasticResult::displacement` and `::stress` tightened to Field types. | #3117 (resolved) |
| task-H ✓ | Tighten `frame` and `ShellStress.top/mid/bottom` to Field types | Confirmed: resolver already supported these forms (per task 3117). `ElasticResult.frame` tightened to `Field<Point3<Length>, Matrix<3,3,Real>>`; `ShellStress.{top,mid,bottom}` tightened to `Field<Point3<Length>, Tensor<2,3,Pressure>>`. Regression-locked by `tests/solver_elastic_tests.rs::{elastic_result_struct_has_correct_param_shape, shell_stress_struct_has_top_mid_bottom_field_params}`. | #3641 (resolved) |
