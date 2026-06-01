# PRD: Geometry transforms, frames & projection — wire the advertised-but-unwired transform surface

**Status:** active — version-agnostic geometry foundation. Authored 2026-06-01.
**Slug:** `geometry-transforms-frames-projection`
**Approach:** **B** for the independent stdlib-surface slices (each a vertical slice through an
existing seam); **B+H** for the **value-decode seam** (the shared `Value::Transform` /
`Value::Plane` / `Value::Axis` decode that four consumers depend on — contract + two-way
boundary tests below).

## Goal

A `.ri` author can use the full geometry **transform / frame / projection** surface the stdlib
reference advertises (§3.1, §3.7, §3.8, §3.10) and get real results instead of silent `Undef`:

```reify
let t   = transform3(orient_axis_angle(vec3(0.0,0.0,1.0), 90deg), vec3(5mm,0mm,0mm))
let g   = apply_transform(box(10mm,10mm,10mm), t)       // rigid place — TODAY: Undef
let r   = rotate(box(10mm,10mm,10mm), orient_look_at(vec3(0.0,0.0,1.0), vec3(0.0,1.0,0.0)))  // TODAY: "expects 5 arguments"
let s   = scale(box(10mm,10mm,10mm), vec3(2.0,1.0,0.5)) // per-axis — TODAY: "factor non-finite"
let m   = mirror(box(10mm,10mm,10mm), plane_xy(0mm))    // TODAY: "expects 7 arguments"
let ring = circular_pattern(box(2mm,2mm,2mm), axis_z(point3(0mm,0mm,0mm)), 6, 60deg)  // TODAY: "expects 9 arguments"
let grid = arbitrary_pattern(box(2mm,2mm,2mm), [t, transform3(orient_identity, vec3(20mm,0mm,0mm))])  // rotation honored — TODAY: drops rotation
let pl   = project(point3(1mm,2mm,3mm), frame3(point3(1mm,0mm,0mm), orient_identity))  // TODAY: Undef
let e    = orient_euler(EulerConvention.XYZ, 10deg, 20deg, 30deg)  // uppercase enum — TODAY: "unresolved name: XYZ"
let ok   = is_closed(box(10mm,10mm,10mm))               // runtime conformance — TODAY: Undef
```

After this PRD each line above evaluates to a real value (`reify eval` / `reify check`), the
geometry-producing ones mesh in the GUI and export to STEP, and the documented Plane/Axis **value
types** become first-class arguments to their documented consumers instead of orphan products.

## Background — the gap register's premise was half wrong (verified empirically 2026-06-01)

The 2026-06-01 stdlib-reference survey (`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`,
clusters **P6 geometry-frames-planes-constructors** + **P7 geometry-transform-query**) flagged this
surface, but its highest-severity claims are **stale**, because the survey grepped only
`reify-compiler` + `reify-eval` and **missed the `reify-stdlib` value-builtin layer** (the generic
`eval_builtin` pass-through, `crates/reify-stdlib/src/lib.rs:103,106`). Ground-truthed against the
live `target/debug/reify` binary on 2026-06-01:

