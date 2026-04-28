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
   via three monomorphic per-trait helpers `is_watertight(g) → Bool`,
   `is_manifold(g) → Bool`, `is_orientable(g) → Bool` in the prelude.
   The generic `conforms<T : Geometry, R : Trait>(g, Type<R>) → Bool` form
   is deferred to v0.2 — see *Out of scope* and *Design notes for task 2320*.
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
- Generic `conforms<T : Geometry, R : Trait>(g, Type<R>) → Bool` — deferred
  to v0.2. Requires type-as-value support that Reify's type system lacks
  today (no `Value::TraitTag`, bare trait names don't resolve as values).
  v0.1 ships only the three monomorphic helpers `is_watertight` /
  `is_manifold` / `is_orientable`. The generic form can be added later as
  syntactic sugar over the helpers without breaking source — call sites
  written as `is_watertight(g)` are forward-compatible.

## Design notes for task 2320

These are PRD-author rulings (2026-04-28) made after the architect's first
attempt timed out at 121 turns. Implementers should treat them as locked.

### Decision 1 — preserve the value-representability invariant

`Type::Geometry` has no `Value` variant; `is_representable_cell_type`
(`crates/reify-eval/src/engine_eval.rs:61`) and `value_type_kind_matches`
(`crates/reify-eval/src/lib.rs:124`) jointly enforce that no `ValueCellDecl`
ever carries `Type::Geometry`. **Do not relax this invariant.** It is
load-bearing for the snapshot/journal/content-hash architecture: a
`Value::Geometry(GeometryHandleId)` would have to participate in
`ContentHash`, `Journal` replay, and `BTreeSet`/`BTreeMap` ordering, but
handle ids are per-realization, per-kernel, non-persistent cookies — none
of those round-trips are well-defined.

Instead, route `is_watertight(g)` / `is_manifold(g)` / `is_orientable(g)`
through a **lookup-by-cell-id** sideband: at the FunctionCall arm, resolve
`g`'s `ValueCellId` to a `GeometryHandleId` via the realization handle
table (`step_handles` / `named_steps` in
`crates/reify-eval/src/engine_build.rs`), submit the kernel query, and
materialize the `Value::Bool` result. The handle never lands in `ValueMap`.

### Decision 2 — per-trait helpers, no generic `conforms<T,R>`

The v0.1 prelude exposes exactly three names: `is_watertight`,
`is_manifold`, `is_orientable`. The generic `conforms<T : Geometry, R :
Trait>(g, Type<R>) → Bool` form requires type-as-value plumbing
(`Value::TraitTag` or equivalent, plus a new identifier-resolution rule for
bare trait names) that Reify's type system lacks today. The seven marker
traits are a closed set in v0.1, so generic dispatch buys nothing
concrete. Generic `conforms` can be added in v0.2 as syntactic sugar over
the helpers without breaking call sites.

### Decision 3 — escape-hatch ships in v0.1, asymmetric per-marker match

Task 2321 ships the compile-time `W_TRAIT_USER_ASSERTED` warning
(`crates/reify-compiler/src/entity.rs:624-635`). Task 2320 ships the
runtime side: `try_eval_conformance_query` short-circuits
`is_watertight(g) → Bool(true)` (and the manifold/orientable peers) when
`g`'s owning template declares the matching marker as a `trait_bound`,
without invoking OCCT.

The match is **asymmetric and per-marker**: declaring `: Watertight`
short-circuits `is_watertight` only — it does NOT short-circuit
`is_manifold` or `is_orientable`. Symmetrically, declaring `: Closed`
does NOT short-circuit `is_watertight`, even though
`Watertight : Closed + Manifold`. Refinement is **not** propagated. This
is the conservative reading: the user's assertion is exactly the trait
they named, no more. Pinned by
`is_watertight_closed_bound_does_not_short_circuit` in
`crates/reify-eval/tests/conformance_runtime.rs`.

**Originally this was deferred to v0.1.1** (no v0.1 motivating case, since
`imported` is out-of-scope and there's no perf-sensitive procedural shape
in current examples). The implementing agent shipped it anyway because
the runtime hook is cheap (~10 LoC, single `template.trait_bounds.iter()`
scan inside the existing post-process pass) and the asymmetry test
materially clarifies the semantics. Accepting the work as-shipped.

### Implementation as shipped

Task 2320 was implemented in a single pass (commit `e091990a6`) after the
PRD-author rulings landed. The shipped surface:

| File | Change |
|---|---|
| `crates/reify-compiler/src/units.rs` | `GEOMETRY_QUERY_HELPER_NAMES` const + `is_geometry_query_helper` classifier |
| `crates/reify-compiler/src/expr.rs` | NoUserFunctions arm forces `Type::Bool` for the three names |
| `crates/reify-compiler/stdlib/geometry_traits.ri` | Doc-only block describing the helpers (name-based dispatch — no `fn` stubs needed) |
| `crates/reify-eval/src/geometry_ops.rs` | `try_eval_conformance_query` free fn — resolves `ValueRef(cell_id)` → `GeometryHandleId` via `named_steps`, dispatches kernel query, applies user-asserted-trait short-circuit |
| `crates/reify-eval/src/engine_build.rs` | Post-processes value cells via `post_process_conformance_queries` after `execute_realization_ops` populates `named_steps`, in both `build()` and `build_snapshot()` |
| `crates/reify-eval/tests/conformance_runtime.rs` | 6 integration tests: kernel-reply true/false, manifold, orientable, user-assertion short-circuit, asymmetry vs `Closed` |

**Why the original A-E split wasn't used:** the implementing agent
correctly identified that the dispatch surface was small enough (~220
LoC across 6 files) to land atomically with the locked decisions in hand,
making sub-task overhead net-negative. The 121-turn architect failure
was rooted in *missing decisions*, not task size — once the PRD locked
Decisions 1-3, the path through the codebase was straightforward.

The original `metadata.files` listed `crates/reify-stdlib/src/geometry.rs`
in error (that's the pure-`Value` `eval_builtin` path with no kernel
access). The correct file list is the table above.

## Acceptance criteria

- All seven traits declared in `geometry_traits.ri`, picked up by stdlib
  loader, visible via `compile_with_stdlib`.
- `cargo test -p reify-compiler -- geometry_traits_inference` covers the
  per-op table, Boolean propagation, and the unbounded-flow diagnostic.
- `cargo test -p reify-kernel-occt -- conformance` covers
  `is_watertight`/`is_manifold`/`is_orientable` against fixture shapes.
- `cargo test -p reify-eval -- conformance_runtime` covers the per-trait
  helper end-to-end path: `is_watertight(g)` / `is_manifold(g)` /
  `is_orientable(g)` for known-good (`box(10mm,10mm,10mm)` → all `true`)
  and known-bad (open-shell BRep → `is_watertight == false`) shapes.
- Specialization escape hatch warning (`W_TRAIT_USER_ASSERTED` /
  `DiagnosticCode::TraitUserAsserted`) fires exactly once per
  `(structure_def, geometry_marker_bound)` pair at compile time. Eval-time
  short-circuit is asymmetric and per-marker: declaring `: Watertight`
  short-circuits `is_watertight` only — `: Closed` does not, despite
  `Watertight : Closed + Manifold`. Pinned by
  `is_watertight_user_assertion_short_circuits_to_true` and
  `is_watertight_closed_bound_does_not_short_circuit` in
  `crates/reify-eval/tests/conformance_runtime.rs`.

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
5. **Per-trait stdlib helpers** `is_watertight(g) → Bool`,
   `is_manifold(g) → Bool`, `is_orientable(g) → Bool`, wiring runtime
   queries from task 4 into eval. **First query-style user-callable stdlib
   functions in Reify** — this task introduces the dispatch surface. See
   *Design notes for task 2320* below for the architectural decisions
   already made (lookup-by-cell-id over `Value::Geometry`, per-trait over
   generic `conforms<T,R>`, escape-hatch deferred). Tests: known-good
   (`box(10mm,10mm,10mm)` → true on all three) and known-bad (open shell
   built via direct BRep — see fixture pattern in
   `crates/reify-kernel-occt/tests/conformance_integration.rs`) shapes.
6. **Specialization escape hatch — compile-time warning + runtime
   short-circuit.** Compile-time: emit `W_TRAIT_USER_ASSERTED` once per
   `(structure_def, geometry_marker_bound)` pair when `: Watertight` (or
   sibling) is user-declared (task 2321). Runtime: when evaluating
   `is_watertight(g)` / `is_manifold(g)` / `is_orientable(g)` against a `g`
   whose owning template declares the matching marker as a `trait_bound`,
   short-circuit to `Bool(true)` without invoking the kernel. The match is
   **asymmetric and per-marker**: `: Closed` does NOT short-circuit
   `is_watertight`, even though `Watertight : Closed + Manifold` —
   refinement does not propagate to the helper. Implementation: see
   `try_eval_conformance_query` in `crates/reify-eval/src/geometry_ops.rs`;
   pinned by `is_watertight_user_assertion_short_circuits_to_true` and
   `is_watertight_closed_bound_does_not_short_circuit` in
   `crates/reify-eval/tests/conformance_runtime.rs`.
