# Function Declarations

Functions (`fn`) are non-entity declarations: no identity, no determinacy state. They are pure computations.

## Syntax

```
fn von_mises(t : Tensor<2, 3, Pressure>) -> Scalar<Pressure> {
    let dx = t.xx - t.yy
    let dy = t.yy - t.zz
    let dz = t.zz - t.xx
    sqrt(0.5 * (dx^2 + dy^2 + dz^2))
}

fn clamp(x : Real, lo : Real, hi : Real) -> Real {
    if x < lo then lo else if x > hi then hi else x
}
```

## Properties

- **Pure** — no side effects, no state
- **Block body** — `{ }` with `let` bindings and a final expression (return value)
- **No `return` keyword** — last expression is the return value
- **Type annotations mandatory** on parameters and return type
- **Can be `pub`** for cross-module reuse
- **Type parameters supported:** `fn distance<Q: Dimension>(a: Point3<Q>, b: Point3<Q>) -> Scalar<Q>`
- **Recursion permitted** (infinite recursion is a runtime error)

## Overloading

Function overloading by parameter types IS permitted:
```
fn area(surface: Surface) -> Scalar<Area> { ... }
fn area(solid: Solid) -> Scalar<Area> { ... }

fn rotate<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G { ... }
fn rotate<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G { ... }
```

Exactly one candidate must match at each call site.

## Lambda Expressions

```
|x| x * 2
|p : Point3<Length>| distance(p, origin)
```

Lambdas are anonymous functions. Parameter types can be inferred from context.