| Gap-register claim | Empirical ground truth (`reify eval`) | Verdict |
|---|---|---|
| `plane_xy/xz/yz`, `axis_x/y/z` **"not implemented — unreachable from source"** (HIGH) | `plane_xy(5mm)` → `plane(point(0,0,0.005), vec(0,0,1))`; `axis_z(...)` → real `Value::Axis` | **FALSE** — they work today (`reify-stdlib/src/geometry.rs:597-604`) |
| `Plane`/`Axis` **"not constructible from source"** | constructible via `plane_*`/`axis_*`; `Value::Plane`/`Value::Axis` are first-class `Value` variants (`reify-ir/src/value.rs:910-918`) | **FALSE** |
| `frame3`/`transform3`/`orient_euler`/`orient_basis` missing | all evaluate to real values today | **FALSE** (not in this PRD's gap set) |
| `project(point/vector, to: Frame)` unregistered (HIGH) | `project(point3(...), f)` → `undef` | **TRUE** |
| `apply_transform(geom, Transform<3>)` "compile error" (HIGH) | not a compile error — silently accepted, evals to `undef` | **TRUE (broken)**, framing off |
| `rotate(geom, Orientation<3>)` overload missing | `rotate(b, oid)` → `error: rotate() expects 5 arguments, got 2` | **TRUE** |
| `scale(geom, Vector3<Real>)` overload missing | `scale(b, vec3(2,1,0.5))` → `error: 'factor' non-finite` | **TRUE** |
| `arbitrary_pattern` translation-only (task 323 phantom) | confirmed: parses `N×(dx,dy,dz)` triples only (`reify-eval/src/geometry_ops.rs:774-811`; live example `examples/pattern_composition.ri:74`) | **TRUE (phantom-partial)** |
| `orient_look_at` never implemented | `orient_look_at(...)` → `undef` | **TRUE** |
| `EulerConvention` enum absent; uppercase rejected | `XYZ` → `unresolved name`; only lowercase strings accepted | **TRUE** |
| `Closed/Convex/Connected/Bounded` markers have no runtime query | `is_closed(b)` / `is_bounded(b)` → `undef` | **TRUE** (low) |

**The real Plane/Axis gap is producer-orphan, not missing-constructor.** `plane_*`/`axis_*` produce
real `Value::Plane`/`Value::Axis`, but **no documented consumer accepts them**: `mirror(g, plane_xy(0mm))`
fails *"expects 7 arguments, got 2"* and `circular_pattern(g, axis_z(...), 4, 90deg)` fails *"expects 9
arguments, got 4"* — both consumers want flattened scalar triples, not the value type the docs
(`§3.8`, `§3.10`) promise. This is exactly the G1 producer-orphan failure mode (cf. C-10
`selector_vocabulary_v2`: 22 fns produced, none consumed). So the in-scope work is **wiring the
existing values into their consumers + doc-reconciling the false "missing" claim**, not
re-implementing constructors that already exist.

This PRD is the **transform/frame/projection sibling** of the in-flight
`docs/prds/geometry-primitive-constructors.md` (the *primitive-constructors* cluster). That PRD
explicitly carves transforms/booleans/patterns out of its scope as a follow-up; this is that
follow-up for the transform half. The two share the shipped `geometry-traits` framework but touch
disjoint mechanisms (see *Cross-PRD relationship*).

## Scope

**In scope** — the genuinely-broken transform/frame/projection surface:

- **`apply_transform(geometry, Transform<3>)`** — register the user-facing stdlib free function;
  it consumes the **already-landed** rigid `GeometryOp::ApplyTransform` op (task **3901**, done) that
  was built for internal sub-placement but never surfaced as a callable name.
- **`project(point, to: Frame<3>)` + `project(vector, to: Frame<3>)`** — both overloads, as pure
  value-algebra in `reify-stdlib` (world→local frame transform; vector overload drops translation).
- **`rotate(geometry, orientation: Orientation<3>)`** — the orientation-quaternion overload, via
  arg-count dispatch in the existing `rotate` compiler arm (reuses the existing `Rotate` op).
- **`scale(geometry, factors: Vector3<Real>)`** — the per-axis non-rigid overload, via a new
  `GeometryOp::ScaleNonUniform` lowering to the **already-landed** `gtransform_shape` FFI (task
  **3959**, done).
- **`arbitrary_pattern(geometry, List<Transform<3>>)`** — honor the full per-instance rigid transform
  (rotation + translation) by decoding `Value::Transform` per instance into the rigid `ApplyTransform`
  op; **supersedes phantom-partial task 323** (translation-only). Keeps the existing translation-triple
  form for back-compat.
- **`orient_look_at(forward, up)`** — Gram-Schmidt look-at constructor (reuses `orient_basis`).
- **`EulerConvention` enum + enum-value path** — declare `enum EulerConvention { XYZ, … }` in a
  geometry stdlib `.ri` (prelude-seeded) and make `orient_euler` / `orient_to_euler` accept the
  qualified enum value `EulerConvention.XYZ` (consuming the **already-landed** enum-value lowering of
  tasks **2525/2558/4108**), keeping the lowercase-string path for back-compat.
- **Plane/Axis value consumers** — wire `mirror(geometry, plane: Plane)` and
  `circular_pattern(geometry, axis: Axis, …)` to accept the `Value::Plane` / `Value::Axis` their
  constructors already produce (a shared decode helper, also consumed by `half_space`), closing the
  producer-orphan. Keep the flattened-scalar forms for back-compat.
- **Runtime trait-conformance queries** `is_closed` / `is_connected` / `is_bounded` — extend the
  shipped `is_watertight`/`is_manifold`/`is_orientable` runtime-query path (tasks **2320/2318**, done);
  declare the `Geometry` / `Transformable` supertraits the trait file already defers.
- Docs + LSP-completion correction + task-303-style reconciliation of the gap register / task 323.

**Out of scope:**

- **`plane_*`/`axis_*`/`frame3`/`transform3`/`orient_euler`(lowercase)/`orient_basis` constructors** —
  **already implemented**; the gap register's "missing" claim is false. Doc-reconcile only (task ι).
- **`scale(geometry, Vector3)` as part of the general affine surface.** The non-rigid *general*
  affine value type + `affine_apply`/`affine_scale`/`affine_compose` surface is owned by the v0_6
  **AffineMap** PRD (`docs/prds/v0_6/affine-map-type.md`, tasks 3959/3960/3963). This PRD owns only the
  documented `scale(geometry, Vector3)` *geometry overload*, lowering to a diagonal `gtransform_shape`
  — a sibling of AffineMap's `AffineApply`, not the general affine surface (see seam table). Task 3963
  explicitly instructs "do NOT redirect the existing uniform scale op through gp_GTrsf", confirming the
  `scale` overload is unowned by AffineMap.
- **`is_convex` runtime query** — deferred. `BRepCheck_Analyzer` (the substrate the other three queries
  use) tests validity/closedness, **not** convexity; a real convexity predicate needs convex-hull
  comparison, a separate numerically-fuzzy feature with its own accuracy bound (G6).
- **`half_space(Plane)`** — owned by tasks 3465/3466 (the `Bounded=false` producer surface). This PRD
  provides the Plane-value decode helper they consume (G4 seam); it does **not** implement `half_space`.
- **Bare (unqualified) enum-variant-as-value** (`orient_euler(XYZ, …)` without the `EulerConvention.`
  qualifier) — Reify enum values are referenced qualified (`EnumName.Variant`); bare-variant resolution
  is a separate cross-cutting compiler feature outside this geometry cluster. The enum-value path here
  uses the working qualified form.
- **Plane/Axis member access** (`plane_xy(0mm).origin`) — member access on built-in struct values is
  blanket-unsupported today (`Frame.origin` also fails); a general member-access feature, not this PRD.
  Consumers decode the components internally without needing user-facing field access.
- **Profile/dimensionality traits, 2D booleans, solid primitives** — owned by
  `geometry-primitive-constructors.md` (task 4155 et al.).

## Sketch of approach

Every in-scope mechanism is an **additive vertical slice over an already-landed substrate** — there
is no new kernel capability and no novel grammar (G3 verified, below). The five-layer template the
existing `translate`/`rotate`/`scale` path uses (units-table → compiler arm → `TransformKind` →
eval construction → kernel execute) is mirrored exactly. Concretely:

- **`apply_transform`** (task α): register the name (`reify-compiler/src/units.rs`
  `GEOMETRY_FUNCTION_NAMES`), add a 2-arg compiler arm (`geometry_transform.rs`) +
  `TransformKind::ApplyTransform` (`types.rs`), and an eval arm (`reify-eval/src/geometry_ops.rs`)
  that decodes `Value::Transform { rotation, translation }` → the existing
  `GeometryOp::ApplyTransform { rotation:[f64;4], translation:[f64;3] }` (`reify-ir/src/geometry.rs:604`,
  kernel `lib.rs:2110`). IR + kernel + FFI already exist.
- **`project`** (task β): two arms in `reify-stdlib/src/geometry.rs::eval_geometry`, mirroring the
  existing `frame_to_frame` decode + `quat_rotate`: `project(Point, Frame) = inverse(basis)·(p − origin)`;
  `project(Vector, Frame) = inverse(basis)·v`. Pure value algebra, no kernel.
- **`rotate(Orientation)`** (task γ): the `rotate` compiler arm dispatches on arg count — 5 args =
  existing axis+angle; 2 args = decode `Value::Orientation` → axis-angle, reuse the existing `Rotate` op.
- **`scale(Vector3)`** (task δ): the `scale` compiler arm dispatches on arg shape — scalar = existing
  uniform `Scale`; `Vector3` = new `GeometryOp::ScaleNonUniform { sx, sy, sz }` → diagonal
  `gtransform_shape(diag(sx,sy,sz))` (FFI `ffi.rs:479`, done). Reject zero/non-finite components.
- **`arbitrary_pattern(List<Transform>)`** (task ε): the compiler/eval arms accept a `List<Transform<3>>`
  arg (alongside the existing scalar-triple form), decoding each `Value::Transform` into a per-instance
  rigid `ApplyTransform`.
- **`orient_look_at` + `EulerConvention`** (task ζ): `reify-stdlib/src/orientation.rs` —
  `orient_look_at(fwd, up)` Gram-Schmidt-orthonormalizes then reuses the `orient_basis` quaternion
  path; declare the enum in a prelude geometry `.ri`; `orient_euler`/`orient_to_euler` match
  `Value::Enum { type_name:"EulerConvention", variant }` (+ keep lowercase strings).
- **Plane/Axis consumers** (task η): `mirror`/`circular_pattern` compiler+eval arms accept a
  `Value::Plane`/`Value::Axis` (decode origin+normal / origin+direction via the shared helper),
  alongside their existing flattened-scalar forms.
- **Trait queries** (task θ): three new arms in the `try_eval_conformance_query` path
  (`geometry_ops.rs`) + `GeometryQuery::{IsClosed,IsConnected,IsBounded}` + OCCT predicates
  (`BRepCheck_Analyzer` closedness, shape connectivity, bounding-box finiteness), mirroring
  is_watertight/is_manifold/is_orientable. Declare `Geometry`/`Transformable` markers.

## The value-decode seam (H — contract + two-way boundary tests)

Four consumers (`apply_transform`, `arbitrary_pattern`, `mirror`, `circular_pattern`) and one
cross-PRD consumer (`half_space`, task 3465) all need to decode the same built-in value variants into
their numeric components. Hand-rolling that decode five times is the drift hazard this H component
prevents.

**Contract.** A single decode surface (helpers in `reify-eval`/`reify-stdlib`, exact module TBD at
task η) with these total functions and round-trip guarantees against the constructors:

| Decode | Input variant | Output | Round-trip invariant |
|---|---|---|---|
| `decode_transform` | `Value::Transform { rotation, translation }` | `([f64;4] unit-quat, [f64;3])` | `decode_transform(transform3(o, v))` ≡ `(quat(o), components(v))` |
| `decode_plane` | `Value::Plane { origin, normal }` | `([f64;3], [f64;3] unit-normal)` | `decode_plane(plane_xy(z))` ≡ `([0,0,z], [0,0,1])` (and xz/yz analogues) |
| `decode_axis` | `Value::Axis { origin, direction }` | `([f64;3], [f64;3] unit-dir)` | `decode_axis(axis_z(p))` ≡ `(components(p), [0,0,1])` |

Each decode is **total**: a malformed/`Undef`/wrong-variant input returns a typed `None`/diagnostic
(never a panic, never silent `0`). Normals/directions are normalized; a zero-magnitude normal/direction
is a diagnostic, not a `(0,0,0)` pass-through.

**Two-way boundary tests** (the H signal): for each decode, a producer→decode round-trip test
(constructor output decodes to the expected components) **and** a consumer-rejection test (a non-matching
variant, e.g. `mirror(box, axis_z(...))` — an Axis where a Plane is required — yields a typed diagnostic,
not a wrong-plane mirror). Task η's observable signal **is** these two rows for Plane/Axis; α's is the
Transform round-trip.

## Resolved design decisions

1. **Plane/Axis = wire the existing values into consumers, not re-implement constructors.** Ground truth:
   the constructors already produce real values; the consumers don't accept them. (User-confirmed
   2026-06-01.) `mirror`/`circular_pattern` gain a `Value::Plane`/`Value::Axis` arm; the false "missing
   constructor" claim is doc-reconciled.
