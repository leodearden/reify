# Function Declarations

Functions (`fn`) are non-entity declarations: no identity, no determinacy state. They are pure computations.

## Syntax

```reify
fn hoop_stress(p : Scalar<Pressure>, r : Length, t : Length) -> Scalar<Pressure> {
    let ratio = r / t;
    p * ratio
}

fn friction_factor(re : Real) -> Real {
    if re < 2300 then 64 / re else 0.316 / re^0.25
}

fn as_percent(fraction : Real) -> Real = fraction * 100
```

## Properties

- **Pure** — no side effects, no state
- **Block body** — `{ }` with `let` bindings, each terminated by `;`, and a final expression (the return value); a body with no bindings may instead be written `= expr` (as `as_percent` shows)
- **No `return` keyword** — last expression is the return value
- **Type annotations mandatory** on parameters and return type
- **Can be `pub`** for cross-module reuse
- **Type parameters supported:** `fn gap<Q: Dimension>(a: Point3<Q>, b: Point3<Q>) -> Scalar<Q>`
- **Recursion permitted** (infinite recursion is a runtime error)

## Overloading

Function overloading by parameter types IS permitted:
```reify-schematic
fn footprint(surface: Surface) -> Scalar<Area> { ... }
fn footprint(solid: Solid) -> Scalar<Area> { ... }

fn align<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G { ... }
fn align<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G { ... }
```

Exactly one candidate must match at each call site.

## Lambda Expressions

```reify-fragment
|x| x * 2
|p : Point3<Length>| distance(p, origin)
```

Lambdas are anonymous functions. Parameter types can be inferred from context.
