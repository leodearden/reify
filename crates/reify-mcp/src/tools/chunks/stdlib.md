# Standard Library Overview

The Reify standard library (`std.*`) provides domain-specific functionality.

## Module Tree

- `std.math` — numeric, trig, linalg, complex
- `std.units` — dimensions, SI, imperial, constants
- `std.geometry` — constructors, primitives, booleans, modify, sweep, transform, pattern, query, traits
- `std.structural` — structural analysis traits
- `std.ports` — base ports, mechanical, electrical, thermal, fluid
- `std.materials` — base, mechanical, thermal, electrical, optical, chemical
- `std.tolerancing` — dimensional, geometric (GD&T), surface
- `std.process` — manufacturing process traits, DFM rules
- `std.io` — import/export (STEP, STL, 3MF, etc.)
- `std.analysis` — analysis trait, stress analysis, results
- `std.fields` — field operations, interpolation, spatial operators
- `std.determinacy` — determinacy predicates, standard purposes

## Key Math Functions

`abs`, `min`, `max`, `clamp`, `lerp`, `sqrt`, `pow`, `log`, `exp`
`sin`, `cos`, `tan` (take `Angle`)
`asin`, `acos`, `atan`, `atan2` (return `Angle`)
`dot`, `cross`, `normalize`, `magnitude`, `determinant`, `inverse`

## Key Geometry Operations

<!-- SYNC: signatures verified by crates/reify-compiler/tests/harness_doc_chunks/stdlib_chunk_geometry_ops_smoke.rs -->

Positions/lengths take a `Length` (e.g. `5mm`); angles take an `Angle` (e.g. `90deg`); direction/axis/normal components and counts are plain numbers.

**Booleans:** `union(a, b)`, `difference(a, b)`, `intersection(a, b)`; variadic `union_all(a, b, …)`, `intersection_all(a, b, …)`
**Modify:** `fillet(solid, radius)` or `fillet(solid, edges, radius)`, `fillet_all(solid, radius)`, `chamfer(solid, distance)` or `chamfer(solid, edges, distance)`, `chamfer_asymmetric(solid, edges, d1, d2)` (the edge selector is mandatory here, unlike `chamfer`), `shell(solid, thickness, faces…)` (thickness first, then optional face indices to remove), `shell_open(solid, thickness, faces)` (takes a curated face SELECTOR, e.g. the result of `faces`, where `shell` takes numeric face indices), `offset_solid(solid, distance)`, `offset_surface(surface, distance)`, `thicken(solid, offset)`, `draft(solid, angle, plane)` or `draft(solid, faces, angle, plane)`, `offset_curve(curve, distance)`
**Sweep:** `extrude(profile, distance)`, `extrude_symmetric(profile, total_distance)` (the distance is the TOTAL extent, half each way), `extrude_infinite(profile, dx, dy, dz, direction)` (yields an UNBOUNDED solid; direction is the string `"positive"`, `"negative"` or `"both"`), `revolve(profile, ox, oy, oz, ax, ay, az, angle)`, `revolve_full(profile, ox, oy, oz, ax, ay, az)` (a full turn, so it takes no angle), `sweep(profile, path)`, `sweep_guided(profile, path, guide)`, `pipe(path, radius)` (the path comes FIRST and there is no profile argument — the circular section is built from the radius), `loft(profile1, profile2, …)`, `loft_guided(profile1, profile2, …, guide)` (the guide is the TRAILING argument)
**Transform:** `translate(geo, dx, dy, dz)`, `rotate(geo, ax, ay, az, angle)` or `rotate(geo, orientation)`, `rotate_around(geo, px, py, pz, ax, ay, az, angle)` (turns about an arbitrary point, not the origin), `mirror(geo, plane)` or `mirror(geo, ox, oy, oz, nx, ny, nz)`, `scale(geo, factor)`, `apply_transform(geo, transform)` (takes a RIGID transform — rotation plus translation), `affine_apply(geo, map)` (takes a GENERAL affine map, so it may also scale non-uniformly or shear)
**Pattern:** `linear_pattern(geo, dx, dy, dz, count, spacing)`, `linear_pattern_2d(geo, dx1, dy1, dz1, count1, spacing1, dx2, dy2, dz2, count2, spacing2)` (each axis is ordered direction, then count, then spacing), `circular_pattern(geo, axis, count, angle)` or `circular_pattern(geo, ox, oy, oz, ax, ay, az, count, angle)`, `arbitrary_pattern(geo, transforms)` (a list of transforms) or `arbitrary_pattern(geo, dx1, dy1, dz1, …)` (explicit offset triples)
**Split:** `split(solid, plane)` → `List<Geometry>` — a topology selector (returns the pieces on each side of the plane), not a CSG boolean
**Curves:** `line_segment(x1, y1, z1, x2, y2, z2)`, `arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)`, `helix(radius, pitch, height)`, `interp(x1, y1, z1, …)` (coordinate triples), `bezier(x1, y1, z1, …)` (coordinate triples), `nurbs(degree, n_points, coords…, weights…)`
Curve coordinates are length-semantic and must be dimensioned — `0mm`, not `0`; a bare number is rejected rather than read as SI metres. That covers every `interp` and `bezier` argument and `nurbs`' `coords…`. What stays dimensionless: `arc`'s angles and `ax`/`ay`/`az`, and `nurbs`' `degree` and `n_points` (counts), its `weights…` (rational blending factors) and its trailing knots (parameter-space values).

## Constants

`pi`, `e`, `g` (gravity), `c` (light speed), `boltzmann`, `avogadro`, `planck`

## Prelude (Auto-imported)

Point/vector constructors, basic geometry constructors, `pi`, `e`, `true`, `false`, primitive types, `Option`, `List`, `Set`, `Map`, `Range`.
