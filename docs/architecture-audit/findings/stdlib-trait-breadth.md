# Audit: Stdlib Trait Long-Tail Breadth

**PRD path:** `docs/prds/stdlib-trait-breadth.md`
**Auditor:** audit-stdlib-trait-breadth
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 7

> **Resolution note (2026-05-14, task 3529):** M-002 is resolved.
> `docs/notes/stdlib-trait-audit.md` has been renamed to
> `docs/notes/stdlib-trait-breadth-audit-v01.md` to match the PRD-named path, and the
> doc has been refreshed to reflect the trait-inheritance state on main (with explicit
> DONE markers and task/commit refs for each resolved gap). The body of this finding
> doc preserves the 2026-05-12 audit snapshot below as a historical record of the
> pre-refresh state; all references to `stdlib-trait-audit.md` in the body should be
> read as the file now at the PRD-named path.

## Top concerns

- **This PRD is almost entirely about declaring trait names and inheritance edges in `.ri` source.** All four PRD tasks (2347/2349/2352/2354) are done; the six trait-bearing stdlib `.ri` files exist; ~3,600 lines of integration tests pass. The PRD explicitly says "v0.1 *declares* every named trait... but **does not** wire conformance machinery beyond trait existence — that's a v0.2 concern." So the local audit surface is narrow: most "mechanisms" are declarations, and almost everything is `WIRED`. The interesting gaps are about **how much the spec was watered down** to match the v0.1 declaration-only scope.
- **DRIFT (M-002): the audit deliverable named in the acceptance criteria (`docs/notes/stdlib-trait-breadth-audit-v01.md`) was never produced.** The acceptance criterion is alternatively satisfied by `docs/notes/stdlib-trait-audit.md` (a pre-existing audit doc from task 2347 dated 2026-04-26, before this PRD's tasks 2349/2352/2354 landed) plus the "Trait Resolution Policy" header block in `materials_mechanical.ri:32-38`. The audit doc therefore documents the *unreconciled* state — it does **not** reflect that 2349/2352 fixed the inheritance gaps it identified. A future maintainer reading `stdlib-trait-audit.md` will believe the §4 and §6.2 parents are still broken when in fact they've been fixed.
- **DRIFT (M-007): `Physical : MaterialSpec` ships, but the spec says `Physical` has no parent and uses `geometry: Solid` / `material: Material` slots.** This PRD's `## Background` notes the headline subset "already has full conformance + worked examples shipped via #327/#328/#329" — but those shipments materially deviated from the spec's geometry-driven shape. The deviation is *documented* in `stdlib-trait-audit.md` (gap (e)) and is gated on `std.geometry.Solid` being usable as a param type — which still isn't (M-013 ORPHAN). v0.1 is fine; the watered-down shape is the carrying compromise.
- **All seven `Pressure` / `Energy` / `Temperature` / `Length` quantity-type references in the spec degrade to `Real` here** (M-012). DimensionVector::PRESSURE exists in `reify-types` but no `type Pressure` alias is exposed to stdlib `.ri` sources. Task 3115 (deferred) tracks the named-dim alias work. Until then, every spec-cited dimensional type is `Real` plus a comment. The PRD's "Out of scope" section explicitly carves this out, but it makes the worked example's `param seal_pressure_rating : Real` (note: even the param *name* differs from the spec's `seal_rating`) a stable-for-v0.1 footgun.

## Mechanisms

### M-001: Stdlib `.ri` file inventory — six material/structural trait files

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/stdlib/structural_physical.ri` (121 lines), `materials_mechanical.ri` (138 lines), `materials_thermal.ri` (55 lines), `materials_electrical.ri` (73 lines), `materials_optical.ri` (32 lines), `materials_chemical.ri` (49 lines). All six files exist on main. Loader in `crates/reify-compiler/src/stdlib_loader.rs` picks them up. Tests under `crates/reify-compiler/tests/` (~3,600 lines across seven `*_tests.rs` files) exercise smoke + named-trait presence + member inheritance + conformance.
- **Blocks:** none
- **Note:** The §6.3-§6.6 modules listed as **MISSING** in `docs/notes/stdlib-trait-audit.md` (task 2347 deliverable, dated 2026-04-26) all landed via task 2354 in the same week. The audit doc was not updated to reflect this.

### M-002: Audit deliverable `docs/notes/stdlib-trait-breadth-audit-v01.md`

- **State:** DRIFT
- **Failure mode:** Documentation rot — PRD names a deliverable filename that was never produced; pre-existing audit doc was reused without updating it to reflect the inheritance reconciliation.
- **Evidence:** `docs/notes/stdlib-trait-audit.md` exists (the pre-existing audit from task 2347, dated 2026-04-26); no `stdlib-trait-breadth-audit-v01.md`. Task 2347 status `done`; its `done_provenance.commit=09cf49b894`. Task 2349/2352 status `done` (commits `bc5c2d69aa`, `b3429254e3`) — these fixed the gaps that `stdlib-trait-audit.md` §(d), §(f) still describe as open. The PRD's acceptance criteria do allow "or as a header comment block in the affected `.ri` files"; the `materials_mechanical.ri:32-38` "Trait Resolution Policy" block + the migration-pointer comments in each of `materials_thermal.ri`, `materials_electrical.ri`, `materials_optical.ri`, `materials_chemical.ri` satisfy that alternative for §6 traits.
- **Blocks:** future readers who consult `stdlib-trait-audit.md` and don't notice the doc is now stale w.r.t. fixed gaps.
- **Note:** Stale-audit-doc DRIFT is mild but pervasive — five of the six "summary recommendations" in `stdlib-trait-audit.md` §"Summary of Gaps and Recommended Follow-ups" are now resolved on main yet still presented as open.

### M-003: `ElasticallyDeformable : Flexible` inheritance edge (was `: Elastic`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `structural_physical.ri:65` `trait ElasticallyDeformable : Flexible`; test `elastically_deformable_refines_flexible_same_module` in `crates/reify-compiler/tests/structural_physical_tests.rs:338-366`; `structure def Rubber : ElasticallyDeformable` conformance test at `:519-573`. The trait's own doc-comment at `:53-64` discusses why the prior `: Elastic` edge was redundant.
- **Blocks:** none
- **Note:** PRD §"Inheritance reconciliation" item 1 fully delivered by task 2349 (commit `bc5c2d69aa`).

### M-004: `Plastic : Flexible` inheritance edge (was stand-alone)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `structural_physical.ri:82` `trait Plastic : Flexible`; test `plastic_refines_flexible` at `structural_physical_tests.rs:370-385`; doc-comment explains Plastic adds permanent-regime params on top of Flexible's lumped-deformation contract.
- **Blocks:** none
- **Note:** PRD §"Inheritance reconciliation" item 2 delivered by task 2349.

### M-005: `ThermallyConductive : Physical` inheritance edge (was stand-alone)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `structural_physical.ri:95` `trait ThermallyConductive : Physical`; test `thermally_conductive_refines_physical` at `structural_physical_tests.rs:389-403`; full-conformance test `structure_conforms_to_thermally_conductive_with_inherited_physical_constraints` at `:429`.
- **Blocks:** none
- **Note:** The PRD inherits this from the spec's §4 entry. The audit-doc-recommended cross-section migration (move `thermal_conductivity`/`max_service_temp` params *out* of §4 and into §6.3 `ThermallyCharacterized`) was NOT done — both the §4 trait and the §6.3 trait now carry overlapping `thermal_conductivity` params. Documented in `stdlib-trait-audit.md` §4 gap text but not resolved.

### M-006: `ElectricallyConductive : Physical` inheritance edge (was stand-alone)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `structural_physical.ri:107` `trait ElectricallyConductive : Physical`; test `electrically_conductive_refines_physical` at `structural_physical_tests.rs:409-423`.
- **Blocks:** none
- **Note:** Same param-overlap with §6.4 `ElectricallyCharacterized` (`resistivity` appears in both); not reconciled.

### M-007: `Physical : MaterialSpec` vs spec-says-no-parent

- **State:** RESOLVED
- **Failure mode:** Shape mismatch between spec and impl — Physical absorbs MaterialSpec where spec slots material in.
- **Evidence:** `structural_physical.ri:20` `trait Physical : MaterialSpec`; spec at `docs/reify-stdlib-reference.md` §4 line 439 declares `trait Physical` with `geometry: Solid` and `material: Material` slot params, no parent. `stdlib-trait-audit.md` §4 row "Physical" lists this as a parent gap + a "param-shape gap" + a "computed-let gap" (spec uses `let mass = volume(geometry) * material.density`; impl uses `let mass = volume * density` over flat params).
- **Blocks:** (resolved) PRD §"Out of scope" defers `geometry: Solid` migration to dimensional-type work and the geometry-as-trait-typed-param surface; M-013 ORPHAN tracks the prerequisite.
- **Note:** Resolved via `docs/prds/v0_3/geometry-handle-runtime.md`. Spec-shape `Physical { param geometry : Solid; param material : Material; let mass = volume(geometry) * material.density }` now lands via GHR-α stdlib registrations (`structural_physical.ri` rewrite, task 3603) + GHR-ζ OCCT kernel dispatch. The deliberate v0.1 flat-scalar trade is retired — `structural_physical.ri` no longer uses flat scalar params for geometry. Owning PRD: `docs/prds/v0_3/geometry-handle-runtime.md`.

### M-008: Same-named-trait collision §4 vs §6 resolution — single declaration in `materials_mechanical.ri`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `materials_mechanical.ri:32-38` "Trait Resolution Policy" comment block: `FatigueRated, FractureTough, ImpactResistant, and Damping each refine MaterialSpec; density and name are inherited transitively. Consumer structures should carry one material : MaterialSpec slot, not one per capability trait.` Each of the four traits declared `: MaterialSpec` at `:104`, `:111`, `:127`, `:135`. Conformance tests in `materials_mechanical_tests.rs` (734 lines).
- **Blocks:** none
- **Note:** PRD §"Resolve §4 vs §6 same-named-trait collision" item 3 fully delivered by task 2352 (commit `b3429254e3`). The §4 versions of these names in the spec are simply not separately declared — the spec text is consistent with the resolution.

### M-009: `: MaterialSpec` parent on §6 sub-traits — eight traits across four modules

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `materials_mechanical.ri:104,111,127,135` (FatigueRated/FractureTough/ImpactResistant/Damping); `materials_thermal.ri:38` (ThermallyCharacterized); `materials_electrical.ri:47` (ElectricallyCharacterized); `materials_optical.ri:27` (OpticallyCharacterized); `materials_chemical.ri:38,47` (CorrosionResistant, Biocompatible). All eight refining traits land the `: MaterialSpec` edge.
- **Blocks:** none
- **Note:** Note four §6.2 traits remain free-standing per the spec: `Elastic`, `Strong`, `Hard`, `Ductile` (see `materials_mechanical.ri:73,83,96,119`). The spec says all eight §6.2 traits should refine `Material`; the Trait Resolution Policy comment at `:36` acknowledges the asymmetry ("Elastic, Strong, Hard, Ductile remain free-standing; see docs/notes/stdlib-trait-audit.md §6.2 audit gap (d)."). This is a documented partial deviation — fine for v0.1 since v0.2 conformance machinery will reckon with it.

### M-010: §6.3-§6.6 gap-fill — eight traits + two enums declared

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `materials_thermal.ri:38` `ThermallyCharacterized`, `:53` `Refractory`; `materials_electrical.ri:47` `ElectricallyCharacterized`, `:58` `Conductive`, `:70` `Insulating`; `materials_optical.ri:27` `OpticallyCharacterized`; `materials_chemical.ri:23` `enum CorrosionClass`, `:31` `enum BiocompatibilityClass`, `:38` `CorrosionResistant`, `:47` `Biocompatible`. All ten artifacts exist + are exercised by `materials_thermal_tests.rs` (242 lines), `materials_electrical_tests.rs` (375 lines), `materials_optical_tests.rs` (200 lines), `materials_chemical_tests.rs` (312 lines).
- **Blocks:** none
- **Note:** Task 2354 (commit recorded as `found_on_main` post-rebase) delivered this. The audit doc `stdlib-trait-audit.md` §"Summary of Gaps... (c)" still lists all eight as MISSING — DRIFT in M-002 covers the doc rot.

### M-011: `Insulating` constraint `determined(dielectric_strength)` workaround

- **State:** DRIFT
- **Failure mode:** Spec predicate dropped; substitute `> 0.0` constraint chosen.
- **Evidence:** `materials_electrical.ri:21-36` (header block "Decision #3") documents that the Reify grammar has no `undef` keyword and no `determined(...)` predicate (cites `io.ri:11-12`). Spec at `docs/reify-stdlib-reference.md` §6.4 line 708 says `Insulating` adds `constraint determined(dielectric_strength)`. Impl substitutes `constraint dielectric_strength > 0.0` (`materials_electrical.ri:72`) as a "weaker claim" that "rejects the obvious placeholder pattern".
- **Blocks:** none observed (no consumer of the spec's `determined` semantics on main)
- **Note:** Author of the file flagged this as `Update (task 2484)`. The DRIFT is documented in-file but is a real semantic gap: a 1e-300 V/m insulator passes, where the spec intent is "known". Phase 3 may want to surface this if `determined(...)` is part of a broader Reify gap.

### M-012: Dimensional-type substitution — `Real` for `Pressure`/`Energy`/`Temperature`/`Length`/`Density`/`ThermalConductivity`/`SpecificHeat`

- **State:** TODO
- **Failure mode:** Placeholder — typed params downgraded to `Real` pending named-dim alias work; deferred by task 3115.
- **Evidence:** Every PRD-cited trait swaps `: Pressure` / `: Energy` / `: Temperature` / `: Length` / `: Density` for `: Real` plus a unit-comment. Examples: `structural_physical.ri:117-119` `seal_pressure_rating : Real` (spec says `seal_rating : Pressure`); `materials_mechanical.ri:73-77` `youngs_modulus : Real // (Pa)` (spec says `: Pressure`); `materials_thermal.ri:38-45` (six fields, all `Real`, six different physical dimensions). DimensionVector::PRESSURE exists at `crates/reify-types/src/dimension.rs:374` but no `.ri`-level alias. Task 3115 status `deferred`; task 3111 status `deferred`. Header comments in each `.ri` file explicitly call this out (`structural_physical.ri:5-9`; `materials_mechanical.ri:6-9`; `materials_thermal.ri:6-9`; etc.).
- **Blocks:** future re-typing pass once 3115 lands; consumers that want unit-safe arithmetic on these fields today must hand-track.
- **Note:** PRD §"Out of scope" line 124 deliberately defers this. The TODO is fully traced (per-field annotations in `docs/notes/stdlib-real-placeholder-audit.md`) — this is the cleanest, best-documented gap in the audit. Per fused-memory: task 3108 *did* ship `DimensionVector::PRESSURE` as a named constant for test-support; the gap is the `.ri`-source-level alias surface.

### M-013: `Solid` as a usable stdlib `.ri` param type

- **State:** WIRED
- **Failure mode:** No PRD-task in scope; this PRD calls it out as out-of-scope but the spec-shape of `Physical` and the worked-example comments (`examples/drivebelt_trait_bounds.ri:10-13`) ride on its absence.
- **Evidence:** At audit time no `: Solid` or `param.*: Solid` site existed in any stdlib `.ri` file (grep returned empty). Per `audit-brief.md` Things-to-take-as-given: `Solid` resolved to `Type::Geometry` in `type_resolution.rs:513` as a builtin alias, but stdlib traits could not use it as a slot type. `examples/drivebelt_trait_bounds.ri:10-13` explicitly noted: "Uses flat inherited params rather than `geometry: Solid` / `material: Material` slots because `Solid` is not yet a usable type (out of scope)." **Now:** `structural_physical.ri:38` declares `param geometry : Solid` — the first stdlib `.ri` slot to use the type, landing via GHR-ζ.
- **Blocks:** (wired) the spec-conformant shape of `Physical` (M-007), and by extension the spec-conformant shape of every §4 trait that refines it; deferred follow-up work to align stdlib with spec.
- **Note:** WIRED via `docs/prds/v0_3/geometry-handle-runtime.md`. `Solid` is now usable in stdlib trait-slot positions via `Value::GeometryHandle` (GHR-β variant + GHR-γ lowering). Geometry-bearing `Physical` evaluates with real volume/centroid (GHR-ζ OCCT kernel dispatch). Owning PRD: `docs/prds/v0_3/geometry-handle-runtime.md`. M-007 (spec-shape Physical) resolved in the same GHR phase series.

### M-014: Composed-bound integration test surface — `DriveBelt + CeramicLiner + Copper + BorosilicateGlass + TitaniumImplant`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `examples/drivebelt_trait_bounds.ri` (155 lines) declares five structures exercising composed bounds: `DriveBelt : ElasticallyDeformable + ImpactResistant + Damping`, `CeramicLiner : Refractory`, `Copper : Conductive`, `BorosilicateGlass : OpticallyCharacterized`, `TitaniumImplant : Biocompatible + CorrosionResistant`. Integration test at `crates/reify-eval/tests/drivebelt_trait_bounds.rs` (236 lines) — four tests: parses + ≥5 templates; DriveBelt trait_bounds + value cells; TitaniumImplant.corrosion_class is enum variant `CorrosionClass.C5`; all constraints Satisfied for every entity. Test compiles cleanly (verified via `cargo test -p reify-eval --test drivebelt_trait_bounds --no-run`).
- **Blocks:** none
- **Note:** PRD §"Tests" / acceptance-criteria fully delivered by task 2354. The PRD originally specified a single `traits_breadth.rs` file; the deliverable is named `drivebelt_trait_bounds.rs` (and adds §6.3-§6.6 gap-fill samples for ALL four absent subsections, not just DriveBelt). Minor name DRIFT — not flagged separately because the larger deliverable is strictly a superset.

## Cross-PRD breadcrumbs

- **M-007 / M-013 (Solid + geometry-driven Physical shape)** — both resolved/wired via `docs/prds/v0_3/geometry-handle-runtime.md` (GHR-α through GHR-ζ). M-007 is RESOLVED (spec-shape `Physical { param geometry : Solid; let mass = volume(geometry) * material.density }` lands); M-013 is WIRED (`Solid` is now a usable trait-slot type via `Value::GeometryHandle`). The historical `geometry-traits.md` intersection (half_space / extrude_infinite FICTION) is now a sequencing dependency owned by GR-018 — the consumer surface is wired; GR-018 ships the producers.
- **M-012 (dimensional types)** transitively gates every trait declaration here, and is also cited in `money-dimension.md` and (implicitly) in `per-purpose-tolerance.md`. The deferred task 3115 is a single choke point.
- **M-009 / M-008 (one-slot-many-traits material composition)** is a design pattern that the FEA PRDs (`structural-analysis-fea.md`, `multi-load-case-fea.md`) lean on heavily once structure-constructor runtime evaluation (GR-001) lands. Currently safe in isolation; if GR-001 picks a structural-conformance route, the "one slot, transitive density/name inheritance" pattern here will need re-examination.
- **M-002 (stale audit doc)** — `stdlib-trait-audit.md` is referenced from multiple per-file header comments (`materials_thermal.ri:11`, `materials_electrical.ri:7`, `materials_optical.ri:10`, `materials_chemical.ri:5`). Any future PRD that consults it as a status snapshot will read pre-2349/2352/2354 state.
