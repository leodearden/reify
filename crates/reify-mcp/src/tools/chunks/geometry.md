# Geometry Types

## Algebraic Types

**Point/Vector distinction (affine space):**
- `Point - Point → Vector` (valid)
- `Point + Vector → Point` (valid)
- `Vector + Vector → Vector` (valid)
- `Point + Point` → type error

Parameterized by dimensionality and quantity:
```
Point<N: Nat, Q: Dimension>     // Position
Vector<N: Nat, Q: Dimension>    // Displacement
Scalar<Q: Dimension>            // Dimensioned number
Tensor<Rank: Nat, N: Nat, Q: Dimension>
Matrix<M: Nat, N: Nat, Q: Dimension>
```

Common aliases: `Point3<Q>`, `Vector3<Q>`, `Point2<Q>`, `Vector2<Q>`

## Opaque Geometry Types

Core geometric entity types are opaque handles — work through operations.

| Type | Description |
|------|-------------|
| `Solid` | Closed region of 3D space |
| `Shell` | Connected set of faces |
| `Surface` | 2D manifold in 3D space |
| `Curve` | 1D manifold in 2D/3D |
| `PointCloud` | Unordered point collection |

Geometric traits: `Closed`, `Manifold`, `Orientable`, `Convex`, `Connected`, `Bounded`, `Watertight`

## Orientation & Transform

```
Orientation.from_quaternion(w, x, y, z)
Orientation.from_axis_angle(axis, angle)
Orientation.from_euler(convention, a, b, c)

Frame<N>:
    origin : Point<N, Length>
    basis  : Orientation<N>

Transform<N>:
    rotation    : Orientation<N>
    translation : Vector<N, Length>
```

Transform is always rigid (rotation + translation). Sub-structure placement uses Transform from child frame to parent frame.

## Geometry Constructors (Prelude)

```
point2(x, y)          point3(x, y, z)
vec2(x, y)            vec3(x, y, z)
line_segment(x1, y1, z1, x2, y2, z2)
arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)
polygon(x1, y1, x2, y2, x3, y3, ...)   rectangle(width, height)
```

## Solid Primitives

```
box(width, depth, height)                          -> Solid
box_centered(width, depth, height)                  -> Solid   // alias of box — see below
cylinder(radius, height)                             -> Solid
cylinder_centered(radius, height)                    -> Solid
cone(bottom_radius, top_radius, height)              -> Solid
sphere(radius)                                       -> Solid
torus(major_radius, minor_radius)                    -> Solid
wedge(width, depth, height, top_width)               -> Solid
tube(outer_radius, inner_radius, height)             -> Solid   // outer cylinder minus inner cylinder
rounded_box(width, depth, height, corner_r)          -> Solid   // box with the 4 vertical edges rounded
```

`rounded_box` requires `corner_r > 0` and `2*corner_r < min(width, depth)`; violations are a compile-time error when the args are constant literals (including constant arithmetic like `10mm + 15mm`). A param-driven `corner_r` that violates the constraint at runtime is **not** caught statically — it fails at evaluation with an opaque kernel error instead of a diagnostic.

**2D profiles** (planar faces in the XY plane at z=0). `rectangle`/`circle`/`ellipse` are centred
at origin (same centring as `box`); `polygon` is the exception — it is positioned by its explicit
vertex coordinates, not auto-centred (see the Anchoring & orientation table below):

```
rectangle(width, height)   circle(radius)
polygon(x1, y1, x2, y2, ...)   ellipse(semi_major, semi_minor)
rounded_rect(width, depth, corner_r)   -> Surface   // rectangle with the 4 corners rounded
```

Note: `circle(radius)` is the only `circle` constructor — an origin-centred 2D profile consumed by
`extrude`/`revolve`/etc. There is no separate center-placed form; `translate` the resulting profile
to move it off-origin.

`rounded_rect` shares `rounded_box`'s constraint (`corner_r > 0` and `2*corner_r < min(width, depth)`) and the same compile-time-only, constant-args-only enforcement caveat above.

### Anchoring & orientation

Three distinct anchor conventions coexist across the solid primitives — they are deliberately
**not** unified (redefining `box`'s corner-at-origin would break ~370 existing call sites and
their world positions; see `docs/prds/geometry-primitive-constructors.md`). Know which family a
primitive belongs to before composing a `translate`. This table is mirrored — with full type
signatures — in `docs/reify-stdlib-reference.md` §3.2-3.3; keep both in sync when a primitive's
anchor convention changes (e.g. a future `wedge_centered` variant):

