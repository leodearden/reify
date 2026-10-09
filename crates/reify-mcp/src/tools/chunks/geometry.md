# Geometry Types

- Measurement and mass-property queries (`volume`, `area`, `centroid`, `bounding_box`, …) are topic
  `measurement` of `reify_language_reference`.
- Topology selectors (`faces`, `edges`, `faces_by_normal`, …) are topic `topology`.

## Algebraic Types

**Point/Vector distinction (affine space):**
- `Point - Point → Vector` (valid)
- `Point + Vector → Point` (valid)
- `Vector + Vector → Vector` (valid)
- `Point + Point` → type error

Parameterized by dimensionality and quantity:
```reify-schematic
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

```reify-schematic
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

```reify-schematic
point2(x, y)          point3(x, y, z)
vec2(x, y)            vec3(x, y, z)
line_segment(x1, y1, z1, x2, y2, z2)
arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)
polygon(x1, y1, x2, y2, x3, y3, …)   rectangle(width, height)
```

## Solid Primitives

```reify-schematic
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
half_space(px, py, pz, nx, ny, nz)                   -> Solid   // UNBOUNDED — Bounded = false
```

`half_space` is the one primitive that is not a finite body: `(px, py, pz)` is a point **on** the
boundary plane (a Length position, so `mm` literals), and `(nx, ny, nz)` is the **outward normal**
pointing toward the side whose material is retained — a direction, so plain dimensionless numbers,
not lengths. Because the result has `Bounded = false` it cannot be used where a Bounded shape is
required; intersect it with a finite solid, as `bounded` does here, to get a bounded result usable
for export and mass-property queries:

```reify
structure def BoundedHalfSpace {
    let bounded = intersection(half_space(0mm, 0mm, 0mm, 0, 0, 1), box(40mm, 40mm, 40mm))
}
```

Worked example: `examples/half_space.ri`.

`rounded_box` requires `corner_r > 0` and `2*corner_r < min(width, depth)`; violations are a compile-time error when the args are constant literals (including constant arithmetic like `10mm + 15mm`). A param-driven `corner_r` that violates the constraint at runtime is **not** caught statically — it fails at evaluation with an opaque kernel error instead of a diagnostic.

**2D profiles** (planar faces in the XY plane at z=0). `rectangle`/`circle`/`ellipse` are centred
at origin (same centring as `box`); `polygon` is the exception — it is positioned by its explicit
vertex coordinates, not auto-centred (see the Anchoring & orientation table below):

