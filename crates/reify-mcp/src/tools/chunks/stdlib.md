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

**Booleans:** `union(a, b)`, `difference(a, b)`, `intersection(a, b)`, `split(solid, surface)`
**Modify:** `fillet(solid, edges, radius)`, `chamfer(solid, edges, distance)`, `shell(solid, faces, thickness)`, `offset_surface(surface, distance)`
**Sweep:** `extrude(profile, direction, distance)`, `revolve(profile, axis, angle)`, `sweep(profile, path)`, `loft(profiles)`
**Transform:** `translate(geo, vector)`, `rotate(geo, axis, angle)`, `mirror(geo, plane)`, `scale(geo, factor)`
**Pattern:** `linear_pattern(geo, direction, count, spacing)`, `circular_pattern(geo, axis, count)`

## Constants

`pi`, `e`, `g` (gravity), `c` (light speed), `boltzmann`, `avogadro`, `planck`

## Prelude (Auto-imported)

Point/vector constructors, basic geometry constructors, `pi`, `e`, `true`, `false`, primitive types, `Option`, `List`, `Set`, `Map`, `Range`.
