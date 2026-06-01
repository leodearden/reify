# PRD: Geometry primitive constructors (fill the advertised-but-unwired gaps)

**Status:** active — version-agnostic geometry foundation. Authored 2026-06-01.
**Slug:** `geometry-primitive-constructors`
**Approach:** B for the solid primitives (vertical slices through the established op-execute seam);
**B+H** for the **profile-trait seam** (the dimensionality refinement + use-site precondition —
contract + two-way boundary tests below).

## Goal

A `.ri` author can call every solid/profile constructor the stdlib reference advertises and get
real geometry:

```reify
let frustum = cone(10mm, 5mm, 20mm)        // truncated cone (BRepPrimAPI_MakeCone)
let ring    = torus(20mm, 5mm)             // (BRepPrimAPI_MakeTorus)
let v       = wedge(20mm, 10mm, 15mm, 5mm) // (BRepPrimAPI_MakeWedge)
let post    = cylinder_centered(5mm, 20mm) // centroid at z=0, not base-at-origin
let plate   = extrude(rectangle(20mm, 10mm), 3mm)  // 2D profile → solid
let disc    = extrude(circle(8mm), 2mm)
```

Today all of these emit the bare `unsupported geometry function: <name>` diagnostic
(`reify-compiler/src/geometry.rs:1270`). After this PRD they evaluate to a non-`Undef`
`Solid`, mesh in the GUI viewport, and export to STEP. The 2D shapes additionally carry
**inferred profile traits** (`dimension = Surface`, `Closed`, `Planar`) so `extrude`/`revolve`/
`sweep`/`loft` reject a solid argument at compile time with a clear diagnostic.

## Background — why this exists

The 2026-06-01 primitives investigation found that **task 303** (a bundle of 303 + 304 + 305,
marked `done` 2026-04-14, titled *"Bundle: stdlib primitives — cone + torus + wedge +
box_centered + cylinder_centered"*) is a **phantom-done**: its description claims "OCCT FFI +
full compiler/eval pipeline wiring" for all five primitives, but `git log -S` confirms the
implementing symbols (`GeometryOp::Cone`, `GeometryOp::Torus`, `PrimitiveKind::Cone`,
`make_cone`, `BRepPrimAPI_MakeTorus`, `BRepPrimAPI_MakeWedge`) **never landed in any commit**.
The `GeometryOp` enum (`reify-ir/src/geometry.rs:517`) has only `Box`/`Cylinder`/`Sphere`/`Tube`
as primitives. `FaceSurfaceKind::{Cone,Torus}` *does* exist (`geometry.rs:1397-1399`) — but that
is a **face-classification** kind for selectors like `%Torus`, not a constructor, which is the
trap that makes a grep look green.

Separately, `docs/reify-stdlib-reference.md` §3.2–3.3 (lines 309-347) advertises a broader
constructor surface than the language has: the five phantom solids **plus** four 2D shapes
(`rectangle`, `circle`, `polygon`, `ellipse`), plus `half_space` and `nurbs_surface`. A user
reading those docs and writing `cone(...)` hits a bare "unsupported" error with no hint the
function was documented-but-unimplemented.

This PRD fills the **constructor** gaps. The proven template is the existing
`box`/`cylinder`/`sphere`/`tube` path, which runs end-to-end through five layers
(cpp FFI → `GeometryOp` variant → `reify-compiler` lowering arm → `reify-eval` construction →
`reify-kernel-occt` execute), exhaustively dispatched with no stub arms.

### Why traits, not new geometry types (the load-bearing modelling decision)

The 2D profiles need to be distinguishable from solids so `extrude` can reject a solid. The
**wrong** way is a new `Surface` value-type (and then, by uniformity, `Closed`/`Planar`/`Manifold`
variants → a 2^N type explosion). Reify already rejected that path: the shipped `geometry-traits`
design (`docs/prds/geometry-traits.md`, tasks 2320/2321 `done`) models geometry refinement as
**marker traits on a single `Type::Geometry`**:

- Seven marker traits in `crates/reify-compiler/stdlib/geometry_traits.ri`: `Bounded`, `Closed`,
  `Manifold`, `Orientable`, `Convex`, `Connected`, `Watertight : Closed + Manifold`. Properties
  **compose** (Watertight = Closed + Manifold) rather than multiplying into named types.
- A per-op inference table (`crates/reify-compiler/src/geometry_traits_inference.rs`,
  `InferredTraits`) computes the constructively-determinable trait set for each constructor/op.
- A **use-site precondition diagnostic** already exists — `DiagnosticCode::GeometryUnbounded`
  (`crates/reify-compiler/src/conformance/mod.rs:387`) fires when a geometry value lacking
  `Bounded` flows into a `Bounded`-requiring call (`volume`, `centroid`, …).

An extrudable profile's real precondition is the **conjunction** `2D ∧ Closed ∧ Planar` — which
traits express as one bound and types can only express by naming every combination. So this PRD
adds the one axis the seven traits don't cover — **dimensionality** (a shape is exactly one of
1D/2D/3D) — plus a `Planar` marker, reuses `Closed`, and routes profile preconditions through the
existing `GeometryUnbounded`-style use-site diagnostic. No new value-type; no parallel refinement
mechanism. (The original value-representability invariant that once forbade geometry-as-value has
since been relaxed — `reify-eval/src/lib.rs:296`, GHR-β/task 3604 — so this is a choice on merit,
not a workaround.)

