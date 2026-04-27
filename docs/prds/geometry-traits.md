# Geometry Trait Declarations + Conformance Machinery

## Goal

Land the seven geometry-conformance traits from stdlib reference §3.10 (`Bounded`,
`Watertight`, `Closed`, `Manifold`, `Orientable`, `Convex`, `Connected`) as
first-class trait declarations, wire compile-time inference for the
constructively-determinable subset (`Bounded`/`Connected`/`Convex`), and add
runtime conformance queries (via OCCT `BRepCheck` analogues) for the
topology-derived subset (`Watertight`/`Manifold`/`Orientable`). Provide a
specialization escape hatch for all seven, and emit a compile diagnostic when an
unbounded solid flows into a `Bounded`-requiring call site.

## Background

Spec/stdlib references:

- `docs/reify-stdlib-reference.md` §3.10 — declares all seven traits with
  `Watertight : Closed + Manifold` inheritance.
- Per-op trait flow: `half_space` and `extrude_infinite` produce **Unbounded**
  solids; every other primitive (Box/Cylinder/Sphere/Tube), Booleans of
  Bounded operands, transforms, sweeps, fillet/chamfer/shell over Bounded
  inputs, etc., **preserve or produce** Bounded.
- Domain libs cite `Bounded`/`Watertight` in worked examples (e.g. printable
  parts must be `Watertight`); these traits cannot wait for v0.2.

Existing infrastructure to lean on:

- `crates/reify-eval/src/geometry_ops.rs` — single source of truth for
  `GeometryOp` variants; this is where per-op trait inference lives.
- `crates/reify-compiler` `CompiledGeometryOp` — the IR layer where the
  inferred trait set is attached to each op.
- `crates/reify-kernel-occt` — the FFI surface where `BRepCheck`/
  `ShapeAnalysis_Shell`/`ShapeAnalysis_Wire`-style runtime queries land. New
  FFIs follow the pattern of `point_in_solid` / `shapes_intersect` from #319.

## Architectural decisions (already approved)

