# Occurrence Declarations

Occurrences represent processes or transformations that act on structures. They consume input structures and produce output structures.

## Syntax

```
occurrence def Welding : Joining {
    param method : WeldMethod
    param filler : Material = auto

    port workpiece_a : in StructurePort
    port workpiece_b : in StructurePort
    port result : out StructurePort

    param current : Current
    param voltage : Voltage
    param travel_speed : Velocity

    let heat_input : Energy / Length = (current * voltage) / travel_speed

    constraint heat_input < workpiece_a.material.max_heat_input
}
```

## Key Properties

- `in`/`out` on ports express flow direction
- Compose sequentially via `connect` on occurrence ports
- Can be chained: `chain casting -> machining -> heat_treat -> finishing`
- Same member kinds as structures: `param`, `port`, `sub`, `let`, `constraint`

## Port Directions

- `in` — input (consumes a structure)
- `out` — output (produces a structure)
- Ports without direction are bidirectional

## Use in Manufacturing Chains

```
occurrence def Machining : Subtractive {
    port blank : in StructurePort
    port finished : out StructurePort

    param tool_diameter : Length
    param feed_rate : Velocity
    param spindle_speed : AngularVelocity
}

// Chain occurrences together
chain casting -> machining -> heat_treat -> finishing
```