## Scope

**In scope** — the advertised constructors:

- **Solids:** `cone`, `torus`, `wedge` (new `BRepPrimAPI_Make*` FFI), `cylinder_centered`
  (compose existing `Cylinder` + `Translate`), `box_centered` (alias of `box`).
- **2D profiles:** `rectangle`, `circle`, `polygon`, `ellipse` — planar faces in the XY plane
  at z=0, consumable by `extrude`/`revolve`/`sweep`/`loft`.
- The **dimensionality refinement** (`GeomDim ∈ {Curve, Surface, Solid}` as an enum field on
  the inferred-traits record) + a new `Planar` marker trait + the dimensionality marker names
  `Curve`/`Surface`/`Solid`, so profile-consuming ops can check their argument.
- Attribute-seeding arms for the new solids (`reify-eval/src/primitive_attribute_seed.rs`, whose
  module doc at line 40 explicitly defers `Cone`/`Torus` "to a separate task before their seeding
  arms are wired here"), so selectors/roles work on the new faces.
- Docs + LSP-completion correction for what actually lands.

**Out of scope** (see *Out of scope* + *Cross-PRD relationship*):

- 2D-profile **booleans and transforms** as composition operators (`difference(rect, circle)`
  before extrude) → follow-up PRD `geometry-2d-profile-ops`. (Transforms are already shape-generic
  at the kernel layer and booleans already route to `BRepAlgoAPI`; the follow-up is about
  coplanarity-preserving result-typing and propagating the `Surface`/`Planar`/`Closed` traits
  through 2D booleans, and it depends on this PRD's dimensionality refinement.)
- `half_space` / `extrude_infinite` — separately tracked as the Bounded=false producer surface
  (existing tasks 3465 / 3466, both pending). Do **not** duplicate.
- `nurbs_surface` — a substantial standalone feature; not a drop-in constructor.

## Sketch of approach

### Solid primitives — drop-in op-execute slices

Each new solid is an independent vertical slice through the §3.1 op-execute seam
(`docs/prds/v0_3/engine-integration-norm.md`), mirroring `Sphere` exactly:

1. **cpp** (`reify-kernel-occt/cpp/occt_wrapper.{h,cpp}`): `make_cone(bottom_r, top_r, height)`
   via `BRepPrimAPI_MakeCone` (handles `top_r==0` pointed apex); `make_torus(major_r, minor_r)`
   via `BRepPrimAPI_MakeTorus`; `make_wedge(dx, dy, dz, ltx)` via `BRepPrimAPI_MakeWedge`.
2. **ffi** (`reify-kernel-occt/src/ffi.rs`): cxx bridge declarations.
3. **IR** (`reify-ir/src/geometry.rs`): `GeometryOp::{Cone, Torus, Wedge}` + `kind_name` arms.
4. **compiler** (`reify-compiler/src/geometry.rs`): match arms with exact arg-count checks
   (`cone`=3, `torus`=2, `wedge`=4) + add names to `GEOMETRY_FUNCTION_NAMES`
   (`reify-compiler/src/units.rs`) + a per-op inference arm (`geometry_traits_inference.rs`).
5. **eval** (`reify-eval/src/geometry_ops.rs`): construct the variant from the call.
6. **kernel execute** (`reify-kernel-occt/src/lib.rs`): validate dims → call FFI. Validation:
   cone radii finite ≥0 (not both 0), height>0; torus both >0 and `minor < major`; wedge dims>0.
7. **seeder** (`reify-eval/src/primitive_attribute_seed.rs`): cone reuses the cylinder cap/side
   normal-classification; torus/wedge use the generic `record_all_faces_as_side` /
   `record_all_edges_as_new_edge`. Inferred traits: cone/wedge `dimension=Solid` + all of
   bounded/connected/convex; **torus `dimension=Solid`, bounded+connected, `convex=false`** (a
   torus is non-convex — distinct from the other primitives).

`cylinder_centered(r, h)` and `box_centered(w, d, h)` need **no FFI / IR change**:

- `cylinder_centered` lowers to two sub-ops — `Cylinder { r, h }` then `Translate { 0, 0, -h/2 }`
  — using the existing sub-op accumulation pattern. Result centroid at z=0.
- `box_centered` is a thin alias: `box` already centres its centroid at origin
  (`make_box` uses `gp_Pnt corner(-w/2, -h/2, -d/2)`, occt_wrapper.cpp:303), so `box_centered`
  emits the identical `Box` op. The convention is documented, not re-implemented.

### 2D profiles + the dimensionality refinement (B+H)

2D shapes are face producers analogous to how the curve constructors are wire producers. Each
makes a planar face in the **XY plane at z=0**, **centred at origin** (matching `box`/`rectangle`
centring and the existing internal `make_circle_face` convention the `Pipe` op already relies on):

- `rectangle(width, height)` → `make_rectangle_face`; `circle(radius)` → expose
  `make_circle_face`; `polygon(points)` → closed planar face from a `List<Point2<Length>>`
  (reuses the existing point-list surface that `interp`/`bezier` already accept);
  `ellipse(semi_major, semi_minor)` → `make_ellipse_face`.
- New `GeometryOp::{RectangleProfile, CircleProfile, PolygonProfile, EllipseProfile}` (or a single
  `ProfileFace { kind, … }` — tactical, see Open questions).
- Their inference-table arms set `dimension=Surface`, `planar=true`, `closed=true`,
  `bounded=true`, `connected=true` (convex = true for rectangle/circle/ellipse; false for a
  general `polygon`).

The **dimensionality refinement** is the load-bearing seam. Geometry refinement today is the
`InferredTraits` record (`{bounded, connected, convex}`) attached per geometry expression; the
seven marker traits are declared but dimensionality is not among them. This PRD extends the record
with a computed mutually-exclusive `dimension: GeomDim {Curve, Surface, Solid}` enum field and a
`planar: bool` (and populates `closed` for the profile constructors), then makes `extrude`/
`revolve`/`sweep`/`loft` check the argument's inferred traits and emit a use-site diagnostic when a
*statically-known* mismatch flows in — exactly mirroring `emit_geometry_unbounded`. See the
**Contract** section for the producer→trait map, consumer→required-precondition map, and the
permissive back-compat rule.

## Resolved design decisions

1. **Centering convention:** `box` is already centroid-centred; `box_centered` is a documented
   alias, `cylinder_centered` is the genuinely-new centred variant. We do **not** redefine `box`
   to corner-at-origin (would break ~370 existing call sites + their world positions).
2. **Traits, not a new type.** Profiles are distinguished by an **inferred dimensionality**
   (`GeomDim {Curve, Surface, Solid}`) + the existing/new marker traits, within the shipped
   `geometry-traits` framework — **not** by a new `Surface` value-type. Rationale: profile
   preconditions are conjunctions of orthogonal properties (`2D ∧ Closed ∧ Planar`) which traits
   compose and types only multiply (the 2^N "plethora"); and it reuses the existing per-op
   inference table + `GeometryUnbounded`-style use-site diagnostic instead of standing up a
   parallel refinement mechanism.
3. **Dimensionality is a computed enum field on the inferred-traits record**, not three more
   marker bits. It is mutually-exclusive (a shape is exactly one of 1D/2D/3D), so an enum models
   it honestly; `Curve`/`Surface`/`Solid` are surfaced as bound-able marker names mapping to it.
   Exclusivity holds by construction — it's computed by the inference table, never user-asserted
   on a builtin result.
4. **Granularity = dimensionality kind + `Planar`, reuse `Closed`.** `extrude`/`revolve`/`loft`
   require `dimension=Surface ∧ Closed ∧ Planar`; `sweep` requires that of its profile and
   `dimension=Curve` of its path; `pipe` requires `dimension=Curve` of its path. (`Closed` is an
   existing marker; this PRD adds its compile-inference for the 2D constructors. `Planar` is the
   one genuinely-new marker.)
5. **Permissive back-compat:** the consumer check hard-rejects only a *statically-known-mismatched*
   operand (e.g. `extrude(box(...))` — `box` is statically `dimension=Solid`). An unrefined
   `Type::Geometry` whose dimension can't be inferred (e.g. a `param p : Solid`) is **accepted**,
   so no existing design breaks.
6. **2D-profile booleans/transforms are a separate PRD** (`geometry-2d-profile-ops`), depending on
   this PRD's dimensionality refinement + profile constructors.
7. **Profile plane:** all 2D shapes are faces in the XY plane at z=0, centred at origin.

## Pre-conditions for activating

None — all substrate exists today:

- `BRepPrimAPI_MakeCone/MakeTorus/MakeWedge` ship in the same OCCT `BRepPrimAPI` module the wrapper
  already uses for `MakeBox`/`MakeCylinder`/`MakeSphere`; the revolve-based torus volume identity is
  already validated in-kernel (`make_torus_profile` tests, `lib.rs:6320`).
- The `geometry-traits` framework (marker-trait stdlib file + per-op `InferredTraits` inference +
  the `GeometryUnbounded` use-site diagnostic) is shipped; this PRD **extends** it (one enum field,
  one new marker, one new diagnostic code, the consumer arms) — task α queues that work explicitly.
- Constructor-call and point-list syntax already parse (same grammar as `box(...)` / `interp([...])`).
- **No novel syntax → G3 grammar gate N/A.**

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometry-traits.md` (shipped, 2320/2321) | extends | the inferred-traits record (+`dimension`,+`planar`) + a new `Planar` marker + a profile-precondition diagnostic | **this-prd** | additive extension of a shipped framework; not a contested seam |
| `geometry-2d-profile-ops` (follow-up, unfiled) | produces-for | the dimensionality refinement + `rectangle`/`circle`/`polygon`/`ellipse` constructors | **this-prd** | this PRD owns + ships them; the ops PRD consumes |
| `persistent-naming-v2` (`reify-eval/src/primitive_attribute_seed.rs`) | extends | new-primitive seeder arms (`Cone`/`Torus`/`Wedge`) | **this-prd** | the module doc (line 40) defers these to "a separate task"; this PRD is that task |
| `engine-integration-norm.md` §3.1 op-execute | consumes | `GeometryOp::{Cone,Torus,Wedge,…Profile}` dispatch | n/a (norm) | the catalogued seam each new op plugs into |

No new contested-ownership pair is introduced (the three known contested seams in the overlay are
untouched).

## Contract — the profile-trait seam (H)

**Refinement record.** Extend `InferredTraits` (`geometry_traits_inference.rs`) from
`{bounded, connected, convex}` to additionally carry `dimension: GeomDim {Curve, Surface, Solid}`
(computed, mutually-exclusive), `planar: bool`, and `closed: bool`. Declare `Planar` (and the
dimensionality marker names `Curve`/`Surface`/`Solid`) in `geometry_traits.ri`.

**Producer → inferred refinement** (the per-op inference table assigns each constructor's record):

| Dimension | Producers | Notes |
|---|---|---|
| `Curve` | `line_segment`, `arc`, `helix`, `interp`, `bezier`, `nurbs` | `planar`/`closed` per constructor (a full `circle`-curve would be closed+planar; a `line_segment` neither) |
| `Surface` | `rectangle`, `circle`, `polygon`, `ellipse` (new) | all `planar=true, closed=true`; `convex=false` for general `polygon` |
| `Solid` | `box`, `box_centered`, `cylinder`, `cylinder_centered`, `sphere`, `tube`, `cone`, `torus`, `wedge`; boolean/sweep/pattern/modifier results | sweep/extrude/revolve **raise** dimension Surface→Solid |

**Consumer → required precondition** (checked at the consumer's compiler arm against the argument's
inferred record):

| Consumer | Argument | Required |
|---|---|---|
| `extrude`, `extrude_symmetric`, `revolve` | profile | `dimension=Surface ∧ Closed ∧ Planar` |
| `loft`, `loft_guided` | each profile | `dimension=Surface ∧ Closed ∧ Planar` |
| `sweep`, `sweep_guided` | profile / path | profile: `Surface ∧ Closed ∧ Planar`; path: `dimension=Curve` |
| `pipe` | path | `dimension=Curve` |

**Permissive back-compat rule (load-bearing).** The consumer raises a **blocking diagnostic only
when the operand's refinement is *statically known* and mismatched** (e.g. `extrude(box(...))` —
`box` is statically `dimension=Solid`). When the operand is an unrefined `Type::Geometry` (a
`param : Solid`/`Geometry`, or any expression whose dimension can't be inferred), the consumer
**accepts it** — no false rejection of existing designs. New diagnostic
`DiagnosticCode::GeometryProfileRequired`, emitted via a sibling of `emit_geometry_unbounded`.
Message: `extrude profile must be a 2D Surface (e.g. rectangle/circle); got a Solid`.

## Boundary-test sketch (H) — faces both sides of the profile seam

| Scenario | Precondition | Postcondition (asserted) |
|---|---|---|
| extrude rejects a known solid | `extrude(box(10mm,10mm,10mm), 5mm)` | `error[GeometryProfileRequired]` at the call site |
| extrude accepts a surface | `extrude(rectangle(20mm,10mm), 3mm)` (task ζ) | compiles; evals to `Solid`, volume ≈ 200mm²·3mm within 2% |
| revolve requires a surface | `revolve(circle(8mm), 0,0,0, 0,1,0, π)` | compiles to a non-`Undef` `Solid` |
| sweep path requires a curve | `sweep(rectangle(...), box(...))` | diagnostic — path is `dimension=Solid`, not `Curve` |
| dimension is computed, not asserted | `arc(...)` (a `Curve`) | usable as a sweep **path**; the same `arc` as an extrude **profile** → rejected |
| generic param stays permissive | `param p : Solid` forwarded to `extrude(p, 5mm)` | **accepted** (no false reject) — back-compat guard |

Task α's observable signal **is** the first + last rows (the rejection diagnostic + the
permissive-acceptance pin); the positive `Surface` rows are ζ/η's signals — closing the G2 loop.

## Decomposition plan

Greek labels are intra-batch; actual IDs assigned at decompose time. Each leaf names its
user-observable signal (G2) and the capabilities it asserts (G6 / manifest).

**Phase 1 — profile-trait seam (extends the shipped geometry-traits framework)**

- **α — dimensionality refinement + `Planar` + profile-precondition diagnostic.**
  Modules: `reify-compiler/src/geometry_traits_inference.rs` (add `dimension`/`planar`/`closed`),
  `reify-compiler/stdlib/geometry_traits.ri` (declare `Planar` + dimensionality markers),
  `reify-compiler/src/conformance/mod.rs` (`GeometryProfileRequired`, sibling of
  `emit_geometry_unbounded`), `reify-compiler/src/geometry.rs` (extrude/revolve/sweep/loft arms
  call the check). **No `reify-core`/`Type` change.**
  *Signal (leaf, CLI diagnostic):* `extrude(box(10mm,10mm,10mm), 5mm)` emits
  `error[GeometryProfileRequired]` via `reify check`; **and** `extrude(p, 5mm)` for `param p : Solid`
  compiles clean (permissive pin). *Unlocks:* ζ, η, and the follow-up ops PRD.

**Phase 2 — solid primitives (independent slices, no dep on α)**

- **β — `cone`.** Modules: occt cpp/ffi/lib, reify-ir, reify-compiler (+inference arm), reify-eval,
  seeder. *Signal (leaf):* `cone(10mm,5mm,20mm)` evals to a non-`Undef` `Solid`; volume within 2%
  of `(π/3)·h·(r1²+r1·r2+r2²)`; `cone(10mm,0mm,20mm)` yields a pointed cone; `cone(-1mm,…)` →
  validation diagnostic; STEP export non-empty. *Achievability basis:* mirrors the
  `assert_volume_near(…, 0.02, …)` tolerance the existing torus tests already meet.
- **γ — `torus`.** *Signal (leaf):* `torus(20mm,5mm)` → `Solid`, volume within 2% of `2π²·R·r²`
  (basis: existing `make_torus_profile` revolve test hits this at 0.02); `torus(5mm,20mm)`
  (minor≥major) → diagnostic; inferred traits mark it `convex=false`.
- **δ — `wedge`.** *Signal (leaf):* `wedge(20mm,10mm,15mm,5mm)` → `Solid` with the expected
  face count and a volume matching the trapezoidal-prism closed form within 2%.
- **ε — `cylinder_centered` + `box_centered`.** Modules: reify-compiler (+ units), reify-eval.
  No FFI/IR. *Signal (leaf):* `cylinder_centered(5mm,20mm)` centroid z ≈ 0 (bbox z∈[−10,10]mm);
  `box_centered(w,d,h)` produces a mesh identical to `box(w,d,h)`.

**Phase 3 — 2D profiles (depend on α)**

- **ζ — `rectangle` + `circle`.** Modules: occt cpp/ffi/lib, reify-ir, reify-compiler (+inference
  arm: `dimension=Surface, planar, closed`), reify-eval.
  *Signal (leaf):* `extrude(rectangle(20mm,10mm), 3mm)` → `Solid` vol ≈ 600mm³ within 2%;
  `circle(8mm)` area ≈ `π·64mm²`; both infer `dimension=Surface` (an `extrude` compiles; using one
  as a `pipe` path is rejected). *Dep:* α.
- **η — `polygon` + `ellipse`.** *Signal (leaf):* `extrude(polygon([p0..pN]), h)` volume ≈
  shoelace-area·h within 2%; `ellipse(10mm,5mm)` area ≈ `π·10·5 mm²`. *Dep:* α.

**Phase 4 — companion correction (depends on all constructor tasks)**

- **θ — docs + LSP completion + task-303 reconciliation.** Modules: `docs/reify-stdlib-reference.md`,
  `reify-lsp/src/completion.rs`. *Signal (leaf):* LSP completion offers `cone`/`torus`/`wedge`/
  `cylinder_centered`/`rectangle`/`circle`/`polygon`/`ellipse`; the stdlib reference no longer lists
  any of them as callable-without-caveat while still-unimplemented entries (`half_space`,
  `nurbs_surface`) are relabelled "planned — see PRD". Reopen/annotate phantom-done task 303 as
  superseded by this PRD. *Dep:* β, γ, δ, ε, ζ, η.

**Dependency edges:** ζ→α, η→α; θ→{β,γ,δ,ε,ζ,η}. β/γ/δ/ε/α have no intra-batch prereqs.

## Out of scope

- 2D-profile **booleans/transforms** (`difference`/`union`/`translate`/… on `Surface`-dimension
  operands) → follow-up PRD `geometry-2d-profile-ops`. Substrate note for that PRD: transforms are
  already shape-generic (`reify-compiler/src/geometry_transform.rs` imposes no restriction) and
  booleans already route to `BRepAlgoAPI` via generic `GeomRef`s; the real work there is
  propagating `dimension=Surface`/`Planar`/`Closed` through 2D booleans and returning a planar Face
  (not a compound).
- `half_space` / `extrude_infinite` — Bounded=false producers, tracked by tasks 3465 / 3466.
- `nurbs_surface` — standalone feature.
- **Runtime planarity/closedness checking** of arbitrary (non-constructor) geometry via `BRepCheck`
  — this PRD only *constructively infers* `Planar`/`Closed` for the new profile constructors (where
  they're true by construction), reusing the geometry-traits runtime-query category for the general
  case is future work.
- Non-XY / arbitrary-plane profiles (all profiles are XY-at-z=0 here; reposition via `translate`/
  `rotate`).
- **User-function profile params** (`fn foo(p : <profile-bounded geometry>)`). The builtin
  consumers check at their own call site; letting a user function *declare* a profile precondition
  needs geometry-trait-bound params, which the geometry-traits PRD left to v0.2.

## Open questions (tactical — decide at implementation)

1. **2D-profile op encoding.** Four `GeometryOp` variants (`RectangleProfile`/…), or one
   `ProfileFace { kind, params }`? *Suggested:* four variants, matching the existing one-variant-per-
   primitive convention. Decide in task ζ.
2. **Closed planar `Curve` as a profile.** Should `extrude` also accept a closed planar *wire*
   (`dimension=Curve` but bounds an area), auto-facing it — or require an explicit `Surface`
   constructor? *Suggested:* require `Surface` for now (our constructors produce faces); revisit if
   a closed-wire idiom emerges. Decide in task α.
3. **`make_wedge` parameterisation.** `BRepPrimAPI_MakeWedge` has several constructors; the
   `(dx, dy, dz, ltx)` form maps to `wedge(width, depth, height, top_width)`. Confirm the axis the
   taper applies to matches the doc signature; decide in task δ.
4. **polygon point-list literal.** Confirm `polygon([…])` reuses the same `List<Point2<Length>>`
   surface `interp`/`bezier` accept (expected yes); if 2D points need a distinct literal, fold into η.
