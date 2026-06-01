# PRD: Geometry primitive constructors (fill the advertised-but-unwired gaps)

**Status:** active — version-agnostic geometry foundation. Authored 2026-06-01.
**Slug:** `geometry-primitive-constructors`
**Approach:** B for the solid primitives (vertical slices through the established op-execute seam); **B+H** for the 2D-profile **Surface-type** seam (contract + two-way boundary tests below).

## Goal

A `.ri` author can call every solid/profile constructor the stdlib reference advertises and get real geometry:

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
`Solid`, mesh in the GUI viewport, and export to STEP. The 2D shapes additionally
carry a new `Surface` value-kind so `extrude`/`revolve`/`sweep`/`loft` type-check their
profile argument.

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

## Scope

**In scope** — the advertised constructors:

- **Solids:** `cone`, `torus`, `wedge` (new `BRepPrimAPI_Make*` FFI), `cylinder_centered`
  (compose existing `Cylinder` + `Translate`), `box_centered` (alias of `box`).
- **2D profiles:** `rectangle`, `circle`, `polygon`, `ellipse` — planar faces in the XY plane
  at z=0, carrying the new `Surface` value-kind, consumable by `extrude`/`revolve`/`sweep`/`loft`.
- The `Surface`/`Curve`/`Solid` **geometry-kind refinement** that lets profile-consuming ops
  type-check their argument.