2. **`apply_transform` is rigid-only and consumes the landed `ApplyTransform` op (3901).** `Transform<3>`
   is rigid (rotation+translation) by definition (stdlib-reference §3.1); the rigid op already exists.
   No AffineMap dependency.
3. **`scale(Vector3)` is a self-contained `ScaleNonUniform` op over the landed `gtransform_shape` FFI
   (3959)** — not a dependency on the deferred AffineMap op layer (3963 pending). Respects 3963's own
   boundary ("do NOT redirect uniform scale through gp_GTrsf"); the two are sibling ops on the
   op-execute seam, distinct user surfaces (`scale`→geometry vs `affine_apply`→geometry-from-AffineMap).
4. **`arbitrary_pattern` is additive: keep the translation-triple form, add the `List<Transform>` form.**
   The live example `examples/pattern_composition.ri:74` uses the triple form — it must keep passing
   (a regression guard). The documented `List<Transform<3>>` form is added alongside.
5. **EulerConvention uses the working qualified enum-value path (`EulerConvention.XYZ`)**, consuming the
   landed enum-value lowering (2525/2558/4108). Lowercase-string back-compat retained. Bare-variant
   resolution is explicitly out of scope. (User-confirmed: full enum-**value** path, qualified form.)
6. **`is_convex` deferred; ship `is_closed`/`is_connected`/`is_bounded`** (kernel-tractable). Convexity
   has no cheap OCCT predicate. (User-confirmed 2026-06-01.)
