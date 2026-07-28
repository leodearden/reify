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
`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2` (take `Angle`)
`dot`, `cross`, `normalize`, `magnitude`, `determinant`, `inverse`

## Key Geometry Operations

<!-- SYNC: signatures verified by crates/reify-compiler/tests/harness_doc_chunks/stdlib_chunk_geometry_ops_smoke.rs -->

Positions/lengths take a `Length` (e.g. `5mm`); angles take an `Angle` (e.g. `90deg`); direction/axis/normal components and counts are plain numbers.

**Booleans:** `union(a, b)`, `difference(a, b)`, `intersection(a, b)`; variadic `union_all(a, b, …)`, `intersection_all(a, b, …)`
**Modify:** `fillet(solid, radius)` or `fillet(solid, edges, radius)`, `chamfer(solid, distance)` or `chamfer(solid, edges, distance)`, `shell(solid, thickness, faces…)` (thickness first, then optional face indices to remove), `offset_solid(solid, distance)`, `thicken(solid, offset)`, `offset_curve(curve, distance)`
**Sweep:** `extrude(profile, distance)`, `revolve(profile, ox, oy, oz, ax, ay, az, angle)`, `sweep(profile, path)`, `loft(profile1, profile2, …)`
**Transform:** `translate(geo, dx, dy, dz)`, `rotate(geo, ax, ay, az, angle)` or `rotate(geo, orientation)`, `mirror(geo, plane)` or `mirror(geo, ox, oy, oz, nx, ny, nz)`, `scale(geo, factor)`
**Pattern:** `linear_pattern(geo, dx, dy, dz, count, spacing)`, `circular_pattern(geo, axis, count, angle)` or `circular_pattern(geo, ox, oy, oz, ax, ay, az, count, angle)`
**Split:** `split(solid, plane)` → `List<Geometry>` — a topology selector (returns the pieces on each side of the plane), not a CSG boolean
**Curves:** `line_segment(x1, y1, z1, x2, y2, z2)`, `arc(cx, cy, cz, radius, start_angle, end_angle, ax, ay, az)`, `helix(radius, pitch, height)`, `interp(x1, y1, z1, …)` (coordinate triples), `bezier(x1, y1, z1, …)` (coordinate triples), `nurbs(degree, n_points, coords…, weights…)`

## Constants

`pi`, `e`, `g` (gravity), `c` (light speed), `boltzmann`, `avogadro`, `planck`

## Prelude (Auto-imported)

Point/vector constructors, basic geometry constructors, `pi`, `e`, `true`, `false`, primitive types, `Option`, `List`, `Set`, `Map`, `Range`.
