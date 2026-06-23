# Stdlib Trait Long-Tail Breadth

## Goal

Declare the named structural and material traits from `docs/reify-stdlib-reference.md`
§4 and §6 that aren't already shipped by the headline traits in #327, #328,
#329 — establishing the named-trait + inheritance-edge surface so user code
that says `: ElasticallyDeformable` or `: Damping` resolves cleanly. v0.1
*declares* every named trait (parameters, inheritance, basic constraints) but
**does not** wire conformance machinery beyond trait existence — that's a v0.2
concern. The headline subset (`Physical`, `Rigid`, `Material`, `Elastic`,
`Strong`, `Hard`) already has full conformance + worked examples shipped via
#327/#328/#329; this PRD fills the breadth around them.

## Background

Headline status (already shipped):

- §4: `Physical`, `Rigid`, `Flexible`, `ThermallyConductive`,
  `ElectricallyConductive`, `Sealed` exist in `structural_physical.ri`
  (#328-ish).
- §6.2: `Elastic`, `Strong`, `Hard`, `FatigueRated`, `FractureTough`,
  `Ductile`, `ImpactResistant`, `Damping` exist in `materials_mechanical.ri`
  (#327).
- §6.3: `ThermallyCharacterized`, `Refractory` exist (#329-ish).
- §6.4 / §6.5 / §6.6: `ElectricallyCharacterized`, `Conductive`,
  `Insulating`, `OpticallyCharacterized`, `CorrosionResistant`,
  `Biocompatible` may or may not exist — this PRD checks and gap-fills.

This PRD's targets — what may still be missing or in need of polish:

- `ElasticallyDeformable` (§4) — declared but as `: Elastic` rather than as
  the spec's `: Flexible` refinement. Reconcile vs spec; spec defines
  `ElasticallyDeformable : Flexible`.
- `Plastic` (§4) — exists but stand-alone; spec defines `: Flexible`.
- `ImpactResistant`, `FractureTough`, `FatigueRated`, `Damping` — declared
  in §6 (material) but spec also references them in §4 (structural sub-traits)
  with the same name. Confirm the trait identity is shared (not duplicated
  under two names) and that the inheritance edge into `Physical` /
  `Material` matches the spec.
- `ThermallyConductive`, `ElectricallyConductive` — currently declared
  stand-alone in `structural_physical.ri`; spec says
  `ThermallyConductive : Physical` and `ElectricallyConductive : Physical`.
  Add the inheritance edges.
- `Sealed` — currently stand-alone, `param seal_pressure_rating : Real`;
  spec says no Physical inheritance and uses `seal_rating : Pressure` (note
  spec uses `Pressure` as quantity type, not `Real`; this PRD opens the
  door to switching once dimensional types are wired).

## Worked examples

```reify
// Composition: an elastomer belt is ElasticallyDeformable + ImpactResistant +
// Damping — three traits drawn from the breadth set, all simultaneously.
structure def DriveBelt : ElasticallyDeformable + ImpactResistant + Damping {
    param geometry : Solid
    param material : Material
    param density : Real            // from MaterialSpec
    param name : String             // from MaterialSpec
    param volume : Real             // from Physical (via Flexible chain)
    param max_elastic_strain : Real
    param max_deflection : Real     // from Flexible (now reachable through ED)
    param impact_energy : Real
    param damping_ratio : Real
    param loss_factor : Real
}

// A sealed enclosure with thermal characterisation.
structure def SealedHousing : Sealed + ThermallyConductive {
    param seal_pressure_rating : Real
    param thermal_conductivity : Real
    param max_service_temp : Real
    // Once ThermallyConductive : Physical edge lands, geometry/material/density/centroid
    // params are inherited too.
}
```

### Related: trait associated functions

This PRD declares trait *existence* (parameters, inheritance edges, basic
constraints) and defers conformance machinery to v0.2. With trait associated
functions now shipped, a required associated `fn` — e.g.
`fn loss_factor(self) -> Real` on the `Damping` trait — lets a long-tail named
trait demand behaviour from its conformers, not just parameters or inheritance
edges (compiler `RequirementKind::Fn`). See
[docs/prds/v0_6/trait-associated-functions.md](v0_6/trait-associated-functions.md)
for the assoc-fn machinery (G4 seam owner: that PRD; this PRD declares trait
existence only).

## Scope

1. **Audit gap-fill**: walk the §4 + §6 trait lists against the existing
   `crates/reify-compiler/stdlib/structural_physical.ri` and
   `materials_mechanical.ri`. Build a table of: trait name, spec section,
   currently-declared status, spec-defined parents, current parents, fields,
   gaps.
2. **Inheritance reconciliation** — make the existing trait declarations
   match the spec's inheritance edges:
   - `ElasticallyDeformable : Flexible` (currently `: Elastic` — fix).
   - `Plastic : Flexible` (currently stand-alone — fix).
   - `ThermallyConductive : Physical`.
   - `ElectricallyConductive : Physical`.
   - All §6 material sub-traits (`FatigueRated`, `FractureTough`,
     `ImpactResistant`, `Damping`) declared `: Material` (i.e.
     `: MaterialSpec` per the existing file's naming).
3. **Resolve §4 vs §6 same-named-trait collision**: `FatigueRated`,
   `FractureTough`, `ImpactResistant`, `Damping` appear in both §4
   (structural sub-traits, refining `Physical`) and §6 (material
   sub-traits, refining `Material`). The spec text is consistent: the
   *trait* declares material-property params; structures get these
   properties via their `material : Material` slot. v0.1 keeps a single
   declaration in `materials_mechanical.ri` (refining `MaterialSpec`).
   Document this resolution in the file's header comments so future
   re-readers don't introduce a duplicate.
4. **Gap declarations**: any §4/§6 trait still missing — declare it with
   the spec's params, dimensional placeholders (`Real` for now until
   dimensional types wire in), and the spec's inheritance edges.
5. **Cross-section sweep §6.3 / §6.4 / §6.5 / §6.6** — confirm
   `ThermallyCharacterized`, `Refractory`, `ElectricallyCharacterized`,
   `Conductive`, `Insulating`, `OpticallyCharacterized`,
   `CorrosionResistant`, `Biocompatible` and their two enums
   (`CorrosionClass`, `BiocompatibilityClass`) exist; gap-fill any
   missing.
6. **Tests**: a single `traits_breadth.rs` integration test that
   compiles a `.ri` source defining a structure with three composed
   trait bounds drawn from the breadth set (e.g. the `DriveBelt`
   example) and asserts compile success + correct inherited param
   resolution.

## Out of scope

- Conformance machinery for any non-headline trait (no automatic
  field/operation flow; structures must explicitly declare `:
  TraitName` and supply params).
- Switching `Real`-typed params to dimensional types like `Pressure`,
  `Energy`, `Temperature` — that's blocked on dimensional-type system
  work tracked elsewhere.
- `std.ports.*` traits (covered by another agent's port-trait scope).
- `std.tolerancing.*` traits (separate scope).

## Acceptance criteria

- Audit table committed under
  `docs/notes/stdlib-trait-breadth-audit-v01.md` (or as a header
  comment block in the affected `.ri` files) listing each §4/§6 trait,
  its spec parents, its current parents, and the action taken.
- `crates/reify-compiler/stdlib/structural_physical.ri` and
  `materials_mechanical.ri` reflect the spec's inheritance edges for
  every trait covered by this PRD.
- A `.ri` test source exercises a structure with composed trait
  bounds drawn from the breadth set (`DriveBelt`-style); compile
  succeeds, all inherited params resolve, no spurious diagnostics.
- All existing tests on `structural_physical.ri` /
  `materials_mechanical.ri` still pass (regression pin).
- File-header comments document the §4/§6 same-named-trait resolution
  so the choice is discoverable to future maintainers.

## Dependencies

Independent — this is purely declarative. References #327 (mechanical
material traits), #328 / #329 (structural / thermal trait sets).

## Task breakdown (queueing aim: 4 tasks)

1. **Audit + gap-fill report**: walk §4/§6 vs the two `.ri` files;
   produce the inheritance-and-presence table; identify exact
   declarations to add or amend. Output: a table block at the top of
   each file (or a single `docs/notes/stdlib-trait-breadth-audit-v01.md`).
   No code changes yet.
2. **Inheritance reconciliation in `structural_physical.ri`**:
   `ElasticallyDeformable : Flexible`, `Plastic : Flexible`,
   `ThermallyConductive : Physical`, `ElectricallyConductive :
   Physical`. Existing tests must still pass (params are unchanged;
   only the parent edge changes).
3. **Inheritance reconciliation in `materials_mechanical.ri`**:
   declare the four shared §4/§6 traits (`FatigueRated`,
   `FractureTough`, `ImpactResistant`, `Damping`) explicitly as
   `: MaterialSpec`. Add header comment documenting the §4/§6
   resolution.
4. **Composed-bound integration test**: `.ri` source defining the
   `DriveBelt`-style composed-bound structure; compile + parameter-
   resolution assertions. Folds in any §6.3-§6.6 gap-fills discovered
   during the audit.
