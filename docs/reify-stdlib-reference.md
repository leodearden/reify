# Reify Standard Library Reference

**Version:** 0.1
**Date:** 2026-03-13
**Status:** Draft
**Companion to:** Reify Language Specification v0.1

---

This document is the complete API reference for the Reify standard library. It is a companion to the Reify Language Specification, which defines the core language. The prelude (automatically imported into every module) is defined in the language specification (Section 7.6). This document covers all `std.*` modules beyond the prelude.

Many standard-library traits (e.g. `Sealed`, `MaterialSpec`, geometry traits in §3.10) appear as bounds on `auto:` type parameters. For the resolution algorithm — per-parameter BFS, cap of 10 candidates, lexicographic tiebreak by FQN, and deferral of cross-parameter backtracking to v0.2 — see `docs/auto-type-param-resolution.md`.

---

## Module Overview

```
std
  prelude
  math
    numeric
    trig
    linalg
    complex
  units
    dimensions
    si
    imperial
    constants
  geometry
    constructors
    primitive
    compound
    boolean
    modify
    sweep
    transform
    pattern
    query
    traits
  structural
    traits
  ports
    mod.ri
    mechanical
    electrical
    thermal
    fluid
  materials
    mod.ri
    mechanical
    thermal
    electrical
    optical
    chemical
  tolerancing
    dimensional
    geometric
    surface
  process
    mod.ri         // Process trait, categories
    dfm            // DFMRule trait
  io
    mod.ri         // Source, Sink, Input, Buy, Output, Discard
    formats        // STEP, STL, 3MF, Display, PointCloud
  analysis
    mod.ri         // Analysis trait
    stress         // von_mises, principal_stresses, safety_factor
    result         // AnalysisResult trait
  fields
    mod.ri         // Field<D,C>
    interpolation  // constant_field, fn_field, from_samples
    spatial        // compose, sample, restrict, differential operators
  determinacy
    mod.ri         // determined(), constrained(), undetermined()
    purposes       // design_review, simulation_ready
  mechanism
    joints       // Prismatic, Revolute, Coupling, transform_at
    builder      // mechanism(), .body() chaining, closed-chain detection
    snapshot     // snapshot(), bodies(), transform_of(), center_of_mass(), bounding_box()
    sweep        // sweep(), sweep_grid()
    query        // interferes(), interferes_with(), min_clearance()
```

---

## 1. `std.math`

### 1.1 `std.math.numeric`

```
fn abs<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>
fn min<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Scalar<Q>
fn max<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Scalar<Q>
fn clamp<Q: Dimension>(x: Scalar<Q>, lo: Scalar<Q>, hi: Scalar<Q>) -> Scalar<Q>
fn lerp<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>, t: Real) -> Scalar<Q>
fn remap(x: Real, from_lo: Real, from_hi: Real, to_lo: Real, to_hi: Real) -> Real
fn sqrt<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q^(1/2)>   // Compiler signature (math_signatures.rs) + eval-time DimensionVector::root(2) (numeric.rs)
fn pow(base: Real, exp: Real) -> Real                      // Dimensionless only; use the `^` operator for dimensioned integer powers
fn log(x: Real) -> Real
fn log10(x: Real) -> Real
fn exp(x: Real) -> Real
fn sign<Q: Dimension>(x: Scalar<Q>) -> Real
fn floor(x: Real) -> Int
fn ceil(x: Real) -> Int
fn round(x: Real) -> Int
fn mod(x: Int, y: Int) -> Int
```

**Dimensional `sqrt`:** `sqrt` propagates dimension via a compiler signature wired in `math_signatures.rs` (halves each exponent to produce `Q^(1/2)`) plus eval-time `DimensionVector::root(2)` in `numeric.rs` — it is not a free-standing compiler intrinsic. `pow` is dimensionless-only (it always returns `Real`; see `numeric.rs:88`). Dimensioned integer powers are handled by the `^` operator (`eval_pow` in `reify-expr`, tasks 3805/4106), not by `pow`. `pow` with non-integer exponents is restricted to dimensionless in v0.1.

### 1.2 `std.math.trig`

```
fn sin(x: Angle) -> Real
fn cos(x: Angle) -> Real
fn tan(x: Angle) -> Real
fn asin(x: Real) -> Angle
fn acos(x: Real) -> Angle
fn atan(x: Real) -> Angle
fn atan2<Q: Dimension>(y: Scalar<Q>, x: Scalar<Q>) -> Angle
fn sinh(x: Real) -> Real
fn cosh(x: Real) -> Real
fn tanh(x: Real) -> Real
```

Trig functions take `Angle` (not `Real`), enforcing the 8th-dimension distinction.

### 1.3 `std.math.linalg`

```
fn dot<N: Nat, Q1: Dimension, Q2: Dimension>(a: Vector<N,Q1>, b: Vector<N,Q2>) -> Scalar<Q1*Q2>
fn cross<Q1: Dimension, Q2: Dimension>(a: Vector<3,Q1>, b: Vector<3,Q2>) -> Vector<3, Q1*Q2>
fn normalize<N: Nat, Q: Dimension>(v: Vector<N,Q>) -> Vector<N, Dimensionless>
fn magnitude<N: Nat, Q: Dimension>(v: Vector<N,Q>) -> Scalar<Q>
fn determinant<N: Nat, Q: Dimension>(m: Matrix<N,N,Q>) -> Scalar<Q^N>
                                                  // any N via dense LA (nalgebra); singular → det ≈ 0
fn inverse<N: Nat, Q: Dimension>(m: Matrix<N,N,Q>) -> Matrix<N,N,Q^(-1)>
                                                  // any N via dense LA (nalgebra); singular → Undef
fn transpose<M: Nat, N: Nat, Q: Dimension>(m: Matrix<M,N,Q>) -> Matrix<N,M,Q>
fn outer<N: Nat, M: Nat, Q1: Dimension, Q2: Dimension>(a: Vector<N,Q1>, b: Vector<M,Q2>) -> Matrix<N,M,Q1*Q2>
fn trace<N: Nat, Q: Dimension>(m: Matrix<N,N,Q>) -> Scalar<Q>
fn eigenvalues<N: Nat, Q: Dimension>(m: Matrix<N,N,Q>) -> List<Scalar<Q>>
                                                  // real spectrum only; returns Undef when any eigenvalue is non-real (documented, not silent)

// Added in v0.6 (task γ)
fn complex_eigenvalues<N: Nat, Q: Dimension>(m: Matrix<N,N,Q>) -> List<Complex<Q>>
                                                  // general complex spectrum, any N (matrix.rs:248-287)

// Added in v0.6 (task α) — construction
fn vec<N: Nat, Q: Dimension>(list: List<Scalar<Q>>) -> Vector<N,Q>
                                                  // N inferred from list length
fn matrix<M: Nat, N: Nat, Q: Dimension>(rows: List<List<Scalar<Q>>>) -> Tensor<2,M,N,Q>
                                                  // rank-2 (M rows × N cols) only; construct.rs:40-43
fn diag<N: Nat, Q: Dimension>(list: List<Scalar<Q>>) -> Tensor<2,N,N,Q>
                                                  // N×N diagonal matrix; N inferred from list length
fn identity(n: Int) -> Tensor<2,N,N,Dimensionless>  // N = n (runtime value)
                                                  // N×N identity matrix
```

`determinant` and `inverse` evaluate for any N via dense linear algebra (nalgebra, task β); they are not limited to 2×2 or 3×3. `eigenvalues` returns the real spectrum `List<Scalar<Q>>` for symmetric or near-real matrices; if any eigenvalue has a non-negligible imaginary part the function returns `Undef` (not a silent projection). Use `complex_eigenvalues` (v0.6) for the full complex spectrum.

`vec`, `matrix`, `diag`, and `identity` (v0.6, task α) build `Vector`/`Tensor` values from list literals (see `construct.rs`). `matrix` is rank-2 only (M×N); use `vec` for rank-1 vectors. Type signatures follow the frozen contract in PRD §3.

### 1.4 `std.math.complex`

```
Complex<Q: Dimension>:
    re : Scalar<Q>
    im : Scalar<Q>

fn complex<Q>(re: Scalar<Q>, im: Scalar<Q>) -> Complex<Q>
fn real<Q>(c: Complex<Q>) -> Scalar<Q>
fn imag<Q>(c: Complex<Q>) -> Scalar<Q>
fn conjugate<Q>(c: Complex<Q>) -> Complex<Q>
fn complex_magnitude<Q>(c: Complex<Q>) -> Scalar<Q>
fn phase<Q>(c: Complex<Q>) -> Angle

// Added in v0.6 (task ζ)
fn abs<Q>(c: Complex<Q>) -> Scalar<Q>           // modulus; alias of complex_magnitude
                                                  // returns Real when Q is Dimensionless
fn arg<Q>(c: Complex<Q>) -> Angle               // argument (phase angle); alias of phase
fn complex_div<Q1,Q2>(a: Complex<Q1>, b: Complex<Q2>) -> Complex<Q1/Q2>
                                                  // also the `/` operator on Complex values
fn complex_exp(c: Complex<Dimensionless>) -> Complex<Dimensionless>
                                                  // dimensionless only; returns Undef on dimensioned input
fn complex_sqrt(c: Complex<Dimensionless>) -> Complex<Dimensionless>
                                                  // principal root; dimensionless only
fn complex_pow<Q>(c: Complex<Q>, n: Int) -> Complex<Q^n>
                                                  // integer exponent; any dimension
```