```reify-schematic
rectangle(width, height)   circle(radius)
polygon(x1, y1, x2, y2, x3, y3, …)   ellipse(semi_major, semi_minor)
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

### Dimensioned arguments
<!-- LENGTH-ARGS-SECTION -->

<!-- SYNC: this section is the per-position CATALOGUE — which argument of which constructor is
     length-semantic. The RULE it applies (bare numbers rejected, bare `0` included, what stays
     dimensionless) belongs to the `units` chunk; neither restates the other, and each points at
     the other, so read the two together.

     Two chunk guards run over this section:
     geometry_chunk_smoke.rs::documented_call_names_in_the_length_section_are_real_registry_entries
     asserts every constructor named here is a real compiler registry entry, so a name that does
     not exist is RED rather than a phantom an author copies. It reads TWO things, and the
     difference matters to whoever edits this section next:
       - the TABLE's FIRST CELL, one row at a time, floored at the live row count — so DELETING a
         catalogue row is RED. A row is a `|`-leading line whose first cell backticks the
         constructor it is about; keep that shape. Later columns are free prose and are NOT
         scanned, which is why they may safely backtick argument names like `n_points`.
       - the section's CALL FORMS, i.e. `name(`, which is what the fence below carries.
     The three sentinel constructors (`translate`, `polygon`, `nurbs`) must appear in BOTH — named
     by a table row AND called by the fence — so neither half can cover for the other losing one.
     fence_gate.rs::every_reify_tagged_fence_compiles_clean compiles the ```reify fence below as a
     whole module, so the migration forms are verified rather than asserted. Both
     scans are scoped BYTE-EXACTLY by the `LENGTH-ARGS-SECTION` marker on the line above,
     NOT by this heading's wording, which is free to change — keep the marker directly under the
     heading it opens.

     What those guards do NOT establish: they check NAMES and compile-acceptance, never that a
     documented argument really carries the DIMENSION claimed for it, and never that a table row's
     PROSE columns are accurate. That half is pinned on the eval side; see the PINNED/UNPINNED
     inventory in the `units` chunk. -->

Geometry constructors take **dimensioned** lengths. At a length-semantic argument position a bare
number is rejected outright with a diagnostic — it is not read as metres, and **bare `0` is not
special-cased**: `0mm`, never `0`. Dividing a length by a bare number preserves the length, so
`-h/2` stays as it is. The `units` chunk — topic `units` of `reify_language_reference` — states the
rule and why it is a rejection rather than a default; this section says which positions it lands on.

| Constructor | Length-semantic arguments | Stays dimensionless |
|---|---|---|
| `translate` | all three components | — |
| `rotate_around` | the pivot point | axis direction, angle |
| `revolve` | the axis origin | axis direction, angle |
| `line_segment` | both endpoints, every coordinate | — |
| `arc` | centre coordinates and radius | axis direction, start/end angles |
| `helix` | **all three arguments** — `helix(radius, pitch, height)` | — |
| `interp` / `bezier` | **every argument**: variadic coordinate triples, at least 6 and always a multiple of 3 | — |
| `nurbs` | the control-point coordinates in the middle only | leading `degree` and `n_points` counts; trailing weights and knots |
| `polygon` | **every argument, at every arity** | — |

Two rows are worth reading twice. `helix` has **no coordinates at all** — it is three lengths and
nothing else, so there is no bare tail to get right. `polygon` has **no dimensionless position at
all**, since a polygon vertex is a point in the XY plane; that is what distinguishes it from
`nurbs`, whose argument list mixes both.

```reify
structure def LengthArguments {
    param h : Length = 40mm

    // Every component of a `translate` is length-semantic — bare `0` included.
    // `-h/2` needs no unit of its own: a Length over a bare number is a Length.
    let centred = translate(cylinder(10mm, h), 0mm, 0mm, -h/2)

    // `rotate_around`: the PIVOT is length-semantic, the AXIS (0, 0, 1) is a
    // unit vector, and the angle carries its own unit.
    let turned = rotate_around(centred, 0mm, 0mm, 0mm, 0, 0, 1, 45deg)

    // `polygon` has NO dimensionless position — every vertex coordinate is a
    // length, at every arity.
    let profile = polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)

    // `helix` takes exactly three arguments and NO coordinates:
    // helix(radius, pitch, height), all three lengths.
    let spring = helix(10mm, 2mm, 50mm)

    // `interp` / `bezier` are variadic COORDINATE TRIPLES: every argument a
    // length, at least 6 of them, always a multiple of 3.
    let spine = interp(0mm, 0mm, 0mm, 10mm, 5mm, 0mm)

    // `nurbs` mixes both in one argument list: `degree` and `n_points` are
    // counts, the control-point coordinates in the middle are lengths, and the
    // trailing weights and knots are dimensionless.
    let rail = nurbs(1, 2, 0mm, 0mm, 0mm, 10mm, 0mm, 0mm, 1, 1, 0, 0, 1, 1)
}
```

**The dimensionless column is an authoring rule you uphold, not one the compiler enforces.** The
gate runs in one direction only: a bare number in a length slot is rejected, but a unit in a
dimensionless slot generally is not. Measured 2026-08-30: `scale(g, 2mm)` is accepted, as is a
dimensioned axis direction in `mirror` or `linear_pattern`. Write those bare because it is what you
mean — nothing will tell you otherwise.

The rejected forms and their migrations are tabulated once, in the `units` chunk under "What it
looks like when you get it wrong" — one row per form, each row executed by
units_chunk_smoke.rs::documented_rejected_forms_are_actually_rejected. Not repeated here.

**`reify check` is not a gate for all of this.** It prints the rejection either way, but its EXIT
CODE is reliable only for the constructors the COMPILER checks. Measured 2026-08-30 and re-measured
2026-09-07: `box`, `translate`, `fillet`, `rotate_around` and the pivot triple of 7-argument
`mirror` exit 1, while `helix`, `polygon`, `arc`, `line_segment`, `interp`, `bezier` and `nurbs`
have no compile-time length slot — they print `error:` and still exit **0**, because their
arguments are checked at build/eval time only. Gate a design on `reify eval` or `reify build`,
never on `reify check`'s exit status alone. The compile-layer half of that split is PINNED by
units_chunk_smoke.rs::documented_eval_only_rejections_are_invisible_to_the_compile_layer, and the
`mirror` exit code specifically by
crates/reify-cli/tests/harness_cli/cli_check.rs::check_rejects_bare_scalar_mirror_origin_before_reaching_build;
the REMAINING exit-code claims are UNPINNED prose (nothing else in these harnesses runs the CLI)
and the residual is tracked in `docs/prds/v0_6/check-diagnostic-truthfulness.md`. Full
PINNED/UNPINNED inventory: the `units` chunk.


## GD&T Tolerance Zones

Constructors that build a geometric-tolerance zone as a real `Solid`, so a zone can be
intersected, differenced and measured like any other body. Every one takes its zone extent
as a **width**, and every one centres the zone on the geometry it is given (`±width/2`):

```reify-schematic
zone_slab(face, width)                                 -> Solid   // face offset ±width/2, capped into a slab
zone_cylinder(axis, width)                             -> Solid   // Ø-zone about an axis wire; width is the DIAMETER
zone_annulus(axis, nominal_radius, width, length)      -> Solid   // annular shell at nominal_radius ± width/2
zone_profile(solid, width)                             -> Solid   // surface-profile shell, ±width/2 about the solid
```

`zone_slab` takes a **face or 2D profile** as its first argument — not a solid — and offsets it
`±width/2`, capping the result into a centred slab. (`zone_profile` is the solid-input sibling.)

`zone_cylinder`'s `width` is the zone **diameter**, not its radius: it lowers to a pipe sweep with
`radius = width * 0.5`. There is deliberately **no length argument** — the axis wire's own length
sets the cylinder extent, so control the zone's length by controlling the wire.

`zone_annulus` lowers to the difference of two pipe sweeps along the axis: an outer sweep of
radius `nominal_radius + width/2` minus an inner one of radius `nominal_radius − width/2`. Its
fourth argument, `length`, is accepted and validated but does **not** drive the result: as with
`zone_cylinder`, the swept extent comes from the axis wire. Pass it for signature completeness,
and size the wire to size the zone.

`zone_profile` lowers to the difference of two `offset_solid` results — the solid offset by
`+width/2` minus the same solid offset by `−width/2` — giving a shell that straddles the input
solid's surface. Every face moves along its normal and sharp edges stay sharp, so for a box of
side `a` the zone volume is exactly `(a+w)³ − (a−w)³`. A width whose inward half reaches past the
solid's inradius is an error. So is an input that is not one single solid bounded by planes,
cylinders, cones, spheres and tori: a disjoint union, or a freeform face such as a loft's, is
refused rather than offset inexactly. For general shapes, query the realized solid rather than
computing its volume by hand.

Worked example of all four: `examples/tolerancing/gdt_zones.ri`.

## Free-form & Implicit Surfaces

Two constructors that build geometry from data rather than from a parametric shape —
a NURBS patch from an explicit control net, and a marching-cubes body from a voxel grid:

```reify-schematic
nurbs_surface(control_points, weights, u_knots, v_knots, u_degree, v_degree)  -> Surface
isosurface(grid)                                     -> Solid  // marching cubes, iso = 0.0
isosurface(grid, iso: level)                         -> Solid
isosurface(grid, iso: level, adaptive: flag)         -> Solid
```

`nurbs_surface`'s six arguments do **not** all have the same shape. `control_points` is a
**nested** (u-major × v) list of `point3(...)`, and `weights` is a matching nested list of reals;
but `u_knots`/`v_knots` are **flat** clamped knot vectors, and `u_degree`/`v_degree` are plain
integers. `patch` below is a bilinear patch (degree 1 × 1, clamped knots `[0,0,1,1]`), one
argument per line:

```reify
structure def BilinearPatch {
    let patch = nurbs_surface(
        [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
        [[1.0,1.0],[1.0,1.0]],
        [0,0,1,1],
        [0,0,1,1],
        1,
        1
    )
}
```

A free-form NURBS patch is neither Closed nor Planar, so it is **not** a valid profile for
`extrude`/`revolve`/`sweep`/`loft` — passing one inline emits `GeometryProfileRequired`.

`isosurface` extracts a surface by marching cubes from a Voxel-repr `grid` operand; a BRep or Mesh
operand is voxelized first (Mesh→Voxel on OpenVDB) and surfaced back Voxel→Mesh.

Its **type** is `Solid`, not a bare mesh handle: the compiler infers the result Bounded, Watertight
and Connected, so it composes with the boolean operations and the export paths like any other solid
body. A mesh is only its internal repr — the Voxel→Mesh step above is part of the lowering, not
something you write.

`iso` and `adaptive` are **optional** trailing arguments, and at most 3 arguments are accepted.
Omitting them is not the same as passing a default at the call site: they are left unset and
resolved during evaluation lowering to `iso = 0.0` and `adaptive = false`.

`iso` is a **Length**, not a bare number — write it with a unit, as the worked example does
(`iso: 3mm`); it is decoded to SI metres for marching cubes. `adaptive` is a **Bool**.

The `iso:`/`adaptive:` labels written above are the **recommended spelling** — they name the slot at
the call site and keep a bare `true` from reading as a mystery flag — but they are **not checked**.
Like every geometry constructor, `isosurface` binds its arguments **positionally**, in source order:
2nd argument → `iso`, 3rd → `adaptive`. Two consequences:

- `isosurface(grid, level)` compiles to exactly the same thing as `isosurface(grid, iso: level)`.
- **`adaptive` cannot be passed without `iso`.** `isosurface(grid, adaptive: flag)` is accepted, and
  binds `flag` into the **iso** slot. Compilation is silent about it, but evaluation is not: a Bool
  is not a Length, so it warns `isosurface: 'iso' argument evaluated to a non-numeric value —
  defaulting to 0.0`, and `adaptive` stays `false` because nothing reached its slot. Pass the iso
  level explicitly — `isosurface(grid, iso: level, adaptive: flag)` — whenever you want the adaptive
  flag.

Worked examples: `examples/multi_kernel/voxel_to_mesh.ri` and
`examples/multi_kernel/voxel_to_mesh_iso.ri`.


## Interference & Clearance Queries
<!-- ORACLE-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs verifies, for all
     five query names: that this section documents each as a call form, that each is a real registry
     entry, that the ```reify fences below call each one, that each `name(...) -> Type`
     signature here is exercised by a fence call at the SAME arity, and that every arity a fence
     calls one at is documented by such a signature. So editing an arity in this section without
     editing the matching fence, or the reverse, is RED. That those fences COMPILE is verified by
     crates/reify-compiler/tests/harness_doc_chunks/fence_gate.rs, over every chunk. It still
     covers names/arity/parse only — argument DIMENSION is unchecked. The RUNTIME claims in
     "Clearance-query traps" are pinned (where they are pinned at all) by the eval/CLI tests mapped
     in the SYNC block at that subsection — read it before relying on a trap, and before changing
     one of those behaviours.

     The `ORACLE-SECTION` marker on the line above is what scopes that guard's scan, matched
     byte-exactly — NOT this heading's wording, which is free to change. Keep the marker directly
     under the heading it opens; the scan runs from it to the next `##` heading. -->

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

    // Pair the boolean with the scalar: `intersects` is exactly `d <= 0.0`
    // with no tolerance argument (trap 6), so `not fouls` alone still admits
    // a zero-gap face touch. `gap > 1mm` is what expresses the tolerance band.
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

    // The other two FORM A queries, same let-bind-first rule: a Bool for one
    // named pair, and the whole upper-triangular pair list for the mechanism.
    let fouls = interferes_with(s, id_a, id_b)
    let pairs = interferes(s)
}
```

Worked references: `examples/tolerancing/vc_bolt_pattern_clearance.ri` (the one example where a
clearance query flips a `reify build` verdict end-to-end) and `examples/kinematic/dock_pickup.ri`.
The swept form `flat_map(snaps, |s| [min_clearance(s, a, b)])` is supported (see `dock_pickup.ri`);
a swept unary `interferes` is not.

### Clearance-query traps

<!--
SYNC: which trap below is pinned by an executable test, and where. The chunk guards
(geometry_chunk_smoke.rs, and fence_gate.rs for the fences) establish only name existence +
fence compile-acceptance, so these runtime claims would otherwise rot silently. Named here so a
behaviour change lands in a file whose grep leads back to this doc — and so the UNPINNED ones
are visibly unpinned rather than looking equally guarded.

FORMAT IS LOAD-BEARING. Every cite is written WHOLE on ONE line as `<path>::<fn_name>`, never
wrapped across lines and never tabulated into a two-column layout.
geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve resolves each one against the
tree — the file must exist and must declare that fn — so a renamed or deleted test is RED
there rather than silently turning a PINNED row into a false claim. A wrapped path is
invisible to that check, so keep one cite per line when editing this block.

  trap 1 (build/eval catches it) — PINNED by
    crates/reify-cli/tests/harness_cli/cli_vc_clearance.rs::build_vc_bolt_pattern_clearance_satisfied
    crates/reify-cli/tests/harness_cli/cli_vc_clearance.rs::build_vc_bolt_pattern_interference_violated
  trap 1 (`check` exits 0) — UNPINNED, prose only.
  trap 2 (inline arg -> undef) — PINNED by
    crates/reify-eval/tests/harness_kernel_realization/kernel_queries_intersects_smoke.rs::intersects_smoke_evals_expected_booleans
    (its IntersectsSmoke.undef_inline assertion: the cell must be None or Undef).
  trap 3 (FORM A trio on plain geometry -> undef) — UNPINNED, prose only. Verified by hand
    while probing; re-verify before trusting it.
  trap 4 (sub `at` pose not carried into snapshot) — UNPINNED, prose only. Substrate fix is
    owned by the constraint-driven-placement PRD track.
  trap 5 (clamp to 0 on overlap; positive when disjoint; self-pair -> undef) — PINNED by
    crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::overlapping_cubes_one_pair_and_zero_clearance
    crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::disjoint_cubes_no_pairs_and_positive_clearance
    crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::single_body_self_pair_excluded
  trap 5 (full CONTAINMENT reads dist 0 / intersects true, under OCCT) — UNPINNED HERE, prose
    only. Measured 2026-08-23: strictly nested boxes, both argument orders, live disjoint
    control. The executable pin is task #6269's, landing in
    crates/reify-eval/tests/harness_kernel_realization/kernel_queries_intersects_smoke.rs.
    OCCT-scoped: Manifold parity on a nested pair is unmeasured (task #6475).
  trap 6 (`d <= 0.0`) — PINNED by
    crates/reify-eval/tests/harness_kernel_realization/kernel_queries_intersects_smoke.rs::intersects_smoke_evals_expected_booleans
    Source of truth is crates/reify-eval/src/geometry_ops.rs's Bool(d <= 0.0). The
    face-TOUCHING edge case specifically is UNPINNED; the test covers overlapping and
    well-apart.

FORM A's posed and swept forms are pinned by
    crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::fk_posed_cubes_no_interference_and_correct_clearance
    crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::swept_min_clearance_monotonic_to_interference
-->

Every one of these is a **silent wrong answer**, not an error — read them before writing a clearance
gate.

1. **Needs a realized kernel — which `reify check` now attaches.** Since task 5748 (landed
   2026-08-28) `cmd_check` realizes geometry for any module that carries some, so on an OCCT build
   BOTH forms get a real verdict under `reify check`: `clearance_oracle.ri` reports `OK` for its
   `intersects`/`distance` constraints, and `vc_bolt_pattern_clearance.ri` reports `OK` for its
   `min_clearance` one (flip either to an impossible bound and check reports `VIOLATED`). Gate
   clearance under `reify check`.
   Where you still get `INDETERMINATE` — and it is a graceful degradation, never a false verdict:
   a build without OCCT, and any kernel-less in-process engine (`Engine::new(.., None)`). Both still
   **exit 0** ("No constraints violated (1 indeterminate)"); only `--strict` flips that, so never
   gate on check's exit status alone. Never "fix" an indeterminate clearance constraint by deleting
   it. Note the asymmetry when it does go indeterminate: `intersects`/`distance` emit an explanatory
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
   objective that minimises penetration is flat inside the interference region. One rider: a self-pair
   `min_clearance(s, id, id)` returns `undef`, not `0`.
   Full containment is **not** a blind spot. Under OCCT a solid strictly **nested** inside another,
   with no boundary contact at all, likewise reads `distance = 0 m` and `intersects = true` — OCCT
   classifies containment explicitly (`BRepExtrema_DistShapeShape::InnerSolution()`: "completely or
   partially inside the solid"), and "non-negative by construction" includes zero. So
   `constraint not fouls` covers nesting as well as boundary-crossing overlap. Measured
   2026-08-23; the executable pin is task #6269's, in `kernel_queries_intersects_smoke.rs`.
   Scoped to **OCCT** — Manifold's answer on a strictly nested pair is unmeasured (task #6475).
6. **`intersects` is `d <= 0.0`.** Face-touching parts therefore read `true`, and there is no
   tolerance argument at any layer. For a tolerance band, write `distance(a, b) > tol` yourself.