7. **Value types stay built-in `Value` variants** (`Transform`/`Frame`/`Plane`/`Axis`), consistent with
   the SO(3)/SE(3) kinematic machinery that already relies on them — **not** re-modelled as stdlib
   `structure def`s. Member access on them is a separate general feature (out of scope).

## Pre-conditions for activating

None — every substrate this PRD consumes is already on `main`:

- Rigid `GeometryOp::ApplyTransform` + `apply_transform_to_handle` kernel path (task **3901**, done,
  commit `623dc77d8d`).
- Non-rigid `gtransform_shape` FFI (`gp_GTrsf`/`BRepBuilderAPI_GTransform`) (task **3959**, done).
- Qualified enum-variant value lowering (`EnumName.Variant` → `Value::Enum`) (tasks **2525/2558/4108**,
  done — verified live: `OutputFormat.STEP` → `OutputFormat::STEP`).
- Runtime conformance-query path (`is_watertight`/`is_manifold`/`is_orientable` via `BRepCheck_Analyzer`)
  (tasks **2320/2318**, done).
- Built-in `Value::{Transform,Frame,Plane,Axis,Orientation}` variants + the `plane_*`/`axis_*`/`frame3`/
  `transform3`/`orient_*` constructors (`reify-stdlib`, shipped).