**Imaginary-literal sugar.** `4.1j` and `3 + 4j` desugar to a DIMENSIONLESS `Complex`
(spec D2/D6). Dimensioned complex values are built via the `complex(re, im)` constructor,
e.g. `complex(50ohm, -30ohm)` → `Complex<Resistance>`.

The spec names for `abs` and `arg` are *modulus* and *argument* respectively.
`std.math.complex` is a documentation module — there is no `complex.ri` file to import.

---

## 2. `std.units`

### 2.1 `std.units.dimensions`

34 named dimension aliases (see Section 3.2).

### 2.2 `std.units.si`

Complete SI base units (`m`, `kg`, `s`, `A`, `K`, `rad`, `mol`, `cd`) with all SI prefixes (quecto through quetta). Derived units include: `N`, `J`, `W`, `Pa`, `V`, `ohm`, `S`, `F`, `H`, `Wb`, `T`, `Hz`, `rpm`, `rad_per_s`, `Pa_s`, `lm`, `lx`, `Bq`, `Gy`, `Sv`, `eV`, `bar`, `mbar`, with all common prefixes. The bare, un-prefixed `A`, `mol`, and `cd` resolve as unit literals (e.g. `1A`, `1mol`, `1cd`); previously only their prefixed forms (`mA`/`kA`, `mmol`, `mcd`) were available.

Temperature offset: `degC : Temperature offset 273.15K`

### 2.3 `std.units.imperial`

Minimal set: `in` (= 25.4mm), `ft`, `thou`, `yd`, `lb`, `oz`, `lbf`, `psi`, `ksi`, `degF`, `fl_oz`, `gal`.

### 2.4 `std.units.constants`

**Dimensionless math constants** — compiler builtins, bare identifiers of type `Real` (usable directly in expressions, e.g. `2 * pi`, `tau`, `e`):

```
pi  : Real  -- ≈ 3.141592653589793  (std::f64::consts::PI)
tau : Real  -- ≈ 6.283185307179586  (std::f64::consts::TAU; 2·π)
e   : Real  -- ≈ 2.718281828459045  (std::f64::consts::E; Euler's number)
```

**Dimensionful physical constants** — zero-arg functions; the return type carries the dimension:

```
fn STANDARD_GRAVITY()          -> Acceleration       -- 9.80665 m/s²             (BIPM/CGPM 1901)
fn SPEED_OF_LIGHT()            -> Velocity           -- 299792458 m/s             (SI exact, 1983)
fn BOLTZMANN_CONSTANT()        -> HeatCapacity       -- 1.380649e-23 J/K          (2019 SI exact)
fn AVOGADRO_CONSTANT()         -> InverseAmount      -- 6.02214076e23 mol⁻¹       (2019 SI exact)
fn PLANCK_CONSTANT()           -> Action             -- 6.62607015e-34 J·s        (2019 SI exact)
fn STEFAN_BOLTZMANN_CONSTANT() -> StefanBoltzmannDim -- 5.670374419e-8 W·m⁻²·K⁻⁴ (CODATA 2018)
fn VACUUM_PERMITTIVITY()       -> Permittivity       -- 8.8541878128e-12 F/m      (CODATA 2018)
fn VACUUM_PERMEABILITY()       -> Permeability       -- 1.25663706212e-6 H/m      (CODATA 2018)
fn MOLAR_GAS_CONSTANT()        -> MolarGasConstant   -- 8.314462618 J·mol⁻¹·K⁻¹  (2019 SI exact; R = N_A·k_B)
fn ELEMENTARY_CHARGE()         -> Charge             -- 1.602176634e-19 C         (2019 SI exact)
```

Reify has no top-level `const`; dimensionful values carry their dimension via the zero-arg function's return type, while dimensionless math constants (`pi`, `tau`, `e`) are bare compiler builtins.

---

## 3. `std.geometry`

### 3.1 `std.geometry.constructors` (in prelude)

```
fn point2<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>) -> Point2<Q>
fn point3<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>, z: Scalar<Q>) -> Point3<Q>
fn vec2<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>) -> Vector2<Q>
fn vec3<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>, z: Scalar<Q>) -> Vector3<Q>

fn orient_axis_angle(axis: Vector3<Dimensionless>, angle: Angle) -> Orientation<3>
fn orient_quaternion(w: Real, x: Real, y: Real, z: Real) -> Orientation<3>
fn orient_euler(convention: EulerConvention, a: Angle, b: Angle, c: Angle) -> Orientation<3>
fn orient_basis(x_axis: Vector3<Dimensionless>, y_axis: Vector3<Dimensionless>, z_axis: Vector3<Dimensionless>) -> Orientation<3>
fn orient_look_at(forward: Vector3<Dimensionless>, up: Vector3<Dimensionless>) -> Orientation<3>
let orient_identity : Orientation<3>

fn frame3(origin: Point3<Length>, basis: Orientation<3>) -> Frame<3>
let frame3_identity : Frame<3>
fn transform3(rotation: Orientation<3>, translation: Vector3<Length>) -> Transform<3>
let transform3_identity : Transform<3>

fn project(point: Point3<Length>, to: Frame<3>) -> Point3<Length>
fn project(vector: Vector3<Length>, to: Frame<3>) -> Vector3<Length>

enum EulerConvention { XYZ, XZY, YXZ, YZX, ZXY, ZYX }
```

#### SO(3) and SE(3) operations (v0.2)

Added to support the closed-chain kinematic loop-closure solver — see
[`v0_2/kinematic-constraints.md`](prds/v0_2/kinematic-constraints.md). All
operations validate inputs and return `Undef` on shape mismatch, wrong
argument count, dimensional mismatch, or non-finite components.

```
// SO(3) — quaternion algebra on Orientation<3>
fn orient_compose(a: Orientation<3>, b: Orientation<3>) -> Orientation<3>
fn orient_inverse(q: Orientation<3>) -> Orientation<3>
fn orient_log(q: Orientation<3>) -> Vector3<Dimensionless>           // axis * angle (rotation vector)
fn orient_exp(rot_vec: Vector3<Dimensionless>) -> Orientation<3>     // inverse of orient_log
fn orient_slerp(a: Orientation<3>, b: Orientation<3>, t: Real) -> Orientation<3>
fn orient_to_axis_angle(q: Orientation<3>) -> Map { axis: Vector3<Dimensionless>, angle: Angle }
fn orient_to_euler(convention: EulerConvention, q: Orientation<3>) -> List<Angle>  // 3 elements

// SE(3) — rigid-body transforms on Transform<3>
fn transform_compose(a: Transform<3>, b: Transform<3>) -> Transform<3>   // bit-equal to a * b
fn transform_inverse(t: Transform<3>) -> Transform<3>
fn transform_log(t: Transform<3>) -> Twist
fn transform_exp(twist: Twist) -> Transform<3>                            // inverse of transform_log
```

`orient_slerp` uses the canonical shortest-path slerp (antipodal flip if
`dot(a, b) < 0`) with a linear-interpolation fallback near `theta = 0`.
`orient_compose` and `transform_compose` are the named-function spellings of
the `Orientation * Orientation` and `Transform * Transform` operators
respectively, and produce bit-identical results to the operator path.

**Twist representation.** SE(3) twists are encoded as a `Map` keyed by
`"angular"` (a `Vector3<Dimensionless>` holding `axis * angle` in radians) and
`"linear"` (a `Vector3<Length>` holding the translational component):

```
type Twist = Map { angular: Vector3<Dimensionless>, linear: Vector3<Length> }
```

A `Map` shape (rather than a 6-component `Vector`) is required because
`Vector` enforces a single shared dimension across components; a twist mixes
dimensionless rotation and `Length` translation. The same `Map` shape is
returned by `joint_jacobian` (§13.1) so that solver code can compose twists
and Jacobian columns uniformly.

**Linear-component dimension convention.** `transform_log` preserves the
input `Transform`'s translation dimension on `linear` verbatim, and
`transform_exp` accepts `linear` with the same polymorphic policy:

| `linear` dimension | accepted? | notes                                   |
|--------------------|-----------|-----------------------------------------|
| `Length`           | ✓         | canonical — matches the `Twist` type    |
| `Dimensionless`    | ✓         | unit-less twists / numerical work       |
| `Angle`, `Mass`, … | ✗         | rejected as `Undef`                     |

