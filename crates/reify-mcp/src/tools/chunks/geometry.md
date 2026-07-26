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