- **No novel syntax → G3 grammar gate verified N/A.** The two not-obviously-existing fragments —
  `enum EulerConvention { … }` declaration and `[transform3(…), …]` list-literal argument — both parse
  under the real `reify` compiler today (the only errors are *semantic*: enum-variant seeding and
  `arbitrary_pattern` arg-handling, which are exactly the implementation work). The standalone
  `tree-sitter parse` CLI is stale here (it rejects even known-good committed examples), so the
  authoritative parser is the `reify` binary, used for the gate.

## Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `geometry-primitive-constructors.md` (task 4155, pending) | sibling | shared `geometry_traits.ri` + `geometry_traits_inference.rs`; this PRD's θ adds `Geometry`/`Transformable` markers + `is_*` queries, 4155 adds `Planar`/dimensionality. **Disjoint mechanisms** (runtime kernel query vs compile-time inference). | each owns its half | soft-coordinate on `geometry_traits.ri` edits |
| v0_6 `affine-map-type.md` (3959 done / 3960,3963 pending) | sibling | both consume the shared `gtransform_shape` FFI; AffineMap owns the general `AffineMap` value-type + `affine_apply`/`affine_scale`; this PRD owns the documented `scale(geometry, Vector3)` overload (diagonal `ScaleNonUniform`). | distinct surfaces | soft-coordinate on `ffi.rs`/`geometry_ops.rs` edits; no functional dep |
| `half_space` (tasks 3465/3466, pending) | produces-for | this PRD's η ships the canonical Plane-value decode helper; `half_space(Plane)` should consume it rather than re-roll | **this-prd** owns the decode | 3465 design still open; soft seam |
| `engine-integration-norm.md` §3.1 op-execute | consumes | `GeometryOp::{ApplyTransform, ScaleNonUniform, Mirror, CircularPattern, ArbitraryPattern}` dispatch | n/a (norm) | the catalogued seam each op plugs into |
| sub-placement `ApplyTransform` op (task 3901, done) | consumes | `apply_transform` + `arbitrary_pattern` decode `Value::Transform` into this rigid op | this PRD consumes | landed |