The pair `transform_log` ↔ `transform_exp` round-trips exactly under both
policies, so a `Transform` whose translation is `Dimensionless` will round-trip
through a `Dimensionless` linear, and likewise for `Length`. `joint_jacobian`
always emits `Dimensionless` on both `angular` and `linear` because joint
parameters are unit-less in the joint's local frame.

### 3.2 `std.geometry.primitive`

**3D solids:**

```
fn box(width: Length, depth: Length, height: Length) -> Solid
fn box_centered(width: Length, depth: Length, height: Length) -> Solid
fn cylinder(radius: Length, height: Length) -> Solid
fn cylinder_centered(radius: Length, height: Length) -> Solid
fn cone(bottom_radius: Length, top_radius: Length, height: Length) -> Solid
fn sphere(radius: Length) -> Solid
fn torus(major_radius: Length, minor_radius: Length) -> Solid
fn wedge(width: Length, depth: Length, height: Length, top_width: Length) -> Solid

// Planned — not yet implemented; see tracking task 3465 / PRD docs/prds/geometry-primitive-constructors.md
// fn half_space(plane: Plane) -> Solid     // Unbounded -- Solid no longer implies Bounded

// Planned — not yet implemented (Bounded=false producer, tracked by task 3466);
// see PRD docs/prds/geometry-primitive-constructors.md §"Out of scope"
// fn extrude_infinite(profile: Surface, direction: Vector3<Length>) -> Solid
```

**2D shapes:**

```
fn rectangle(width: Length, height: Length) -> Surface
fn circle(radius: Length) -> Surface
fn polygon(vertices: List<Point2<Length>>) -> Surface
fn ellipse(semi_major: Length, semi_minor: Length) -> Surface
```

**Curves:**

```
fn line_segment<N: Nat>(start: Point<N,Length>, end: Point<N,Length>) -> Curve
fn arc(center: Point3<Length>, radius: Length, start_angle: Angle, end_angle: Angle) -> Curve
fn helix(radius: Length, pitch: Length, height: Length) -> Curve
fn interp<N: Nat>(points: List<Point<N,Length>>) -> Curve
fn bezier<N: Nat>(control_points: List<Point<N,Length>>) -> Curve
fn nurbs<N: Nat>(control_points: List<Point<N,Length>>, weights: List<Real>, knots: List<Real>, degree: Int) -> Curve

// Planned — not yet implemented; standalone feature; see PRD docs/prds/geometry-primitive-constructors.md
// fn nurbs_surface(/* NURBS surface parameters */) -> Surface
```

### 3.3 `std.geometry.compound`

```
fn tube(outer_radius: Length, inner_radius: Length, height: Length) -> Solid
fn pipe(path: Curve, radius: Length) -> Solid
```

### 3.4 `std.geometry.boolean`

```
fn union(a: Solid, b: Solid) -> Solid
fn union_all(solids: List<Solid>) -> Solid
fn intersection(a: Solid, b: Solid) -> Solid
fn intersection_all(solids: List<Solid>) -> Solid
fn difference(a: Solid, b: Solid) -> Solid
// Same for Surface (no _all variants)
fn union(a: Surface, b: Surface) -> Surface
fn intersection(a: Surface, b: Surface) -> Surface
fn difference(a: Surface, b: Surface) -> Surface
```

### 3.5 `std.geometry.modify`

```
fn fillet(solid: Solid, edges: List<Curve>, radius: Length) -> Solid
fn fillet_all(solid: Solid, radius: Length) -> Solid
fn chamfer(solid: Solid, edges: List<Curve>, distance: Length) -> Solid
fn chamfer_asymmetric(solid: Solid, edges: List<Curve>, distance1: Length, distance2: Length) -> Solid
fn shell(solid: Solid, thickness: Length) -> Solid
fn shell_open(solid: Solid, thickness: Length, open_faces: List<Surface>) -> Solid
fn offset_solid(solid: Solid, distance: Length) -> Solid
fn offset_surface(surface: Surface, distance: Length) -> Surface
fn offset_curve(curve: Curve, distance: Length) -> Curve             // 2D unambiguous
fn offset_curve(curve: Curve, distance: Length, reference: Surface) -> Curve  // 3D with reference
fn offset_curve(curve: Curve, distance: Length, direction: Vector3<Dimensionless>) -> Curve  // 3D with direction
fn draft(solid: Solid, faces: List<Surface>, angle: Angle, neutral_plane: Plane) -> Solid
fn split(solid: Solid, tool: Plane) -> List<Solid>                   // + multiple overloads
fn thicken(surface: Surface, thickness: Length) -> Solid
fn thicken_asymmetric(surface: Surface, thickness_above: Length, thickness_below: Length) -> Solid
```

### 3.6 `std.geometry.sweep`

```
fn extrude(profile: Surface, distance: Length) -> Solid
fn extrude_to(profile: Surface, target: Surface) -> Solid
fn extrude_symmetric(profile: Surface, distance: Length) -> Solid
fn revolve(profile: Surface, axis: Axis, angle: Angle) -> Solid
fn revolve_full(profile: Surface, axis: Axis) -> Solid
fn sweep(profile: Surface, path: Curve) -> Solid
fn sweep_guided(profile: Surface, path: Curve, guide: Curve) -> Solid
fn loft(profiles: List<Surface>) -> Solid
fn loft_guided(profiles: List<Surface>, guides: List<Curve>) -> Solid
```

### 3.7 `std.geometry.transform`

```
fn translate<G: Transformable>(geometry: G, displacement: Vector3<Length>) -> G
fn rotate<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G
fn rotate<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G
fn rotate_around<G: Transformable>(geometry: G, point: Point3<Length>, axis: Vector3<Dimensionless>, angle: Angle) -> G
fn scale<G: Transformable>(geometry: G, factor: Real) -> G              // Uniform
fn scale<G: Transformable>(geometry: G, factors: Vector3<Real>) -> G    // Per-axis (non-rigid)
fn apply_transform<G: Transformable>(geometry: G, transform: Transform<3>) -> G
```

Note: `scale` is non-rigid -- does not compose with `Transform<3>`.

### 3.8 `std.geometry.pattern`

```
fn mirror<G: Transformable>(geometry: G, plane: Plane) -> G
fn linear_pattern<G: Transformable>(geometry: G, direction: Vector3<Length>, count: Int, spacing: Length) -> List<G>
fn circular_pattern<G: Transformable>(geometry: G, axis: Axis, count: Int, angle: Angle) -> List<G>
fn linear_pattern_2d<G: Transformable>(geometry: G, dir1: Vector3<Length>, count1: Int, spacing1: Length, dir2: Vector3<Length>, count2: Int, spacing2: Length) -> List<G>
fn arbitrary_pattern<G: Transformable>(geometry: G, transforms: List<Transform<3>>) -> List<G>
```

Patterns return `List` for per-instance constraints; compose with `union_all` for merged solid.

### 3.9 `std.geometry.query`

**Distance and containment:**

```
fn distance<G1: Geometry, G2: Geometry>(a: G1, b: G2) -> Scalar<Length>
fn closest_point<G: Geometry>(point: Point3<Length>, geometry: G) -> Point3<Length>
fn contains(solid: Solid, point: Point3<Length>) -> Bool
fn is_on<G: Geometry>(point: Point3<Length>, geometry: G) -> Bool
fn intersects(a: Geometry, b: Geometry) -> Bool
fn geo_equiv(a: Geometry, b: Geometry, tolerance: Length) -> Bool
```

**Angular:**

```
fn angle(a: Vector3<Dimensionless>, b: Vector3<Dimensionless>) -> Angle
fn angle_between_surfaces(a: Surface, b: Surface) -> Angle
```

**Measurement:**

```
fn area(surface: Surface) -> Scalar<Area>
fn area(solid: Solid) -> Scalar<Area>          // Surface area
fn volume(solid: Solid) -> Scalar<Volume>
fn length(curve: Curve) -> Scalar<Length>
fn perimeter(surface: Surface) -> Scalar<Length>
```

**Mass properties:**

```
fn centroid(solid: Solid) -> Point3<Length>
fn center_of_mass(solid: Solid, density: Scalar<Density>) -> Point3<Length>
fn moment_of_inertia(solid: Solid, density: Scalar<Density>) -> Tensor<2, 3, MomentOfInertia>
fn bounding_box<G: Geometry>(geometry: G) -> BoundingBox
```

**Surface/curve analysis:**

```
fn normal(surface: Surface, at: Point3<Length>) -> Vector3<Dimensionless>
fn curvature(curve: Curve, at: Point3<Length>) -> Scalar<Length^(-1)>
fn curvature(surface: Surface, at: Point3<Length>) -> Matrix<2, 2, Length^(-1)>
```

**Topology selectors:**