| Primitive | Anchor | Notes |
|---|---|---|
| `box` | **centred at origin**, all 3 axes | corner at `(-w/2, -h/2, -d/2)` internally; already centroid-centred |
| `box_centered` | **centred at origin**, all 3 axes | op-identical alias of `box` — exists for symmetry with `cylinder_centered` so a designer doesn't have to remember box is the odd one out |
| `sphere` | **centred at origin** | radius extends equally in all directions from `(0,0,0)` |
| `torus` | **centred at origin**; axis is **+Z** | major/minor radii both measured from the ring centred on the origin |
| `cylinder` | **base at z=0**, axis **+Z**, x/y **centred at origin** | top face at `z = height`; NOT centred on z — a common hand-centering workaround is `translate(cylinder(r, h), 0mm, 0mm, -h/2)` — note the dimensioned zeros; `translate`'s components are length-semantic and a bare `0` is rejected |
| `cylinder_centered` | **z-centred at origin**, axis **+Z**, x/y centred | equivalent to `cylinder` + `translate(z=-height/2)`, composed for you — prefer this over the hand-rolled workaround above |
| `cone` | **base at z=0**, axis **+Z**, x/y centred at origin | same base-anchor convention as `cylinder`; base radius at z=0, top radius at z=height |
| `tube` | **base at z=0**, axis **+Z**, x/y centred at origin | composed from `outer cylinder − inner cylinder`, so it inherits `cylinder`'s base-at-z0 anchor |
| `wedge` | **min-corner at origin**, occupying the **+X/+Y/+Z octant** | the one primitive anchored at a corner rather than centred or base-centred; no `wedge_centered` variant exists yet |
| `rounded_box` | **centred at origin**, all 3 axes | same anchor as `box`; the 4 vertical (plan-view) edges are rounded to `corner_r` |
| 2D profiles (`rectangle`, `circle`, `ellipse`) | planar in the **XY plane at z=0**, **centred at origin** | consumed by `extrude`/`revolve`/`sweep`/`loft` |
| `rounded_rect` (2D profile) | planar in the **XY plane at z=0**, **centred at origin** | same anchor as `rectangle`; all 4 corners rounded to `corner_r` |
| `polygon` (2D profile) | planar in the **XY plane at z=0**; position set by its **explicit vertices** — not auto-centred | same consumers as above; a caller-supplied vertex set can sit off-origin, unlike the other 2D profiles |
| `extrude(profile, distance)` | extrudes along the profile plane's normal, starting at the profile's own z=0 plane | inherits the profile's XY centring |
| `revolve(profile, ox, oy, oz, ax, ay, az, angle)` | sweeps the profile about a caller-supplied origin + axis direction (6 scalars) | anchor is whatever the profile + axis define — no implicit centring |

**Rule of thumb:** `box`-family and `sphere`/`torus` are centred; `cylinder`-family (`cylinder`,
`cone`, `tube`) sits base-first on the origin along +Z; `wedge` sits corner-first in the +octant.
When in doubt, prefer the `_centered` variant over a manual
`translate(primitive(...), 0mm, 0mm, -h/2)` workaround.

**Units:** every `translate` component is length-semantic, so all three must be dimensioned —
`0mm`, not `0`. A bare number would be read as SI **metres** (1000× a plausible mm value), so it is
rejected outright with a diagnostic. `-h/2` is fine as-is: dividing a length by a bare number
preserves the length. The same rule applies to `rotate_around`'s pivot, `revolve`'s axis origin,
`line_segment` / `arc` / `helix` coordinates and radii, the `interp` / `bezier` coordinate triples
(every argument), `nurbs`' control-point coordinates, and `polygon`'s vertex coordinates — every
argument, at every arity, since a polygon vertex is a point in the XY plane:
`polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)`, never `polygon(0, 0, 10, 0, 5, 10)`. `polygon` has NO
dimensionless position at all, which is what distinguishes it from `nurbs` below and from
`arc` / `rotate_around`, whose direction components and angles stay bare. Direction vectors, counts
and angles stay dimensionless — and so does `nurbs`' dimensionless tail: its `degree` and `n_points`
are counts, its weights are rational blending factors and its knots are parameter-space values, so
those must NOT be dimensioned. Only the control-point coordinates in the middle are lengths:
`nurbs(1, 2, 0mm, 0mm, 0mm, 10mm, 0mm, 0mm, 1, 1, 0, 0, 1, 1)`.