No new contested-ownership pair is introduced (the three known contested seams in the overlay are
untouched).

## Boundary-test sketch (H) — value-decode round-trips both ways

| Scenario | Precondition | Postcondition (asserted) |
|---|---|---|
| rigid place preserves volume | `apply_transform(box(10,10,10), transform3(orient_axis_angle(z,90°), vec3(5mm,0,0)))` | non-`Undef` Solid; `volume ≈ 1000mm³` (within 0.1%); centroid x shifts +5mm |
| Transform decode round-trip | `decode_transform(transform3(o, v))` | `≡ (quat(o), components(v))` bit/ε-exact |
| Plane consumer accepts a value | `mirror(box(10,10,10), plane_xy(0mm))` | compiles + evals to a Solid (today: "expects 7 arguments") |
| Axis consumer accepts a value | `circular_pattern(box(2,2,2), axis_z(point3(0,0,0)), 6, 60°)` | 6-instance list; `union_all` volume ≈ 6× single (no overlap) |
| wrong-variant rejected | `mirror(box, axis_z(...))` (Axis where Plane required) | typed diagnostic, **not** a wrong-plane mirror |
| pattern honors rotation | `arbitrary_pattern(box(2,2,10), [transform3(orient_axis_angle(z,90°), vec3(0,0,0))])` | the instance's bbox reflects the 90° rotation (≠ the un-rotated box) |
| pattern back-compat | `arbitrary_pattern(box, 10,0,0, 0,20,0)` (triple form) | still produces 2 translated instances |
| projection world→local | `project(point3(1mm,2mm,3mm), frame3(point3(1mm,0,0), orient_identity))` | `≈ point3(0mm,2mm,3mm)`; vector overload keeps no translation |

## Decomposition plan

Greek labels are intra-batch; actual IDs assigned at decompose time. Each leaf names its
user-observable signal (G2 — CLI `reify eval`/`check`) and the capabilities it asserts (G6 / manifest).
Tasks α–θ are independent vertical slices (no intra-batch prereqs); the orchestrator's narrow file
locks serialize the ones sharing `geometry_transform.rs` (α,γ,δ) / `geometry_ops.rs` (α,δ,ε,η,θ) —
no artificial dep edges needed. ι depends on all of them.

