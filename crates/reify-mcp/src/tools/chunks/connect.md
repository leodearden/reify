# Connect and Chain

## Connect Statement

`connect` creates connections between ports, generating constraints and optional connector instances.

```reify-fragment
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

```reify-fragment
connect housing.bore -> shaft.journal : ShrinkFit {
    interference = 0.02mm
    assembly_temperature_delta = 150degC
}
```

## Port Mapping

```reify-fragment
connect motor.nema17 -> adapter.side_a {
    shaft -> input_bore
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

## Ad-hoc Connections

```reify-fragment
connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive
connect pipe@region(outer_surface, z = 0mm..50mm) -> clamp@region(inner_surface)
```

The `@` operator creates ad-hoc ports by designating geometric regions.

## Chain Statement

Sugar for connecting sequential occurrences. Each element contributes a port
usable as `out` where it sources a hop and one usable as `in` where it receives
one; candidates are tiered, so ports declared in the needed direction win and an
element declaring none in that direction falls back to its `bidi` ports. Given
`occurrence def Step { port stock : in Workpiece  port part : out Workpiece }`:
```reify-fragment
chain casting -> machining -> heat_treat -> finishing
// Desugars to:
connect casting.part -> machining.stock
connect machining.part -> heat_treat.stock
connect heat_treat.part -> finishing.stock
```

An element offering several candidate ports for its role — or none — is a
compile error. Name the port on that element instead (`chain casting.part ->
machining`); any element may be dotted, but a named port is used verbatim in
both of that element's roles, so naming one on an interior element pins it for
the hop arriving and the hop leaving alike. An element must denote exactly one
occurrence: naming a `List<T>`/`Keyed<T>` sub without an indexer is an error —
index it (`vents[0]`) or chain its occurrences with `forall v in vents: chain v
-> hub`. The same inference applies inside a `forall … : chain …` body.
