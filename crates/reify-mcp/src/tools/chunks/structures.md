# Structure Declarations

Structures are the primary entity kind in Reify. They compose spatially via containment of sub-structures. No Part/Assembly distinction — a structure containing sub-structures is a composite structure.

## Syntax

```
structure def Bracket<M: Material> : Rigid {
    param thickness : Length
    param width : Length = 50mm
    param material : M

    port mount_face : MechanicalPort {
        direction = in
        frame = Frame3 { origin = point3(0mm, 0mm, 0mm) }
    }

    sub rib : Rib { height = thickness * 0.8 }

    // Illustrative PARAMETER arithmetic, showing what a `let` member and a
    // field access on a type param look like. It is NOT a measurement of the
    // realized part — see the note under this example.
    let volume = thickness * width * width
    let mass = volume * material.density

    constraint thickness > 1mm
    constraint thickness < width / 2
}
```

> **The `volume` above is arithmetic, not a measurement.** `thickness * width * width` derives a
> number from the parameters and never sees the realized solid, so it silently stops describing the
> part the moment a fillet, a shell, a boolean or a pattern changes it — and nothing flags the
> divergence. To measure realized geometry, ask the kernel: `volume(solid)` and `centroid(solid)`
> for the geometric quantities, `center_of_mass(solid, density)` for the density-weighted one. The
> `geometry` chunk's "Measurement & Mass-Property Queries" section documents the whole family,
> including the let-bind-the-operand rule those calls require.

> **Note:** `def` is optional. Bare `structure Bracket { ... }` (omitting `def`) is a silently-accepted, equal-status alias — the grammar parses both forms identically. This document uses the canonical `def` spelling, but existing code may use either form.

## Key Properties

- Structures are immutable within the design system
- Compose spatially (containment of sub-structures)
- Type parameters in angle brackets: `<M: Material>`
- Trait conformance after colon: `: Rigid`
- Members: `param`, `port`, `sub`, `let`, `constraint`, `type`, `meta`

## Instantiation

Sub-structures are instantiated with `sub`:
```
sub motor : ElectricMotor { shaft_diameter = 8mm }
sub vents : List<Vent>
```

Parameters can be set in the curly-brace block. Omitted parameters get their default or remain `undef`.

## Meta Blocks

```
structure def Bracket : Rigid {
    meta {
        description = "L-shaped mounting bracket"
        part_number = "BRK-2024-001"
    }
    // ... members
}
```

Metadata is informational only — no constraint participation.