- **α — `apply_transform(geometry, Transform<3>)`.** Modules: `reify-compiler/src/units.rs`,
  `geometry_transform.rs`, `types.rs`, `reify-eval/src/geometry_ops.rs` (+ the `decode_transform`
  helper). Consumes landed `GeometryOp::ApplyTransform` (3901). *Signal:* `apply_transform(box(10mm,
  10mm,10mm), transform3(orient_axis_angle(vec3(0.0,0.0,1.0),90deg), vec3(5mm,0mm,0mm)))` evals to a
  non-`Undef` Solid; `volume` ≈ 1000mm³ (rigid ⇒ volume-preserving, within 0.1%); STEP export non-empty;
  malformed Transform → diagnostic, not panic.
- **β — `project(point, Frame)` + `project(vector, Frame)`.** Modules: `reify-stdlib/src/geometry.rs`.
  *Signal:* `project(point3(1mm,2mm,3mm), frame3(point3(1mm,0mm,0mm), orient_identity))` ≈
  `point3(0mm,2mm,3mm)`; the `vector` overload of the same offset keeps `(1,2,3)` (no translation);
  non-Frame 2nd arg → `Undef`.
- **γ — `rotate(geometry, Orientation<3>)` overload.** Modules: `reify-compiler/src/geometry_transform.rs`
  (arg-count dispatch), `reify-eval/src/geometry_ops.rs`. *Signal:* `rotate(box(10mm,10mm,10mm),
  orient_axis_angle(vec3(0.0,0.0,1.0),90deg))` produces geometry whose bbox equals
  `rotate(box(10mm,10mm,10mm), 0,0,1, 90deg)` (today: "expects 5 arguments").
- **δ — `scale(geometry, Vector3<Real>)` per-axis overload.** Modules: `reify-compiler` (scale arm +
  `TransformKind`/op), `reify-ir/src/geometry.rs` (`ScaleNonUniform`), `reify-eval/src/geometry_ops.rs`,
  `reify-kernel-occt/src/lib.rs` (diagonal `gtransform_shape`). *Signal:* `scale(box(10mm,10mm,10mm),
  vec3(2.0,1.0,0.5))` → bbox 20×10×5mm, `volume` ≈ 1000mm³; `scale(box, vec3(0.0,1.0,1.0))` → zero-factor
  diagnostic. *Achievability:* `gtransform_shape` already exact at B-rep for `diag(1,1,2)` per task 3959's
  landed kernel test.
- **ε — `arbitrary_pattern(geometry, List<Transform<3>>)` (supersede 323).** Modules: `reify-compiler/
  src/geometry.rs`, `reify-eval/src/geometry_ops.rs`, `reify-ir/src/geometry.rs` (extend
  `ArbitraryPattern` to carry per-instance rotation, or emit per-instance `ApplyTransform`). *Signal:*
  `arbitrary_pattern(box(2mm,2mm,10mm), [transform3(orient_axis_angle(vec3(0.0,0.0,1.0),90deg),
  vec3(0mm,0mm,0mm))])` yields an instance whose bbox reflects the 90° rotation (≠ the un-rotated box);
  the legacy `arbitrary_pattern(w, 10,0,0, 0,20,0)` triple form in `examples/pattern_composition.ri`
  still produces 2 translated instances. *Reconciles:* task 323.
- **ζ — `orient_look_at` + `EulerConvention` enum-value path.** Modules: `reify-stdlib/src/
  orientation.rs`, a geometry stdlib `.ri` (enum decl). *Signal:* `orient_look_at(vec3(0.0,0.0,1.0),
  vec3(0.0,1.0,0.0))` → a non-`Undef` Orientation (matches the known Gram-Schmidt quaternion);
  `orient_euler(EulerConvention.XYZ, 10deg, 20deg, 30deg)` evals equal to `orient_euler("xyz", 10deg,
  20deg, 30deg)` (both non-`Undef`); `orient_to_euler(EulerConvention.ZYX, q)` returns a 3-element angle
  list. Consumes landed enum-value lowering (2525/2558/4108).