1. **Three categories**:
   - **Compile-time inferred** for `Bounded`, `Connected`, `Convex` —
     constructive: a fixed lookup table `GeometryOp → trait set produced` plus
     propagation rules through Booleans/transforms/sweeps.
   - **Runtime-checked** for `Watertight`, `Manifold`, `Orientable` — query
     the realized OCCT shape via `BRepCheck` (or analogue).
   - **Specialization escape hatch** for all seven — a structure may declare
     `: Watertight` explicitly (acting as the user's promise), suppressing the
     OCCT runtime check at the cost of trusting the user.
2. **Compile diagnostic at use site**: a call requiring `Bounded` (e.g.
   `volume`, `bounding_box`, `centroid`) on a value statically known to lack
   `Bounded` (i.e. produced by `half_space`/`extrude_infinite` and not
   subsequently bounded by intersection) emits `error[E_GEOMETRY_UNBOUNDED]` at
   the call site. `Watertight`/`Manifold` violations are runtime — no
   compile-time diagnostic.
3. **Trait flow rules** (per-op):
   - Primitives: `Box`/`Cylinder`/`Sphere`/`Tube` → `Bounded + Connected +
     Convex (Tube only when shell-thickness < radius)`. `half_space`,
     `extrude_infinite` → `Unbounded`.
   - Booleans: `Union(A, B)` → `Bounded` iff both `Bounded`; `Connected` is
     **not** preserved (union may produce two pieces); `Convex` is **not**
     preserved. `Difference(A, B)` → inherits `Bounded` from `A`. `Intersection`
     → inherits `Bounded` from either; `Convex` is preserved if both Convex.
   - Transforms (translate/rotate/scale/mirror): preserve all three.
   - Sweeps (extrude/revolve/loft): `Bounded` and `Connected` inherited from
     profile; `Convex` lost.
   - Fillet/chamfer/shell/draft/thicken: preserve `Bounded` and `Connected`;
     `Convex` lost.
4. **Stdlib trait-decl surface**: declare all seven traits in
   `crates/reify-compiler/stdlib/geometry_traits.ri` with no fields (marker
   traits), plus inheritance edge `Watertight : Closed + Manifold`. The
   compiler attaches inferred conformance via metadata, not via user-written
   parameters.

## Worked examples

```reify
// Compile-time error: half_space is Unbounded, volume requires Bounded.
fn err_unbounded_volume() -> Real {
    let h = half_space(plane_xy(0mm))
    volume(h)            // error[E_GEOMETRY_UNBOUNDED] at this call site
}

// OK: intersection with a Bounded solid restores Bounded.
fn ok_clipped_volume() -> Real {
    let h = half_space(plane_xy(0mm))
    let clip = box(10mm, 10mm, 10mm)
    volume(intersection(h, clip))   // Bounded inferred → no diagnostic
}

// Runtime conformance check: Watertight queried on the realized shape.
purpose ready_to_print(p : Solid) {
    constraint conforms(p, Watertight)   // BRepCheck-based runtime query
}

// Specialization escape hatch: user asserts Watertight, no runtime check.
structure def TrustedShell : Watertight {
    param geometry : Solid
}
```

## Scope

1. **Stdlib trait file** `crates/reify-compiler/stdlib/geometry_traits.ri`
   declaring the seven marker traits with the `Watertight : Closed + Manifold`
   inheritance edge. Loader picks it up via the existing prelude
   discovery glob.
2. **Per-op trait inference** — extend `CompiledGeometryOp` with an
   `inferred_traits : TraitSet` field; populate it during the compile pass
   that translates AST → `CompiledGeometryOp`. Lookup table lives in
   `crates/reify-compiler/src/geometry_traits_inference.rs` (new file, ~150L).
3. **Diagnostic at call sites** — when resolving calls whose signatures bind
   a `Bounded` (or `: Bounded`-refining) trait parameter, check the inferred
   set on the argument; emit `E_GEOMETRY_UNBOUNDED` if absent. Reuse the
   diagnostic infrastructure already used for trait-bound mismatches.
4. **Runtime conformance queries** — three new OCCT FFI entry points:
   `is_watertight(shape) → bool`, `is_manifold(shape) → bool`,
   `is_orientable(shape) → bool`. Wire to `BRepCheck_Analyzer` /
   `ShapeAnalysis_Shell` / `ShapeAnalysis_Wire` in `kernel-occt/cpp`. Expose
   via a `conforms<T : Geometry, R : Trait>(g : T, _ : Type<R>) → Bool`
   stdlib function (or per-trait helpers `is_watertight`, etc.).
5. **Specialization escape hatch** — when a structure declares `:
   Watertight` (or any of the seven) explicitly, the compiler treats this as
   a user assertion, skips the runtime check, and surfaces a
   `warn[W_TRAIT_USER_ASSERTED]` diagnostic the first time the structure is
   determined. (Compile-time inferred traits aren't relevant to the escape
   hatch — there's nothing to escape from.)

   *As implemented (task 2321):* compile-time emission lives in
   `crates/reify-compiler/src/entity.rs` (per-bound iteration in the existing
   `trait_bounds` loop). The diagnostic code is
   `DiagnosticCode::TraitUserAsserted` (PRD mnemonic `W_TRAIT_USER_ASSERTED`).
   Canonical message form:
   `"geometry trait '<Trait>' on '<Entity>' is treated as a user assertion; runtime conformance check is suppressed"`.
   The warning carries a single label at the bound's source span. Detection is
   name-based via `crates/reify-compiler/src/geometry_traits_inference.rs`
   `is_geometry_marker_trait(name: &str) -> bool` (case-sensitive, covers all
   seven). **Runtime-side suppression is a forward stub** — the eval-time
   BRepCheck hook (PRD tasks 4/5) is not yet wired into eval, so today the
   warning is the only observable effect.
6. **Tests**: per-op trait inference (full table), Boolean propagation rules,
   `E_GEOMETRY_UNBOUNDED` diagnostic, `is_watertight`/`is_manifold` against
   known-good (`box`) and known-bad (`union(box, translated_box)` separated)
   shapes, escape-hatch warning fires once.

## Out of scope

- Field-aware trait inference (e.g. trait values flowing through fields) —
  v0.2.
- Solvespace-style attribute-persistent conformance attestations — v0.2 (ties
  to feature-tag work in `topology-selectors.md`).
- `imported` field source kind, multi-kernel — v0.2.

## Acceptance criteria

- All seven traits declared in `geometry_traits.ri`, picked up by stdlib
  loader, visible via `compile_with_stdlib`.
- `cargo test -p reify-compiler -- geometry_traits_inference` covers the
  per-op table, Boolean propagation, and the unbounded-flow diagnostic.
- `cargo test -p reify-kernel-occt -- conformance` covers
  `is_watertight`/`is_manifold`/`is_orientable` against fixture shapes.
- `cargo test -p reify-eval -- conformance_runtime` covers the
  `conforms(g, Watertight)` end-to-end path.
- Specialization escape hatch warning (`W_TRAIT_USER_ASSERTED` /
  `DiagnosticCode::TraitUserAsserted`) fires exactly once per
  `(structure_def, geometry_marker_bound)` pair at compile time; eval-time
  suppression of the runtime conformance check is deferred until the OCCT
  runtime hook (PRD tasks 4/5) is wired into eval.

## Task breakdown (queueing aim: 6 tasks)

1. **Declare the seven geometry traits** in
   `crates/reify-compiler/stdlib/geometry_traits.ri`. Confirm stdlib loader
   picks them up; add a parse+compile smoke test asserting all seven trait
   names resolve from prelude. (Cheap; unblocks all other tasks here.)
2. **Compile-time trait inference table** for `Bounded`/`Connected`/`Convex`
   over all `CompiledGeometryOp` variants. New module
   `geometry_traits_inference.rs`; populate `inferred_traits` field on
   `CompiledGeometryOp`. Unit tests for every variant.
3. **Diagnostic `E_GEOMETRY_UNBOUNDED`** at call sites that bind a `Bounded`
   trait parameter to an argument lacking it. Reuse trait-bound error path.
   Tests: `volume(half_space(...))` errors; `volume(intersection(half_space,
   box))` does not.
4. **OCCT FFI: `is_watertight`/`is_manifold`/`is_orientable`** with
   `BRepCheck_Analyzer`/`ShapeAnalysis_*` backing. Following #319's FFI
   pattern. Fixture-based tests in `reify-kernel-occt`.
5. **Stdlib `conforms` function + per-trait helpers**, wiring runtime queries
   from task 4 into eval. Tests: known-good/known-bad shapes.
6. **Specialization escape hatch + `W_TRAIT_USER_ASSERTED` warning**. When
   `: Watertight` (or sibling) is user-declared on a structure, suppress
   runtime check, emit warning once. Test: warning count == 1 across
   repeated determinations.
