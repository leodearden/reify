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
     geometry_chunk_smoke.rs::reify_tagged_fences_in_geometry_chunk_compile compiles the ```reify
     fence below as a whole module, so the migration forms are verified rather than asserted. Both
     scans are scoped BYTE-EXACTLY by the `<!-- LENGTH-ARGS-SECTION -->` marker on the line above,
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


## Interference & Clearance Queries
<!-- ORACLE-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs verifies, for all
     five query names: that this section documents each as a call form, that each is a real registry
     entry, that the ```reify fences below COMPILE, and that each `name(...) -> Type` signature here
     is exercised by a fence call at the SAME arity. So editing an arity in this section without
     editing the matching fence is RED. It still covers names/arity/parse only — argument DIMENSION
     is unchecked. The RUNTIME claims in "Clearance-query traps" are pinned (where they are pinned at
     all) by the eval/CLI tests mapped in the SYNC block at that subsection — read it before relying
     on a trap, and before changing one of those behaviours.

     The `<!-- ORACLE-SECTION -->` marker on the line above is what scopes that guard's scan, matched
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
SYNC: which trap below is pinned by an executable test, and where. The chunk guard
(geometry_chunk_smoke.rs) establishes only name existence + fence compile-acceptance, so
these runtime claims would otherwise rot silently. Named here so a behaviour change lands in
a file whose grep leads back to this doc — and so the UNPINNED ones are visibly unpinned
rather than looking equally guarded.

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


## Measurement & Mass-Property Queries
<!-- MEASUREMENT-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs verifies that
     this section documents EVERY member of reify_compiler::GEOMETRY_QUERY_NAMES as a call form.
     That test iterates the registry DIRECTLY rather than a list copied into the test, so a query
     added to the compiler and not to this section is RED on the next run — there is no second
     list to forget.

     FORMAT IS LOAD-BEARING in one place only: for `volume` / `area` / `centroid` /
     `bounding_box`, the literal `-> <Type>` after the call form is what marks the form as a
     SIGNATURE whose arity is cross-checked against the worked fence. Tabulating those four into
     a table with the return type in its own column is RED even though nothing regressed.
     Everything else here — bolding, wrapping, ordering, the heading's wording — is free.

     Chunk-side guards (all in that one file, cited whole on one line each):
       geometry_chunk_smoke.rs::measurement_query_family_documented_in_geometry_chunk
       geometry_chunk_smoke.rs::reify_tagged_fences_in_geometry_chunk_compile
       geometry_chunk_smoke.rs::documented_measurement_arities_are_exercised_by_a_compiling_fence
       geometry_chunk_smoke.rs::the_undef_trap_example_is_a_query_the_hoist_does_not_cover

     That last guard scopes the region BETWEEN `<!-- NOT-HOISTED-TRAP -->` and
     `<!-- /NOT-HOISTED-TRAP -->` below — both markers matched byte-exactly, and a missing
     closing one is RED rather than a silent widening — and pins exactly ONE
     claim: the call form the arg-shape trap exhibits is drawn from OUTSIDE
     reify_compiler::WHOLE_HANDLE_GEOMETRY_QUERY_NAMES, so the trap cannot illustrate "an inline
     argument yields undef" with one of the four names the next sentence exempts. It reads the
     registry directly, so a name entering or leaving the hoisted set moves it. What it does NOT
     pin: the traps' wording, the binder-scope claim in trap 2, and the `undef` behaviour itself —
     all UNPINNED prose here. Trap 2 is pinned on the compiler side instead, by
     crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::assert_hoist_scope
     whose three binder cases each assert exactly zero hoists. Naming a whole-handle query as a
     bare backticked identifier in the carve-out sentence is fine — only the CALL FORM is refused.
     The closing marker sits directly after the third trap, so the **Worked reference** paragraph
     that ends this section is OUTSIDE the guarded region and may name any call form it likes.

     RUNTIME claims below (which names resolve to real numbers, and when they do not) are pinned
     separately, on the eval side:
       crates/reify-eval/tests/harness_kernel_realization/kernel_queries_integration.rs::all_queries_walk_evals_top_level_helpers_to_non_undef
       crates/reify-eval/tests/harness_kernel_realization/kernel_queries_integration.rs::multi_feature_part_sub_handle_queries_return_non_undef
       crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::whole_handle_geometry_query_oracle_parity
       crates/reify-compiler/tests/harness_geometry_solver/geometry_query_inline_arg_tests.rs::compile_inline_volume_torus_hoists_into_realization
     The OCCT-absence claim in "When a query yields `undef`" is UNPINNED prose — verified by
     reading the gate in crates/reify-kernel-occt/src/lib.rs, not by a test in this harness.

     The `<!-- MEASUREMENT-SECTION -->` marker on the line above is what scopes the guard's scan,
     matched byte-exactly — NOT this heading's wording. Keep it directly under the heading it
     opens; the scan runs from it to the next `##` heading. -->