- **η — Plane/Axis value consumers (`mirror(g, Plane)` + `circular_pattern(g, Axis)`) + the shared
  decode helper.** Modules: `reify-compiler/src/geometry.rs`, `reify-eval/src/geometry_ops.rs`
  (+ `decode_plane`/`decode_axis`). *Signal:* `mirror(box(10mm,10mm,10mm), plane_xy(0mm))` and
  `circular_pattern(box(2mm,2mm,2mm), axis_z(point3(0mm,0mm,0mm)), 6, 60deg)` compile + eval to real
  geometry (today: "expects 7/9 arguments"); `mirror(box, axis_z(...))` (wrong variant) → typed
  diagnostic; the legacy flattened-scalar forms still work. *Closes:* the plane/axis producer-orphan.
- **θ — runtime trait queries `is_closed`/`is_connected`/`is_bounded` + `Geometry`/`Transformable`
  markers.** Modules: `reify-compiler/src/units.rs` (`GEOMETRY_QUERY_HELPER_NAMES`),
  `reify-eval/src/geometry_ops.rs` (`try_eval_conformance_query`), `reify-ir/src/geometry.rs`
  (`GeometryQuery::{IsClosed,IsConnected,IsBounded}`), `reify-kernel-occt` (OCCT predicates),
  `crates/reify-compiler/stdlib/geometry_traits.ri` (declare `Geometry`/`Transformable`). *Signal:*
  `is_closed(box(10mm,10mm,10mm))`, `is_connected(...)`, `is_bounded(...)` all eval `true`;
  `is_closed(box) and is_bounded(box)` composes. *Defers:* `is_convex` (noted in code + docs).
  *Soft-coordinate:* 4155 on `geometry_traits.ri`.
- **ι — docs + LSP completion + gap-register/task-323 reconciliation.** Modules:
  `docs/reify-stdlib-reference.md`, `reify-lsp/src/completion.rs`,
  `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`. *Signal:* the stdlib reference
  marks `plane_*`/`axis_*`/`frame3`/`transform3`/`orient_euler`/`orient_basis` as **implemented** (not
  missing), and `apply_transform`/`project`/`scale(Vector3)`/`rotate(Orientation)`/`orient_look_at`/
  `EulerConvention`/`mirror(Plane)`/`circular_pattern(Axis)`/`is_closed`/`is_connected`/`is_bounded` as
  implemented-by-this-PRD; LSP completion offers the newly-wired names; the gap register's P6/P7 false-HIGH
  rows are annotated; task 323 is annotated phantom-partial-resolved-by-ε. *Dep:* α,β,γ,δ,ε,ζ,η,θ.

**Dependency edges:** ι → {α,β,γ,δ,ε,ζ,η,θ}. All of α–θ have no intra-batch prereqs.

## Open questions (tactical — decide at implementation)

1. **`ScaleNonUniform` op shape (task δ).** A dedicated `GeometryOp::ScaleNonUniform { sx,sy,sz }`, or
   extend the existing `Scale` to carry an optional `[f64;3]`? *Suggested:* a dedicated variant, matching
   the one-variant-per-op convention and keeping the uniform fast-path untouched. Decide in δ.
2. **`arbitrary_pattern` IR shape (task ε).** Extend `ArbitraryPattern { transforms: Vec<[f64;3]> }` to
   carry per-instance `(quat, translation)`, or lower to a `Vec<ApplyTransform>` of per-instance ops?
   *Suggested:* widen the IR variant to `Vec<{rotation:[f64;4], translation:[f64;3]}>` (translation-only
   instances get the identity quat), preserving one pattern op. Decide in ε.
3. **`decode_*` helper home (tasks α/η).** `reify-eval` (next to the op-construction arms) vs `reify-stdlib`
   (next to `decompose_transform`). *Suggested:* `reify-eval`, since the geometry-op consumers live there;
   `reify-stdlib`'s value-algebra consumers (`project`) already have `decompose_transform`. Decide in α.
4. **`EulerConvention` enum file (task ζ).** Add to existing `geometry_traits.ri`, or a new
   `geometry_orientation.ri`? *Suggested:* whichever is already in the prelude self-compile set; confirm
   the new enum's NAME is seeded for value-position lowering (the 4108 mechanism). Decide in ζ.