```
fn edges(solid: Solid) -> List<Curve>
fn faces(solid: Solid) -> List<Surface>
fn adjacent_faces(solid: Solid, face: Surface) -> List<Surface>
fn shared_edges(face1: Surface, face2: Surface) -> List<Curve>
fn edges_by_length(solid: Solid, range: Range<Length>) -> List<Curve>
fn faces_by_area(solid: Solid, range: Range<Area>) -> List<Surface>
fn faces_by_normal(solid: Solid, direction: Vector3<Dimensionless>, tolerance: Angle) -> List<Surface>
fn edges_parallel_to(solid: Solid, direction: Vector3<Dimensionless>, tolerance: Angle) -> List<Curve>
fn edges_at_height(solid: Solid, height: Length, tolerance: Length) -> List<Curve>
```

**Kernel note (mesh vs B-rep).** `faces()`/`edges()` cardinality and the
indices returned by `adjacent_faces()`/`shared_edges()` are **kernel-dependent**
(selected by the `#kernel(...)` pragma). On the **B-rep** kernel (OCCT) a face
is a smooth parametric surface patch, so a box has 6 faces; on the **mesh**
kernel (Manifold) a face is a mesh triangle, so the same box has 12 faces. Face
counts and the triangle-vs-patch indices the adjacency selectors return are
therefore **not comparable across kernels**. `edges_by_length` is **B-rep-only**
(the mesh kernel has no curves — querying it on a mesh solid is unsupported);
the other selectors and the mass properties (`center_of_mass`,
`moment_of_inertia`) have parity on both kernels.

**Eval status (2026-06, `docs/prds/v0_3/kernel-geometry-queries.md`):** every query and topology-selector helper in this section now dispatches to the geometry kernel and returns a typed value at eval time; prior to this PRD these helpers were registered/compile-typed but eval-returned `Undef`.

### 3.10 `std.geometry.traits`

```
trait Geometry                                    // Supertrait for all geometric entities
trait Transformable                               // Can be spatially transformed
trait Closed                                      // Boundary is closed
trait Manifold                                    // 2-manifold faces
trait Orientable                                  // Consistently orientable
trait Convex                                      // Convex geometry
trait Connected                                   // Single connected component
trait Bounded                                     // Finite extent
trait Watertight : Closed + Manifold              // Printable/meshable

structure def Plane {
    param origin : Point3<Length>
    param normal : Vector3<Dimensionless>
}

structure def Axis {
    param origin : Point3<Length>
    param direction : Vector3<Dimensionless>
}

structure def BoundingBox {
    param min : Point3<Length>
    param max : Point3<Length>
    let size = max - min
    let center = point3((min.x + max.x) / 2, (min.y + max.y) / 2, (min.z + max.z) / 2)
}

fn plane_xy(z: Length) -> Plane
fn plane_xz(y: Length) -> Plane
fn plane_yz(x: Length) -> Plane
fn axis_x(origin: Point3<Length>) -> Axis
fn axis_y(origin: Point3<Length>) -> Axis
fn axis_z(origin: Point3<Length>) -> Axis
```

---

## 4. `std.structural`

```
trait Physical {
    param geometry : Solid
    param material : Material
    let mass = volume(geometry) * material.density
    let centroid = centroid(geometry)
    constraint material.density > 0
}

trait Rigid : Physical {
    param moment_of_inertia : MomentOfInertia
    constraint moment_of_inertia > 0.0 * 1kg * 1m * 1m
}

trait Flexible {
    param stiffness : Stiffness
    param max_deflection : Length
    constraint stiffness > 0.0 * 1N / 1m
    constraint max_deflection > 0.0 * 1m
}

trait ElasticallyDeformable : Flexible {
    param max_elastic_strain : Real
    constraint max_elastic_strain > 0
}

trait Plastic : Flexible {
    param plastic_strain : Real
    param hardening_modulus : Pressure
    constraint hardening_modulus > 0.0 * 1Pa
    constraint plastic_strain >= 0
}

trait ThermallyConductive : Physical {
    param thermal_conductivity : ThermalConductivity
    param max_service_temp : Temperature
    constraint thermal_conductivity > 0W/(m*K)
    constraint max_service_temp > 0.0 * 1K
}

trait ElectricallyConductive : Physical {
    param electrical_conductivity : ElectricalConductivity
    param resistivity : ElectricResistivity
    constraint electrical_conductivity > 0.0 * 1S / 1m
}

trait Sealed {
    param seal_pressure_rating : Pressure
    constraint seal_pressure_rating > 0.0 * 1Pa
}
```

`Flexible` ships the lumped `stiffness` + `max_deflection` contract. A continuum spatially-varying stiffness field — `Field<Point3<Length>, Tensor<2,3,Pressure>>` — is deferred future work (PRD γ); no consumer today.

Geometry-derived moment of inertia via the `moment_of_inertia(solid, density)` query builtin is a separate facility (returns a Tensor; auto-binding it into a `let` on `Rigid` is deferred — PRD δ).

Yield strength is a material property, not a body member — see `materials_mechanical.Strong.yield_strength` / `Analysis.yield_strength`; that is why `Plastic` carries `plastic_strain` + `hardening_modulus` and `yield_point` is gone.

---

## 5. `std.ports`

### 5.1 Base Ports

```
trait Port {
    param direction : Directionality = Directionality.Bidi
}

enum Directionality { In, Out, Bidi }

structure def Frame3 {
    param origin : Vector3<Length>
    param x_axis : Vector3<Length>
    param y_axis : Vector3<Length>
    param z_axis : Vector3<Length>
}

trait LocatedPort : Port {
    param frame : Frame3
}

trait RegionPort : LocatedPort {
    param region : Geometry
}
```

Compatibility rules: `In` <-> `Out` (valid), `Bidi` <-> anything (valid), `In` <-> `In` (type error), `Out` <-> `Out` (type error).

### 5.2 `std.ports.mechanical`

```
trait MechanicalPort : LocatedPort {
    param max_load : Option<Force> = none
    param max_torque : Option<Torque> = none
}

// MatingFace + parameterized Bore/Shaft (diameter/depth/fit:FitType) not yet shipped — bare markers today
trait Bore : MechanicalPort {}
trait Shaft : MechanicalPort {}
trait StructurePort : MechanicalPort {}

trait ThreadedPort : MechanicalPort {
    param thread_spec : ThreadSpec
}

structure def ThreadSpec {
    param system : ThreadSystem
    param nominal_diameter : Length
    param pitch : Length
    param thread_class : ThreadClass
    param tightening : ThreadTighteningDirection = ThreadTighteningDirection.Clockwise
    param thread_form : Option<Geometry> = none

    let minor_diameter = nominal_diameter - pitch * 1.0825
    let pitch_diameter = nominal_diameter - pitch * 0.6495
    let tap_drill = nominal_diameter - pitch
    let clearance_hole = nominal_diameter + pitch * 0.5
}

enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
enum ThreadClass { Class_6g6H, Class_4g6H }
enum ThreadTighteningDirection { Clockwise, Counterclockwise }

trait MotivePort : MechanicalPort
trait RotaryPort : MotivePort {
    param max_speed : AngularVelocity
    param max_torque : Torque
    param axis : Vector3<Length>
}
trait LinearPort : MotivePort {
    param max_speed : Velocity
    param max_force : Force
    param stroke : Length
    param axis : Vector3<Length>
}
trait GuidePort : MechanicalPort {
    param degrees_of_freedom : Int
}
trait LinearGuidePort : GuidePort {
    constraint degrees_of_freedom == 1
}
trait RotaryGuidePort : GuidePort {
    constraint degrees_of_freedom == 1
    param max_radial_load : Force
    param max_axial_load : Force
}
```

### 5.3 `std.ports.electrical`

```
trait ElectricalPort : Port {
    param voltage_rating : Voltage
    param current_rating : Current
}
trait PowerPort : ElectricalPort {
    param power_rating : Power
}
trait SignalPort : ElectricalPort {
    param signal_kind : SignalKind
    param impedance : Option<Resistance> = none
}
enum SignalKind { Analog, Digital, PWM, Differential }
trait PinPort : ElectricalPort + LocatedPort {
    param pin_id : String
}
```

### 5.4 `std.ports.thermal`

```
pub type HeatFlux = Power / Area
pub type ThermalResistance = Temperature / Power

trait ThermalPort : Port {
    param temperature : Option<Temperature> = none
    param heat_flow : Power
    param heat_flux : Option<HeatFlux> = none
    param thermal_resistance : Option<ThermalResistance> = none
}
trait ThermalContactPort : ThermalPort + RegionPort {
    param contact_area : Area
    param contact_conductance : Option<ThermalConductivity> = none
}
```

### 5.5 `std.ports.fluid`

