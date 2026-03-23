# Field Declarations

Fields are first-class entities representing spatially-varying quantities. They have a domain → codomain type signature and a source.

## Type

```
Field<D, C>    // D = domain type, C = codomain type
```

Examples:
```
Field<Point3<Length>, Scalar<Temperature>>      // Temperature distribution
Field<Point3<Length>, Vector3<Force>>            // Force field
Field<Real, Scalar<Length>>                      // 1D profile
Field<Point3<Length>, Tensor<2, 3, Pressure>>   // Stress tensor field
```

Composition is type-safe: `Field<A,B>` composed with `Field<B,C>` yields `Field<A,C>`.

## Source Kinds

```
field def temperature_distribution : Point3<Length> -> Scalar<Temperature> {
    source = analytical {
        |p| 300K + 50K * exp(-distance(p, heat_source) / 10mm)
    }
}

field def material_density : Point3<Length> -> Scalar<Density> {
    source = sampled {
        grid = RegularGrid3 { spacing = 0.5mm, bounds = part.bounding_box }
        interpolation = trilinear
        data = import("density_field.vdb")
    }
}
```

| Source kind | Meaning |
|------------|---------|
| `analytical` | Closed-form expression (lambda) |
| `sampled` | Discrete samples with interpolation |
| `composed` | Combination of other fields |
| `imported` | External data file (OpenVDB, CSV, HDF5) |

## Standard Library Field Functions

- `constant_field(value)` — uniform field
- `fn_field(lambda)` — field from function
- `from_samples(grid, data, interpolation)` — from discrete data
- `compose(f, g)` — function composition
- `sample(field, point)` — evaluate at a point
- `restrict(field, region)` — limit domain
