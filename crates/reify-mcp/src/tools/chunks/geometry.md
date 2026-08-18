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
half_space(px, py, pz, nx, ny, nz)                   -> Solid   // UNBOUNDED — Bounded = false
```

`half_space` is the one primitive that is not a finite body: `(px, py, pz)` is a point **on** the
boundary plane (a Length position, so `mm` literals), and `(nx, ny, nz)` is the **outward normal**
pointing toward the side whose material is retained — a direction, so plain dimensionless numbers,
not lengths. Because the result has `Bounded = false` it cannot be used where a Bounded shape is
required; intersect it with a finite solid to get a bounded result usable for export and
mass-property queries:

```
intersection(half_space(0mm, 0mm, 0mm, 0, 0, 1), box(40mm, 40mm, 40mm))
```

Worked example: `examples/half_space.ri`.

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
| `cylinder` | **base at z=0**, axis **+Z**, x/y **centred at origin** | top face at `z = height`; NOT centred on z — a common hand-centering workaround is `translate(cylinder(r, h), 0, 0, -h/2)` |
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
When in doubt, prefer the `_centered` variant over a manual `translate(primitive(...), 0, 0, -h/2)`
workaround.

## GD&T Tolerance Zones

Constructors that build a geometric-tolerance zone as a real `Solid`, so a zone can be
intersected, differenced and measured like any other body. Every one takes its zone extent
as a **width**, and every one centres the zone on the geometry it is given (`±width/2`):

```
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

`zone_profile` lowers to the difference of two OCCT thicken results — the solid thickened by
`+width/2` minus the same solid thickened by `−width/2` — giving a shell that straddles the input
solid's surface. It has no closed-form volume; expect roughly `surface_area × width`, and query
the realized solid rather than computing it by hand.

Worked example of all four: `examples/tolerancing/gdt_zones.ri`.

## Free-form & Implicit Surfaces

Two constructors that produce a surface from data rather than from a parametric shape —
a NURBS patch from an explicit control net, and a marching-cubes mesh from a voxel grid:

```
nurbs_surface(control_points, weights, u_knots, v_knots, u_degree, v_degree)  -> Surface
isosurface(grid)                                     -> Mesh   // marching cubes, iso = 0.0
isosurface(grid, iso: level)                         -> Mesh
isosurface(grid, iso: level, adaptive: flag)         -> Mesh
```

`nurbs_surface`'s six arguments do **not** all have the same shape. `control_points` is a
**nested** (u-major × v) list of `point3(...)`, and `weights` is a matching nested list of reals;
but `u_knots`/`v_knots` are **flat** clamped knot vectors, and `u_degree`/`v_degree` are plain
integers. A bilinear patch (degree 1 × 1, clamped knots `[0,0,1,1]`):

```
nurbs_surface(
    [[point3(0mm,0mm,0mm),point3(0mm,10mm,0mm)],[point3(10mm,0mm,0mm),point3(10mm,10mm,5mm)]],
    [[1.0,1.0],[1.0,1.0]],
    [0,0,1,1],
    [0,0,1,1],
    1,
    1
)
```

A free-form NURBS patch is neither Closed nor Planar, so it is **not** a valid profile for
`extrude`/`revolve`/`sweep`/`loft` — passing one inline emits `GeometryProfileRequired`.

`isosurface` extracts a surface by marching cubes from a Voxel-repr `grid` operand; a BRep or Mesh
operand is voxelized first (Mesh→Voxel on OpenVDB) and surfaced back Voxel→Mesh. `iso` and
`adaptive` are **optional** trailing arguments, and at most 3 arguments are accepted. Omitting them
is not the same as passing a default at the call site: they are left unset and resolved during
evaluation lowering to `iso = 0.0` and `adaptive = false`.

The `iso:`/`adaptive:` labels written above are the **recommended spelling** — they name the slot at
the call site and keep a bare `true` from reading as a mystery flag — but they are **not checked**.
Like every geometry constructor, `isosurface` binds its arguments **positionally**, in source order:
2nd argument → `iso`, 3rd → `adaptive`. Two consequences:

- `isosurface(grid, level)` compiles to exactly the same thing as `isosurface(grid, iso: level)`.
- **`adaptive` cannot be passed without `iso`.** `isosurface(grid, adaptive: flag)` is accepted, and
  silently binds `flag` into the **iso** slot. Pass the iso level explicitly —
  `isosurface(grid, iso: level, adaptive: flag)` — whenever you want the adaptive flag.

Worked examples: `examples/multi_kernel/voxel_to_mesh.ri` and
`examples/multi_kernel/voxel_to_mesh_iso.ri`.
