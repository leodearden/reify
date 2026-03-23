# Geometry Types

## Algebraic Types

**Point/Vector distinction (affine space):**
- `Point - Point → Vector` (valid)
- `Point + Vector → Point` (valid)
- `Vector + Vector → Vector` (valid)
- `Point + Point` → type error

Parameterized by dimensionality and quantity:
```
Point<N: Nat, Q: Dimension>     // Position
Vector<N: Nat, Q: Dimension>    // Displacement
Scalar<Q: Dimension>            // Dimensioned number
Tensor<Rank: Nat, N: Nat, Q: Dimension>
Matrix<M: Nat, N: Nat, Q: Dimension>
```

Common aliases: `Point3<Q>`, `Vector3<Q>`, `Point2<Q>`, `Vector2<Q>`

## Opaque Geometry Types

Core geometric entity types are opaque handles — work through operations.

| Type | Description |
|------|-------------|
| `Solid` | Closed region of 3D space |
| `Shell` | Connected set of faces |
| `Surface` | 2D manifold in 3D space |
| `Curve` | 1D manifold in 2D/3D |
| `PointCloud` | Unordered point collection |

Geometric traits: `Closed`, `Manifold`, `Orientable`, `Convex`, `Connected`, `Bounded`, `Watertight`

## Orientation & Transform

```
Orientation.from_quaternion(w, x, y, z)
Orientation.from_axis_angle(axis, angle)
Orientation.from_euler(convention, a, b, c)

Frame<N>:
    origin : Point<N, Length>
    basis  : Orientation<N>

Transform<N>:
    rotation    : Orientation<N>
    translation : Vector<N, Length>
```

Transform is always rigid (rotation + translation). Sub-structure placement uses Transform from child frame to parent frame.

## Geometry Constructors (Prelude)

```
point2(x, y)          point3(x, y, z)
vec2(x, y)            vec3(x, y, z)
line(start, end)      arc(center, radius, start_angle, end_angle)
circle(center, radius)
polygon(points)       rectangle(width, height)
```