```
pub type VolumetricFlowRate = Volume / Time

trait FluidPort : Port {
    param pressure : Pressure
    param flow_rate : VolumetricFlowRate
    param medium : String
    param fluid_type : FluidType
}
enum FluidType { Liquid, Gas, TwoPhase }
enum FittingStandard { NPT, BSP, JIC, ORFS }
trait PipedFluidPort : FluidPort + LocatedPort {
    param inner_diameter : Length
    param connection_type : PipeConnectionType
}
enum PipeConnectionType { Threaded, Flanged, Compression, PushFit, Welded }
```

### 5.6 Multi-Domain Ports

Interfaces (port bundles) are simply traits that require multiple ports with geometric constraints. No new concept is needed. Multi-domain ports compose via trait inheritance:

```
trait HydraulicPort : FluidPort + MechanicalPort {
    param fitting_type : FittingStandard
}
```

---

## 6. `std.materials`

### 6.1 Base

> **Breaking change (task #1876 / #2411):** The identifier `Material` is now a canonical
> first-class **struct** (see below); the base trait has been renamed to `MaterialSpec`.
> Consumers referencing `: Material` as a trait base must use `: MaterialSpec`; consumers
> referencing `param m : Material` as a trait-typed param must use `: MaterialSpec`; concrete
> value slots (`param m : Material`) resolve to the struct unchanged.
> No deprecation alias is provided (a trait and a struct cannot share the same identifier).

```
// Base trait — every material-conforming structure satisfies this contract.
// Note: density shown as Density (aspirational dimensioned type); shipped MaterialSpec
// uses density : Real pending dimensional-type tightening (#3111-family).
trait MaterialSpec {
    param density : Density
    param name : String
}

// Canonical first-class material value type (shipped in materials_mechanical.ri,
// task #1876 / #2411). Use MaterialSpec above for trait-typed params; use this struct
// when you want a concrete material value (e.g. Material(name: "steel", ...)).
structure def Material {
    param name : String
    param density : Real
    param youngs_modulus : Real
}

trait TemperatureDependent {
    param reference_temperature : Temperature = 293.15K
}
```

### 6.2 `std.materials.mechanical`

```
// Dimensioned-type note: the Pressure/Energy param types shown below (youngs_modulus,
// yield_strength, ultimate_tensile_strength, compressive_strength, shear_modulus,
// fatigue_limit, fatigue_strength_at, charpy_impact, izod_impact) are the target of the
// deferred #3111-family dimensional tightening and are currently Real placeholders in the
// shipped stdlib. The thermal/electrical/optical/fracture dimensioned types shown in §6.3–§6.5
// have been realized by tasks #3112/#3113/#3115 (ThermalExpansion, ElectricResistivity,
// DielectricStrength, AbsorptionCoeff, FractureToughness). Do not downgrade the Pressure/Energy
// types shown here — they remain the documented aspiration pending #3111.

// Elastic, Strong, Hard, Ductile are free-standing (no MaterialSpec base) — deliberate
// design drift #3487. Conformers carry density/name via a separate `material : MaterialSpec`
// slot rather than inheriting from the base trait directly.
trait Elastic {
    param youngs_modulus : Pressure  // shipped: Real (aspiration pending #3111)
    param poissons_ratio : Real
    param shear_modulus : Pressure = undef  // shipped: Real (aspiration pending #3111)
    constraint 0 < poissons_ratio < 0.5
}
trait Strong {
    param yield_strength : Pressure  // shipped: Real (aspiration pending #3111)
    param ultimate_tensile_strength : Pressure  // shipped: Real (aspiration pending #3111)
    param compressive_strength : Pressure = undef  // shipped: Real (aspiration pending #3111)
    constraint ultimate_tensile_strength >= yield_strength
}
trait Hard {
    param hardness_value : Real
    param hardness_scale : HardnessScale
}
enum HardnessScale { Rockwell_A, Rockwell_B, Rockwell_C, Brinell, Vickers, Shore_A, Shore_D }
trait FatigueRated : MaterialSpec {
    param fatigue_limit : Pressure = undef  // shipped: Real (aspiration pending #3111)
    param fatigue_strength_at : Pressure = undef  // shipped: Real (aspiration pending #3111)
    param fatigue_cycles : Int = undef
}
trait FractureTough : MaterialSpec {
    param fracture_toughness : FractureToughness  // Pa·√m — tightened from Scalar<> by task #3115
}
trait Ductile {
    param elongation_at_break : Real
    param reduction_of_area : Real = undef
}
trait ImpactResistant : MaterialSpec {
    param charpy_impact : Energy = undef  // shipped: Real (aspiration pending #3111)
    param izod_impact : Energy = undef  // shipped: Real (aspiration pending #3111)
}
trait Damping : MaterialSpec {
    param damping_ratio : Real  // fraction of critical damping (dimensionless)
    param loss_factor : Real    // ratio of dissipated to stored energy per cycle (dimensionless)
}
```

### 6.3 `std.materials.thermal`

```
trait ThermallyCharacterized : MaterialSpec {
    param thermal_conductivity : ThermalConductivity
    param specific_heat : SpecificHeat
    param thermal_expansion : ThermalExpansion       // 1/K — tightened by task #3112/#3115
    param melting_point : Temperature = undef
    param max_service_temperature : Temperature = undef
    param glass_transition : Temperature = undef
}
// Refractory threshold: 1500.0 K (≈ 1226.85 °C). This is a substantive threshold
// lowering from the former `>= 1500degC` aspiration — 1500 K is approximately 273 K
// (≈ 273 °C) below 1500 °C, a significant relaxation of the physical refractory
// criterion. The threshold was intentionally redefined at 1500 K in task #3112 (not a
// neutral K-typed re-expression of 1500 °C; 1500 °C ≈ 1773 K).
trait Refractory : ThermallyCharacterized {
    constraint max_service_temperature >= 1500.0K
}
```

### 6.4 `std.materials.electrical`

```
trait ElectricallyCharacterized : MaterialSpec {
    param resistivity : ElectricResistivity         // Ω·m — tightened by task #3115
    param dielectric_constant : Real = undef        // dimensionless
    param dielectric_strength : DielectricStrength = undef  // V/m — tightened by task #3115
    param magnetic_permeability : Real = undef      // dimensionless
}
trait Conductive : ElectricallyCharacterized {
    constraint resistivity < 0.0001ohm*m
}
// Insulating: dielectric_strength constraint replaced by a positivity bound (task #2484).
// When a conformer omits dielectric_strength (optional), `> 0.0V/m` evaluates as
// `Undef > 0.0V/m → Undef` (Kleene, arch §2.5), producing Satisfaction::Indeterminate
// + a ConstraintIndeterminate Warning rather than a hard error.
trait Insulating : ElectricallyCharacterized {
    constraint resistivity > 1000000ohm*m
    constraint dielectric_strength > 0.0V/m
}
```

### 6.5 `std.materials.optical`

```
trait OpticallyCharacterized : MaterialSpec {
    param refractive_index : Real
    param absorption_coefficient : AbsorptionCoeff = undef  // 1/m — tightened by task #3115
    param transmittance : Real = undef
    param reference_thickness : Length = undef
}
```

### 6.6 `std.materials.chemical`

```
trait CorrosionResistant : MaterialSpec {
    param corrosion_class : CorrosionClass
}
enum CorrosionClass { C1, C2, C3, C4, C5 }
trait Biocompatible : MaterialSpec {
    param biocompatibility_class : BiocompatibilityClass
}
enum BiocompatibilityClass { USP_Class_I, USP_Class_VI, ISO_10993 }
```

---

## 7. `std.tolerancing`

### 7.1 `std.tolerancing.dimensional`

```
structure def DimensionalTolerance {
    param nominal : Length
    param upper_deviation : Length
    param lower_deviation : Length
    let upper_limit = nominal + upper_deviation
    let lower_limit = nominal + lower_deviation
    let tolerance_band = upper_deviation - lower_deviation
    constraint upper_deviation >= lower_deviation
}

fn symmetric_tolerance(nominal: Length, deviation: Length) -> DimensionalTolerance
fn limit_tolerance(upper: Length, lower: Length) -> DimensionalTolerance

structure def Fit {
    param hole_tolerance : DimensionalTolerance
    param shaft_tolerance : DimensionalTolerance
    param fit_type : FitCategory
    let max_clearance = hole_tolerance.upper_limit - shaft_tolerance.lower_limit
    let min_clearance = hole_tolerance.lower_limit - shaft_tolerance.upper_limit
}
enum FitCategory { Clearance, Transition, Interference }

structure def ISOToleranceGrade {
    param grade : Int
    param nominal_min : Length
    param nominal_max : Length
    let tolerance_value = iso_it_tolerance(grade, nominal_min, nominal_max)  // ISO 286-1 IT5–IT18, nominal ≤500mm
}
```

### 7.2 `std.tolerancing.geometric`

```
trait GeometricTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param material_condition : MaterialCondition = MaterialCondition.RFS
    let nominal_zone = ...  // scalar effective zone SIZE (Length) = effective_tolerance_zone(tolerance_value, material_condition, departure); geometric-region form deferred (needs zone-construction kernel op — out of scope)
}
enum MaterialCondition { MMC, LMC, RFS }

structure def Datum {
    param label : String
    param feature : Geometry
}

// Form tolerances (no datum)
trait FormTolerance : GeometricTolerance
structure def Flatness : FormTolerance { ... }
structure def Straightness : FormTolerance { ... }
structure def Circularity : FormTolerance { ... }
structure def Cylindricity : FormTolerance { ... }

// Orientation tolerances (require datums)
trait OrientationTolerance : GeometricTolerance { ... }
structure def Parallelism : OrientationTolerance { ... }
structure def Perpendicularity : OrientationTolerance { ... }
structure def Angularity : OrientationTolerance {
    param nominal_angle : Angle
}

// Location tolerances
trait LocationTolerance : GeometricTolerance { ... }
structure def Position : LocationTolerance { ... }
structure def Concentricity : LocationTolerance { ... }
structure def Symmetry : LocationTolerance { ... }

// Runout
structure def CircularRunout : GeometricTolerance { ... }
structure def TotalRunout : GeometricTolerance { ... }

// Profile
structure def ProfileOfSurface : GeometricTolerance { ... }
structure def ProfileOfLine : GeometricTolerance { ... }

// Universal conformance
constraint def Conforms {
    param tolerance : GeometricTolerance
    // Handles MMC/LMC/RFS material condition expansion
}
```

### 7.3 `std.tolerancing.surface`

```
structure def SurfaceFinish {
    param parameter : SurfaceParameter
    param value : Length
    param direction : SurfaceDirection = SurfaceDirection.Multidirectional
    param process : String = ""
}
enum SurfaceParameter { Ra, Rz, Rq, Rt, Rp, Rv, Rsk, Rku }
enum SurfaceDirection { Parallel, Perpendicular, Crossed, Multidirectional, Circular, Radial }

fn require_finish(feature: Geometry, finish: SurfaceFinish) -> Bool
```

---

## 8. `std.process`

```
trait Process {
    param duration : Time = undef
    param cost : Scalar<Money> = undef
}
```

**Process categories (gerund-form traits):**

```
trait Subtracting : Process {
    param tool_access : Geometry
    param min_feature_size : Length
    param achievable_finish : Length
}
trait Adding : Process {
    param layer_thickness : Length
    param min_feature_size : Length
    param build_volume : Solid
    param max_overhang_angle : Angle
}
trait Forming : Process {
    param min_bend_radius : Length
    param max_draw_depth : Length
    param draft_angle : Angle
}
trait Joining : Process {
    param joint_strength : Pressure
    param reversible : Bool
}
trait Parting : Process {
    param kerf_width : Length
    param min_feature_size : Length
}
trait SurfaceTreating : Process {
    param coating_thickness : Length = undef
    param achievable_finish : Length
}
trait HeatTreating : Process {
    param treatment_temperature : Temperature
    param hold_duration : Time
}
```

**DFM framework:**

```
enum DFMSeverity { Info, Warning, Error }

trait DFMRule {
    param rule_name  : String
    param severity   : DFMSeverity
    param applies_to : Process
    param subject    : Solid
}
```

At `reify check` time (with a geometry kernel), the engine realizes each
`DFMRule.subject : Solid` and auto-measures it against the bound process
capability — no hand-declared measured feature:

- **`Adding.max_overhang_angle`** → emits `{I,W,E}_DFM_OVERHANG` at the
  rule's declared `severity` when the solid has unsupported faces dipping
  beyond the threshold. Default build direction: +Z.
- **`Forming.draft_angle`** → emits `{I,W,E}_DFM_DRAFT` at the rule's
  declared `severity` when wall-face draft is insufficient. Also emits
  `E_DFM_UNDERCUT` (always `Error`, regardless of rule severity) when a
  re-entrant wall is detected. Default pull direction: +Z.

When no geometry kernel is present, the pass is a safe no-op — Indeterminate,
never a false violation.

See `examples/process/std_process_dfm_metrology.ri` for a complete worked
example covering overhang, draft, undercut, and a conforming rule that emits
nothing.

---

## 9. `std.io`

**Boundary abstractions:**

```
trait Source                  // Something enters design scope
trait Sink                   // Something leaves design scope

trait Input : Source {
    param source : String
    param provenance : Provenance = undef
}

trait Buy : Source {
    param supplier : String
    param part_number : String
    param unit_cost : Scalar<Money>
    param lead_time : Time = undef
}

trait Output : Sink {
    param format : OutputFormat = undef
}

trait Discard : Sink {
    param reason : DiscardReason
    param disposal_method : DisposalMethod
}

enum DiscardReason { Offcut, Scrap, FailedInspection, Waste }
enum DisposalMethod { Recycle, Landfill, Reprocess }

structure def Provenance {
    param source_tool : String
    param source_version : String = undef
    param timestamp : String = undef        // ISO 8601 string; no Date type in v0.1
    param tolerance_guarantee : Length = undef
}

enum OutputFormat { STEP, STL, ThreeMF, Display }
```

**Format occurrences (`std.io.formats`):**

```
occurrence def STEPOutput : Output {
    param subject : Structure
    param version : STEPVersion = STEPVersion.AP214
    constraint determined(subject.geometry)
}
enum STEPVersion { AP203, AP214, AP242 }

occurrence def STLOutput : Output {
    param subject : Solid
    param resolution : Length
}

occurrence def ThreeMFOutput : Output {
    param subject : Structure
    param include_materials : Bool = true
    param include_colors : Bool = true
}

occurrence def DisplayOutput : Output {
    param subject : Geometry
    param pane : Int = 0
    param style : DisplayStyle = undef
}

structure def DisplayStyle {
    param color : Vector3<Dimensionless> = vec3(0.7, 0.7, 0.7)
    param opacity : Real = 1.0
    param wireframe : Bool = false
}

occurrence def STEPInput : Input {
    param result : Structure
    param version : STEPVersion = undef
}

occurrence def PointCloudInput : Input {
    param result : PointCloud
    param format : PointCloudFormat = undef
}
enum PointCloudFormat { PLY, PCD, XYZ, LAS }
```

---

## 10. `std.analysis`

**Type aliases.** `Stress` (= `Pressure`) and `Strain` (= `Dimensionless`) are user-facing spelling aliases — both resolve to the same `Type::Scalar` dimension as their base and are interchangeable in dimensional algebra.

```
trait Analysis {
    param yield_strength : Real        // material yield strength for safety-factor (Pa; Real placeholder)
    constraint yield_strength > 0
}

trait AnalysisResult {
    param von_mises_stress    : Real
    param principal_stress_1  : Real
    param principal_stress_2  : Real
    param principal_stress_3  : Real
    param max_shear_stress    : Real
    param safety_factor_value : Real
    constraint von_mises_stress >= 0
    constraint max_shear_stress >= 0
    constraint safety_factor_value > 0
}
```

`AnalysisResult` is a **structural contract**: each param uses `Real` as a
dimension-agnostic placeholder (the runtime stress builtins below produce
correctly-dimensioned values, e.g. `Scalar<Pressure>` for the stresses and a
dimensionless `Real` for `safety_factor_value`), and the trait does **not**
participate in dimension checking — it will not reject dimensioned conforming
values. (The v0.1 doc's `mesh_resolution`/`convergence_target` on `Analysis`
and `source`/`mesh` on `AnalysisResult` were never shipped — task 341.)

**Stress post-processing (`std.analysis.stress`):**

```
fn von_mises(stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>) -> Field<Point3<Length>, Scalar<Pressure>>
fn principal_stresses(stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>) -> List<Field<Point3<Length>, Scalar<Pressure>>>
fn safety_factor(stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>, yield_strength: Pressure) -> Field<Point3<Length>, Scalar<Dimensionless>>
fn max_shear(stress: Field<Point3<Length>, Tensor<2, 3, Pressure>>) -> Field<Point3<Length>, Scalar<Pressure>>
```

---

## 11. `std.fields`

**Interpolation (`std.fields.interpolation`):**

```
enum InterpolationMethod { Linear, Bilinear, Trilinear, NearestNeighbor, RBF, Kriging }

fn constant_field<D, C>(value: C) -> Field<D, C>
fn fn_field<D, C>(f: fn(D) -> C) -> Field<D, C>
fn from_samples<D, C>(points: List<D>, values: List<C>, method: InterpolationMethod = InterpolationMethod.Linear) -> Field<D, C>
```

**Spatial operations (`std.fields.spatial`):**

```
fn compose<A, B, C>(f: Field<A, B>, g: Field<B, C>) -> Field<A, C>
fn sample<D, C>(field: Field<D, C>, at: D) -> C
fn restrict<D, C, G: Geometry>(field: Field<D, C>, region: G) -> Field<D, C>
fn clamp_field<D, Q: Dimension>(field: Field<D, Scalar<Q>>, lo: Scalar<Q>, hi: Scalar<Q>) -> Field<D, Scalar<Q>>
fn remap_field<D, Q: Dimension>(field: Field<D, Scalar<Q>>, from_range: Range<Scalar<Q>>, to_range: Range<Scalar<Q>>) -> Field<D, Scalar<Q>>
fn threshold<D, Q: Dimension>(field: Field<D, Scalar<Q>>, value: Scalar<Q>) -> Field<D, Bool>
```

**Differential operators** — prelude builtins implemented natively in
`reify-expr` (`calculus.rs`) / `reify-stdlib`, **not** `.ri` `fn` declarations.
The `@optimized` annotation attaches only to `.ri` `fn` / `constraint def`
bodies (e.g. the solver/modal/dynamics fns), so it does **not** apply to these
built-in operators:

```
fn gradient<N: Nat, Q: Dimension>(field: Field<Point<N,Length>, Scalar<Q>>) -> Field<Point<N,Length>, Vector<N, Q/Length>>
fn divergence<N: Nat, Q: Dimension>(field: Field<Point<N,Length>, Vector<N,Q>>) -> Field<Point<N,Length>, Scalar<Q/Length>>
fn curl<Q: Dimension>(field: Field<Point3<Length>, Vector3<Q>>) -> Field<Point3<Length>, Vector3<Q/Length>>
fn laplacian<N: Nat, Q: Dimension>(field: Field<Point<N,Length>, Scalar<Q>>) -> Field<Point<N,Length>, Scalar<Q/Length^2>>
```

---

## 12. `std.determinacy`

**Predicates (compiler intrinsics, in prelude):**

```
fn determined(param_ref) -> Bool
fn constrained(param_ref) -> Bool
fn undetermined(param_ref) -> Bool
fn partially_determined(param_ref) -> Bool    // constrained && !determined
```

**Purpose-body intrinsics (compiler-recognized, valid ONLY inside a `purpose` body):**

```
AllParamsDetermined(subject : Structure)
// Compiler intrinsic — desugars at compile time to:
//   forall __p in subject.params: determined(__p)
// Valid ONLY inside a purpose body; used elsewhere → E_DETERMINACY_INTRINSIC_SCOPE.

AllGeometryDetermined(subject : Structure)
// Compiler intrinsic — desugars at compile time to:
//   forall __p in subject.geometric_params: determined(__p)
// Valid ONLY inside a purpose body; used elsewhere → E_DETERMINACY_INTRINSIC_SCOPE.
```

Both intrinsics are recognized by the compiler and transformed into reflective `forall`
quantifiers over the subject's param set at compile time (PRD §8.1).  They are **not**
ordinary `constraint def` declarations — a `constraint def` cannot take a `Structure`
entity-ref, and `.params` / `.geometric_params` reflective queries are valid only in
purpose bodies.

**`RepresentationWithin` — post-realization assertion:**

`RepresentationWithin(subject, bound)` is a structural constraint placed directly in a
structure body.  It asserts that the **sampled facet-chord deviation** of the realized
mesh for `subject` does not exceed `bound` (a `Length` value).

Semantics (three-valued, evaluated after `tessellate_realizations`):

- **Satisfied** — maximum sampled facet-chord deviation ≤ `bound`.
- **Violated** — at least one sampled facet-chord deviation > `bound`.
- **Indeterminate** — `subject` was not realized (stub build without OCCT, or
  realization not requested) → never a false `Violated` (C1 graceful degradation).

> **PRD §8.3 caveat — sampled lower bound:** the metric is a sampled lower bound on the
> true Hausdorff chord error (4 interior sample points per facet).  `Satisfied` means
> "no sampled point exceeded the bound" — it is **not** an everywhere-within-tolerance
> guarantee.  Use a finer `#precision` directive to reduce the measurement gap.

```
structure CheckSpec {
    param subject : MyGeom
    constraint RepresentationWithin(subject, 1mm)  // Violated if sampled deviation > 1mm
}
```

**Example purposes (`std.determinacy.purposes`):**

```
purpose design_review(subject : Structure) {
    constraint AllParamsDetermined(subject)
}

purpose simulation_ready(subject : Physical) {
    constraint AllGeometryDetermined(subject)
    constraint determined(subject.material)
}
```

---

## 13. `std.mechanism`

The `std.mechanism` library adds stdlib-level kinematic mechanism modelling. v0.1 ships **forward kinematics over open chains** with **prismatic, revolute, and position-coupling** joints, a **batch-sweep** API, and an **interference/clearance query**. Closed chains — bodies reachable via two distinct joint paths — are detected as a build-time error in v0.1 and deferred to v0.2 (see `v0_2/kinematic-constraints.md`). No new core syntax or IR is introduced: joints, mechanisms, and motion variables are pure stdlib values. This honours the language spec rationale (spec line 36): "Domain complexity (GD&T, DFM rules, kinematic joints, material databases) belongs in community-driven libraries."

### 13.1 `std.mechanism.joints`

Joint primitives are first-class stdlib values. Each joint type internally exposes `transform_at`, which returns a `Transform<3>` for a given motion-variable value. User code reaches it through the mechanism builder and snapshot APIs rather than calling `transform_at` directly.

**Joint types and traits:**

```
trait Joint          // any joint kind; participates in the MotionValue<Self> type family
trait DrivingJoint: Joint {}   // Prismatic and Revolute only; may be bound or swept directly
                               // Coupling<P> derives its motion and does NOT implement DrivingJoint

type Prismatic : DrivingJoint  // 1-DOF translation; MotionValue<Prismatic> = Length
type Revolute  : DrivingJoint  // 1-DOF rotation;    MotionValue<Revolute>  = Angle
type Coupling<P: DrivingJoint> : Joint  // derives motion from joint P;
                                        // MotionValue<Coupling<P>> = MotionValue<P>
```

**Constructors:**

```
fn prismatic(axis: Vector3<Dimensionless>, range: Range<Length>) -> Prismatic
fn revolute(axis: Axis, range: Range<Angle>) -> Revolute
fn couple<P: DrivingJoint>(other: P, ratio: Real, offset: MotionValue<P> = zero) -> Coupling<P>
```

`Prismatic` models 1-DOF translation along a fixed axis with motion-range bounds. `Revolute` models 1-DOF rotation about a fixed axis with angle-range bounds. `Coupling` derives its motion variable from another joint: `value = ratio * other.value + offset`. A negative ratio produces the counter-mass direction reversal shown in the worked examples (§13.6).

**Axis, range, and ratio accessors:**

```
fn axis(j: Prismatic) -> Vector3<Dimensionless>
fn range(j: Prismatic) -> Range<Length>
fn axis(j: Revolute) -> Axis
fn range(j: Revolute) -> Range<Angle>
fn ratio(j: Coupling<P>) -> Real
fn offset(j: Coupling<P>) -> MotionValue<P>
fn transform_at(j: Prismatic, v: Length) -> Transform<3>
fn transform_at(j: Revolute, v: Angle) -> Transform<3>
fn transform_at(j: Coupling<P>, v: MotionValue<P>) -> Transform<3>
```

**Jacobian (v0.2).** `joint_jacobian` returns the analytic SE(3) twist column
for a single joint, used by the closed-chain loop-closure solver — see
[`v0_2/kinematic-constraints.md`](prds/v0_2/kinematic-constraints.md). The
returned `Twist` shape (`Map { angular, linear }`) is the same one used by
`transform_log` / `transform_exp` (§3.1), so solver code can compose joint
Jacobian columns and twists uniformly.

```
fn joint_jacobian(j: Prismatic) -> Twist          // angular = 0,        linear  = unit(axis)
fn joint_jacobian(j: Revolute)  -> Twist          // angular = unit(axis), linear = 0
fn joint_jacobian(j: Coupling<P>) -> Twist        // ratio * joint_jacobian(parent)
```

The axis is unit-normalized in the return value (matching `transform_at`'s
normalization). For `Coupling<P>`, the result is the parent's Jacobian
componentwise multiplied by the coupling ratio. Finite-difference Jacobians
for new joint types (cylindrical, planar, spherical) are deferred to v0.2's
joint-type-expansion task and are not part of this stdlib surface.

**Motion-variable units.** Each joint type has an associated motion-variable unit, exposed as the type family `MotionValue<J>`:

```
type MotionValue<Prismatic>    = Length
type MotionValue<Revolute>     = Angle
type MotionValue<Coupling<P>>  = MotionValue<P>  // inherits parent joint's motion-variable unit
```

`MotionValue<J>` parameterises the binding and range types in §13.3–§13.4 so that `0mm .. 500mm` (a `Range<Length>`) is the natural sweep range for a `Prismatic` joint, `0deg .. 90deg` (a `Range<Angle>`) for a `Revolute`, and so on. For a `Coupling<Prismatic>` (such as the counter-mass in the worked example) the motion variable is also a `Length`, preserving dimensional coherence in the formula `value = ratio * other.value + offset`.

### 13.2 `std.mechanism.builder`

`mechanism()` returns an empty `Mechanism`. Bodies are attached with `.body()` chaining; each call returns a fresh `Mechanism`. `world` is the pre-declared ground-frame sentinel — a `Joint` value with no motion variable that serves as the fixed root anchor of every mechanism DAG:

```
let world : Joint   // ground/world frame; the implicit fixed root of every mechanism DAG
```

```
fn mechanism() -> Mechanism
fn body(m: Mechanism, solid: Solid, at: Joint, parent: Joint = world, pose: Transform<3> = transform3_identity) -> Mechanism
fn body_id_of(m: Mechanism, solid: Solid) -> BodyId
```

`at` is the joint that positions the body; `parent` is the upstream joint (default `world` for bodies attached to the ground frame). `pose` is an additional static offset applied after the joint's own transform. `BodyId` is a stable, opaque identifier used later by snapshot accessors and query functions (see §13.3 and §13.5). To recover the `BodyId` of a particular `solid` after building, call `body_id_of(m, solid)` against the final `Mechanism` (it returns the id assigned when that `solid` was added, or raises if the solid is not in the mechanism). The builder is immutable: each `.body()` call returns a fresh `Mechanism` value. Each `solid` value must be unique within a given `Mechanism` (by referential identity); inserting the same `solid` value twice raises `error[E_MECHANISM_DUPLICATE_SOLID]` at build time, keeping `body_id_of` unambiguous even when two bodies have identical geometry — use distinct constructor calls to create distinct solids before passing them to `.body()`.

**Closed-chain detection.** `mechanism()` builds a directed acyclic graph (DAG) of bodies connected through joints. If any body is reachable via two distinct joint paths, the compiler emits `error[E_KINEMATIC_CLOSED_CHAIN]`, naming both paths in the diagnostic:

```
error[E_KINEMATIC_CLOSED_CHAIN]: body is reachable via two distinct joint paths
  --> mechanism build site
  |
  | path 1: world -> joint_a -> joint_b -> body
  | path 2: world -> joint_c -> body
```

Closed chains are a v0.1 error; v0.2 introduces a cyclic solver.

### 13.3 `std.mechanism.snapshot`

`snapshot` evaluates forward kinematics for a set of joint-value bindings, producing a concrete configuration with world-frame transforms for every body.

```
fn snapshot(m: Mechanism, bindings: List<JointBinding>) -> Snapshot
fn bind<J: DrivingJoint>(joint: J, value: MotionValue<J>) -> JointBinding
```

Each entry binds a driving joint to a typed motion-variable value via `bind(joint, value)`: `Length` for `Prismatic`, `Angle` for `Revolute`. `Coupling<P>` joints are excluded — their motion variable is derived from the parent joint's binding and cannot be overridden (`Coupling<P>` implements `Joint` but not `DrivingJoint`, so passing a coupling to `bind` is a type error). `JointBinding` is a sum type with one variant per `DrivingJoint` kind; its concrete variants are `bind(j: Prismatic, v: Length) -> JointBinding` and `bind(j: Revolute, v: Angle) -> JointBinding`. A single bindings list can mix the two driving-joint kinds while remaining type-safe (see `MotionValue<J>` in §13.1). Joints absent from `bindings` take their range midpoint.

**Snapshot accessors:**

```
fn bodies(s: Snapshot) -> List<BodyId>
fn transform_of(s: Snapshot, body: BodyId) -> Transform<3>
fn center_of_mass(s: Snapshot, densities: Map<BodyId, Density> = undef) -> Point3<Length>
fn bounding_box(s: Snapshot) -> BoundingBox
```

`center_of_mass` with `densities = undef` (the default) uses uniform density across all bodies; an empty map (`{}`) is treated identically to `undef`. A partial map uses the specified density for each listed body and falls back to uniform density for any body absent from the map. `bounding_box` returns the axis-aligned bounding box of all body geometry in the snapshot, expressed in world coordinates. `BoundingBox` is defined in §3.10. `BodyId` is the opaque identifier returned by `.body()` (§13.2).

### 13.4 `std.mechanism.sweep`

`sweep` and `sweep_grid` produce lists of snapshots by varying one or more joints over a range.

```
fn sweep<J: DrivingJoint>(m: Mechanism, joint: J, range: Range<MotionValue<J>>, steps: Int) -> List<Snapshot>
fn sweep_grid(m: Mechanism, dims: List<SweepDim>) -> List<Snapshot>
fn dim<J: DrivingJoint>(joint: J, range: Range<MotionValue<J>>, steps: Int) -> SweepDim
```

`sweep` produces `steps` snapshots evenly spaced over `range` (the range carries the joint's motion-variable unit per `MotionValue<J>` in §13.1, so `0mm .. 500mm` for a `Prismatic` and `0deg .. 90deg` for a `Revolute` are both well-typed). The first snapshot matches `snapshot(m, [bind(joint, range.start)])` and the last matches `snapshot(m, [bind(joint, range.end)])`. All joints not mentioned take their range midpoint.

`sweep_grid` computes the cross-product of the joint ranges (each constructed via `dim(joint, range, steps)`) in lexicographic order: the last dimension varies fastest. The total snapshot count is the product of all `steps` values. `SweepDim` is the sum type analogous to `JointBinding` that lets a grid mix driving-joint kinds (e.g. a prismatic `Range<Length>` alongside a revolute `Range<Angle>`). Its concrete variants are `dim(j: Prismatic, range: Range<Length>, steps: Int) -> SweepDim` and `dim(j: Revolute, range: Range<Angle>, steps: Int) -> SweepDim`. Couplings cannot appear in sweep dims for the same reason as in §13.3 — their motion is derived from the driving joint that is already being swept.

### 13.5 `std.mechanism.query`

Interference and clearance queries operate on a `Snapshot`, testing OCCT BREP geometry of placed bodies.

```
fn interferes(s: Snapshot) -> List<(BodyId, BodyId)>
fn interferes_with(s: Snapshot, a: BodyId, b: BodyId) -> Bool
fn min_clearance(s: Snapshot, a: BodyId, b: BodyId) -> Length
```

`interferes` returns all body pairs whose OCCT BREP intersection is non-empty, subject to a configurable tolerance. Excluded by default: pairs where one body is the immediate joint-frame parent of the other (they share an edge by construction), and self-pairs.

`interferes_with` is the targeted scalar form — returns `true` iff the BREP intersection of `a` and `b` is non-empty.

`min_clearance` computes the minimum separation distance between `a` and `b` using OCCT's `BRepExtrema_DistShapeShape`. Returns `0mm` when the bodies intersect.

### 13.6 Worked examples

The two examples below are the primary acceptance-test drivers for `std.mechanism`. Both are reproduced verbatim from `docs/prds/kinematic-constraints.md`.

**Toolchanger dock-approach clearance check.** A toolhead riding on a gantry that itself rides on a Y-rail sweeps its dock-approach path; the interference query asserts no collision with the parked tool anywhere along the path except at the final dock pose.

```reify
fn toolchanger_dock_check() -> Bool {
    // Two prismatic joints, declared as stdlib values.
    let y_axis = prismatic(axis: Y_HAT, range: 0mm .. 800mm);
    let x_axis = prismatic(axis: X_HAT, range: 0mm .. 500mm);

    // Mechanism assembly: bodies bound to joint frames.
    let m = mechanism()
        .body(frame_solid(), at: world)
        .body(gantry_solid(), at: y_axis)
        .body(toolhead_solid(), at: x_axis, parent: y_axis)
        .body(parked_tool_solid(), at: world, pose: dock_pose);

    // Sweep the head over its dock-approach path.
    let snapshots = sweep(m, x_axis, 0mm .. 500mm, steps: 50);

    // Interference query — toolhead must not collide with parked tool
    // anywhere along the path except at the final dock pose.
    let collisions = snapshots.map(|s| interferes(s));
    forall i in 0..50 - 1: collisions[i].is_empty()
}
```

**Counter-mass COM stationarity check.** A coupled counter-mass (ratio −1.0) keeps the system centre of mass stationary as the toolhead traverses its range.

```reify
fn counter_mass_balance() -> Bool {
    let x_axis = prismatic(axis: X_HAT, range: 0mm .. 500mm);
    // Counter-mass tracks -1× the head along the same X.
    let cm_axis = couple(x_axis, ratio: -1.0);

    let m = mechanism()
        .body(toolhead_solid(), at: x_axis)
        .body(counter_mass_solid(), at: cm_axis);

    // At every position along the sweep, the system COM must stay fixed.
    let snapshots = sweep(m, x_axis, 0mm .. 500mm, steps: 11);
    let coms = snapshots.map(|s| s.center_of_mass());
    forall pair in coms.windows(2): (pair[1] - pair[0]).norm() < 0.1mm
}
```

See `docs/prds/kinematic-constraints.md` for the full specification, acceptance criteria, and task breakdown.
