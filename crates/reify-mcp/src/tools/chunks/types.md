# Type System

## Primitive Types

| Type | Description |
|------|-------------|
| `Bool` | Predicates, flags, gating |
| `Int` | Counts, indices, discrete quantities |
| `Real` | Dimensionless real number (= `Scalar<Dimensionless>`) |
| `String` | Names, labels, descriptions |

`Int` promotes to `Real` implicitly. `Real` does NOT promote to `Int`.

## Type Parameters

```
structure def FlexibleCoupling<DriverPort: RotaryPort, DrivenPort: RotaryPort> {
    param max_torque : Torque
}
```

Bounds: trait bounds (`T: SomeTrait`), kind bounds (`N: Nat`), composite (`T: TraitA + TraitB`).

Defaults: `structure def Fastener<HeadStyle: HeadType = Hex> { ... }`

`auto` for type parameters: `sub bearing1 : Bearing<auto: Seal> { bore_diameter = 25mm }`

## Type Inference

Conservative — infer type parameters when unambiguous. Never infer value parameters (determinacy model handles that).

## Function Types

```
Point3<Length> -> Scalar<Temperature>    // Spatial temperature field
(Length, Length) -> Bool                  // Binary predicate
```

## Type Aliases

```
type Pressure = Force / Area
type StressTensor = Tensor<2, 3, Pressure>
type Point3<Q> = Point<3, Q>
```

Transparent — `Pressure` and `Force / Area` are the same type.

## Determinacy and Types

Determinacy is tracked orthogonally, not baked into types. Parameter types are plain `Length`, `Force`, etc. Determinacy (`undef`/constrained/`auto`/determined) is a separate property tracked by the design system.

## Limited Dependent Typing

`Int` and `Bool` value parameters can appear in type-level positions (collection sizes, conditional presence gating, array dimensions).