Reify measures **realized geometry**. Never hand-compute a volume, an area, a centroid or an
inertia from the parameters that built the part — ask the kernel. Parameter arithmetic
(`thickness * width * width`) is a *different number*: it silently stops describing the part the
moment a fillet, a shell, a boolean or a pattern changes it, and nothing flags the divergence. The
family below is the supported way to ask.

**Measurement.** `volume(solid) -> Scalar<Volume>`, `area(surface) -> Scalar<Area>` (also
`area(solid) -> Scalar<Area>`, total surface area), `length(curve) -> Scalar<Length>`,
`perimeter(surface) -> Scalar<Length>`.

**Mass properties.** `centroid(solid) -> Point3<Length>` is the purely geometric centre — no
density argument, no material. `bounding_box(geometry) -> BoundingBox` is the axis-aligned extent.
For the *density-weighted* pair, `center_of_mass(solid, density)` and
`moment_of_inertia(solid, density)`, see the Topology Selectors table below: they are registered in
the topology-selector family, not this one, and both take a **dimensioned** `Density` (a bare
number yields a warning and `undef`).

**Predicates.** `contains(solid, point) -> Bool`, `intersects(a, b) -> Bool`,
`geo_equiv(a, b, tolerance) -> Bool` (tolerance must carry a Length unit).

> `contains(solid, point)` is the free-function GEOMETRY query documented here. It is a different
> thing from the `contains` **method** on `List` / `Set` / `Range` in the `collections` chunk — same
> word, unrelated dispatch. Reaching for the collection method on a solid, or this query on a list,
> gets you a type error at best and `undef` at worst.

**Analysis.** `distance(a, b) -> Scalar<Length>` (true minimum surface gap — see the
interference/clearance section above, which owns it), `normal(surface, at) -> Vector3<Dimensionless>`,
`curvature(curve, at) -> Scalar<Curvature>` (the surface overload is
`curvature(surface, at) -> Matrix<2, 2, Curvature>`), `angle(a, b) -> Angle` — note `angle` takes two
dimensionless **vectors**, not two surfaces; the surface form is `angle_between_surfaces` in the
topology-selector family. `max_deviation(actual, nominal) -> Scalar<Length>` compares a realized
geometry against a nominal one.

**Provenance.** `feature(geometry) -> Feature` is the explicit projection from a realized handle to
the feature that produced it. It returns a `Feature`, not selectable geometry — it is the *input* to
the provenance selectors (`created_by_feature`, `split_by_feature`) in the table below.