- Attribute-seeding arms for the new solids (`reify-eval/src/primitive_attribute_seed.rs`, whose
  module doc at line 40 explicitly defers `Cone`/`Torus` "to a separate task before their seeding
  arms are wired here"), so selectors/roles work on the new faces.
- Docs + LSP-completion correction for what actually lands.

**Out of scope** (see *Out of scope* + *Cross-PRD relationship*):

- 2D-profile **booleans and transforms** as composition operators (`difference(rect, circle)`
  before extrude) → follow-up PRD `geometry-2d-profile-ops`. (Transforms are already shape-generic
  at the kernel layer and booleans already route to `BRepAlgoAPI`; the follow-up is about
  coplanarity result-typing and accepting `Surface` operands, and it depends on this PRD's
  `Surface` type.)
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
   (`reify-compiler/src/units.rs`).
5. **eval** (`reify-eval/src/geometry_ops.rs`): construct the variant from the call.
6. **kernel execute** (`reify-kernel-occt/src/lib.rs`): validate dims → call FFI. Validation:
   cone radii finite ≥0 (not both 0), height>0; torus both >0 and `minor < major`; wedge dims>0.
7. **seeder** (`reify-eval/src/primitive_attribute_seed.rs`): cone reuses the cylinder cap/side
   normal-classification; torus/wedge use the generic `record_all_faces_as_side` /
   `record_all_edges_as_new_edge`. Trait inference: cone/wedge `all()` (convex); **torus
   `bounded + connected, convex=false`** (a torus is non-convex — distinct from the other
   primitives' `InferredTraits::all()`).

`cylinder_centered(r, h)` and `box_centered(w, d, h)` need **no FFI / IR change**:

- `cylinder_centered` lowers to two sub-ops — `Cylinder { r, h }` then `Translate { 0, 0, -h/2 }`
  — using the existing sub-op accumulation pattern. Result centroid at z=0.
- `box_centered` is a thin alias: `box` already centres its centroid at origin
  (`make_box` uses `gp_Pnt corner(-w/2, -h/2, -d/2)`, occt_wrapper.cpp:303), so `box_centered`
  emits the identical `Box` op. The convention is documented, not re-implemented.

### 2D profiles + the Surface-kind seam (B+H)

2D shapes are face producers analogous to how the curve constructors are wire producers. Each
makes a planar face in the **XY plane at z=0**, **centred at origin** (matching `box`/`rectangle`
centring and the existing internal `make_circle_face` convention the `Pipe` op already relies on):

- `rectangle(width, height)` → `make_rectangle_face`; `circle(radius)` → expose
  `make_circle_face`; `polygon(points)` → closed planar face from a `List<Point2<Length>>`
  (reuses the existing point-list surface that `interp`/`bezier` already accept);
  `ellipse(semi_major, semi_minor)` → `make_ellipse_face`.
- New `GeometryOp::{RectangleProfile, CircleProfile, PolygonProfile, EllipseProfile}` (or a single
  `ProfileFace { kind, … }` — tactical, see Open questions).

The **Surface-kind refinement** is the load-bearing seam. Geometry is currently mono-typed
(`Type::Geometry`, refined only by `InferredTraits`); `"Surface"`/`"Curve"` don't resolve as type
names. This PRD adds a `GeomKind ∈ {Curve, Surface, Solid}` dimensionality refinement so
profile-consuming ops can require the right kind. See the **Contract** section for the exact
producer→kind map, consumer→required-kind map, and the **permissive back-compat rule** (only
statically-known-3D operands are hard-rejected; unrefined `Type::Geometry` params stay accepted).

## Resolved design decisions

1. **Centering convention:** `box` is already centroid-centred; `box_centered` is a documented
   alias, `cylinder_centered` is the genuinely-new centred variant. We do **not** redefine `box`
   to corner-at-origin (would break ~370 existing call sites + their world positions).
2. **2D profiles get a distinct `Surface` value-kind** (not generic handles) so
   `extrude`/`revolve`/`sweep`/`loft` type-check the profile argument and reject a solid with a
   clear diagnostic. The three-kind taxonomy is `Curve` (1D) / `Surface` (2D profile) / `Solid` (3D).
3. **Permissive kind-checking for back-compat:** the consumer kind-check hard-rejects only
   *statically-known-mismatched* operands (e.g. `extrude(box(...))`); an unrefined `Type::Geometry`
   (e.g. a `param p : Solid`) is accepted, so no existing design breaks.
4. **2D-profile booleans/transforms are a separate PRD** (`geometry-2d-profile-ops`), depending on
   this PRD's `Surface` type + profile constructors.
5. **Profile plane:** all 2D shapes are faces in the XY plane at z=0, centred at origin.
6. **Torus is non-convex** in trait inference (the other new solids are convex).

## Pre-conditions for activating

None — all substrate exists today:

- `BRepPrimAPI_MakeCone/MakeTorus/MakeWedge` ship in the same OCCT `BRepPrimAPI` module the wrapper
  already uses for `MakeBox`/`MakeCylinder`/`MakeSphere`; the revolve-based torus volume identity is
  already validated in-kernel (`make_torus_profile` tests, `lib.rs:6320`).
- Constructor-call and point-list syntax already parse (same grammar as `box(...)` / `interp([...])`).
- **No novel syntax → G3 grammar gate N/A.** The only substrate gap is type-resolution +
  the geometry-kind refinement, which task α queues explicitly.

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometry-2d-profile-ops` (follow-up, unfiled) | produces-for | the `Surface` value-kind + `rectangle`/`circle`/`polygon`/`ellipse` constructors | **this-prd** | this PRD owns + ships them; the ops PRD consumes |
| `persistent-naming-v2` (`reify-eval/src/primitive_attribute_seed.rs`) | extends | new-primitive seeder arms (`Cone`/`Torus`/`Wedge`) | **this-prd** | the module doc (line 40) defers these to "a separate task"; this PRD is that task. Not a contested seam |
| `engine-integration-norm.md` §3.1 op-execute | consumes | `GeometryOp::{Cone,Torus,Wedge}` dispatch | n/a (norm) | the catalogued seam each new solid plugs into |

No new contested-ownership pair is introduced (the three known contested seams in the overlay are
untouched).

## Contract — the geometry-kind / `Surface`-type seam (H)

**Kind taxonomy.** Introduce `GeomKind { Curve, Surface, Solid }` as a compile-inferred
refinement of geometry expressions, carried alongside `InferredTraits`. Make the type-annotation
names `"Surface"` and `"Curve"` resolve (today only `"Solid"` does, → `Type::Geometry`).

**Producer → kind map** (every geometry-producing builtin is assigned a kind):

| Kind | Producers |
|---|---|
| `Curve` | `line_segment`, `arc`, `helix`, `interp`, `bezier`, `nurbs` |
| `Surface` | `rectangle`, `circle`, `polygon`, `ellipse` (new) |
| `Solid` | `box`, `box_centered`, `cylinder`, `cylinder_centered`, `sphere`, `tube`, `cone`, `torus`, `wedge`; boolean/sweep/pattern/modifier results |

**Consumer → required-kind map:**

| Consumer | Argument | Required kind |
|---|---|---|
| `extrude`, `extrude_symmetric`, `revolve` | profile | `Surface` |
| `loft`, `loft_guided` | each profile | `Surface` |
| `sweep`, `sweep_guided` | profile / path | `Surface` (profile), `Curve` (path) |
| `pipe` | path | `Curve` |

**Permissive back-compat rule (load-bearing).** The kind-check raises a **blocking diagnostic only
when the operand's kind is *statically known* and mismatched** (e.g. `extrude(box(...))` — `box` is
statically `Solid`). When the operand is an unrefined `Type::Geometry` (a `param : Solid`/`Geometry`,
or an expression whose kind can't be inferred), the consumer **accepts it** — no false rejection of
existing designs. Diagnostic text: `extrude profile must be a 2D Surface (e.g. rectangle/circle);
got a Solid`.

**Representation** (Type-enum variant vs `InferredTraits` field) is tactical — see Open questions.
The *semantics* above are fixed.

## Boundary-test sketch (H) — faces both sides of the kind seam

| Scenario | Precondition | Postcondition (asserted) |
|---|---|---|
| extrude rejects a known solid | `extrude(box(10mm,10mm,10mm), 5mm)` | compile diagnostic "extrude profile must be a 2D Surface … got a Solid" |
| extrude accepts a surface | `extrude(rectangle(20mm,10mm), 3mm)` (task ζ) | compiles; evals to `Solid`, volume ≈ 200mm²·3mm within 2% |
| revolve requires surface | `revolve(circle(8mm), 0,0,0, 0,1,0, π)` | compiles to a non-`Undef` `Solid` |
| sweep path requires curve | `sweep(rectangle(...), box(...))` | diagnostic "sweep path must be a Curve" |
| curve is curve-kinded | `arc(...)` used as a sweep **path** | accepted; same `arc` as an extrude **profile** → rejected |
| generic param stays permissive | `param p : Solid` forwarded to `extrude(p, 5mm)` | **accepted** (no false reject) — back-compat guard |

Task α's observable signal **is** the first + last rows of this table (the rejection diagnostic +
the permissive-acceptance pin); the positive `Surface` rows are ζ/η's signals — closing the G2 loop.

## Decomposition plan

Greek labels are intra-batch; actual IDs assigned at decompose time. Each leaf names its
user-observable signal (G2) and the capabilities it asserts (G6 / manifest).

**Phase 1 — kind-seam foundation**

- **α — geometry-kind refinement (`Curve`/`Surface`/`Solid`) + permissive consumer check.**
  Modules: `reify-core/src/ty.rs`, `reify-compiler/{type_resolution,geometry,types}.rs`.
  *Signal (leaf, CLI diagnostic):* `extrude(box(10mm,10mm,10mm), 5mm)` emits the
  "must be a 2D Surface … got a Solid" diagnostic via `reify check`; **and** `extrude(p, 5mm)`
  for `param p : Solid` compiles clean (permissive pin). *Unlocks:* ζ, η, and the follow-up
  ops PRD. *G6:* the rejection capability (static-kind inference for `box`) is delivered by α
  itself; no downstream dependency.

**Phase 2 — solid primitives (independent slices, no dep on α)**

- **β — `cone`.** Modules: occt cpp/ffi/lib, reify-ir, reify-compiler, reify-eval, seeder.
  *Signal (leaf):* `cone(10mm,5mm,20mm)` evals to a non-`Undef` `Solid`; volume within 2% of
  `(π/3)·h·(r1²+r1·r2+r2²)`; `cone(10mm,0mm,20mm)` yields a pointed cone; `cone(-1mm,…)` →
  validation diagnostic; STEP export non-empty. *Achievability basis:* mirrors the
  `assert_volume_near(…, 0.02, …)` tolerance the existing torus tests already meet.
- **γ — `torus`.** *Signal (leaf):* `torus(20mm,5mm)` → `Solid`, volume within 2% of `2π²·R·r²`
  (basis: existing `make_torus_profile` revolve test hits this at 0.02); `torus(5mm,20mm)`
  (minor≥major) → diagnostic; inferred traits mark it **non-convex**.
- **δ — `wedge`.** *Signal (leaf):* `wedge(20mm,10mm,15mm,5mm)` → `Solid` with the expected
  face count and a volume matching the trapezoidal-prism closed form within 2%.
- **ε — `cylinder_centered` + `box_centered`.** Modules: reify-compiler (+ units), reify-eval.
  No FFI/IR. *Signal (leaf):* `cylinder_centered(5mm,20mm)` centroid z ≈ 0 (bbox z∈[−10,10]mm);
  `box_centered(w,d,h)` produces a mesh identical to `box(w,d,h)`.

**Phase 3 — 2D profiles (depend on α)**

- **ζ — `rectangle` + `circle`.** Modules: occt cpp/ffi/lib, reify-ir, reify-compiler, reify-eval.
  *Signal (leaf):* `extrude(rectangle(20mm,10mm), 3mm)` → `Solid` vol ≈ 600mm³ within 2%;
  `circle(8mm)` area ≈ `π·64mm²`; both are `Surface`-kinded (an `extrude` of them compiles, a
  `pipe(rectangle(...), …)` path-use is rejected). *Dep:* α.
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

- 2D-profile **booleans/transforms** (`difference`/`union`/`translate`/… on `Surface` operands) →
  follow-up PRD `geometry-2d-profile-ops`. Substrate note for that PRD: transforms are already
  shape-generic (`reify-compiler/src/geometry_transform.rs` imposes no kind restriction) and
  booleans already route to `BRepAlgoAPI` via generic `GeomRef`s; the real work there is
  coplanarity-preserving result-typing (a planar boolean returns a `Surface` face, not a compound)
  and admitting `Surface` operands to the kind-check.
- `half_space` / `extrude_infinite` — Bounded=false producers, tracked by tasks 3465 / 3466.
- `nurbs_surface` — standalone feature.
- Non-XY / arbitrary-plane profiles (all profiles are XY-at-z=0 here; reposition via `translate`/
  `rotate`).

## Open questions (tactical — decide at implementation)

1. **GeomKind representation.** Carry the kind as a new `Type::Geometry(GeomKind)` payload, or as a
   `kind` field on `InferredTraits`? *Suggested:* the latter (mirrors how Bounded/Connected/Convex
   are already tracked, minimises churn to the widely-used `Type::Geometry`). Decide in task α.
2. **2D-profile op encoding.** Four `GeometryOp` variants (`RectangleProfile`/…), or one
   `ProfileFace { kind, params }`? *Suggested:* four variants, matching the existing one-variant-per-
   primitive convention. Decide in task ζ.
3. **`make_wedge` parameterisation.** `BRepPrimAPI_MakeWedge` has several constructors; the
   `(dx, dy, dz, ltx)` form maps to `wedge(width, depth, height, top_width)`. Confirm the axis the
   taper applies to matches the doc signature; decide in task δ.
4. **polygon point-list literal.** Confirm `polygon([…])` reuses the same `List<Point2<Length>>`
   surface `interp`/`bezier` accept (expected yes); if 2D points need a distinct literal, fold into η.
