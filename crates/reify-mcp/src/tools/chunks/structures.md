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

    let volume = thickness * width * width
    let mass = volume * material.density

    constraint thickness > 1mm
    constraint thickness < width / 2
}
```

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
