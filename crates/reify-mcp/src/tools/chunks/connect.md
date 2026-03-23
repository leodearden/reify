# Connect and Chain

## Connect Statement

`connect` creates connections between ports, generating constraints and optional connector instances.

```
connect motor.shaft -> coupling.driver
connect coupling.driven -> gearbox.input : SplineConnection { tooth_count = 24 }
connect plate_a.face <-> plate_b.face : ButtWeld
```

- `->` indicates directed connection
- `<->` for bidirectional connections
- Optional connector type after `:` with parameters in `{}`

## Semantic Decomposition

A `connect` statement desugars into:
1. **Connector structure instance** (if connector type specified)
2. **Port compatibility constraints** (trait matching, direction checking)
3. **Connector-port binding constraints**
4. **Frame alignment constraints** (when ports are geometrically located)
5. **Topology edge** in the assembly graph

## Connector Parameterization

```
connect housing.bore -> shaft.journal : ShrinkFit {
    interference = 0.02mm
    assembly_temperature_delta = 150degC
}
```

## Port Mapping

```
connect motor.nema17 -> adapter.side_a {
    shaft -> input_bore
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

## Ad-hoc Connections

```
connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive
connect pipe@region(outer_surface, z = 0mm..50mm) -> clamp@region(inner_surface)
```

The `@` operator creates ad-hoc ports by designating geometric regions.

## Chain Statement

Sugar for connecting sequential occurrences via default ports:
```
chain casting -> machining -> heat_treat -> finishing
// Desugars to:
connect casting.default_out -> machining.default_in
connect machining.default_out -> heat_treat.default_in
connect heat_treat.default_out -> finishing.default_in
```