```reify
structure def MeasuredBracket {
    // Let-bind the geometry FIRST. That is the arg-shape contract, not style.
    let plate = box(60mm, 40mm, 8mm)

    // Ask the kernel. Never re-derive these from 60mm * 40mm * 8mm: the moment a
    // fillet or a pocket lands, the arithmetic silently stops describing the part
    // and the queries keep up.
    let v = volume(plate)
    let a = area(plate)
    let c = centroid(plate)
    let bb = bounding_box(plate)

    // These four whole-handle queries are the only ones that ALSO accept an
    // inline geometry argument -- `volume(box(60mm, 40mm, 8mm))` works, because
    // the compiler hoists the inline call into a synthetic let for you. Every
    // other query in the family needs the let-bound form above, so writing
    // let-bound everywhere is the one rule that never bites.

    // A measured scalar driving a real gate, not a dangling let.
    constraint v < 25000mm^3
}
```

### Eval status, and when a query yields `undef`

Every one of these fifteen names has live eval dispatch. There is **no** "compile-time typed but
never evaluated" subset in this family. What there is, is a resolution *stage*: these are
kernel-bearing consumers, resolved during `reify build` against a realized handle. Under
kernel-less `reify eval` / `reify check` they stay `Value::Undef`, and a constraint over one reads
`INDETERMINATE` while the process still exits 0 (`--strict` flips that). This is the same stage
split the interference/clearance section documents at length.

Three things make a query silently `undef` even under `reify build`, and all three are worth knowing
before you debug the number:

<!-- NOT-HOISTED-TRAP -->
1. **Arg shape.** A geometry operand must be a **let-bound** reference. An inline call
   (`perimeter(rectangle(40mm, 20mm))`) is not resolved against the named-step map and yields
   `undef` with no diagnostic. The one carve-out is the four whole-handle queries — `volume`,
   `area`, `centroid`, `bounding_box` — where the compiler hoists an inline geometry argument into
   a synthetic let for you. Every other query in the family requires the let-bound form, so write
   let-bound everywhere and the rule never bites.
2. **Binder scope — where that carve-out stops.** The hoist is a post-process over structure MEMBER
   value cells, and it treats a lambda, a quantifier and a `match` as **opaque**: it neither
   descends into one nor rewrites inside one, because the argument is lifted verbatim out to the
   member list, where a name bound by the lambda param / quantifier variable / match binder would
   be unbound. So the carve-out simply does not apply under a binder — a quantifier predicate
   applying `volume` to an inline `box(s, s, s)` keeps the base `undef` behaviour, whole-handle or
   not. The guard is structural rather than a free-identifier analysis, so this is conservative in
   both directions: an inline query that happens to use no bound name is left un-hoisted too.
   Remedy: let-bind the geometry at member level. When the geometry depends on the bound variable
   itself there is no let-bound rewrite to reach for, and that cell stays `undef` today.
3. **No OCCT.** The kernel gate is all-or-nothing, not per-query: with OCCT unavailable the whole
   geometry pipeline is skipped, every query cell stays `undef`, and the exit code is still 0. There
   is no per-query fallback to a cheaper representation, so do not write code that expects one.
<!-- /NOT-HOISTED-TRAP -->

**Worked reference:** `examples/kernel_queries/all_queries_walk.ri` calls the family end-to-end over
a multi-feature part. Read it as a runnable example, not as normative prose — its own header is
partly stale about selector result types. The normative signature tables are
`docs/reify-stdlib-reference.md` §3.9 (`std.geometry.query`), which this section deliberately does
not restate. `max_deviation` is the one member §3.9 does not yet list; its signature above comes
from the compiler registry in `crates/reify-compiler/src/units.rs`.


## Topology Selectors
<!-- TOPOLOGY-SECTION -->

