# AffineMap Type (non-rigid transforms)

Status: **deferred** (spec-gap-filling batch `spec-gap-2026-05-27`, cluster `affine-map-type`). Authored 2026-05-27. Implements spec §3.3.1 ("Non-rigid maps (scaling, shearing) are a separate type") and the deferred-features table §18 item 16 (`AffineMap` type for non-rigid transforms).

> Reify has a rigid `Transform<N>` (rotation + translation) but no type for the non-rigid
> maps the spec already promises: non-uniform scaling, shearing, and general affine maps.
> A `scale(solid, factor)` op exists, but only **uniform** scale (a single factor lowered
> to OCCT `gp_Trsf::SetScale`). Anisotropic scale ("make this part 1.5× taller without
> widening it"), shear, and reflections have no value-level representation and no kernel
> path. This PRD adds the `AffineMap` value type, its construction + algebra
> (compose / invert / determinant), its application to geometry, and the rigid-`Transform`
> ⊆ `AffineMap` widening relationship.

---

## §0 — Purpose and scope

**Purpose.** Introduce `AffineMap` as a first-class value type representing a general 3D
affine map `x ↦ A·x + b` where `A` is a dimensionless 3×3 linear part (may be non-orthogonal:
scale, shear, reflect) and `b` is a `Length` translation. Provide:

1. **The value type** — `Value::AffineMap { linear: [[f64;3];3], translation: [Length;3] }`
   and the `Type::AffineMap` type-system entry, plus its surface name.
2. **Construction** — constructor free-functions (`affine_scale`, `affine_shear_*`,
   `affine_map`, `affine_from_transform`, ...) — no new grammar.
3. **Algebra** — `affine_compose`, `affine_inverse`, `determinant` (free functions over
   `AffineMap`).
4. **Application to geometry** — apply an `AffineMap` to a realized solid, producing a
   genuinely deformed OCCT shape via the **non-rigid** kernel path (`gp_GTrsf` /
   `BRepBuilderAPI_GTransform`).
5. **Transform ⊆ AffineMap relationship** — a rigid `Transform` widens to an `AffineMap`
   (its linear part is the rotation's orthogonal matrix, det = +1); they compose.

**In scope:** N=3 only (matching how `Transform3`/`Frame3` are the only realized dimensions
today). The 2D case is out of scope (§10).

**Not in scope:** projective/perspective maps (4×4 homogeneous with a non-trivial bottom
row); NURBS-control-point free-form deformation; per-vertex morphing (that is mesh-morph
PRD territory). See §10.

---

## §1 — Spec grounding

`docs/reify-language-spec.md` already commits, normatively, to this type existing as a
*separate* type from `Transform`:

> **§3.3.1, line 368 (verbatim):** "`Transform` is always rigid (rotation + translation).
> Non-rigid maps (scaling, shearing) are a separate type."

> **§18 deferred-features table, item 16 (verbatim):** "`AffineMap` type for non-rigid
> transforms — Deferred — Scaling, shearing transforms."

This PRD implements an already-specified type, not a new semantic invention. The spec does
not specify the construction surface or the kernel application; this PRD designs those.

The §3.3.1 affine-space algebra (`Point - Point → Vector`, `Point + Vector → Point`) is the
governing model: an `AffineMap` is precisely a map that respects the point/vector
distinction — it acts on `Point3<Length>` to give `Point3<Length>`, and its **linear part**
acts on `Vector3<Length>` to give `Vector3<Length>` (translation drops out for vectors, as
affine maps require).

---

## §2 — Why deferred / activation status

This PRD is filed **deferred** in a spec-gap-filling batch. It has a named live consumer
today (§3) but is not on a critical path; the coordinator decides activation ordering. Its
one hard cross-PRD dependency is the kernel geometry-op application path owned by
`v0_6/sub-placement-and-surfacing.md` (the `ApplyTransform` rigid primitive); see §6 (G4).

**Pre-conditions for activating:**
- No grammar prerequisite — all construction syntax is constructor-function-based and
  parses today (G3 verified, §7).
- The kernel non-rigid application op (§5) is **new work owned by this PRD**; it does not
  pre-exist. It is the sibling/generalization of `sub-placement`'s rigid `ApplyTransform`
  (§6). This PRD may be decomposed and the type/algebra tasks (α–δ) executed independently
  of `sub-placement`; only the kernel-application task (ζ) carries the cross-PRD seam
  relationship.

---

## §3 — Consumer (G1)

Every mechanism introduced names a consumer. The mechanisms are: the `AffineMap` value
type, its constructors, its algebra, and its geometry application.

**Primary user-observable consumer — a stdlib `.ri` example + CLI eval.** A designer writes
a parametric part that is non-uniformly scaled:

```reify
structure def TaperedSpacer : Rigid {
  param height_scale : Real = 1.5
  aux let blank = box(20mm, 20mm, 10mm)
  // stretch only in Z — not expressible with uniform scale() today
  let body = affine_apply(self.blank, affine_scale(1.0, 1.0, self.height_scale))
}
```

`reify build tapered_spacer.ri` (or `reify eval`) produces a solid whose Z-extent is 1.5× the
input and whose X/Y-extents are unchanged — the observable signal is the bounding-box /
volume delta (§9 leaf signals). This example ships under `examples/` and runs in CI.

**Mechanism-level consumers:**

| Mechanism | Consumer |
|---|---|
| `Value::AffineMap` / `Type::AffineMap` | the constructor free-functions (§4), the geometry-apply op (§5), and the example `.ri` (above) |
| Constructors (`affine_scale`, `affine_shear_*`, `affine_map`, `affine_from_transform`) | the example `.ri`; `affine_apply`; CLI eval prints the resulting `AffineMap` value |
| `affine_compose` / `affine_inverse` / `determinant` | the example `.ri` (composed shear+scale) and a CLI-eval numeric check (`determinant` = volume-scale, asserted in the leaf signal) |
| `affine_apply` geometry op (+ kernel `gtransform_shape`) | the `.ri` example through `reify build`; the GUI viewport renders the deformed solid; STEP export emits it |
| Transform → AffineMap widening (`affine_from_transform`) | `affine_compose(affine_from_transform(rigid_t), shear)` in the example — proves Transform composes with AffineMap |

The geometry-apply op is an in-engine seam; per G1's engine-integration sub-check it plugs
into **op-execute (`engine-integration-norm.md` §3.1)** — the same seam through which all
`GeometryOp` variants (`Scale`, `Translate`, `Rotate`, `Mirror`) already execute. It is not
a new seam.

---

## §4 — Value type, construction, and algebra (contract)

### 4.1 Value representation

```rust
// reify-ir/src/value.rs (Value enum)
/// General 3D affine map x ↦ linear·x + translation.
/// `linear` is dimensionless (row-major 3×3); `translation` carries Length (meters).
AffineMap {
    linear: [[f64; 3]; 3],
    translation: [f64; 3],   // meters, like Transform's translation slot
}
```

```rust
// reify-core/src/ty.rs (Type enum)
/// General (non-rigid) affine map in N-dimensional space.
AffineMap(usize),   // N=3 only realized; mirrors Transform(usize)/Frame(usize)
```

Display: `AffineMap3`. `is_numeric` → false. `as_name` → None. The `Type::AffineMap(usize)`
shape deliberately mirrors `Type::Transform(usize)` / `Type::Frame(usize)` /
`Type::Orientation(usize)` (the existing `(usize)`-dimension geometric-type pattern) so the
parametric resolver and all match-arm consumers extend uniformly.

**Dimensional contract (G6).** The linear part is **dimensionless** — scale factors and
shear coefficients are pure ratios. The translation is **Length**. Applying an `AffineMap`
to a `Point3<Length>` yields `Point3<Length>` (dimension preserved: dimensionless·Length +
Length = Length); applying its linear part to a `Vector3<Length>` yields `Vector3<Length>`.
A non-uniform scale or shear mixes the three spatial axes, but since all three coordinates
share the same dimension (Length), the result is dimensionally homogeneous. This is *why* the
linear part must be dimensionless: a dimensioned linear part would make `linear·x` carry
`Length²` and break the affine-space algebra. Constructor functions reject dimensioned
linear-part arguments (scale factors are `Real`, not `Length`).

### 4.2 Constructors (free functions; no method-call syntax — GR-040)

| Function | Signature (surface) | Result |
|---|---|---|
| `affine_scale(sx, sy, sz)` | `(Real, Real, Real) -> AffineMap` | diag(sx,sy,sz), zero translation |
| `affine_shear_xy(k)` | `(Real) -> AffineMap` | shear X by k·Y (one off-diagonal); siblings `affine_shear_xz`, `affine_shear_yx`, `affine_shear_yz`, `affine_shear_zx`, `affine_shear_zy` |
| `affine_map(linear, translation)` | `(Matrix3x3<Real>, Vector3<Length>) -> AffineMap` | general construction from a 3×3 + translation |
| `affine_from_transform(t)` | `(Transform3) -> AffineMap` | widening: linear = rotation's orthogonal matrix, translation = t.translation (§4.4) |
| `affine_translate(dx, dy, dz)` | `(Length, Length, Length) -> AffineMap` | identity linear + translation (convenience; equals `affine_from_transform(transform3(orient_identity(), vec3(...)))`) |
| `affine_identity()` | `() -> AffineMap` | identity linear, zero translation |

Each constructor returns `Value::Undef` on arity/type mismatch (the established
`reify-stdlib/src/geometry.rs` builtin-validation convention — same as `transform3`).
`affine_scale` accepts negative factors (a negative factor is a reflection, det<0,
orientation-reversing); zero factors are rejected (degenerate, det=0, non-invertible) with
a diagnostic, mirroring the existing `scale` op's zero-factor rejection.

### 4.3 Algebra (free functions)

| Function | Signature | Semantics |
|---|---|---|
| `affine_compose(a, b)` | `(AffineMap, AffineMap) -> AffineMap` | composition `a ∘ b` — **apply b first, then a**: `(a∘b)(x) = a(b(x))`. Linear part = `a.linear · b.linear` (matrix product); translation = `a.linear · b.translation + a.translation`. |
| `affine_inverse(a)` | `(AffineMap) -> Option<AffineMap>` | inverse if `det(a.linear) ≠ 0`; `none` (not `Undef`) for singular maps so authors can branch. Linear part = `a.linear⁻¹`; translation = `-a.linear⁻¹ · a.translation`. |
| `determinant(a)` | `(AffineMap) -> Real` | `det(a.linear)` — the **signed volume-scale factor**. det>0 orientation-preserving; det<0 orientation-reversing (reflection); |det| = volume ratio. (Overloads the existing `determinant` if one exists on matrices; otherwise new.) |

**Composition order is left-applied (`a ∘ b` = "a after b")**, matching standard math
convention and OCCT's `gp_GTrsf::Multiply` (`A.Multiply(B)` ⇒ `A∘B`). This is the load-bearing
algebra decision; §9 task δ pins it with a numeric test (compose a known scale and shear,
apply to a point, assert the hand-computed result).

### 4.4 Transform ⊆ AffineMap (the relationship)

A rigid `Transform3 { rotation: Orientation, translation: Vector3<Length> }` is the special
case of an `AffineMap` whose linear part is the rotation quaternion's orthogonal 3×3 matrix
(det = +1, orthonormal columns). `affine_from_transform(t)` performs this widening
(quaternion → rotation matrix). The reverse narrowing (`AffineMap → Transform`) is **not**
provided: not every affine map is rigid, and a "best rigid approximation" (polar
decomposition) is a different, lossy operation deliberately excluded here (§10). Composition
across the boundary is via widening: `affine_compose(affine_from_transform(t), affine_map)`.

This means **both** rigid and non-rigid placement share one application path at the geometry
level (§5/§6): a `Transform` is applied either through `sub-placement`'s rigid
`ApplyTransform` (cheaper; `gp_Trsf`) or, once widened, through this PRD's non-rigid
`affine_apply` (`gp_GTrsf`) — both produce identical geometry for a rigid input. §5 specifies
when each is used.

---

## §5 — Kernel application (the load-bearing new capability)

**Premise validated (G6).** The current geometry transform path — `scale_shape`,
`rotate_shape`, `translate`, `mirror_shape`, and `sub-placement`'s planned rigid
`ApplyTransform` — all lower to OCCT `gp_Trsf` + `BRepBuilderAPI_Transform`
(`crates/reify-kernel-occt/cpp/occt_wrapper.cpp`). **`gp_Trsf` cannot represent non-uniform
scale or shear** — it is restricted to rigid motions plus a single uniform scale factor
(`SetScale`). Verified: `gp_GTrsf` / `BRepBuilderAPI_GTransform` (the general-transform path
that *does* support arbitrary 3×3 linear parts) is **not used anywhere** in the codebase
today. So a non-rigid affine application is genuinely new kernel work, not a re-wrap of an
existing op.

Contract:

1. **OCCT wrapper (new FFI).** Add `gtransform_shape(shape, m00..m22, tx, ty, tz)
   -> Result<UniquePtr<OcctShape>>` to `reify-kernel-occt` (`ffi.rs` + `occt_wrapper.cpp`),
   building a `gp_GTrsf`, calling `SetValues(...)` with the 3×4 affine matrix
   (3×3 linear + translation column), and running `BRepBuilderAPI_GTransform(shape, gtrsf,
   /*copy=*/true)`. Returns a new shape; the source shape is untouched.
2. **Compiled op.** Add `GeometryOp::AffineApply { target, linear: [[f64;3];3],
   translation: [f64;3] }` (`reify-ir/src/geometry.rs`) and the corresponding
   `CompiledGeometryOp` arm + `compile_geometry_op` lowering for the `affine_apply` free
   function (`reify-eval/src/geometry_ops.rs`), routed through the **op-execute** seam exactly
   like `Scale`/`Mirror`. The op carries a fully-evaluated `AffineMap` (the linear matrix +
   translation), distinguishing it from the source-level-scalar `Transform { kind, args }`
   op family.
3. **Degenerate-input guard.** Before dispatch, reject `det(linear) == 0` with a diagnostic
   ("affine_apply dropped: linear part is singular (det=0), produces degenerate geometry") —
   the same defensive posture as the existing zero-scale rejection. `BRepBuilderAPI_GTransform`
   on a singular `gp_GTrsf` would otherwise emit a non-manifold/zero-volume shape.
4. **Exactness (G6).** Application is exact at the BRep level (OCCT `gp_GTrsf` is exact
   linear algebra on control points / surfaces for planar+quadric geometry); tessellating the
   transformed handle yields vertices equal to the source vertices mapped by `x ↦ linear·x +
   translation` within tessellation tolerance. The leaf test asserts AABB-corner mapping.
   **Caveat (documented, not a blocker):** non-uniform `gp_GTrsf` turns a cylinder's circular
   cross-section into an ellipse — OCCT represents the result as a B-spline/elliptical surface,
   which is correct but heavier; this is expected, not an error.

**Relationship to uniform `scale`.** The existing `scale(solid, factor)` op
(`TransformKind::Scale` → `gp_Trsf::SetScale`) stays as-is for the uniform case (cheaper
`gp_Trsf` path). `affine_scale(s,s,s)` (isotropic) is mathematically identical to
`scale(_, s)` but routes through `gp_GTrsf`; authors get the same geometry either way. We
do **not** redirect `scale` through the new path — keeping the cheap path for the common
uniform case is the deliberate choice (§11 tactical: a future optimization could detect
rigid+uniform `AffineMap`s and downgrade to `gp_Trsf`).

---

## §6 — Cross-PRD relationship (G4 seam ownership)

The one load-bearing seam is **geometry-level application of a transform**. `sub-placement`
owns the **rigid** application primitive (`ApplyTransform` / `apply_transform_to_handle`,
`gp_Trsf`); this PRD owns the **non-rigid** generalization (`AffineApply` /
`gtransform_shape`, `gp_GTrsf`). They are **siblings on the same op-execute seam**, not a
contested ownership.

**Seam resolution decision: separate ops, shared seam, no reuse of `ApplyTransform`.**
Rationale: `ApplyTransform` carries a `Transform` value and lowers to `gp_Trsf`;
`AffineApply` carries an `AffineMap` (full 3×3) and lowers to `gp_GTrsf`. The OCCT primitives
are different C++ types (`gp_Trsf` vs `gp_GTrsf`), so a single op cannot serve both without a
runtime branch that buys nothing — `gp_Trsf` is strictly a subset of `gp_GTrsf`'s
expressiveness but is cheaper for the rigid case `sub-placement` targets. Keeping two ops
keeps each path honest: rigid placement never pays for the general path; non-rigid maps never
silently lose precision. The Transform→AffineMap widening (§4.4) is the bridge — a rigid
`Transform` *can* be applied via either op and yields identical geometry.

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/sub-placement-and-surfacing.md` | sibling (this PRD generalizes its rigid op) | op-execute seam: `GeometryOp::ApplyTransform` (rigid, `gp_Trsf`) vs `GeometryOp::AffineApply` (non-rigid, `gp_GTrsf`) | **sub-placement** owns `ApplyTransform`; **this PRD** owns `AffineApply` | declared (not wired); coordinator wires the cross-PRD edge |
| `v0_3/mesh-morphing.md` | unrelated | per-vertex morph is a different mechanism; `AffineMap` is a global linear map, not free-form deformation | n/a | no seam — disambiguated in §10 |

**Exact dependency named for the coordinator:** This PRD's kernel-application task (§9
task ζ) is the **non-rigid sibling** of `sub-placement` task **3903** (the
`CompiledGeometryOp::ApplyTransform` + OCCT `apply_transform_to_handle` task, §5/T3 of that
PRD). The relationship is **shared-seam coordination, not a hard dependency**: ζ can be
implemented before, after, or concurrently with 3903 because the OCCT functions
(`gtransform_shape` vs `apply_transform_to_handle`) and the op variants (`AffineApply` vs
`ApplyTransform`) are disjoint. The coordinator should add a **soft ordering edge** (ζ
*after* 3903) only to avoid two agents simultaneously editing `ffi.rs` / `occt_wrapper.cpp` /
`geometry_ops.rs` and colliding under the narrow-file-lock model — **not** because ζ needs
3903's output. If the coordinator prefers concurrency, no correctness edge is required.

---

## §7 — Grammar gate (G3)

**Result: PASS — no grammar work needed.** All `AffineMap` construction, algebra, and
application is expressed through constructor / free-function calls with colon-form named args
where applicable, matching the existing `transform3(...)` / `frame3(...)` / `scale(...)`
idiom. No method-call syntax (GR-040 preserved), no new operators, no new declaration forms.

Fixtures parse-tested with `tree-sitter parse --quiet` from `tree-sitter-reify/` (exit 0, no
ERROR nodes), `2026-05-27`:

- `/tmp/prd-gate-fixtures/affine-1.ri` — `affine_scale(2.0, 1.0, 0.5)`, `affine_shear_xy(0.3)`,
  `affine_compose(m, s)`, `affine_from_transform(transform3_identity())`,
  `affine_apply(box(10mm,10mm,10mm), m)` → **exit 0**.
- `/tmp/prd-gate-fixtures/affine-2.ri` — `affine_map(matrix3x3(...), vec3(...))`,
  `determinant(m)`, `affine_inverse(m)` → **exit 0**.

Every decomposition task therefore carries `grammar_confirmed=true`. (The `matrix3x3(...)`
constructor in fixture 2 is illustrative; the concrete general-construction surface for
`affine_map` is a §11 tactical question — the *grammar* parses regardless of which matrix
constructor backs it.)

---

## §8 — Approach choice (G5) and boundary-test sketch

**G5 heuristic evaluation:** Cross-crate blast radius = 4 (`reify-core`/`reify-ir` for the
type+value, `reify-stdlib` for constructors+algebra, `reify-eval` for op lowering,
`reify-kernel-occt` for the FFI). Mechanism count ≈ 6 (type, constructors, algebra, kernel
FFI, op lowering, widening). Touches a load-bearing seam (geometry op-execute + a brand-new
OCCT FFI). One cross-PRD sibling. **≥3 crates and a new kernel FFI ⇒ B + H.** This PRD uses
**B (vertical slice) + H (contract + boundary-test sketch)** — §4/§5 are the contract;
below is the boundary-test sketch.

### 8.1 Boundary tests (facing both ways)

Two seams: the **value/algebra** seam (reify-stdlib ↔ reify-ir/reify-core) and the
**eval ↔ kernel** seam (reify-eval ↔ reify-kernel-occt).

**Algebra seam (value semantics):**

| Scenario | Preconditions | Postconditions |
|---|---|---|
| Scale construction | `affine_scale(2,3,4)` | linear = diag(2,3,4); translation = 0; `determinant` = 24 |
| Shear construction | `affine_shear_xy(0.5)` | linear has 0.5 in the (X,Y) off-diagonal, 1 on diagonal; `determinant` = 1 (shear preserves volume) |
| Composition order | `affine_compose(affine_scale(2,1,1), affine_shear_xy(1))` applied to point (1,1,0) | equals scale-after-shear: shear→(2,1,0), then scale-X→(4,1,0). Hand-computed; pins left-applied convention. |
| Inverse round-trip | `affine_inverse(a)` for invertible `a`, then `affine_compose(a, a⁻¹)` | ≈ identity (linear ≈ I, translation ≈ 0) within 1e-12 |
| Singular inverse | `affine_inverse(affine_scale(1,1,0))`-equivalent singular map | returns `none` (not Undef); det = 0 |
| Transform widening | `affine_from_transform(transform3(rot90_z, vec3(5mm,0,0)))` | linear = the 90°-Z rotation matrix (orthonormal, det=+1); translation = (0.005,0,0) m |

**eval ↔ kernel seam (`AffineApply`):**

| Scenario | Side | Preconditions | Postconditions |
|---|---|---|---|
| Anisotropic scale on a box | producer (reify-eval emits op) | `affine_apply(box(10mm,10mm,10mm), affine_scale(1,1,2))` | tessellated AABB Z-extent = 20mm, X/Y = 10mm; volume = 2× source |
| Round-trip | consumer (occt looks inward) | apply `A` then `affine_inverse(A)` to a box | recovered AABB ≈ source within tolerance |
| Identity no-op | consumer | `affine_apply(box, affine_identity())` | AABB unchanged |
| Singular guard | consumer | `affine_apply(box, <det-0 map>)` | op dropped, diagnostic emitted, source-equivalent or Failed (no crash, no non-manifold output) |
| Reflection (negative scale) | consumer | `affine_apply(box, affine_scale(-1,1,1))` | AABB mirrored across X; det < 0; shape still valid solid |
| Rigid parity | cross-seam | `affine_apply(box, affine_from_transform(t))` vs `sub-placement` `ApplyTransform(box, t)` for the same rigid `t` | identical AABB within tolerance (the widening produces the same geometry the rigid path does) |

The **rigid-parity** test is the cross-PRD boundary check facing `sub-placement` — it closes
the G4 seam by proving the two ops agree on their shared (rigid) input domain.

---

## §9 — Decomposition plan (the DAG)

Vertical-slice spine (C-as-integration-gate): **α → β → γ → ζ → η** (type → constructors →
algebra → kernel-apply → integration example). Greek labels; task IDs assigned at decompose.

### Phase 1 — Value type + type-system entry
- **Task α — `Value::AffineMap` + `Type::AffineMap(usize)`.**
  - *Crates:* reify-ir (value.rs), reify-core (ty.rs).
  - *Signal (leaf-ish; foundation roped to η):* Rust unit tests pin construction, equality,
    hash, `Display` → `AffineMap3`, `is_numeric`=false, `as_name`=None, and the value's
    discriminant/serialization slot — mirroring the existing `Type::Transform` test block in
    `ty.rs`. `grammar_confirmed=true`.
  - *Prereqs:* none.

### Phase 2 — Constructors + algebra (vertical slice toward a CLI-eval signal)
- **Task β — Constructor free-functions.** `affine_scale`, `affine_shear_{xy,xz,yx,yz,zx,zy}`,
  `affine_translate`, `affine_identity`, `affine_map`, `affine_from_transform`.
  - *Crates:* reify-stdlib (geometry.rs), reify-compiler (builtin registration / units.rs /
    type inference).
  - *Signal:* `reify eval` of a `.ri` `let m = affine_scale(2.0, 1.0, 0.5)` prints an
    `AffineMap` value with the expected linear matrix; `affine_from_transform(transform3_identity())`
    yields the identity AffineMap. Builtin-validation rejects dimensioned scale factors and
    zero factors (CLI diagnostic).
  - *Prereqs:* α.
- **Task γ — Algebra free-functions.** `affine_compose`, `affine_inverse`, `determinant`.
  - *Crates:* reify-stdlib (linalg.rs / geometry.rs), reify-compiler (registration).
  - *Signal:* `reify eval` of a `.ri` that composes a scale and a shear and prints
    `determinant(composed)` = the hand-computed signed volume factor; `affine_inverse` of a
    singular map evaluates to `none`; inverse round-trip ≈ identity (the §8.1 algebra-seam
    table, as CLI-eval assertions).
  - *Prereqs:* β.
- **Task δ — Composition-order + dimensional-contract pin.**
  - *Crates:* reify-stdlib (tests), reify-compiler (tests).
  - *Signal:* the §8.1 "Composition order" and "Transform widening" rows as exact numeric
    tests (left-applied `a∘b`; applying composed map to a known point gives the
    hand-computed result), plus the inverse round-trip ≈ identity within 1e-12. This is the
    G6 premise-pinning task. *(Intermediate — unlocks the integration example η by
    guaranteeing the algebra convention; named downstream consumer: η.)*
  - *Prereqs:* γ.
  - *Scope note (esc-3962-294, 2026-06-02):* the dimensional test "`affine_apply` to a
    `Point3<Length>` preserves Length" is RE-HOMED to task ζ — `affine_apply` is owned and
    implemented by ζ, which depends on δ, so δ (upstream) cannot exercise it. δ's remaining
    assertions all GREEN on the γ surface.

### Phase 3 — Kernel application
- **Task ε — OCCT `gtransform_shape` FFI (`gp_GTrsf` / `BRepBuilderAPI_GTransform`).**
  - *Crates:* reify-kernel-occt (ffi.rs, cpp/occt_wrapper.cpp).
  - *Signal:* a Rust kernel integration test builds a box `OcctShape`, applies a known
    non-uniform `gp_GTrsf` (e.g. diag(1,1,2) + translation), tessellates, and asserts the AABB
    is stretched/translated as expected (vertex-exact within tolerance); identity is a no-op;
    a singular `gp_GTrsf` returns an error (no crash). *(Intermediate — kernel capability;
    downstream consumer: ζ.)*
  - *Prereqs:* none (kernel-only; parallel with Phase 1/2). **Soft ordering with
    `sub-placement` 3903** to avoid `ffi.rs`/`occt_wrapper.cpp` edit collision (§6) — not a
    correctness edge.
- **Task ζ — `GeometryOp::AffineApply` + `affine_apply` op lowering through op-execute.**
  - *Crates:* reify-ir (geometry.rs), reify-eval (geometry_ops.rs), reify-compiler
    (geometry.rs / geometry_transform.rs for the `affine_apply` free-fn lowering).
  - *Signal:* `reify build` (or eval+tessellate) of `affine_apply(box(10mm,10mm,10mm),
    affine_scale(1,1,2))` produces a solid whose tessellated AABB has Z=20mm, X=Y=10mm; the
    singular-map guard drops the op with a diagnostic; reflection (negative scale) produces a
    valid mirrored solid. *(Leaf for the kernel path; observable via mesh AABB.)*
  - *Dimensional contract (re-homed from δ, esc-3962-294):* also assert the §4.1 dimensional
    contract holds for `affine_apply` — applying it to a `Point3<Length>` yields a
    `Point3<Length>` (dimensionless linear part · Length + Length translation = Length).
    `affine_apply` is implemented here, so this is the task that can actually exercise it.
    (NB §4.1/§3: `affine_apply` acts on geometry/`Point3<Length>`; the geometry-op AABB
    signal above and this point-level dimensional assertion are distinct facets of the same op.)
  - *Prereqs:* α, ε, δ. **Soft ordering with `sub-placement` 3903** (shared seam, §6).

### Phase 4 — Integration example + spec update (integration gate)
- **Task η — Stdlib `.ri` example + end-to-end CLI signal (the integration gate).**
  - *Crates:* examples/ (+ CI wiring), tests.
  - *Signal:* `examples/affine_tapered_spacer.ri` (the §3 example, extended with a composed
    shear+scale and an `affine_from_transform`-widened rigid compose) runs under
    `reify build`/CI: the output solid's bounding box / volume matches the analytic
    affine-mapped values; `determinant` of the composed map printed by `reify eval` equals
    the expected volume-scale factor; the GUI viewport (reify-debug `viewport_state` /
    screenshot) renders the deformed solid. This is the C-as-integration-gate leaf that ties
    α–ζ together. *(Leaf — the user-observable end-to-end signal.)*
  - *Prereqs:* ζ.
- **Task θ — Spec update (companion correction).**
  - *Crates:* docs.
  - *Signal:* `docs/reify-language-spec.md` §3.3.1 marks the AffineMap type realized (links
    this PRD); §18 item 16 status flips Deferred → implemented; the spec's example `.ri`
    parses. No code. *(Leaf — doc lint passes.)*
  - *Prereqs:* η.

### Dependency view
```
α ─→ β ─→ γ ─→ δ ─┐
                  ├─→ ζ ─→ η ─→ θ
ε ────────────────┘
        (ε, ζ: soft ordering after sub-placement 3903 — collision-avoidance, not correctness)
```

---

## §10 — Out of scope

- **2D affine maps** (`AffineMap2`). Only N=3 is realized, matching `Transform3`/`Frame3`.
  The `Type::AffineMap(usize)` shape leaves the door open; no 2D constructors here.
- **Projective / perspective maps** (4×4 homogeneous with a non-affine bottom row). A
  separate type if ever needed; affine maps keep parallel lines parallel.
- **AffineMap → Transform narrowing** (polar decomposition / "best rigid fit"). Lossy,
  different operation; excluded (§4.4).
- **Per-vertex / free-form deformation** (NURBS control-point morphing, lattice/FFD). That is
  `v0_3/mesh-morphing.md` territory — a *per-element* deformation field, categorically
  different from a single global linear map. `affine_apply` is one matrix for the whole shape.
- **Routing the existing uniform `scale` op through `gp_GTrsf`.** Kept on the cheap
  `gp_Trsf::SetScale` path; an `AffineMap`-detects-rigid-uniform downgrade optimization is a
  §11 tactical follow-up, not a deliverable.
- **AffineMap as a `sub ... at` placement pose.** `sub-placement` §10 explicitly scopes `at`
  to rigid `Transform` only. Extending `at` to accept an `AffineMap` (non-rigid placement) is
  future work that would build on *both* PRDs; not in scope here.

---

## §11 — Open (tactical) questions

1. **General-construction surface for `affine_map`.** The 3×3-from-9-scalars vs.
   `Matrix3x3<Real>`-value form. A `matrix3x3(...)` constructor may or may not exist;
   confirm the concrete matrix-construction free-function during task β and pick the form
   that matches existing matrix-literal handling. *Suggested:* accept a `Matrix3x3<Real>`
   value (reuse `Value::Matrix`) for generality; add a 9-scalar convenience overload only if
   ergonomics demand. Decide in β.
2. **`determinant` overload vs. new name.** If a `determinant`/`det` already exists on
   `Matrix`/`Tensor`, reuse it (add an `AffineMap` arm); otherwise introduce `determinant`.
   Confirm during γ; if a name clash forces it, use `affine_determinant`.
3. **Rigid-uniform fast-path downgrade.** Whether `affine_apply` should detect a rigid or
   uniform-scale `AffineMap` and route through the cheaper `gp_Trsf` path (matching
   `sub-placement`'s `ApplyTransform`). *Suggested:* not in v1 — correctness first; profile in
   η, file a follow-up if `gp_GTrsf` overhead on rigid inputs is material.
4. **Cylinder-under-anisotropic-scale surface fidelity.** `gp_GTrsf` turns a circular section
   into an ellipse represented as a B-spline; confirm the tessellation tolerance and STEP
   export handle the resulting heavier surfaces acceptably. Profile in ε; not expected to
   block.
5. **Negative-determinant (reflection) downstream effects.** A det<0 `affine_apply` produces
   an orientation-reversed solid; confirm boolean ops / FEA meshing / mass-properties treat
   it correctly (winding/normals). Verify in ζ; OCCT generally handles this, but pin a test.

---

*Decompose note:* under decompose-mode, each task files with `planning_mode=True`, carries
`user_observable_signal` / `consumer_ref` / `grammar_confirmed` (all true) metadata, wires the
§9 dependency edges (and the §6 soft cross-PRD ordering edge to `sub-placement` 3903 if the
coordinator opts for it), and the batch flips `deferred → pending` together. The orchestrator
does not yet read those metadata fields (F-infra follow-up substrate).