## Interference & Clearance Queries

<!-- SYNC: the five query NAMES here, and the compile-acceptance of the ```reify fences below, are
     verified by crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs. That guard
     covers names + parse/compile only. The RUNTIME claims in "Clearance-query traps" are pinned
     (where they are pinned at all) by the eval/CLI tests mapped in the SYNC block at that
     subsection — read it before relying on a trap, and before changing one of those behaviours. -->

Reify **does** have a static interference/clearance oracle. Never hand-roll a bounding-box overlap
test or hand-compute a gap from parameters — ask the kernel. Two forms; prefer FORM B unless you
need poses or multiple bodies.

**FORM B — raw geometry, lowest ceremony.** Over let-bound geometry:
`intersects(a, b) -> Bool` (2-arg: do they overlap?) and `distance(a, b) -> Length` (2-arg: true
minimum surface gap). Canonical idiom file, with the full arg-shape contract spelled out:
`examples/best_practices/clearance_oracle.ri`.

```reify
structure def ClearanceGate {
    // Let-bind every operand FIRST — this is the arg-shape contract, not style.
    let housing = cylinder_centered(10mm, 40mm)
    let bracket = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    // Two questions the kernel can answer exactly.
    let fouls = intersects(housing, bracket)
    let gap = distance(housing, bracket)

    // Pair the boolean with the scalar: `gap` alone would pass on a fully
    // nested part, which reports a positive distance (trap 5).
    constraint not fouls
    constraint gap > 1mm
}
```

**FORM A — posed, multi-body, or swept.** Build a mechanism, then query a snapshot:
`mechanism()` → `body(m, "<let name>", fixed())` → `body_id_of(m, "<let name>")` → `snapshot(m, [])`,
then

- `min_clearance(s, id_a, id_b) -> Length` — 3-arg.
- `interferes_with(s, id_a, id_b) -> Bool` — 3-arg.
- `interferes(s) -> List<Map>` — 1-arg; every interfering pair. Each Map has exactly the keys `"a"`
  and `"b"` holding `Int` body ids, enumerated upper-triangular (`i < j`: no self-pairs, no
  duplicate orderings).

```reify
structure def PosedClearance {
    // Bake placement into local lets — a sub's `at` pose is NOT carried into a
    // snapshot (trap 4), and `body` looks its string up by bare let name.
    let part_a = cylinder_centered(10mm, 40mm)
    let part_b = translate(box(20mm, 20mm, 20mm), 30mm, 0mm, 0mm)

    let m0 = mechanism()
    let m1 = body(m0, "part_a", fixed())
    let m2 = body(m1, "part_b", fixed())

    let id_a = body_id_of(m2, "part_a")
    let id_b = body_id_of(m2, "part_b")
    let s = snapshot(m2, [])

    // MUST be let-bound. Writing `constraint min_clearance(s, id_a, id_b) > 2mm`
    // inline bypasses kinematic dispatch (a post-process over value cells only)
    // and yields INDETERMINATE "operator undefined for these operand kinds: Map".
    let clr = min_clearance(s, id_a, id_b)
    constraint clr > 2mm
}
```

Worked references: `examples/tolerancing/vc_bolt_pattern_clearance.ri` (the one example where a
clearance query flips a `reify build` verdict end-to-end) and `examples/kinematic/dock_pickup.ri`.
The swept form `flat_map(snaps, |s| [min_clearance(s, a, b)])` is supported (see `dock_pickup.ri`);
a swept unary `interferes` is not.

### Clearance-query traps

<!--
SYNC: which trap below is pinned by an executable test, and where. The chunk guard
(geometry_chunk_smoke.rs) establishes only name existence + fence compile-acceptance, so
these runtime claims would otherwise rot silently. Named here so a behaviour change lands in
a file whose grep leads back to this doc — and so the UNPINNED ones are visibly unpinned
rather than looking equally guarded.

  trap 1 (build/eval catches it)  crates/reify-cli/tests/harness_cli/cli_vc_clearance.rs
                                  :: build_vc_bolt_pattern_clearance_satisfied
                                  :: build_vc_bolt_pattern_interference_violated
  trap 1 (check exits 0)          UNPINNED — prose only.
  trap 2 (inline arg -> undef)    crates/reify-eval/tests/harness_kernel_realization/
                                  kernel_queries_intersects_smoke.rs, which asserts the
                                  IntersectsSmoke.undef_inline cell is None or Undef.
  trap 3 (FORM A trio on plain    UNPINNED — prose only. Verified by hand while probing;
          geometry -> undef)      re-verify before trusting it.
  trap 4 (sub `at` pose not       UNPINNED — prose only. Substrate fix is owned by the
          carried into snapshot)  constraint-driven-placement PRD track.
  trap 5 (clamp to 0 on overlap;  crates/reify-eval/tests/harness_mechanism/
          positive when disjoint; mechanism_interference_smoke.rs
          self-pair -> undef)     :: overlapping_cubes_one_pair_and_zero_clearance
                                  :: disjoint_cubes_no_pairs_and_positive_clearance
                                  :: single_body_self_pair_excluded
  trap 5 (fully NESTED solid      UNPINNED — prose only. This is the rider that defeats the
          reports POSITIVE dist)  obvious "overlap => 0" reading, so treat it as the
                                  highest-value gap here.
  trap 6 (`d <= 0.0`)             kernel_queries_intersects_smoke.rs
                                  :: intersects_smoke_evals_expected_booleans. Source of
                                  truth is reify-eval/src/geometry_ops.rs's Bool(d <= 0.0).
                                  The face-TOUCHING edge case specifically is UNPINNED; the
                                  test covers overlapping and well-apart.

FORM A's posed and swept forms are pinned by mechanism_interference_smoke.rs
:: fk_posed_cubes_no_interference_and_correct_clearance and
:: swept_min_clearance_monotonic_to_interference.
-->

Every one of these is a **silent wrong answer**, not an error — read them before writing a clearance
gate.

1. **Eval/build only.** `reify check` reports these constraints `INDETERMINATE` and still **exits
   0** ("No constraints violated (1 indeterminate)"); only `--strict` flips that. A clearance gate
   must run under `reify build` or `reify eval`. Never "fix" an indeterminate clearance constraint by
   deleting it. Note the asymmetry: `intersects`/`distance` at least emit an explanatory
   "geometry-consumer builtins require a realized geometry kernel" diagnostic, but the FORM A trio is
   not on that allow-list and goes Indeterminate with **no explanatory diagnostic at all**.
2. **Let-bind twice.** Both the query CALL *and* its geometry/snapshot ARGUMENTS must be let-bound.
   Dispatch is a post-process over value cells only, so an inline call inside a `constraint` is never
   visited → `INDETERMINATE … operator undefined for these operand kinds: Map`. An inline geometry
   *argument* (`intersects(box(10mm, 10mm, 10mm), bracket)`) yields a silent `undef` with no
   diagnostic at all — live demo cell `undef_inline` in `examples/kernel_queries/intersects_smoke.ri`.
3. **The FORM A trio takes `(Snapshot, Int, Int)` only.** Handed plain geometry,
   `min_clearance(a, b)` silently yields `undef`; the 2-arg Structure/Geometry overload is an
   unimplemented v0.6 PRD item, not a supported form. Use FORM B for raw geometry.
4. **Sub `at` placement is not carried into a snapshot.** `body(m, "name", …)` looks its string up
   flat in the build's named-step map, which is keyed by BARE name for local lets and by compound
   `"<sub>.<member>"` for sub instances — so a bare sub name does not resolve at all. And a sub's `at`
   pose is applied only in the export/tessellate walk, never written back to that map, so any handle
   reached this way is **unposed**: it sits at the child's local origin. Bake placement into local
   lets (`translate(...)`) rather than relying on sub `at`. Nothing validates the body string at
   compile time. (Substrate fix is owned by the constraint-driven-placement PRD track.)
5. **No penetration depth.** `min_clearance` and `distance` both report `0` on boundary-crossing
   overlap — a kernel property, since OCCT's `BRepExtrema_DistShapeShape::Value()` is non-negative by
   construction and Manifold's `min_gap` matches — so neither can rank interference severity, and an
   objective that minimises penetration is flat inside the interference region. Two traps ride along:
   a solid fully **nested** inside another with no boundary contact returns a **positive** distance
   (so "overlap ⇒ 0" is not a general rule — pair the scalar with `interferes_with`/`intersects` for
   the boolean), and a self-pair `min_clearance(s, id, id)` returns `undef`, not `0`.
6. **`intersects` is `d <= 0.0`.** Face-touching parts therefore read `true`, and there is no
   tolerance argument at any layer. For a tolerance band, write `distance(a, b) > tol` yourself.