<!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs verifies BOTH
     directions over the table below — every member of
     reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES has a row (the registry is iterated
     directly, so a selector added to the compiler and not to this table is RED on the next run),
     and every name in the table's FIRST COLUMN is a real member of that registry rather than a
     phantom or a name borrowed from a sibling family. A row-count floor guards against the table
     being reformatted into a shape the scan cannot read.

       geometry_chunk_smoke.rs::topology_selector_family_documented_in_geometry_chunk

     THE TABLE SHAPE IS LOAD-BEARING for that guard: a catalogue row is a `|`-leading line whose
     FIRST cell backticks the selector it is about. Rewriting this into bullets is RED. The other
     columns are free-form — nothing matches on them.

     THE CALL FORM COLUMN is therefore UNPINNED, and it is the column whose errors are silent.
     A wrong arity is not a compile error: the compiler types a topology-selector call from its
     NAME alone, and eval's arity gate — crates/reify-eval/src/geometry_ops.rs::expected_arity —
     simply declines to dispatch, so the cell stays `Value::Undef` with no diagnostic. That is
     exactly the shape of the 2026-07-24 finding (`rotate(geo, axis, angle)` documented at a
     signature the compiler had never been shown, tasks #5347 / #5364). Check an argument list
     against that arity table, or against docs/reify-stdlib-reference.md §3.9, before trusting it;
     making this column registry-checkable the way the first column is needs that arity table
     exposed outside reify-eval.

     The Result and Eval columns are transcribed from
     crates/reify-compiler/src/units.rs::topology_selector_result_type and from the eval dispatch
     in crates/reify-eval/src/geometry_ops.rs. NOTHING IN THIS HARNESS CHECKS THEM — they are
     UNPINNED prose, and a result-type change in units.rs will not turn this table red. Re-read
     both sources before relying on a cell.

     The `<!-- TOPOLOGY-SECTION -->` marker on the line above is what scopes the scan, matched
     byte-exactly — NOT this heading's wording. Keep it directly under the heading it opens; the
     scan runs from it to the next `##` heading. -->

**A selector is not a list.** This is the one fact to carry away before reading the table. Most of
these constructors evaluate to a symbolic `Selector` value — a *query* over a body's topology, not
the resolved sub-handles. The compiler bridges `Selector` to `List<Geometry>` by inserting a
`ResolveSelector` coercion, and it does so at exactly **three** consumption sites: binding the
selector to a function/feature **parameter**, passing it to `single()` or another list helper, and
**indexing** it (`sel[0]`). Anywhere else — a bare `let all_faces = faces(b)`, or the value handed to
something that expects a list without being one of those three sites — it is still a `Selector`, and
you get a silent wrong answer rather than an error.
Note this differs from `docs/reify-stdlib-reference.md` §3.9, which documents the *post-coercion*
surface type (`fn faces(solid: Solid) -> List<Surface>`); §3.9 describes what a consumption site
sees, this table describes what the value IS.

**`edges` and `faces` are selectors here, argument names elsewhere.** Everywhere above in this
chunk, `edges` and `faces` appear only as the ARGUMENT of a fillet / chamfer / shell call
(`fillet(solid, edges, radius)`), where they name a parameter and say nothing about how to produce
one. This table is where they are documented as the selectors that produce it — that is what fills
that argument.

| Selector | Call form | Result | Eval |
|---|---|---|---|
| `faces` | `faces(solid)` | `Selector(Face)` | kernel-free mint |
| `edges` | `edges(solid)` | `Selector(Edge)` | kernel-free mint |
| `vertices` | `vertices(geometry)` | `Selector(Vertex)` | kernel-free mint |
| `mid_surface` | `mid_surface(body)` | `Selector(Face)` | kernel-free mint (shell-extract mid-surface faces) |
| `face` | `face(geometry, name)` | `Selector(Face)` | kernel-free mint (named leaf) |
| `edge` | `edge(geometry, name)` | `Selector(Edge)` | kernel-free mint (named leaf) |
| `vertex` | `vertex(geometry, name)` | `Selector(Vertex)` | kernel-free mint (named leaf) |
| `solid_body` | `solid_body(geometry, name)` | `Selector(Body)` | kernel-free mint (named leaf; `body` is the RBD mechanism ctor, not this) |
| `faces_by_area` | `faces_by_area(solid, range)` | `Selector(Face)` | kernel-free mint |
| `edges_by_length` | `edges_by_length(solid, range)` | `Selector(Edge)` | kernel-free mint |
| `faces_by_normal` | `faces_by_normal(solid, direction, tol)` | `Selector(Face)` | kernel-free mint; `tol` is an **Angle** |
| `edges_parallel_to` | `edges_parallel_to(solid, direction, tol)` | `Selector(Edge)` | kernel-free mint; `tol` is an **Angle** |
| `edges_at_height` | `edges_at_height(solid, height, tol)` | `Selector(Edge)` | kernel-free mint; both are **Length** |
| `faces_perpendicular_to` | `faces_perpendicular_to(solid, direction, tol)` | `Selector(Face)` | kernel-free mint |
| `edges_perpendicular_to` | `edges_perpendicular_to(solid, direction, tol)` | `Selector(Edge)` | kernel-free mint |
| `faces_by_surface_kind` | `faces_by_surface_kind(solid, kind)` | `Selector(Face)` | kernel-free mint |
| `edges_by_curve_kind` | `edges_by_curve_kind(solid, kind)` | `Selector(Edge)` | kernel-free mint |
| `extremal_by_bbox` | `extremal_by_bbox(solid, axis, sense, tol)` | `Selector(Face)` | kernel-free mint |
| `extremal_by_centroid` | `extremal_by_centroid(solid, axis, sense, tol)` | `Selector(Face)` | kernel-free mint |
| `created_by_feature` | `created_by_feature(solid, f)` | `Selector(Face)` | kernel-free mint; `f` comes from `feature(g)` |
| `split_by_feature` | `split_by_feature(solid, f)` | `Selector(Face)` | kernel-free mint; matches a split at ANY history position |
| `adjacent_faces` | `adjacent_faces(solid, face)` | `List<Geometry>` | kernel-bearing — needs a realized handle |
| `shared_edges` | `shared_edges(face1, face2)` | `List<Geometry>` | kernel-bearing |
| `siblings_of_face` | `siblings_of_face(parent, face)` | `List<Geometry>` | kernel-bearing (every face of `parent` except `face`) |
| `ancestor_faces_of_edge` | `ancestor_faces_of_edge(parent, edge)` | `List<Geometry>` | kernel-bearing (the faces owning `edge`) |
| `split` | `split(solid, plane)` | `List<Geometry>` | kernel-bearing |
| `closest_point` | `closest_point(point, geometry)` | `Point3<Length>` | kernel-bearing |
| `is_on` | `is_on(point, geometry)` | `Bool` | kernel-bearing |
| `angle_between_surfaces` | `angle_between_surfaces(a, b)` | `Angle` | kernel-bearing |
| `center_of_mass` | `center_of_mass(solid, density)` | `Point3<Length>` | kernel-bearing; `density` must be **dimensioned** |
| `moment_of_inertia` | `moment_of_inertia(solid, density)` | `Tensor<2, 3, MomentOfInertia>` | kernel-bearing; `density` must be **dimensioned** |

**Reading the Eval column.** *Kernel-free mint* means the value is produced without OCCT: the
selector is symbolic, so it survives a kernel-less `reify eval` and only its RESOLUTION needs a
realized body. *Kernel-bearing* means the call resolves against a realized handle during
`reify build`, so under `reify eval` / `reify check` — or with OCCT unavailable — the cell stays
`undef` and a constraint over it reads `INDETERMINATE`. The same let-bind-the-operand arg-shape rule
from the measurement section applies to every row.

The relational rows (`adjacent_faces`, `shared_edges`, `siblings_of_face`,
`ancestor_faces_of_edge`) take a face or edge HANDLE as their second argument, not a selector. The
canonical way to produce one is `single(...)` over a selector, inlined at the call site —
`adjacent_faces(body, single(faces_by_normal(body, up, 1deg)))`.

**Worked reference:** `examples/topology_selectors/all_topology_selectors_wiring.ri` wires the
family end-to-end; read it as a runnable example rather than as normative prose, since its own
header still describes eval dispatch as pending. `docs/reify-stdlib-reference.md` §3.9's "Topology
selectors" block holds the normative signatures, which this table deliberately does not restate.
